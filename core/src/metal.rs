use anyhow::{bail, Result};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use zk_pow::api::proof::PeriodicPattern;

#[repr(C)]
struct PmCtxOpaque {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PmShape {
    pub k: u32,
    pub r: u32,
    pub h: u32,
    pub w: u32,
    pub rows: [u32; 6],
    pub cols: [u32; 6],
}

impl PmShape {
    pub fn from_patterns(
        k: u32,
        r: u32,
        rows_pattern: &PeriodicPattern,
        cols_pattern: &PeriodicPattern,
    ) -> Self {
        let mut rows = [0u32; 6];
        let mut cols = [0u32; 6];
        for i in 0..3 {
            rows[2 * i] = rows_pattern.shape[i].0;
            rows[2 * i + 1] = rows_pattern.shape[i].1;
            cols[2 * i] = cols_pattern.shape[i].0;
            cols[2 * i + 1] = cols_pattern.shape[i].1;
        }
        Self {
            k,
            r,
            h: rows_pattern.size(),
            w: cols_pattern.size(),
            rows,
            cols,
        }
    }
}

extern "C" {
    fn pm_create(err: *mut c_char, errlen: usize) -> *mut PmCtxOpaque;
    fn pm_destroy(ctx: *mut PmCtxOpaque);
    fn pm_device_info(
        ctx: *mut PmCtxOpaque,
        name: *mut c_char,
        namelen: usize,
        max_threadgroup_mem: *mut u64,
        max_threads_per_tg: *mut u64,
    ) -> c_int;
    fn pm_sweep_name(ctx: *mut PmCtxOpaque) -> *const c_char;
    fn pm_compile(
        ctx: *mut PmCtxOpaque,
        shape: *const PmShape,
        err: *mut c_char,
        errlen: usize,
    ) -> c_int;
    fn pm_alloc(ctx: *mut PmCtxOpaque, bytes: usize) -> *mut c_void;
    fn pm_contents(buf: *mut c_void) -> *mut c_void;
    fn pm_release(buf: *mut c_void);
    fn pm_blake3_64(
        ctx: *mut PmCtxOpaque,
        msgs: *mut c_void,
        keys: *mut c_void,
        out: *mut c_void,
        count: u32,
        err: *mut c_char,
        errlen: usize,
    ) -> c_int;
    fn pm_noise_uniform(
        ctx: *mut PmCtxOpaque,
        seed: *const u8,
        key: *const u8,
        out: *mut c_void,
        rows: u32,
        err: *mut c_char,
        errlen: usize,
    ) -> c_int;
    fn pm_noise_pairs(
        ctx: *mut PmCtxOpaque,
        seed: *const u8,
        key: *const u8,
        out: *mut c_void,
        err: *mut c_char,
        errlen: usize,
    ) -> c_int;
    fn pm_noise_apply(
        ctx: *mut PmCtxOpaque,
        base: *mut c_void,
        table: *mut c_void,
        pairs: *mut c_void,
        out: *mut c_void,
        rows: u32,
        err: *mut c_char,
        errlen: usize,
    ) -> c_int;
    fn pm_pow_sweep(
        ctx: *mut PmCtxOpaque,
        an: *mut c_void,
        bnt: *mut c_void,
        row_bases: *mut c_void,
        n_row_bases: u32,
        col_bases: *mut c_void,
        n_col_bases: u32,
        a_seed: *const u8,
        bound: *const u8,
        hits: *mut c_void,
        hits_capacity: u32,
        digests_out: *mut c_void,
        err: *mut c_char,
        errlen: usize,
    ) -> c_int;
    fn pm_pow_sweep2(
        ctx: *mut PmCtxOpaque,
        an: *mut c_void,
        bnt: *mut c_void,
        band_lo: u32,
        n_bands: u32,
        n_col_bases: u32,
        a_seed: *const u8,
        bound: *const u8,
        hits: *mut c_void,
        hits_capacity: u32,
        digests_out: *mut c_void,
        err: *mut c_char,
        errlen: usize,
    ) -> c_int;
}

pub const HITS_CAPACITY: usize = 64;
pub const HITS_BUF_BYTES: usize = (1 + 2 * HITS_CAPACITY) * 4;

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub max_threadgroup_mem: u64,
    pub max_threads_per_tg: u64,
}

