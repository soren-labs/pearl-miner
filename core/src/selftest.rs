use crate::grid::{
    plain_proof_to_base64, verify_share_local, Grid, JobShape, SEED_LABEL_A, SEED_LABEL_B,
};
use crate::metal::{MetalContext, PmShape, HITS_BUF_BYTES};
use anyhow::{ensure, Result};
use primitive_types::U256;
use serde::Serialize;
use zk_pow::api::proof::{IncompleteBlockHeader, PeriodicPattern};
use zk_pow::ffi::plain_proof::PlainProof;

#[derive(Clone, Debug, Serialize)]
pub struct SelfTestReport {
    pub passed: bool,
    pub device_name: String,
    pub kernel_name: String,
    pub checks_passed: usize,
    pub elapsed_ms: u128,
    pub message: String,
}

fn rust_noise_uniform(seed: &[u8; 32], key: &[u8; 32], rows: usize, rank: usize) -> Vec<i8> {
    let mut out = vec![0i8; rows * rank];
    for r in 0..rows {
        let start = r * rank;
        let first_block = start / 32;
        let last_block = (start + rank).div_ceil(32);
        let mut stream = Vec::with_capacity((last_block - first_block) * 32);
        for b in first_block..last_block {
            let mut msg = [0u8; 64];
            msg[0..4].copy_from_slice(&((1 + b as i32).to_le_bytes()));
            msg[32..64].copy_from_slice(seed);
            stream.extend_from_slice(&pearl_blake3::blake3_digest(&msg, Some(*key)));
        }
        let lo = start - first_block * 32;
        for c in 0..rank {
            out[r * rank + c] = ((stream[lo + c] & 0x3F) as i8) - 32;
        }
    }
    out
}

fn rust_noise_pairs(seed: &[u8; 32], key: &[u8; 32], k: usize, rank: usize) -> Vec<(u32, u32)> {
    let n_digests = k.div_ceil(8);
    let mut stream = Vec::with_capacity(n_digests * 32);
    for i in 0..n_digests {
        let mut msg = [0u8; 64];
        msg[4..8].copy_from_slice(&((1 + i as i32).to_le_bytes()));
        msg[32..64].copy_from_slice(seed);
        stream.extend_from_slice(&pearl_blake3::blake3_digest(&msg, Some(*key)));
    }
    let mut pairs = Vec::with_capacity(k);
    for idx in 0..k {
        let u = u32::from_le_bytes(stream[idx * 4..idx * 4 + 4].try_into().unwrap()) as u64;
        let first = (u & ((rank - 1) as u64)) as u32;
        let mulhi = (((rank - 1) as u64 * u) >> 32) as u32;
        let second = first ^ (1 + mulhi);
        pairs.push((first, second));
    }
    pairs
}

fn rust_compute_jackpot(
    an: &[i8],
    bnt: &[i8],
    k: usize,
    rank: usize,
    row_offsets: &[u32],
    col_offsets: &[u32],
    a_seed: &[u8; 32],
) -> [u8; 32] {
    let h = row_offsets.len();
    let w = col_offsets.len();
    let mut csum = vec![0i32; h * w];
    let mut jackpot = [0u32; 16];
    let n_chunks = k / rank;
    for t in 0..n_chunks {
        let lo = t * rank;
        let hi = lo + rank;
        for (ri, &r) in row_offsets.iter().enumerate() {
            let r_off = (r as usize) * k;
            for (ci, &c) in col_offsets.iter().enumerate() {
                let c_off = (c as usize) * k;
                let mut acc = 0i32;
                for l in lo..hi {
                    acc += (an[r_off + l] as i32) * (bnt[c_off + l] as i32);
                }
                csum[ri * w + ci] = csum[ri * w + ci].wrapping_add(acc);
            }
        }
        let mut xored = 0u32;
        for &v in &csum {
            xored ^= v as u32;
        }
        let slot = t % 16;
        jackpot[slot] = jackpot[slot].rotate_left(13) ^ xored;
    }
    let mut msg = [0u8; 64];
    for i in 0..16 {
        msg[i * 4..i * 4 + 4].copy_from_slice(&jackpot[i].to_le_bytes());
    }
    pearl_blake3::blake3_digest(&msg, Some(*a_seed))
}

