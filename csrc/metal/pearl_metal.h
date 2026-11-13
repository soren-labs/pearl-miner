/* pearl_metal — C API over the Metal backend, consumed via ctypes.
 *
 * Lifecycle: pm_create → pm_compile(shape) → pm_alloc/dispatch… → pm_destroy.
 * All dispatches are synchronous and return 0 on success; on failure they
 * return non-zero and write a message into the caller's error buffer.
 *
 * Shaders are compiled from embedded source at pm_compile time with the job
 * shape bound as Metal function constants (ADR-0004): nothing a pool might
 * dictate is hardcoded (ADR-0006).
 */
#ifndef PEARL_METAL_H
#define PEARL_METAL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct pm_ctx pm_ctx;

/* Job shape, bound as function constants at compile time.
 * rows/cols hold the PeriodicPattern shape as (stride,length) ×3. */
typedef struct pm_shape {
  uint32_t k;       /* common dimension */
  uint32_t r;       /* noise rank */
  uint32_t h;       /* |rows_pattern| */
  uint32_t w;       /* |cols_pattern| */
  uint32_t rows[6]; /* s0,l0,s1,l1,s2,l2 */
  uint32_t cols[6];
} pm_shape;

pm_ctx *pm_create(char *err, size_t errlen);
void pm_destroy(pm_ctx *ctx);

/* Device facts, queried not assumed. Returns 0 on success. */
int pm_device_info(pm_ctx *ctx, char *name, size_t namelen,
                   uint64_t *max_threadgroup_mem, uint64_t *max_threads_per_tg);

/* Selected fast kernel; valid until context destruction. */
const char *pm_sweep_name(pm_ctx *ctx);

/* Optional per-kernel GPU/host timings, enabled by PEARL_METAL_PROFILE=1.
 * Arrays have capacity entries (up to 16); returns the kernel count. */
int pm_profile(pm_ctx *ctx, uint64_t *counts, double *gpu_seconds,
               double *wall_seconds, uint32_t capacity, uint32_t reset);

/* Compile every kernel with the shape's function constants. Idempotent per
 * shape; recompiles when the shape changes. */
int pm_compile(pm_ctx *ctx, const pm_shape *shape, char *err, size_t errlen);

/* Shared-storage buffers (unified memory). */
void *pm_alloc(pm_ctx *ctx, size_t bytes);
void *pm_contents(void *buf);
void pm_release(void *buf);

/* Keyed BLAKE3 of `count` independent 64-byte messages (test primitive).
 * msgs: count×64 B, keys: count×32 B, out: count×32 B. */
int pm_blake3_64(pm_ctx *ctx, void *msgs, void *keys, void *out, uint32_t count,
                 char *err, size_t errlen);

/* Noise generation. seed/key are 32 bytes each.
 * uniform: out is `rows`×R int8 (EAL or EBR table).
 * pairs:   out is k × (u32 first, u32 second). */
int pm_noise_uniform(pm_ctx *ctx, const uint8_t *seed, const uint8_t *key,
                     void *out, uint32_t rows, char *err, size_t errlen);
int pm_noise_pairs(pm_ctx *ctx, const uint8_t *seed, const uint8_t *key,
                   void *out, char *err, size_t errlen);

/* Noised operand: out[row,l] = base[row,l] + table[row,first(l)] − table[row,second(l)].
 * base/out: rows×k int8, table: rows×R int8. */
int pm_noise_apply(pm_ctx *ctx, void *base, void *table, void *pairs, void *out,
                   uint32_t rows, char *err, size_t errlen);

/* The PoW sweep. One threadgroup per hash tile.
 * an:  m×k int8 noised A (row-major), bnt: n×k int8 noised Bᵀ.
 * row_bases/col_bases: u32 arrays of valid tile base offsets.
 * a_seed: 32 B pow key.  bound: 32 B little-endian inclusive bound.
 * hits: u32 count then (u32 base_r, u32 base_c) pairs, capacity entries.
 * digests_out: optional (may be NULL) n_tiles×32 B, tile-major
 *   (row_base index × n_col_bases + col_base index) — for differential tests.
 */
int pm_pow_sweep(pm_ctx *ctx, void *an, void *bnt, void *row_bases,
                 uint32_t n_row_bases, void *col_bases, uint32_t n_col_bases,
                 const uint8_t *a_seed, const uint8_t *bound, void *hits,
                 uint32_t hits_capacity, void *digests_out, char *err,
                 size_t errlen);

/* Blocked fast-path sweep: rows [0,32], cols [0..63], r ≤ 128, 64 | m, 64 | n.
 * Sweeps bands [band_lo, band_lo+n_bands) × all n_col_bases col tiles. */
int pm_pow_sweep2(pm_ctx *ctx, void *an, void *bnt, uint32_t band_lo,
                  uint32_t n_bands, uint32_t n_col_bases, const uint8_t *a_seed,
                  const uint8_t *bound, void *hits, uint32_t hits_capacity,
                  void *digests_out, char *err, size_t errlen);

/* Fast-path debug captures use absolute tile-major, then chunk-major order.
 * Allocate for (band_lo+n_bands)*32*n_col_bases tiles, including prefix. */
int pm_pow_sweep2_debug(pm_ctx *ctx, void *an, void *bnt, uint32_t band_lo,
                       uint32_t n_bands, uint32_t n_col_bases, const uint8_t *a_seed,
                       const uint8_t *bound, void *hits, uint32_t hits_capacity,
                       void *digests_out, void *csums_out, void *transcripts_out,
                       char *err, size_t errlen);

/* Debug variant for small shapes: also writes, per tile, every R-boundary's
 * cumulative tile (h·w i32 × n_chunks) and transcript (16 u32 × n_chunks).
 * Layouts are tile-major then chunk-major. */
int pm_pow_sweep_debug(pm_ctx *ctx, void *an, void *bnt, void *row_bases,
                       uint32_t n_row_bases, void *col_bases,
                       uint32_t n_col_bases, const uint8_t *a_seed,
                       const uint8_t *bound, void *hits, uint32_t hits_capacity,
                       void *digests_out, void *csums_out, void *transcripts_out,
                       char *err, size_t errlen);

#ifdef __cplusplus
}
#endif
#endif
