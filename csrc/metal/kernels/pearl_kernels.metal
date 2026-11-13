// Pearl PoW Metal kernels.
//
// Consensus reference: zk-pow/src in pearl-research-labs/pearl (ISC), pinned
// in PINNED_PEARL_COMMIT.txt; NumPy restatement in pearl_metal_miner/reference.py.
// Every stage here is differentially tested against that reference — exact
// integers, no tolerances.
//
// The job shape arrives as function constants (ADR-0004/0006): the compiler
// folds k, r, h, w and both PeriodicPattern shapes into the code as literals.

#include <metal_stdlib>
using namespace metal;

constant uint FC_K [[function_constant(0)]]; // common dimension
constant uint FC_R [[function_constant(1)]]; // noise rank
constant uint FC_H [[function_constant(2)]]; // |rows_pattern|
constant uint FC_W [[function_constant(3)]]; // |cols_pattern|
constant uint FC_ROW_S0 [[function_constant(4)]];
constant uint FC_ROW_L0 [[function_constant(5)]];
constant uint FC_ROW_S1 [[function_constant(6)]];
constant uint FC_ROW_L1 [[function_constant(7)]];
constant uint FC_ROW_S2 [[function_constant(8)]];
constant uint FC_ROW_L2 [[function_constant(9)]];
constant uint FC_COL_S0 [[function_constant(10)]];
constant uint FC_COL_L0 [[function_constant(11)]];
constant uint FC_COL_S1 [[function_constant(12)]];
constant uint FC_COL_L1 [[function_constant(13)]];
constant uint FC_COL_S2 [[function_constant(14)]];
constant uint FC_COL_L2 [[function_constant(15)]];

// PeriodicPattern element p, in to_list order:
//   off(p) = (p % l0)·s0 + ((p / l0) % l1)·s1 + (p / (l0·l1))·s2
inline uint row_pattern_offset(uint p) {
  return (p % FC_ROW_L0) * FC_ROW_S0 + ((p / FC_ROW_L0) % FC_ROW_L1) * FC_ROW_S1 +
         (p / (FC_ROW_L0 * FC_ROW_L1)) * FC_ROW_S2;
}
inline uint col_pattern_offset(uint p) {
  return (p % FC_COL_L0) * FC_COL_S0 + ((p / FC_COL_L0) % FC_COL_L1) * FC_COL_S1 +
         (p / (FC_COL_L0 * FC_COL_L1)) * FC_COL_S2;
}

// ── Keyed BLAKE3, single 64-byte block ──────────────────────────────────────
// The only BLAKE3 the GPU ever needs: every message is exactly one block, so
// flags = CHUNK_START|CHUNK_END|ROOT|KEYED_HASH, counter 0, len 64.

constant uint B3_IV0 = 0x6A09E667u;
constant uint B3_IV1 = 0xBB67AE85u;
constant uint B3_IV2 = 0x3C6EF372u;
constant uint B3_IV3 = 0xA54FF53Au;
constant uint B3_FLAGS = 1u | 2u | 8u | 16u; // CHUNK_START|CHUNK_END|ROOT|KEYED_HASH

inline uint rotr32(uint x, uint n) { return (x >> n) | (x << (32u - n)); }

inline void b3_g(thread uint *st, uint a, uint b, uint c, uint d, uint mx, uint my) {
  st[a] = st[a] + st[b] + mx;
  st[d] = rotr32(st[d] ^ st[a], 16u);
  st[c] = st[c] + st[d];
  st[b] = rotr32(st[b] ^ st[c], 12u);
  st[a] = st[a] + st[b] + my;
  st[d] = rotr32(st[d] ^ st[a], 8u);
  st[c] = st[c] + st[d];
  st[b] = rotr32(st[b] ^ st[c], 7u);
}

inline void b3_round(thread uint *st, thread const uint *m) {
  b3_g(st, 0, 4, 8, 12, m[0], m[1]);
  b3_g(st, 1, 5, 9, 13, m[2], m[3]);
  b3_g(st, 2, 6, 10, 14, m[4], m[5]);
  b3_g(st, 3, 7, 11, 15, m[6], m[7]);
  b3_g(st, 0, 5, 10, 15, m[8], m[9]);
  b3_g(st, 1, 6, 11, 12, m[10], m[11]);
  b3_g(st, 2, 7, 8, 13, m[12], m[13]);
  b3_g(st, 3, 4, 9, 14, m[14], m[15]);
}

constant uchar B3_PERM[16] = {2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8};

