use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::{CFRelease, CFTypeRef};
use core_foundation_sys::dictionary::{
    CFDictionaryCreateMutableCopy, CFDictionaryGetValue, CFDictionaryRef, CFMutableDictionaryRef,
};
use core_foundation_sys::string::CFStringRef;
use std::collections::HashMap;
use std::marker::{PhantomData, PhantomPinned};
use std::os::raw::{c_char, c_int, c_void};
use std::process::Command;
use std::ptr;
use std::time::Instant;

// ── IOKit FFI for AppleSMC ──────────────────────────────────────────────────

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn mach_task_self() -> u32;
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
    fn IOServiceOpen(service: u32, owning_task: u32, conn_type: u32, connect: *mut u32) -> c_int;
    fn IOServiceClose(connect: u32) -> c_int;
    fn IOObjectRelease(object: u32) -> c_int;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input_struct: *const u8,
        input_size: usize,
        output_struct: *mut u8,
        output_size: *mut usize,
    ) -> c_int;
}

const KERNEL_INDEX_SMC: u32 = 2;
const SMC_CMD_READ_BYTES: u8 = 5;
const SMC_CMD_READ_KEYINFO: u8 = 9;
const TYPE_FLT: u32 = 0x666c7420; // "flt "
const TYPE_IOF: u32 = 0x696f6620; // "iof "
const TYPE_SP78: u32 = 0x73703738; // "sp78"

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcVers {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcPLimitData {
    version: u16,
    length: u16,
    cpu_p_limit: u32,
    gpu_p_limit: u32,
    mem_p_limit: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcKeyInfoData {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcKeyData {
    key: u32,
    vers: SmcVers,
    p_limit_data: SmcPLimitData,
    key_info: SmcKeyInfoData,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

fn fourcc(s: &str) -> u32 {
    let b = s.as_bytes();
    if b.len() < 4 {
        return 0;
    }
    ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32)
}

pub struct SmcPowerReader {
    conn: u32,
    key_info_cache: HashMap<u32, SmcKeyInfoData>,
}

unsafe impl Send for SmcPowerReader {}

impl SmcPowerReader {
    pub fn open() -> Option<Self> {
        unsafe {
            let matching = IOServiceMatching(c"AppleSMC".as_ptr());
            if matching.is_null() {
                return None;
            }
            let service = IOServiceGetMatchingService(0, matching);
            if service == 0 {
                return None;
            }
            let mut conn: u32 = 0;
            let kr = IOServiceOpen(service, mach_task_self(), 0, &mut conn);
            IOObjectRelease(service);
            if kr != 0 {
                return None;
            }
            Some(Self {
                conn,
                key_info_cache: HashMap::new(),
            })
        }
    }

    fn call(&self, input: &SmcKeyData) -> Option<SmcKeyData> {
        unsafe {
            let mut output = SmcKeyData::default();
            let mut out_size = std::mem::size_of::<SmcKeyData>();
            let kr = IOConnectCallStructMethod(
                self.conn,
                KERNEL_INDEX_SMC,
                input as *const SmcKeyData as *const u8,
                std::mem::size_of::<SmcKeyData>(),
                &mut output as *mut SmcKeyData as *mut u8,
                &mut out_size,
            );
            if kr != 0 || output.result != 0 {
                return None;
            }
            Some(output)
        }
    }

    fn read_key_info(&mut self, key: u32) -> Option<SmcKeyInfoData> {
        if let Some(&cached) = self.key_info_cache.get(&key) {
            return Some(cached);
        }
        let input = SmcKeyData {
            key,
            data8: SMC_CMD_READ_KEYINFO,
            ..Default::default()
        };
        let out = self.call(&input)?;
        self.key_info_cache.insert(key, out.key_info);
        Some(out.key_info)
    }

    pub fn read_watts(&mut self, key_str: &str) -> Option<f64> {
        let key = fourcc(key_str);
        let info = self.read_key_info(key)?;
        if info.data_size == 0 || info.data_size > 32 {
            return None;
        }
        let input = SmcKeyData {
            key,
            data8: SMC_CMD_READ_BYTES,
            key_info: info,
            ..Default::default()
        };
        let out = self.call(&input)?;
        let val = match info.data_type {
            TYPE_FLT | TYPE_IOF => {
                f32::from_le_bytes([out.bytes[0], out.bytes[1], out.bytes[2], out.bytes[3]]) as f64
            }
            TYPE_SP78 => (i16::from_be_bytes([out.bytes[0], out.bytes[1]]) as f64) / 256.0,
            _ => return None,
        };
        if val.is_finite() && (0.05..=600.0).contains(&val) {
            Some(val)
        } else {
            None
        }
    }

    pub fn read_system_watts(&mut self) -> Option<f64> {
        self.read_watts("PSTR").or_else(|| self.read_watts("PDTR"))
    }
}

impl Drop for SmcPowerReader {
    fn drop(&mut self) {
        if self.conn != 0 {
            unsafe { IOServiceClose(self.conn) };
            self.conn = 0;
        }
    }
}

// ── IOReport FFI for GPU Energy ─────────────────────────────────────────────

#[repr(C)]
struct IOReportSubscriptionRaw {
    _data: [u8; 0],
    _pin: PhantomData<(*mut u8, PhantomPinned)>,
}
type IOReportSubscriptionRef = *mut IOReportSubscriptionRaw;

#[link(name = "IOReport", kind = "dylib")]
extern "C" {
    fn IOReportCopyChannelsInGroup(
        group: CFStringRef,
        subgroup: CFStringRef,
        c: u64,
        d: u64,
        e: u64,
    ) -> CFDictionaryRef;
    fn IOReportCreateSubscription(
        a: *const c_void,
        desired: CFMutableDictionaryRef,
        subd: *mut CFMutableDictionaryRef,
        channel_id: u64,
        e: CFTypeRef,
    ) -> IOReportSubscriptionRef;
    fn IOReportCreateSamples(
        sub: IOReportSubscriptionRef,
        subd: CFMutableDictionaryRef,
        a: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportCreateSamplesDelta(
        prev: CFDictionaryRef,
        cur: CFDictionaryRef,
        a: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportChannelGetChannelName(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetUnitLabel(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportSimpleGetIntegerValue(ch: CFDictionaryRef, idx: i32) -> i64;
}

unsafe fn cfstring_to_string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    CFString::wrap_under_get_rule(s).to_string()
}

pub struct IoReportGpuReader {
    sub: IOReportSubscriptionRef,
    subd: CFMutableDictionaryRef,
    prev_sample: Option<(CFDictionaryRef, Instant)>,
    channels_key: CFString,
}

unsafe impl Send for IoReportGpuReader {}

impl IoReportGpuReader {
    pub fn open() -> Option<Self> {
        unsafe {
            let group = CFString::new("Energy Model");
            let channels =
                IOReportCopyChannelsInGroup(group.as_concrete_TypeRef(), ptr::null(), 0, 0, 0);
            if channels.is_null() {
                return None;
            }
            let desired = CFDictionaryCreateMutableCopy(ptr::null(), 0, channels);
            CFRelease(channels as CFTypeRef);
            if desired.is_null() {
                return None;
            }
            let mut subd: CFMutableDictionaryRef = ptr::null_mut();
            let sub = IOReportCreateSubscription(ptr::null(), desired, &mut subd, 0, ptr::null());
            CFRelease(desired as CFTypeRef);
            if sub.is_null() || subd.is_null() {
                if !subd.is_null() {
                    CFRelease(subd as CFTypeRef);
                }
                return None;
            }
            let sample = IOReportCreateSamples(sub, subd, ptr::null());
            let prev_sample = if sample.is_null() {
                None
            } else {
                Some((sample, Instant::now()))
            };
            Some(Self {
                sub,
                subd,
                prev_sample,
                channels_key: CFString::new("IOReportChannels"),
            })
        }
    }

    pub fn sample_gpu_watts(&mut self) -> Option<f64> {
        unsafe {
            let cur = IOReportCreateSamples(self.sub, self.subd, ptr::null());
            if cur.is_null() {
                return None;
            }
            let now = Instant::now();
            let (prev_dict, prev_ts) = self.prev_sample.replace((cur, now))?;
            let dt_s = now.duration_since(prev_ts).as_secs_f64();
            let delta = IOReportCreateSamplesDelta(prev_dict, cur, ptr::null());
            CFRelease(prev_dict as CFTypeRef);
            if delta.is_null() {
                return None;
            }
            if dt_s < 0.1 {
                CFRelease(delta as CFTypeRef);
                return None;
            }
            let arr = CFDictionaryGetValue(
                delta,
                self.channels_key.as_concrete_TypeRef() as *const c_void,
            ) as CFArrayRef;
            let mut gpu_watts = None;
            if !arr.is_null() {
                let count = CFArrayGetCount(arr);
                for i in 0..count {
                    let ch = CFArrayGetValueAtIndex(arr, i) as CFDictionaryRef;
                    if ch.is_null() {
                        continue;
                    }
                    let name = cfstring_to_string(IOReportChannelGetChannelName(ch));
                    if name == "GPU Energy" || name.contains("GPU Energy") {
                        let unit = cfstring_to_string(IOReportChannelGetUnitLabel(ch));
                        let raw_val = IOReportSimpleGetIntegerValue(ch, 0);
                        let scale = match unit.trim() {
                            "mJ" => 1e-3,
                            "uJ" => 1e-6,
                            "nJ" => 1e-9,
                            _ => continue,
                        };
                        let w = (raw_val as f64 * scale) / dt_s;
                        if w.is_finite() && (0.0..=500.0).contains(&w) {
                            gpu_watts = Some(w);
                            break;
                        }
                    }
                }
            }
            CFRelease(delta as CFTypeRef);
            gpu_watts
        }
    }
}

impl Drop for IoReportGpuReader {
    fn drop(&mut self) {
        unsafe {
            if let Some((prev, _)) = self.prev_sample.take() {
                CFRelease(prev as CFTypeRef);
            }
            if !self.subd.is_null() {
                CFRelease(self.subd as CFTypeRef);
                self.subd = ptr::null_mut();
            }
            if !self.sub.is_null() {
                CFRelease(self.sub as CFTypeRef);
                self.sub = ptr::null_mut();
            }
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PowerReading {
    pub system_watts: Option<f64>,
    pub gpu_watts: Option<f64>,
    pub is_estimated: bool,
    pub on_battery: bool,
}

pub struct PowerTelemetry {
    smc: Option<SmcPowerReader>,
    ioreport: Option<IoReportGpuReader>,
}

impl Default for PowerTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerTelemetry {
    pub fn new() -> Self {
        Self {
            smc: SmcPowerReader::open(),
            ioreport: IoReportGpuReader::open(),
        }
    }

    pub fn sample(&mut self, device_name: &str, intensity: u8) -> PowerReading {
        let sys_w = self.smc.as_mut().and_then(|s| s.read_system_watts());
        let gpu_w = self.ioreport.as_mut().and_then(|r| r.sample_gpu_watts());
        let is_estimated = sys_w.is_none() || gpu_w.is_none();
        let fallback_gpu = crate::economics::gpu_watts_fallback(device_name, intensity);
        PowerReading {
            system_watts: sys_w.or(Some(fallback_gpu + 6.0)),
            gpu_watts: gpu_w.or(Some(fallback_gpu)),
            is_estimated,
            on_battery: is_on_battery(),
        }
    }
}

pub fn is_on_battery() -> bool {
    let Ok(out) = Command::new("pmset").args(["-g", "batt"]).output() else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.contains("Battery Power")
}

pub fn hid_idle_seconds() -> f64 {
    let Ok(out) = Command::new("ioreg")
        .args(["-c", "IOHIDSystem", "-d", "4"])
        .output()
    else {
        return 0.0;
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        if line.contains("HIDIdleTime") {
            if let Some(val_str) = line.split('=').next_back() {
                if let Ok(ns) = val_str.trim().parse::<u64>() {
                    return (ns as f64) / 1e9;
                }
            }
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_reads_without_panic() {
        let mut t = PowerTelemetry::new();
        std::thread::sleep(std::time::Duration::from_millis(150));
        let r = t.sample("Apple M6", 100);
        assert!(r.system_watts.unwrap_or(0.0) > 0.0);
    }
}