pub struct MetalBuffer {
    handle: *mut c_void,
    ptr: *mut u8,
    len: usize,
}

unsafe impl Send for MetalBuffer {}

impl MetalBuffer {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub fn as_u32_slice(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.ptr as *const u32, self.len / 4) }
    }

    pub fn as_u32_mut_slice(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut u32, self.len / 4) }
    }
}

impl Drop for MetalBuffer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { pm_release(self.handle) };
            self.handle = ptr::null_mut();
            self.ptr = ptr::null_mut();
        }
    }
}

pub struct MetalContext {
    ctx: *mut PmCtxOpaque,
}

unsafe impl Send for MetalContext {}

fn err_buf_to_string(buf: &[c_char]) -> String {
    unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

impl MetalContext {
    pub fn new() -> Result<Self> {
        let mut err = [0 as c_char; 512];
        let ctx = unsafe { pm_create(err.as_mut_ptr(), err.len()) };
        if ctx.is_null() {
            bail!("Metal init failed: {}", err_buf_to_string(&err));
        }
        Ok(Self { ctx })
    }

    pub fn device_info(&self) -> Result<DeviceInfo> {
        let mut name = [0 as c_char; 256];
        let mut max_tg_mem: u64 = 0;
        let mut max_threads: u64 = 0;
        let rc = unsafe {
            pm_device_info(
                self.ctx,
                name.as_mut_ptr(),
                name.len(),
                &mut max_tg_mem,
                &mut max_threads,
            )
        };
        if rc != 0 {
            bail!("pm_device_info failed");
        }
        let dev_name = unsafe { CStr::from_ptr(name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Ok(DeviceInfo {
            name: dev_name,
            max_threadgroup_mem: max_tg_mem,
            max_threads_per_tg: max_threads,
        })
    }

    pub fn sweep_kernel_name(&self) -> String {
        let ptr = unsafe { pm_sweep_name(self.ctx) };
        if ptr.is_null() {
            return "unknown".to_string();
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn compile(&self, shape: &PmShape) -> Result<()> {
        let mut err = [0 as c_char; 1024];
        let rc = unsafe { pm_compile(self.ctx, shape as *const PmShape, err.as_mut_ptr(), err.len()) };
        if rc != 0 {
            bail!("Metal compile failed: {}", err_buf_to_string(&err));
        }
        Ok(())
    }

    pub fn alloc(&self, bytes: usize) -> Result<MetalBuffer> {
        let bytes = bytes.max(16);
        let handle = unsafe { pm_alloc(self.ctx, bytes) };
        if handle.is_null() {
            bail!("Metal buffer allocation failed for {bytes} bytes");
        }
        let ptr = unsafe { pm_contents(handle) } as *mut u8;
        if ptr.is_null() {
            unsafe { pm_release(handle) };
            bail!("Metal buffer contents pointer was null");
        }
        Ok(MetalBuffer {
            handle,
            ptr,
            len: bytes,
        })
    }

    pub fn alloc_u32_slice(&self, data: &[u32]) -> Result<MetalBuffer> {
        let mut buf = self.alloc(data.len() * 4)?;
        buf.as_u32_mut_slice()[..data.len()].copy_from_slice(data);
        Ok(buf)
    }

    pub fn blake3_64(
        &self,
        msgs: &mut MetalBuffer,
        keys: &mut MetalBuffer,
        out: &mut MetalBuffer,
        count: u32,
    ) -> Result<()> {
        let mut err = [0 as c_char; 512];
        let rc = unsafe {
            pm_blake3_64(
                self.ctx,
                msgs.handle,
                keys.handle,
                out.handle,
                count,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if rc != 0 {
            bail!("pm_blake3_64 failed: {}", err_buf_to_string(&err));
        }
        Ok(())
    }

    pub fn noise_uniform(
        &self,
        seed: &[u8; 32],
        key: &[u8; 32],
        out: &mut MetalBuffer,
        rows: u32,
    ) -> Result<()> {
        let mut err = [0 as c_char; 512];
        let rc = unsafe {
            pm_noise_uniform(
                self.ctx,
                seed.as_ptr(),
                key.as_ptr(),
                out.handle,
                rows,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if rc != 0 {
            bail!("pm_noise_uniform failed: {}", err_buf_to_string(&err));
        }
        Ok(())
    }

    pub fn noise_pairs(
        &self,
        seed: &[u8; 32],
        key: &[u8; 32],
        out: &mut MetalBuffer,
    ) -> Result<()> {
        let mut err = [0 as c_char; 512];
        let rc = unsafe {
            pm_noise_pairs(
                self.ctx,
                seed.as_ptr(),
                key.as_ptr(),
                out.handle,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if rc != 0 {
            bail!("pm_noise_pairs failed: {}", err_buf_to_string(&err));
        }
        Ok(())
    }

    pub fn noise_apply(
        &self,
        base: &MetalBuffer,
        table: &MetalBuffer,
        pairs: &MetalBuffer,
        out: &mut MetalBuffer,
        rows: u32,
    ) -> Result<()> {
        let mut err = [0 as c_char; 512];
        let rc = unsafe {
            pm_noise_apply(
                self.ctx,
                base.handle,
                table.handle,
                pairs.handle,
                out.handle,
                rows,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if rc != 0 {
            bail!("pm_noise_apply failed: {}", err_buf_to_string(&err));
        }
        Ok(())
    }

    pub fn pow_sweep(
        &self,
        an: &MetalBuffer,
        bnt: &MetalBuffer,
        row_bases: &MetalBuffer,
        n_row_bases: u32,
        col_bases: &MetalBuffer,
        n_col_bases: u32,
        a_seed: &[u8; 32],
        bound: &[u8; 32],
        hits: &mut MetalBuffer,
        digests_out: Option<&mut MetalBuffer>,
    ) -> Result<()> {
        let mut err = [0 as c_char; 512];
        let digests_ptr = digests_out.map_or(ptr::null_mut(), |b| b.handle);
        let rc = unsafe {
            pm_pow_sweep(
                self.ctx,
                an.handle,
                bnt.handle,
                row_bases.handle,
                n_row_bases,
                col_bases.handle,
                n_col_bases,
                a_seed.as_ptr(),
                bound.as_ptr(),
                hits.handle,
                HITS_CAPACITY as u32,
                digests_ptr,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if rc != 0 {
            bail!("pm_pow_sweep failed: {}", err_buf_to_string(&err));
        }
        Ok(())
    }

    pub fn pow_sweep2(
        &self,
        an: &MetalBuffer,
        bnt: &MetalBuffer,
        band_lo: u32,
        n_bands: u32,
        n_col_bases: u32,
        a_seed: &[u8; 32],
        bound: &[u8; 32],
        hits: &mut MetalBuffer,
        digests_out: Option<&mut MetalBuffer>,
    ) -> Result<()> {
        let mut err = [0 as c_char; 512];
        let digests_ptr = digests_out.map_or(ptr::null_mut(), |b| b.handle);
        let rc = unsafe {
            pm_pow_sweep2(
                self.ctx,
                an.handle,
                bnt.handle,
                band_lo,
                n_bands,
                n_col_bases,
                a_seed.as_ptr(),
                bound.as_ptr(),
                hits.handle,
                HITS_CAPACITY as u32,
                digests_ptr,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if rc != 0 {
            bail!("pm_pow_sweep2 failed: {}", err_buf_to_string(&err));
        }
        Ok(())
    }
}

impl Drop for MetalContext {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { pm_destroy(self.ctx) };
            self.ctx = ptr::null_mut();
        }
    }
}

pub fn set_metal_kernel_env(kernel: &str) {
    if let Ok(val) = CString::new(kernel) {
        let key = CString::new("PEARL_METAL_KERNEL").unwrap();
        unsafe { libc::setenv(key.as_ptr(), val.as_ptr(), 1) };
    }
}
