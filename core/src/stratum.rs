use crate::config::DialectKind;
use anyhow::{bail, Context, Result};
use primitive_types::U256;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const HEADER_BYTES: usize = 76;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub job_id: String,
    pub header_bytes: Vec<u8>,
    pub target: U256,
    pub height: u64,
    pub cert_version: u32,
}

#[derive(Clone, Debug)]
pub struct ShareResult {
    pub msg_id: u64,
    pub accepted: bool,
    pub raw: String,
}

#[derive(Clone, Debug)]
pub enum PoolEvent {
    Job(Job),
    Share(ShareResult),
    Disconnected(String),
}

fn parse_job_params(p: &Value) -> Result<Job> {
    let job_id = p
        .get("job_id")
        .and_then(|v| v.as_str())
        .context("missing job_id")?
        .to_string();
    let header_hex = p
        .get("header")
        .and_then(|v| v.as_str())
        .context("missing header")?;
    let header_bytes = hex::decode(header_hex).context("invalid header hex")?;
    if header_bytes.len() != HEADER_BYTES {
        bail!(
            "job {}: header is {} bytes, expected {}",
            job_id,
            header_bytes.len(),
            HEADER_BYTES
        );
    }
    let target_hex = p
        .get("target")
        .and_then(|v| v.as_str())
        .context("missing target")?;
    let target = U256::from_str_radix(target_hex.trim_start_matches("0x"), 16)
        .map_err(|e| anyhow::anyhow!("invalid target hex: {e}"))?;
    if target.is_zero() {
        bail!("job {}: target is zero", job_id);
    }
    let height = p.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
    let cert_version = p
        .get("cert_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as u32;
    if !matches!(cert_version, 1 | 2 | 3) {
        bail!("job {}: unsupported cert_version {}", job_id, cert_version);
    }
    Ok(Job {
        job_id,
        header_bytes,
        target,
        height,
        cert_version,
    })
}

fn handshake_messages(dialect: DialectKind, address: &str, worker: &str) -> Vec<Value> {
    let agent = format!("PearlMiner/{}", env!("CARGO_PKG_VERSION"));
    match dialect {
        DialectKind::Kryptex => vec![
            json!({
                "id": 1,
                "method": "mining.subscribe",
                "params": [agent],
            }),
            json!({
                "id": 3,
                "method": "mining.authorize",
                "params": [format!("{address}.{worker}"), "x"],
            }),
        ],
        DialectKind::Luckypool => vec![json!({
            "id": 2,
            "method": "mining.authorize",
            "params": {
                "wallet": address,
                "worker": worker,
                "agent": agent,
            },
        })],
    }
}

fn submit_message(
    dialect: DialectKind,
    msg_id: u64,
    address: &str,
    worker: &str,
    job_id: &str,
    proof_b64: &str,
) -> Value {
    match dialect {
        DialectKind::Kryptex => json!({
            "id": msg_id,
            "method": "mining.submit",
            "params": {
                "worker": format!("{address}.{worker}"),
                "job_id": job_id,
                "plain_proof": proof_b64,
            }
        }),
        DialectKind::Luckypool => json!({
            "id": msg_id,
            "method": "mining.submit",
            "params": {
                "wallet": address,
                "worker": worker,
                "job_id": job_id,
                "plain_proof": proof_b64,
            }
        }),
    }
}

fn parse_line(line: &str) -> Option<Result<PoolEvent>> {
    let v: Value = match serde_json::from_str(line) {
        Ok(val) => val,
        Err(e) => return Some(Err(anyhow::anyhow!("JSON parse error: {e}"))),
    };
    if v.get("method").and_then(|m| m.as_str()) == Some("mining.notify") {
        let params = v.get("params")?;
        return Some(parse_job_params(params).map(PoolEvent::Job));
    }
    if v.get("result").is_some() {
        if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
            let err_null = v.get("error").map(|e| e.is_null()).unwrap_or(true);
            let ok = v.get("result").and_then(|r| r.as_bool()) == Some(true) && err_null;
            return Some(Ok(PoolEvent::Share(ShareResult {
                msg_id: id,
                accepted: ok,
                raw: line.to_string(),
            })));
        }
    }
    None
}