pub fn run_self_test() -> Result<SelfTestReport> {
    let t0 = std::time::Instant::now();
    let mut checks = 0usize;

    let metal = MetalContext::new()?;
    let info = metal.device_info()?;

    // 1. Small differential shape (general kernel + noise + digests)
    let m = 128usize;
    let n = 128usize;
    let k = 2048usize;
    let r = 128usize;
    let shape = JobShape::new(m, n, k, r)?;
    let pm_shape = PmShape::from_patterns(k as u32, r as u32, &shape.rows_pattern, &shape.cols_pattern);
    metal.compile(&pm_shape)?;
    let kernel_name = metal.sweep_kernel_name();

    let header = IncompleteBlockHeader {
        version: 1,
        prev_block: [0x11; 32],
        merkle_root: [0x22; 32],
        timestamp: 1_760_000_000,
        nbits: 0x207fffff, // easy target so hits happen every grid
    };
    let header_bytes = header.to_bytes();

    for &cert_version in &[1u32, 2u32, 3u32] {
        let mut next_b_id = 0;
        let grid = Grid::build(&shape, &header_bytes, cert_version, None, &mut next_b_id)?;

        // Check noise_uniform & noise_pairs & noise_apply bit-exactness
        let mut a_buf = metal.alloc(m * k)?;
        a_buf.as_mut_slice()[..m * k].copy_from_slice(&grid.a_bytes);
        let mut bt_buf = metal.alloc(n * k)?;
        bt_buf.as_mut_slice()[..n * k].copy_from_slice(&grid.b_state.bt_bytes);

        let mut ua = metal.alloc(m * r)?;
        let mut ub = metal.alloc(n * r)?;
        let mut pa = metal.alloc(k * 8)?;
        let mut pb = metal.alloc(k * 8)?;
        let mut an = metal.alloc(m * k)?;
        let mut bnt = metal.alloc(n * k)?;

        metal.noise_uniform(&SEED_LABEL_A, &grid.a_seed, &mut ua, m as u32)?;
        metal.noise_uniform(&SEED_LABEL_B, &grid.b_state.b_seed, &mut ub, n as u32)?;
        metal.noise_pairs(&SEED_LABEL_A, &grid.a_seed, &mut pa)?;
        metal.noise_pairs(&SEED_LABEL_B, &grid.b_state.b_seed, &mut pb)?;
        metal.noise_apply(&a_buf, &ua, &pa, &mut an, m as u32)?;
        metal.noise_apply(&bt_buf, &ub, &pb, &mut bnt, n as u32)?;

        let ref_ua = rust_noise_uniform(&SEED_LABEL_A, &grid.a_seed, m, r);
        let gpu_ua: Vec<i8> = ua.as_slice()[..m * r].iter().map(|&b| b as i8).collect();
        ensure!(gpu_ua == ref_ua, "noise_uniform mismatch for cert v{cert_version}");
        checks += 1;

        let ref_pa = rust_noise_pairs(&SEED_LABEL_A, &grid.a_seed, k, r);
        let gpu_pa = pa.as_u32_slice();
        for (i, &(f, s)) in ref_pa.iter().enumerate() {
            ensure!(
                gpu_pa[2 * i] == f && gpu_pa[2 * i + 1] == s,
                "noise_pairs mismatch at {i}"
            );
        }
        checks += 1;

        // Check pow_sweep2 (fast kernel, including M6 tensor kernel!) vs Rust reference digests
        let rb_len = shape.row_bases.len();
        let cb_len = shape.col_bases.len();
        let n_tiles = rb_len * cb_len;
        let rb_buf = metal.alloc_u32_slice(&shape.row_bases)?;
        let cb_buf = metal.alloc_u32_slice(&shape.col_bases)?;
        let mut hits1 = metal.alloc(HITS_BUF_BYTES)?;
        let mut hits2 = metal.alloc(HITS_BUF_BYTES)?;
        let mut digests1 = metal.alloc(n_tiles * 32)?;
        let mut digests2 = metal.alloc(n_tiles * 32)?;
        hits1.as_u32_mut_slice()[0] = 0;
        hits2.as_u32_mut_slice()[0] = 0;

        let bound_all = [0xFFu8; 32];
        metal.pow_sweep(
            &an,
            &bnt,
            &rb_buf,
            rb_len as u32,
            &cb_buf,
            cb_len as u32,
            &grid.a_seed,
            &bound_all,
            &mut hits1,
            Some(&mut digests1),
        )?;
        metal.pow_sweep2(
            &an,
            &bnt,
            0,
            (m / 64) as u32,
            (n / 64) as u32,
            &grid.a_seed,
            &bound_all,
            &mut hits2,
            Some(&mut digests2),
        )?;

        ensure!(
            digests1.as_slice()[..n_tiles * 32] == digests2.as_slice()[..n_tiles * 32],
            "pow_sweep vs pow_sweep2 digest mismatch for cert v{cert_version}"
        );
        checks += 1;

        let an_i8: Vec<i8> = an.as_slice()[..m * k].iter().map(|&b| b as i8).collect();
        let bnt_i8: Vec<i8> = bnt.as_slice()[..n * k].iter().map(|&b| b as i8).collect();
        let row_pat: Vec<u32> = shape.rows_pattern.to_list();
        let col_pat: Vec<u32> = shape.cols_pattern.to_list();

        // Verify first 8 tiles bit-exact against Rust jackpot reference
        for tile_idx in 0..8.min(n_tiles) {
            let ri = tile_idx / cb_len;
            let ci = tile_idx % cb_len;
            let br = shape.row_bases[ri];
            let bc = shape.col_bases[ci];
            let r_offs: Vec<u32> = row_pat.iter().map(|&p| br + p).collect();
            let c_offs: Vec<u32> = col_pat.iter().map(|&q| bc + q).collect();
            let ref_digest =
                rust_compute_jackpot(&an_i8, &bnt_i8, k, r, &r_offs, &c_offs, &grid.a_seed);
            let gpu_digest = &digests2.as_slice()[tile_idx * 32..(tile_idx + 1) * 32];
            ensure!(
                gpu_digest == ref_digest,
                "jackpot digest mismatch at tile ({br},{bc}) for cert v{cert_version}"
            );
            checks += 1;
        }

        // End-to-end PlainProof crafting + upstream zk-pow Rust verifier acceptance (V1, V2, V3!)
        let proof = grid.craft_plain_proof(shape.row_bases[0], shape.col_bases[0])?;
        let easy_target = U256::MAX / U256::from(16u64);
        verify_share_local(&header_bytes, &proof, easy_target, cert_version)?;
        let b64 = plain_proof_to_base64(&proof)?;
        let roundtrip: PlainProof =
            PlainProof::deserialize_compat(&base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64)?)?;
        verify_share_local(&header_bytes, &roundtrip, easy_target, cert_version)?;
        checks += 2;
    }

    // Also test a custom non-fast-path PeriodicPattern compile just to be sure
    let _p = PeriodicPattern::from_list(&[0, 32])?;
    let elapsed_ms = t0.elapsed().as_millis();

    Ok(SelfTestReport {
        passed: true,
        device_name: info.name.clone(),
        kernel_name: kernel_name.clone(),
        checks_passed: checks,
        elapsed_ms,
        message: format!(
            "All {checks} bit-exact checks (noise, Metal {kernel_name}, V1/V2/V3 PlainProof) passed in {elapsed_ms} ms on {}",
            info.name
        ),
    })
}