// msg: 16 words (the 64-byte block, little-endian words); key: 8 words; out: 8 words.
inline void blake3_64_keyed(thread const uint *msg, thread const uint *key, thread uint *out) {
  uint st[16];
  for (int i = 0; i < 8; i++) st[i] = key[i];
  st[8] = B3_IV0;
  st[9] = B3_IV1;
  st[10] = B3_IV2;
  st[11] = B3_IV3;
  st[12] = 0u; // counter lo
  st[13] = 0u; // counter hi
  st[14] = 64u; // block length
  st[15] = B3_FLAGS;
  uint m[16], t[16];
  for (int i = 0; i < 16; i++) m[i] = msg[i];
  for (int r = 0; r < 7; r++) {
    if (r != 0) {
      for (int i = 0; i < 16; i++) t[i] = m[B3_PERM[i]];
      for (int i = 0; i < 16; i++) m[i] = t[i];
    }
    b3_round(st, m);
  }
  for (int i = 0; i < 8; i++) out[i] = st[i] ^ st[i + 8];
}

// ── blake3_64: batch primitive (test surface for the function above) ────────

kernel void blake3_64(device const uint *msgs [[buffer(0)]],  // count × 16 words
                      device const uint *keys [[buffer(1)]],  // count × 8 words
                      device uint *out [[buffer(2)]],         // count × 8 words
                      constant uint &count [[buffer(3)]],
                      uint gid [[thread_position_in_grid]]) {
  if (gid >= count) return;
  uint m[16], k[8], d[8];
  for (int i = 0; i < 16; i++) m[i] = msgs[gid * 16 + i];
  for (int i = 0; i < 8; i++) k[i] = keys[gid * 8 + i];
  blake3_64_keyed(m, k, d);
  for (int i = 0; i < 8; i++) out[gid * 8 + i] = d[i];
}

// ── Noise generation (pearl_noise.rs) ───────────────────────────────────────
// Message: eight i32 slots with slot `prepend` ← 1+index, then the 32-byte
// seed label. One digest yields 32 uniform bytes or 8 permutation pairs.

inline void noise_digest(uint index, uint prepend_slot, device const uint *seed,
                         device const uint *key, thread uint *d) {
  uint m[16] = {0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u};
  m[prepend_slot] = 1u + index;
  for (int i = 0; i < 8; i++) m[8 + i] = seed[i];
  uint k[8];
  for (int i = 0; i < 8; i++) k[i] = key[i];
  blake3_64_keyed(m, k, d);
}

// One thread per 32-byte block of the uniform table: value = (byte & 63) − 32.
kernel void noise_uniform(device const uint *seed [[buffer(0)]],
                          device const uint *key [[buffer(1)]],
                          device char *out [[buffer(2)]],
                          constant uint &nblocks [[buffer(3)]],
                          uint gid [[thread_position_in_grid]]) {
  if (gid >= nblocks) return;
  uint d[8];
  noise_digest(gid, 0u, seed, key, d);
  device char *o = out + gid * 32u;
  for (int i = 0; i < 8; i++) {
    uint w = d[i];
    for (int b = 0; b < 4; b++) {
      o[i * 4 + b] = (char)((int)((w >> (8u * b)) & 63u) - 32);
    }
  }
}

// One thread per digest = 8 permutation pairs:
//   first = u & (R−1);  second = first ^ (1 + mulhi(R−1, u))
kernel void noise_pairs(device const uint *seed [[buffer(0)]],
                        device const uint *key [[buffer(1)]],
                        device uint2 *out [[buffer(2)]],
                        constant uint &ndigests [[buffer(3)]],
                        uint gid [[thread_position_in_grid]]) {
  if (gid >= ndigests) return;
  uint d[8];
  noise_digest(gid, 1u, seed, key, d);
  for (uint j = 0; j < 8; j++) {
    uint kk = gid * 8u + j;
    if (kk >= FC_K) return;
    uint u = d[j];
    uint first = u & (FC_R - 1u);
    uint second = first ^ (1u + mulhi(FC_R - 1u, u));
    out[kk] = uint2(first, second);
  }
}

// Noised operand: out[row,l] = base[row,l] + table[row,first(l)] − table[row,second(l)].
// The sum provably fits int8 (committed [−64,64], noise [−63,63] → [−127,127]).
kernel void noise_apply(device const char *base [[buffer(0)]],
                        device const char *table [[buffer(1)]],
                        device const uint2 *pairs [[buffer(2)]],
                        device char *out [[buffer(3)]],
                        constant uint &rows [[buffer(4)]],
                        uint2 gid [[thread_position_in_grid]]) {
  if (gid.x >= FC_K || gid.y >= rows) return;
  uint2 p = pairs[gid.x];
  int noise = (int)table[gid.y * FC_R + p.x] - (int)table[gid.y * FC_R + p.y];
  out[gid.y * FC_K + gid.x] = (char)((int)base[gid.y * FC_K + gid.x] + noise);
}

// ── The PoW sweep (jackpot/helper.rs) ───────────────────────────────────────
// One threadgroup per hash tile; one thread per tile element (h·w ≤ 256).
// Per R-chunk: cooperative stage of the h and w operand slices, cumulative
// int32 accumulate, XOR-reduce the tile, rotate-left-13-and-XOR into the
// transcript slot (chunk mod 16). At the end, keyed BLAKE3 and an inclusive
// little-endian comparison against the bound.

