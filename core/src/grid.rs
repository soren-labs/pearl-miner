use anyhow::{bail, ensure, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use pearl_blake3::{blake3_digest, pad_to_chunk_boundary, MerkleTree};
use primitive_types::U256;
use rand::Rng;
use rayon::prelude::*;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use zk_pow::api::proof::{
    IncompleteBlockHeader, MMAType, MiningConfiguration, PeriodicPattern, SeedDerivation,
};
use zk_pow::api::sanity_checks::{penalized_target_bound, public_params_sanity_check};
use zk_pow::api::verify::verify_plain_proof;
use zk_pow::ffi::plain_proof::{
    check_cert_version_eligible, CertificateVersion, MatrixMerkleProof, PlainProof,
};
use zk_pow::v1::api::proof as v1_proof;
use zk_pow::v1::api::verify as v1_verify;

pub const SIGNAL_MIN: i8 = -64;
pub const SIGNAL_MAX: i8 = 64;
pub const SEED_LABEL_A: [u8; 32] = [
    b'A', b'_', b't', b'e', b'n', b's', b'o', b'r', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
];
pub const SEED_LABEL_B: [u8; 32] = [
    b'B', b'_', b't', b'e', b'n', b's', b'o', b'r', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
];

#[derive(Clone, Debug)]
pub struct JobShape {
    pub m: usize,
    pub n: usize,
    pub k: usize,
    pub r: usize,
    pub rows_pattern: PeriodicPattern,
    pub cols_pattern: PeriodicPattern,
    pub mining_config: MiningConfiguration,
    pub row_bases: Vec<u32>,
    pub col_bases: Vec<u32>,
}

impl JobShape {
    pub fn preset_for_kernel(kernel_name: &str) -> Result<Self> {
        if kernel_name == "pow_sweep_m6_tensor" {
            // Measured optimal M6 preset: M=8192, N=65536, K=2048, R=128
            Self::new(8192, 65536, 2048, 128)
        } else {
            // Balanced cross-Apple-Silicon preset (M1-M5 scalar kernel)
            Self::new(8192, 8192, 4096, 128)
        }
    }

    pub fn new(m: usize, n: usize, k: usize, r: usize) -> Result<Self> {
        let rows_pattern = PeriodicPattern::from_list(&[0, 32])?;
        let cols_vec: Vec<u32> = (0..64).collect();
        let cols_pattern = PeriodicPattern::from_list(&cols_vec)?;
        let mining_config = MiningConfiguration {
            common_dim: k as u32,
            rank: r as u16,
            mma_type: MMAType::Int7xInt7ToInt32,
            rows_pattern,
            cols_pattern,
            moe: None,
        };
        ensure!(m > 0 && n > 0, "m and n must be positive");
        ensure!(
            m.is_multiple_of(rows_pattern.period() as usize),
            "m must be a multiple of rows pattern period"
        );
        ensure!(
            n.is_multiple_of(cols_pattern.period() as usize),
            "n must be a multiple of cols pattern period"
        );
        // Validate against upstream sanity checks using a dummy zero-base PublicProofParams
        let dummy_params = zk_pow::api::proof::PublicProofParams {
            block_header: IncompleteBlockHeader {
                version: 1,
                prev_block: [0; 32],
                merkle_root: [0; 32],
                timestamp: 0,
                nbits: 0x207fffff,
            },
            seed_derivation: SeedDerivation::Salted,
            mining_config,
            hash_a: [0; 32],
            hash_b: [0; 32],
            hash_jackpot: [0; 32],
            m: m as u32,
            n: n as u32,
            t_rows: 0,
            t_cols: 0,
            moe: None,
        };
        public_params_sanity_check(&dummy_params)?;

        let row_bases: Vec<u32> = (0..m as u32)
            .filter(|&o| rows_pattern.offset_is_valid(o))
            .collect();
        let col_bases: Vec<u32> = (0..n as u32)
            .filter(|&o| cols_pattern.offset_is_valid(o))
            .collect();

        Ok(Self {
            m,
            n,
            k,
            r,
            rows_pattern,
            cols_pattern,
            mining_config,
            row_bases,
            col_bases,
        })
    }

    pub fn tiles_per_grid(&self) -> usize {
        self.row_bases.len() * self.col_bases.len()
    }

    pub fn penalized_factor(&self) -> u64 {
        let h = self.rows_pattern.size() as u64;
        let w = self.cols_pattern.size() as u64;
        let dpl = (self.k - (self.k % self.r)) as u64;
        h * w * (dpl / self.r as u64) * 128
    }

    pub fn is_fast_path(&self) -> bool {
        self.rows_pattern.shape[0] == (32, 2)
            && self.rows_pattern.shape[1].1 == 1
            && self.cols_pattern.shape[0] == (1, 64)
            && self.cols_pattern.shape[1].1 == 1
            && self.r <= 128
            && self.m.is_multiple_of(64)
            && self.n.is_multiple_of(64)
    }
}

pub fn compute_job_key(header_bytes: &[u8], config: &MiningConfiguration) -> [u8; 32] {
    let cfg_bytes = config.to_bytes();
    let mut buf = Vec::with_capacity(header_bytes.len() + cfg_bytes.len());
    buf.extend_from_slice(header_bytes);
    buf.extend_from_slice(&cfg_bytes);
    blake3_digest(&buf, None)
}

pub fn target_to_nbits(target: U256) -> u32 {
    if target.is_zero() {
        return 0;
    }
    let bits = target.bits();
    let mut size = bits.div_ceil(8);
    let mut mantissa: u32 = if size <= 3 {
        (target << (8 * (3 - size))).as_u32()
    } else {
        (target >> (8 * (size - 3))).as_u32()
    };
    if mantissa & 0x0080_0000 != 0 {
        mantissa >>= 8;
        size += 1;
    }
    ((size as u32) << 24) | (mantissa & 0x00ff_ffff)
}

pub fn scaled_bound_le(target: U256, config: &MiningConfiguration) -> Option<([u8; 32], U256)> {
    let bound = penalized_target_bound(target, config)?;
    let mut le = [0u8; 32];
    bound.to_little_endian(&mut le);
    Some((le, bound))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridIdentity {
    pub job_key: [u8; 32],
    pub m: usize,
    pub n: usize,
    pub k: usize,
    pub r: usize,
    pub rows_shape: [(u32, u32); 3],
    pub cols_shape: [(u32, u32); 3],
    pub cert_version: u32,
}

impl GridIdentity {
    pub fn from_job(shape: &JobShape, header_bytes: &[u8], cert_version: u32) -> Self {
        let job_key = compute_job_key(header_bytes, &shape.mining_config);
        Self {
            job_key,
            m: shape.m,
            n: shape.n,
            k: shape.k,
            r: shape.r,
            rows_shape: shape.rows_pattern.shape,
            cols_shape: shape.cols_pattern.shape,
            cert_version,
        }
    }
}

pub struct CachedBState {
    pub id: u64,
    pub identity: GridIdentity,
    pub bt_bytes: Vec<u8>,
    pub tree_bt: MerkleTree,
    pub b_seed: [u8; 32],
}

pub struct Grid {
    pub identity: GridIdentity,
    pub a_bytes: Vec<u8>,
    pub tree_a: MerkleTree,
    pub b_state: Arc<CachedBState>,
    pub b_reused: bool,
    pub a_seed: [u8; 32],
    pub shape: JobShape,
}

fn fill_random_signal_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    let chunk_size = (len / rayon::current_num_threads().max(1)).max(65536);
    buf.par_chunks_mut(chunk_size).for_each(|chunk| {
        let mut rng = rand::rng();
        for byte in chunk.iter_mut() {
            let val: i8 = rng.random_range(SIGNAL_MIN..=SIGNAL_MAX);
            *byte = val as u8;
        }
    });
    buf
}

impl Grid {
    pub fn build(
        shape: &JobShape,
        header_bytes: &[u8],
        cert_version: u32,
        cached_b: Option<Arc<CachedBState>>,
        next_b_id: &mut u64,
    ) -> Result<Self> {
        let identity = GridIdentity::from_job(shape, header_bytes, cert_version);
        let seed_derivation = CertificateVersion::try_from(cert_version)?.seed_derivation();

        let a_bytes = fill_random_signal_bytes(shape.m * shape.k);
        let a_padded = pad_to_chunk_boundary(&a_bytes);
        let tree_a = MerkleTree::new(&a_padded, identity.job_key);
        let hash_a = tree_a.root();

        let b_reused = cached_b.as_ref().is_some_and(|b| b.identity == identity);

        let b_state = if let Some(b) = cached_b.filter(|b| b.identity == identity) {
            b
        } else {
            let bt_bytes = fill_random_signal_bytes(shape.n * shape.k);
            let bt_padded = pad_to_chunk_boundary(&bt_bytes);
            let tree_bt = MerkleTree::new(&bt_padded, identity.job_key);
            let hash_b = tree_bt.root();
            let (_, bound_b) =
                seed_derivation.bind_roots(&hash_a, &hash_b, shape.m as u32, shape.n as u32);
            let mut b_in = [0u8; 64];
            b_in[..32].copy_from_slice(&identity.job_key);
            b_in[32..64].copy_from_slice(&bound_b);
            let b_seed = blake3_digest(&b_in, None);
            *next_b_id += 1;
            Arc::new(CachedBState {
                id: *next_b_id,
                identity: identity.clone(),
                bt_bytes,
                tree_bt,
                b_seed,
            })
        };

        let (bound_a, _) = seed_derivation.bind_roots(
            &hash_a,
            &b_state.tree_bt.root(),
            shape.m as u32,
            shape.n as u32,
        );
        let mut a_in = [0u8; 64];
        a_in[..32].copy_from_slice(&b_state.b_seed);
        a_in[32..64].copy_from_slice(&bound_a);
        let a_seed = blake3_digest(&a_in, None);

        Ok(Self {
            identity,
            a_bytes,
            tree_a,
            b_state,
            b_reused,
            a_seed,
            shape: shape.clone(),
        })
    }

    pub fn craft_plain_proof(&self, base_r: u32, base_c: u32) -> Result<PlainProof> {
        let rows: Vec<usize> = self
            .shape
            .rows_pattern
            .to_list()
            .into_iter()
            .map(|p| (base_r + p) as usize)
            .collect();
        let cols: Vec<usize> = self
            .shape
            .cols_pattern
            .to_list()
            .into_iter()
            .map(|q| (base_c + q) as usize)
            .collect();

        let la = MerkleTree::compute_leaf_indices_from_rows(&rows, (self.shape.m, self.shape.k));
        let lb = MerkleTree::compute_leaf_indices_from_rows(&cols, (self.shape.n, self.shape.k));

        let proof_a = self.tree_a.get_multileaf_proof(&la);
        let proof_b = self.b_state.tree_bt.get_multileaf_proof(&lb);

        Ok(PlainProof {
            m: self.shape.m,
            n: self.shape.n,
            k: self.shape.k,
            noise_rank: self.shape.r,
            a: MatrixMerkleProof {
                proof: proof_a,
                row_indices: rows,
            },
            bt: MatrixMerkleProof {
                proof: proof_b,
                row_indices: cols,
            },
            moe: None,
        })
    }
}

pub fn plain_proof_to_base64(proof: &PlainProof) -> Result<String> {
    let bytes = bincode::serialize(proof)?;
    Ok(BASE64.encode(bytes))
}

pub fn verify_share_local(
    header_bytes: &[u8],
    proof: &PlainProof,
    pool_target: U256,
    cert_version: u32,
) -> Result<()> {
    let header = IncompleteBlockHeader::from_bytes(header_bytes)?;
    let nbits = target_to_nbits(pool_target);
    ensure!(nbits != 0, "pool target nbits rounded to zero");
    match check_cert_version_eligible(cert_version, proof)? {
        CertificateVersion::ZkDense => {
            let v1_h = v1_proof::IncompleteBlockHeader {
                version: header.version,
                prev_block: header.prev_block,
                merkle_root: header.merkle_root,
                timestamp: header.timestamp,
                nbits: header.nbits,
            };
            v1_verify::verify_plain_proof(&v1_h, proof, Some(nbits))
        }
        CertificateVersion::ZkMoe => {
            verify_plain_proof(&header, proof, Some(nbits), SeedDerivation::Legacy)
        }
        CertificateVersion::ZkV3 => {
            verify_plain_proof(&header, proof, Some(nbits), SeedDerivation::Salted)
        }
    }
}

pub struct GridFactory {
    tx: Option<Sender<FactoryMsg>>,
    latest_rx: Mutex<Option<Receiver<Result<Grid>>>>,
    target_identity: Arc<Mutex<Option<GridIdentity>>>,
    handle: Option<thread::JoinHandle<()>>,
}

#[allow(clippy::large_enum_variant)] // At most one prefetched build is queued.
enum FactoryMsg {
    Build {
        shape: JobShape,
        header_bytes: Vec<u8>,
        cert_version: u32,
        reply: Sender<Result<Grid>>,
    },
    Stop,
}

impl Default for GridFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl GridFactory {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<FactoryMsg>();
        let target_identity = Arc::new(Mutex::new(None));
        let handle = thread::spawn(move || {
            let mut cached_b: Option<Arc<CachedBState>> = None;
            let mut next_b_id: u64 = 0;
            while let Ok(msg) = rx.recv() {
                match msg {
                    FactoryMsg::Stop => break,
                    FactoryMsg::Build {
                        shape,
                        header_bytes,
                        cert_version,
                        reply,
                    } => {
                        let id = GridIdentity::from_job(&shape, &header_bytes, cert_version);
                        if cached_b.as_ref().is_some_and(|b| b.identity != id) {
                            cached_b = None;
                        }
                        let res = Grid::build(
                            &shape,
                            &header_bytes,
                            cert_version,
                            cached_b.clone(),
                            &mut next_b_id,
                        );
                        if let Ok(ref g) = res {
                            cached_b = Some(Arc::clone(&g.b_state));
                        }
                        let _ = reply.send(res);
                    }
                }
            }
        });
        Self {
            tx: Some(tx),
            latest_rx: Mutex::new(None),
            target_identity,
            handle: Some(handle),
        }
    }

    pub fn prefetch(&self, shape: &JobShape, header_bytes: &[u8], cert_version: u32) {
        let id = GridIdentity::from_job(shape, header_bytes, cert_version);
        *self.target_identity.lock().unwrap() = Some(id);
        let mut guard = self.latest_rx.lock().unwrap();
        if guard.is_some() {
            return;
        }
        if let Some(ref tx) = self.tx {
            let (r_tx, r_rx) = mpsc::channel();
            if tx
                .send(FactoryMsg::Build {
                    shape: shape.clone(),
                    header_bytes: header_bytes.to_vec(),
                    cert_version,
                    reply: r_tx,
                })
                .is_ok()
            {
                *guard = Some(r_rx);
            }
        }
    }

    pub fn take(&self, shape: &JobShape, header_bytes: &[u8], cert_version: u32) -> Result<Grid> {
        let wanted = GridIdentity::from_job(shape, header_bytes, cert_version);
        let pending = self.latest_rx.lock().unwrap().take();
        if let Some(rx) = pending {
            if let Ok(Ok(grid)) = rx.recv() {
                if grid.identity == wanted {
                    return Ok(grid);
                }
            }
        }
        // Build synchronously if no matching prefetched grid is available
        let (r_tx, r_rx) = mpsc::channel();
        if let Some(ref tx) = self.tx {
            tx.send(FactoryMsg::Build {
                shape: shape.clone(),
                header_bytes: header_bytes.to_vec(),
                cert_version,
                reply: r_tx,
            })?;
            return r_rx.recv()?;
        }
        bail!("GridFactory stopped")
    }
}

impl Drop for GridFactory {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(FactoryMsg::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zk_pow::api::proof_utils::nbits_to_difficulty;

    #[test]
    fn target_to_nbits_floor_invariant() {
        let cases = [
            U256::from(1u64) << 224,
            (U256::from(0x00ab_cdefu64) << 210) + U256::from(12345u64),
            U256::from_str_radix(
                "00000000ffff0000000000000000000000000000000000000000000000000000",
                16,
            )
            .unwrap(),
        ];
        for t in cases {
            let nb = target_to_nbits(t);
            let back = nbits_to_difficulty(nb);
            assert!(back > U256::zero() && back <= t);
        }
    }
}