pub struct PoolConnection {
    dialect: DialectKind,
    host: String,
    port: u16,
    address: String,
    worker: String,
    stream: Arc<Mutex<Option<TcpStream>>>,
    events_rx: Receiver<PoolEvent>,
    events_tx: Sender<PoolEvent>,
    pub dead: Arc<AtomicBool>,
    msg_id: AtomicU64,
    last_rx_epoch_ms: Arc<AtomicU64>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl PoolConnection {
    pub fn new(
        dialect: DialectKind,
        host: String,
        port: u16,
        address: String,
        worker: String,
    ) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            dialect,
            host,
            port,
            address,
            worker,
            stream: Arc::new(Mutex::new(None)),
            events_rx,
            events_tx,
            dead: Arc::new(AtomicBool::new(true)),
            msg_id: AtomicU64::new(10),
            last_rx_epoch_ms: Arc::new(AtomicU64::new(now_ms())),
        }
    }

    pub fn connect(&mut self) -> Result<()> {
        self.close();
        while self.events_rx.try_recv().is_ok() {}

        let addr_str = format!("{}:{}", self.host, self.port);
        let addrs: Vec<SocketAddr> = addr_str
            .to_socket_addrs()
            .with_context(|| format!("DNS lookup failed for {}", self.host))?
            .collect();
        if addrs.is_empty() {
            bail!("no addresses resolved for {}", self.host);
        }

        let mut last_err = None;
        let mut connected = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, Duration::from_secs(15)) {
                Ok(s) => {
                    connected = Some(s);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let stream = connected.ok_or_else(|| {
            anyhow::anyhow!(
                "connect to {}:{} failed: {}",
                self.host,
                self.port,
                last_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown error".to_string())
            )
        })?;
        stream.set_nodelay(true).ok();

        let reader_stream = stream.try_clone()?;
        {
            let mut guard = self.stream.lock().unwrap();
            *guard = Some(stream);
        }
        self.dead.store(false, Ordering::SeqCst);
        self.last_rx_epoch_ms.store(now_ms(), Ordering::SeqCst);

        for msg in handshake_messages(self.dialect, &self.address, &self.worker) {
            self.send_json(&msg)?;
        }

        let tx = self.events_tx.clone();
        let dead = Arc::clone(&self.dead);
        let last_rx = Arc::clone(&self.last_rx_epoch_ms);
        thread::spawn(move || {
            let mut reader = BufReader::new(reader_stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        dead.store(true, Ordering::SeqCst);
                        let _ = tx.send(PoolEvent::Disconnected("EOF from pool".into()));
                        break;
                    }
                    Ok(_) => {
                        last_rx.store(now_ms(), Ordering::SeqCst);
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Some(Ok(ev)) = parse_line(trimmed) {
                            let _ = tx.send(ev);
                        }
                    }
                    Err(e) => {
                        dead.store(true, Ordering::SeqCst);
                        let _ = tx.send(PoolEvent::Disconnected(e.to_string()));
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    pub fn send_json(&self, val: &Value) -> Result<()> {
        let mut raw = serde_json::to_string(val)?;
        raw.push('\n');
        let mut guard = self.stream.lock().unwrap();
        let Some(ref mut s) = *guard else {
            self.dead.store(true, Ordering::SeqCst);
            bail!("not connected");
        };
        if let Err(e) = s.write_all(raw.as_bytes()).and_then(|_| s.flush()) {
            self.dead.store(true, Ordering::SeqCst);
            bail!("send failed: {e}");
        }
        Ok(())
    }

    pub fn submit(&self, job_id: &str, proof_b64: &str) -> Result<u64> {
        let id = self.msg_id.fetch_add(1, Ordering::SeqCst) + 1;
        let msg = submit_message(
            self.dialect,
            id,
            &self.address,
            &self.worker,
            job_id,
            proof_b64,
        );
        self.send_json(&msg)?;
        Ok(id)
    }

    pub fn try_recv(&self) -> Option<PoolEvent> {
        self.events_rx.try_recv().ok()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<PoolEvent> {
        self.events_rx.recv_timeout(timeout).ok()
    }

    pub fn silence_seconds(&self) -> f64 {
        let last = self.last_rx_epoch_ms.load(Ordering::SeqCst);
        let now = now_ms();
        now.saturating_sub(last) as f64 / 1000.0
    }

    pub fn close(&self) {
        self.dead.store(true, Ordering::SeqCst);
        let mut guard = self.stream.lock().unwrap();
        if let Some(s) = guard.take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Drop for PoolConnection {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn backoff_delay(attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(4);
    let secs = (5u64 * (1u64 << exp)).min(60);
    Duration::from_secs(secs)
}

pub fn elapsed_secs(since: Instant) -> u64 {
    since.elapsed().as_secs()
}