struct PowParams {
  uint n_row_bases;
  uint n_col_bases;
  uint hits_cap;
  uint flags; // bit0: write digests_out; bit1: write debug captures
};

kernel void pow_sweep(device const char *an [[buffer(0)]],
                      device const char *bnt [[buffer(1)]],
                      device const uint *row_bases [[buffer(2)]],
                      device const uint *col_bases [[buffer(3)]],
                      device const uint *a_seed [[buffer(4)]],
                      device const uchar *bound [[buffer(5)]],
                      device atomic_uint *hits [[buffer(6)]],
                      constant PowParams &P [[buffer(7)]],
                      device uchar *digests_out [[buffer(8)]],
                      device int *csums_out [[buffer(9)]],
                      device uint *transcripts_out [[buffer(10)]],
                      threadgroup uchar *shmem [[threadgroup(0)]],
                      uint2 tgid [[threadgroup_position_in_grid]],
                      uint tid [[thread_index_in_threadgroup]],
                      uint lane [[thread_index_in_simdgroup]],
                      uint sgid [[simdgroup_index_in_threadgroup]],
                      uint nsg [[simdgroups_per_threadgroup]]) {
  const uint nthreads = FC_H * FC_W;
  const uint u = tid / FC_W;
  const uint v = tid % FC_W;
  const uint base_r = row_bases[tgid.y];
  const uint base_c = col_bases[tgid.x];
  const uint tile_idx = tgid.y * P.n_col_bases + tgid.x;
  const uint nchunks = FC_K / FC_R; // k mod R tail contributes nothing

  threadgroup uchar *As = shmem;                       // FC_H·FC_R bytes
  threadgroup uchar *Bs = shmem + FC_H * FC_R;         // FC_W·FC_R bytes
  threadgroup uint *scratch =                          // per-simdgroup XOR
      (threadgroup uint *)(shmem + FC_H * FC_R + FC_W * FC_R);
  threadgroup uint *tr = scratch + 8;                  // 16-word transcript

  if (tid < 16u) tr[tid] = 0u;

  int acc = 0; // this thread's CUMULATIVE int32 tile element

  for (uint t = 0; t < nchunks; t++) {
    const uint klo = t * FC_R;
    // Stage the h rows and w cols of this chunk.
    for (uint i = tid; i < FC_H * FC_R; i += nthreads) {
      uint rr = i / FC_R, ll = i % FC_R;
      As[i] = as_type<uchar>(an[(ulong)(base_r + row_pattern_offset(rr)) * FC_K + klo + ll]);
    }
    for (uint i = tid; i < FC_W * FC_R; i += nthreads) {
      uint cc = i / FC_R, ll = i % FC_R;
      Bs[i] = as_type<uchar>(bnt[(ulong)(base_c + col_pattern_offset(cc)) * FC_K + klo + ll]);
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);

    threadgroup const char *arow = (threadgroup const char *)(As + u * FC_R);
    threadgroup const char *brow = (threadgroup const char *)(Bs + v * FC_R);
    int s = 0;
    for (uint l = 0; l < FC_R; l += 4) {
      char4 a4 = *((threadgroup const char4 *)(arow + l));
      char4 b4 = *((threadgroup const char4 *)(brow + l));
      s += (int)a4.x * (int)b4.x + (int)a4.y * (int)b4.y + (int)a4.z * (int)b4.z +
           (int)a4.w * (int)b4.w;
    }
    acc += s;

    // XOR-reduce the cumulative tile (order-free: XOR commutes).
    uint xr = simd_xor(as_type<uint>(acc));
    if (lane == 0) scratch[sgid] = xr;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    if (tid == 0) {
      uint x = 0u;
      for (uint i = 0; i < nsg; i++) x ^= scratch[i];
      uint slot = t % 16u;
      uint prev = tr[slot];
      tr[slot] = ((prev << 13u) | (prev >> 19u)) ^ x;
    }
    if (P.flags & 2u) {
      csums_out[((ulong)tile_idx * nchunks + t) * nthreads + tid] = acc;
      threadgroup_barrier(mem_flags::mem_threadgroup);
      if (tid == 0)
        for (uint i = 0; i < 16u; i++)
          transcripts_out[((ulong)tile_idx * nchunks + t) * 16u + i] = tr[i];
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
  }

  if (tid != 0) return;

  uint msg[16], key[8], d[8];
  for (int i = 0; i < 16; i++) msg[i] = tr[i];
  for (int i = 0; i < 8; i++) key[i] = a_seed[i];
  blake3_64_keyed(msg, key, d);

  if (P.flags & 1u) {
    device uchar *o = digests_out + (ulong)tile_idx * 32u;
    for (int i = 0; i < 8; i++) {
      uint w = d[i];
      o[i * 4 + 0] = w & 0xFFu;
      o[i * 4 + 1] = (w >> 8) & 0xFFu;
      o[i * 4 + 2] = (w >> 16) & 0xFFu;
      o[i * 4 + 3] = (w >> 24) & 0xFFu;
    }
  }

  // Inclusive little-endian comparison: digest ≤ bound.
  bool win = true;
  for (int i = 31; i >= 0; i--) {
    uchar hb = (d[i / 4] >> (8 * (i % 4))) & 0xFFu;
    uchar bb = bound[i];
    if (hb != bb) {
      win = hb < bb;
      break;
    }
  }
  if (win) {
    uint idx = atomic_fetch_add_explicit(&hits[0], 1u, memory_order_relaxed);
    if (idx < P.hits_cap) {
      atomic_store_explicit(&hits[1 + 2 * idx], base_r, memory_order_relaxed);
      atomic_store_explicit(&hits[2 + 2 * idx], base_c, memory_order_relaxed);
    }
  }
}

// ── pow_sweep_v2: blocked fast path ─────────────────────────────────────────
// Requirements (host-enforced): rows_pattern == [0, S] (FC_ROW_S0 = S,
// FC_ROW_L0 = 2, upper levels trivial), cols_pattern == [0..63] (FC_COL_S0 = 1,
// FC_COL_L0 = 64). One threadgroup sweeps a 2S×64 output block = S hash tiles
// sharing one staged copy of the operands; each thread owns a 2×8 sub-tile of
// exactly one hash tile. Bit-exactness is unchanged — same cumulative int32,
// same fold, same digest; only the work decomposition differs.

kernel void pow_sweep_v2(device const char *an [[buffer(0)]],
                         device const char *bnt [[buffer(1)]],
                         device const uint *params2 [[buffer(2)]], // band_lo, n_col_bases
                         device const uint *a_seed [[buffer(4)]],
                         device const uchar *bound [[buffer(5)]],
                         device atomic_uint *hits [[buffer(6)]],
                         constant PowParams &P [[buffer(7)]],
                         device uchar *digests_out [[buffer(8)]],
                         device int *csums_out [[buffer(9)]],
                         device uint *transcripts_out [[buffer(10)]],
                         uint2 tgid [[threadgroup_position_in_grid]],
                         uint tid [[thread_index_in_threadgroup]]) {
  const uint S = FC_ROW_S0;          // 32: bases per band; tile rows {o, o+S}
  const uint W = FC_COL_L0;          // 64: contiguous cols per tile
  const uint band = params2[0] + tgid.y;
  const uint base_c = tgid.x * W;
  const uint row0 = band * 2 * S;    // first row of this band
  const uint nchunks = FC_K / FC_R;

  // Only Bᵀ is staged: each Bs row is read by all S tiles' threads, while an
  // A row is shared by just 8 threads — the device cache covers that, and the
  // smaller footprint (10 KB vs 18 KB) lets 3 threadgroups reside per core.
  threadgroup uchar Bs[64 * 128];         // W × R
  threadgroup uint tr[32 * 16];           // S transcripts

  const uint o = tid / 8;            // this thread's tile (row base offset)
  const uint j0 = (tid % 8) * 8;     // first of its 8 columns

  for (uint i = tid; i < S * 16u; i += S * 8u) tr[i] = 0u;

  // This thread's cumulative int32 tile elements. These can exceed 2²⁴ and
  // must stay integer; only the bounded per-chunk partials use fp32 (below).
  int acc[2][8];
  for (int a = 0; a < 2; a++)
    for (int b = 0; b < 8; b++) acc[a][b] = 0;

  const uint nthreads = S * 8u;      // 256
  for (uint t = 0; t < nchunks; t++) {
    const uint klo = t * FC_R;
    // Cooperative stage of Bᵀ as u32 words (K and R are multiples of 4).
    device const uint *bnt32 = (device const uint *)bnt;
    threadgroup uint *Bs32 = (threadgroup uint *)Bs;
    const uint bwords = W * FC_R / 4;
    for (uint i = tid; i < bwords; i += nthreads) {
      uint cc = i / (FC_R / 4), lw = i % (FC_R / 4);
      Bs32[i] = bnt32[(ulong)(base_c + cc) * (FC_K / 4) + klo / 4 + lw];
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);

    device const char4 *a0 =
        (device const char4 *)(an + (ulong)(row0 + o) * FC_K + klo);
    device const char4 *a1 =
        (device const char4 *)(an + (ulong)(row0 + o + S) * FC_K + klo);
    // Chunk partials in scalar fp32 FMA chains. Exact: every partial is an
    // integer ≤ R·127² = 2,064,512 < 2²⁴ and fast-math is off (IEEE). The
    // cumulative tile value (up to 66M > 2²⁴) lives only in int32 `acc`.
    float facc[2][8];
    for (int a = 0; a < 2; a++)
      for (int b = 0; b < 8; b++) facc[a][b] = 0.0f;
    for (uint l4 = 0; l4 < FC_R / 4; l4++) {
      float4 av0 = float4(a0[l4]);
      float4 av1 = float4(a1[l4]);
      for (uint b = 0; b < 8; b++) {
        float4 bv = float4(((threadgroup const char4 *)(Bs + (j0 + b) * FC_R))[l4]);
        facc[0][b] = fma(av0.x, bv.x, facc[0][b]);
        facc[0][b] = fma(av0.y, bv.y, facc[0][b]);
        facc[0][b] = fma(av0.z, bv.z, facc[0][b]);
        facc[0][b] = fma(av0.w, bv.w, facc[0][b]);
        facc[1][b] = fma(av1.x, bv.x, facc[1][b]);
        facc[1][b] = fma(av1.y, bv.y, facc[1][b]);
        facc[1][b] = fma(av1.z, bv.z, facc[1][b]);
        facc[1][b] = fma(av1.w, bv.w, facc[1][b]);
      }
    }
    for (int a = 0; a < 2; a++)
      for (int b = 0; b < 8; b++) acc[a][b] += (int)facc[a][b];

    if (P.flags & 2u) {
      const ulong tile = (ulong)(band * S + o) * P.n_col_bases + tgid.x;
      for (uint a = 0; a < 2; ++a)
        for (uint b = 0; b < 8; ++b)
          csums_out[(tile * nchunks + t) * 128u + a * 64u + j0 + b] = acc[a][b];
    }
    uint my = 0u;
    for (int a = 0; a < 2; a++)
      for (int b = 0; b < 8; b++) my ^= as_type<uint>(acc[a][b]);
    // 8 threads per tile, consecutive lanes: butterfly XOR within the team.
    my ^= simd_shuffle_xor(my, 4u);
    my ^= simd_shuffle_xor(my, 2u);
    my ^= simd_shuffle_xor(my, 1u);
    threadgroup_barrier(mem_flags::mem_threadgroup);
    if ((tid & 7u) == 0u) {
      uint slot = t % 16u;
      uint prev = tr[o * 16u + slot];
      tr[o * 16u + slot] = ((prev << 13u) | (prev >> 19u)) ^ my;
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    if ((P.flags & 2u) && tid < S) {
      const ulong tile = (ulong)(band * S + tid) * P.n_col_bases + tgid.x;
      for (uint i = 0; i < 16; ++i)
        transcripts_out[(tile * nchunks + t) * 16u + i] = tr[tid * 16u + i];
      // Debug readers finish before a later chunk can update shared transcripts.
    }
    if (P.flags & 2u) threadgroup_barrier(mem_flags::mem_threadgroup);
  }

  if (tid >= S) return;
  uint msg[16], key[8], d[8];
  for (int i = 0; i < 16; i++) msg[i] = tr[tid * 16 + i];
  for (int i = 0; i < 8; i++) key[i] = a_seed[i];
  blake3_64_keyed(msg, key, d);

  const uint base_r = row0 + tid;
  if (P.flags & 1u) {
    const uint tile_idx = (band * S + tid) * P.n_col_bases + tgid.x;
    device uchar *dst = digests_out + (ulong)tile_idx * 32u;
    for (int i = 0; i < 8; i++) {
      uint w = d[i];
      dst[i * 4 + 0] = w & 0xFFu;
      dst[i * 4 + 1] = (w >> 8) & 0xFFu;
      dst[i * 4 + 2] = (w >> 16) & 0xFFu;
      dst[i * 4 + 3] = (w >> 24) & 0xFFu;
    }
  }
  bool win = true;
  for (int i = 31; i >= 0; i--) {
    uchar hb = (d[i / 4] >> (8 * (i % 4))) & 0xFFu;
    uchar bb = bound[i];
    if (hb != bb) {
      win = hb < bb;
      break;
    }
  }
  if (win) {
    uint idx = atomic_fetch_add_explicit(&hits[0], 1u, memory_order_relaxed);
    if (idx < P.hits_cap) {
      atomic_store_explicit(&hits[1 + 2 * idx], base_r, memory_order_relaxed);
      atomic_store_explicit(&hits[2 + 2 * idx], base_c, memory_order_relaxed);
    }
  }
}

#if defined(PEARL_M6_EXPERIMENTAL_MATRICES)
// Experimental exact simdgroup_matrix replacement for pow_sweep_v2.
// Splice after the common BLAKE3/PowParams definitions in pearl_kernels.metal.
// Host dispatch remains (256 threads, n_col_bases x n_bands).
//
// Numeric contract: signed int8 operands in [-127,127], R <= 128. Half input
// conversion is exact; products and every FP32 chunk partial are integers with
// magnitude <= R*127^2 <= 2,064,512 < 2^24. Only the per-chunk matrix accumulator
// uses FP32. Each chunk is converted exactly into persistent int32 registers.
// Cross-chunk accumulation never uses FP32. No fast-math permission is needed.
//
// Matrix element distribution is unspecified by the MSL API. We discover row
// and column identities with small load operations instead of assuming an
// Apple lane-to-matrix layout. Eight SGs own eight output rows each.
#include <metal_simdgroup_matrix>

kernel void pow_sweep_m6_simd(device const char *an [[buffer(0)]],
                         device const char *bnt [[buffer(1)]],
                         device const uint *params2 [[buffer(2)]],
                         device const uint *a_seed [[buffer(4)]],
                         device const uchar *bound [[buffer(5)]],
                         device atomic_uint *hits [[buffer(6)]],
                         constant PowParams &P [[buffer(7)]],
                         device uchar *digests_out [[buffer(8)]],
                         device int *csums_out [[buffer(9)]],
                         device uint *transcripts_out [[buffer(10)]],
                         uint2 tgid [[threadgroup_position_in_grid]],
                         uint tid [[thread_index_in_threadgroup]],
                         uint lane [[thread_index_in_simdgroup]],
                         uint sg [[simdgroup_index_in_threadgroup]]) {
  const uint S = 32u;
  const uint band = params2[0] + tgid.y;
  const uint base_c = tgid.x * 64u;
  const uint row0 = band * 64u;
  const uint nchunks = FC_K / FC_R;
  threadgroup half Bs[64 * 128];
  threadgroup half element_ids[64];
  threadgroup float output_rows[64];
  threadgroup uint row_fold[64];
  threadgroup uint tr[32 * 16];

  if (tid < 64u) {
    element_ids[tid] = half(tid);
    output_rows[tid] = float(tid);
  }
  for (uint i = tid; i < 512u; i += 256u) tr[i] = 0u;
  threadgroup_barrier(mem_flags::mem_threadgroup);
  simdgroup_half8x8 ids;
  simdgroup_float8x8 row_ids;
  simdgroup_load(ids, element_ids, 8u, ulong2(0), false);
  simdgroup_load(row_ids, output_rows, 8u, ulong2(0), false);
  const uint ai0 = uint(ids.thread_elements()[0]);
  const uint ai1 = uint(ids.thread_elements()[1]);
  const uint oi0 = uint(row_ids.thread_elements()[0]);
  const uint oi1 = uint(row_ids.thread_elements()[1]);
  const uint out_row0 = oi0 / 8u;
  const uint out_row1 = oi1 / 8u;

  int accum[8][2];
#pragma unroll
  for (uint j = 0; j < 8u; ++j) {
    accum[j][0] = 0;
    accum[j][1] = 0;
  }

  for (uint t = 0; t < nchunks; ++t) {
    const uint klo = t * FC_R;
    // B has the broad reuse: 8 SGs share each exact char -> half conversion.
    for (uint i = tid; i < 64u * FC_R; i += 256u) {
      const uint c = i / FC_R;
      const uint k = i % FC_R;
      Bs[i] = half(bnt[(ulong)(base_c + c) * FC_K + klo + k]);
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    simdgroup_float8x8 chunk[8];
#pragma unroll
    for (uint j = 0; j < 8u; ++j)
      chunk[j] = make_filled_simdgroup_matrix<float, 8, 8>(0.0f);

    for (uint k = 0; k < FC_R; k += 8u) {
      simdgroup_half8x8 am;
      am.thread_elements()[0] = half(
          an[(ulong)(row0 + sg * 8u + ai0 / 8u) * FC_K + klo + k + ai0 % 8u]);
      am.thread_elements()[1] = half(
          an[(ulong)(row0 + sg * 8u + ai1 / 8u) * FC_K + klo + k + ai1 % 8u]);
#pragma unroll
      for (uint j = 0; j < 8u; ++j) {
        simdgroup_half8x8 bm;
        // Storage is B^T[col,k], so this load transposes an 8x8 submatrix.
        simdgroup_load(bm, Bs + j * 8u * FC_R + k, FC_R, ulong2(0), true);
        simdgroup_multiply_accumulate(chunk[j], am, bm, chunk[j]);
      }
    }
    uint xor0 = 0u, xor1 = 0u;
#pragma unroll
    for (uint j = 0; j < 8u; ++j) {
      accum[j][0] += int(chunk[j].thread_elements()[0]);
      accum[j][1] += int(chunk[j].thread_elements()[1]);
      xor0 ^= as_type<uint>(accum[j][0]);
      xor1 ^= as_type<uint>(accum[j][1]);
      if (P.flags & 2u) {
        const uint gr0 = sg * 8u + out_row0;
        const uint gr1 = sg * 8u + out_row1;
        const ulong tile0 = (band * S + gr0 % S) * P.n_col_bases + tgid.x;
        const ulong tile1 = (band * S + gr1 % S) * P.n_col_bases + tgid.x;
        csums_out[((tile0 * nchunks + t) * 2u + gr0 / S) * 64u + j * 8u + oi0 % 8u] = accum[j][0];
        csums_out[((tile1 * nchunks + t) * 2u + gr1 / S) * 64u + j * 8u + oi1 % 8u] = accum[j][1];
      }
    }
    // Row-wise fold independent of the matrix fragment distribution. All lanes
    // execute every reduction; each row is written by precisely one lane.
#pragma unroll
    for (uint r = 0; r < 8u; ++r) {
      uint x = (out_row0 == r ? xor0 : 0u) ^ (out_row1 == r ? xor1 : 0u);
      x = simd_xor(x);
      if (lane == r) row_fold[sg * 8u + r] = x;
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    if (tid < S) {
      const uint slot = t & 15u;
      const uint prev = tr[tid * 16u + slot];
      tr[tid * 16u + slot] = ((prev << 13u) | (prev >> 19u)) ^
                            row_fold[tid] ^ row_fold[tid + S];
      if (P.flags & 2u) {
        const ulong tile = (band * S + tid) * P.n_col_bases + tgid.x;
        for (uint j = 0; j < 16u; ++j)
          transcripts_out[(tile * nchunks + t) * 16u + j] = tr[tid * 16u + j];
      }
    }
    // Protect row_fold from the next iteration and make the final transcript
    // visible to the hashing lanes. Also makes shared B safe to overwrite.
    threadgroup_barrier(mem_flags::mem_threadgroup);
  }

  if (tid >= S) return;
  uint msg[16], key[8], d[8];
  for (uint i = 0; i < 16u; ++i) msg[i] = tr[tid * 16u + i];
  for (uint i = 0; i < 8u; ++i) key[i] = a_seed[i];
  blake3_64_keyed(msg, key, d);
  const uint base_r = row0 + tid;
  if (P.flags & 1u) {
    const uint tile_idx = (band * S + tid) * P.n_col_bases + tgid.x;
    device uchar *dst = digests_out + (ulong)tile_idx * 32u;
    for (uint i = 0; i < 8u; ++i) {
      const uint w = d[i];
      dst[i * 4u + 0u] = w & 255u;
      dst[i * 4u + 1u] = (w >> 8u) & 255u;
      dst[i * 4u + 2u] = (w >> 16u) & 255u;
      dst[i * 4u + 3u] = (w >> 24u) & 255u;
    }
  }
  bool win = true;
  for (int i = 31; i >= 0; --i) {
    const uchar hb = (d[i / 4] >> (8 * (i % 4))) & 255u;
    if (hb != bound[i]) {
      win = hb < bound[i];
      break;
    }
  }
  if (win) {
    uint idx = atomic_fetch_add_explicit(&hits[0], 1u, memory_order_relaxed);
    if (idx < P.hits_cap) {
      atomic_store_explicit(&hits[1 + 2 * idx], base_r, memory_order_relaxed);
      atomic_store_explicit(&hits[2 + 2 * idx], base_c, memory_order_relaxed);
    }
  }
}

// Metal 4.1 int8 TensorOps candidate m6: linear two-row aggregation with a generic fallback.
// Uses documented tensor coordinates, supports any cooperative layout.
// Only 32 atomic row folds plus transcripts are stored in threadgroup memory.
// Integrate after the common PowParams/BLAKE3 helpers in pearl_kernels.metal.
// Host eligibility: normal fast-path shape AND r == 128.
// Use the same configurable SIMDgroup count in the host dispatch and TensorOps.
// Requires MSL 4.1 with MTLMathModeSafe. No relaxed precision, no floating point.
#include <metal_tensor>
#include <MetalPerformancePrimitives/MetalPerformancePrimitives.h>
using namespace mpp;
using namespace mpp::tensor_ops;

kernel void pow_sweep_m6_tensor(device int8_t *an [[buffer(0)]],
                                device int8_t *bnt [[buffer(1)]],
                                device const uint *params2 [[buffer(2)]],
                                device const uint *a_seed [[buffer(4)]],
                                device const uchar *bound [[buffer(5)]],
                                device atomic_uint *hits [[buffer(6)]],
                                constant PowParams &P [[buffer(7)]],
                                device uchar *digests_out [[buffer(8)]],
                                device int *csums_out [[buffer(9)]],
                                device uint *transcripts_out [[buffer(10)]],
                                uint2 tgid [[threadgroup_position_in_grid]],
                                uint tid [[thread_index_in_threadgroup]]) {
  const uint band = params2[0] + tgid.y;
  const uint row0 = band * 64u;
  const uint base_c = tgid.x * 64u;
  const uint nchunks = FC_K / 128u;
  constexpr auto desc = matmul2d_descriptor(64,64,128,false,true,false,
                                           matmul2d_descriptor::mode::multiply_accumulate);
  matmul2d<desc, execution_simdgroups<PEARL_M6_TENSOR_SIMDGROUPS>> op;
  using Operand = tensor<device int8_t, extents<int,128,64>, tensor_inline>;
  auto c = op.get_destination_cooperative_tensor<Operand,Operand,int>();
  #pragma unroll
  for(uint16_t i=0;i<c.get_capacity();i++) if(c.is_valid_element(i)) c[i]=0;
  threadgroup atomic_uint row_folds[32];
  threadgroup uint tr[32*16];
  for(uint i=tid;i<32*16;i+=32u*PEARL_M6_TENSOR_SIMDGROUPS) tr[i]=0;
  for(uint t=0;t<nchunks;t++) {
    if(tid<32u) atomic_store_explicit(&row_folds[tid],0u,memory_order_relaxed);
    threadgroup_barrier(mem_flags::mem_threadgroup);
    Operand a(an+(ulong)row0*FC_K+t*128u,extents<int,128,64>(),array<int,2>{1,(int)FC_K});
    Operand b(bnt+(ulong)base_c*FC_K+t*128u,extents<int,128,64>(),array<int,2>{1,(int)FC_K});
    op.run(a,b,c);
    // One linear scan handles the common two-unique-row layout. Row IDs
    // are discovered through the public coordinate API, not lane formulas.
    // No atomic is issued until we know the fast aggregation is complete.
    uint row0_id=~0u, row1_id=~0u, fold0=0u, fold1=0u;
    bool overflow=false;
    #pragma unroll
    for(uint16_t i=0;i<c.get_capacity();i++) if(c.is_valid_element(i)) {
      const uint row=(uint)c.get_multidimensional_index(i)[1]&31u;
      const uint value=as_type<uint>(c[i]);
      if(row0_id==~0u || row==row0_id) {
        row0_id=row; fold0^=value;
      } else if(row1_id==~0u || row==row1_id) {
        row1_id=row; fold1^=value;
      } else {
        overflow=true;
      }
    }
    if(!overflow) {
      if(row0_id!=~0u) atomic_fetch_xor_explicit(&row_folds[row0_id],fold0,memory_order_relaxed);
      if(row1_id!=~0u) atomic_fetch_xor_explicit(&row_folds[row1_id],fold1,memory_order_relaxed);
    } else {
      // A different compiler/device layout may own more than two rows.
      // Recompute every local contribution exactly once with the generic
      // unique-row algorithm; partial fast-path sums have not been emitted.
      // Aggregate each distinct row once; used only for other layouts.
      uint seen=0u;
    #pragma unroll
    for(uint16_t i=0;i<c.get_capacity();i++) if(c.is_valid_element(i)) {
      const uint row=(uint)c.get_multidimensional_index(i)[1]&31u;
      const uint bit=1u<<row;
      if((seen&bit)==0u) {
        seen|=bit;
        uint fold=0u;
        #pragma unroll
        for(uint16_t j=0;j<c.get_capacity();j++) if(c.is_valid_element(j)) {
          if(((uint)c.get_multidimensional_index(j)[1]&31u)==row) fold^=as_type<uint>(c[j]);
        }
        atomic_fetch_xor_explicit(&row_folds[row],fold,memory_order_relaxed);
      }
    }
    }
    if(P.flags&2u) {
      #pragma unroll
      for(uint16_t i=0;i<c.get_capacity();i++) if(c.is_valid_element(i)) {
        auto xy=c.get_multidimensional_index(i);
        const uint tile=(band*32u+((uint)xy[1]&31u))*P.n_col_bases+tgid.x;
        csums_out[((ulong)tile*nchunks+t)*128u+(uint)xy[1]/32u*64u+(uint)xy[0]]=c[i];
      }
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    // One owner per tile reads the completed shared XOR.
    if(tid<32u) {
      const uint fold=atomic_load_explicit(&row_folds[tid],memory_order_relaxed);
      const uint slot=t&15u;
      // Adjacent owners access adjacent words for each transcript slot.
      const uint prev=tr[slot*32u+tid];
      tr[slot*32u+tid]=((prev<<13u)|(prev>>19u))^fold;
      if(P.flags&2u) {
        const uint tile=(band*32u+tid)*P.n_col_bases+tgid.x;
        for(uint i=0;i<16u;i++) transcripts_out[((ulong)tile*nchunks+t)*16u+i]=tr[i*32u+tid];
      }
    }
    // The same owner resets its row on the next iteration; that iteration's
    // leading barrier keeps resets ahead of all following atomic writers.
  }
  if(tid>=32u) return;
  uint msg[16],key[8],d[8];
  for(uint i=0;i<16u;i++) msg[i]=tr[i*32u+tid];
  for(uint i=0;i<8u;i++) key[i]=a_seed[i];
  blake3_64_keyed(msg,key,d);
  const uint base_r=row0+tid;
  if(P.flags&1u) {
    const uint tile=(band*32u+tid)*P.n_col_bases+tgid.x;
    device uchar *dst=digests_out+(ulong)tile*32u;
    for(uint i=0;i<8u;i++) for(uint j=0;j<4u;j++) dst[i*4u+j]=(d[i]>>(8u*j))&255u;
  }
  bool win=true;
  for(int i=31;i>=0;i--) {
    uchar hb=(d[i/4]>>(8*(i%4)))&255u;
    uchar bb=bound[i];
    if(hb!=bb){win=hb<bb;break;}
  }
  if(win) {
    const uint idx=atomic_fetch_add_explicit(&hits[0],1u,memory_order_relaxed);
    if(idx<P.hits_cap) {
      atomic_store_explicit(&hits[1u+2u*idx],base_r,memory_order_relaxed);
      atomic_store_explicit(&hits[2u+2u*idx],base_c,memory_order_relaxed);
    }
  }
}

#endif
