// Objective-C++ host for the Pearl Metal backend.
//
// Device selection: MTLCopyAllDevices()[0] — MTLCreateSystemDefaultDevice()
// returns nil for plain command-line binaries on this platform (ADR-0004,
// measured). Shaders compile from embedded source at pm_compile time with the
// job shape as function constants; there is no .metallib step.

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

#include <cstring>
#include <string>
#include <chrono>
#include <cstdlib>

#include "pearl_metal.h"
#include "kernels_embedded.h"

namespace {

void set_err(char *err, size_t errlen, NSString *msg) {
  if (err && errlen > 0) {
    snprintf(err, errlen, "%s", msg ? msg.UTF8String : "unknown error");
  }
}

constexpr uint32_t kNumKernels = 8;
const char *const kKernelNames[kNumKernels] = {
    "blake3_64", "noise_uniform", "noise_pairs", "noise_apply", "pow_sweep",
    "pow_sweep_v2", "pow_sweep_m6_simd", "pow_sweep_m6_tensor",
};

// These are enum ABI values, not new linked symbols. Keep runtime detection
// available when a distributable is built against an older SDK (as on CI).
// The OS and GPU checks below still gate every use of Metal 4.1.
constexpr MTLGPUFamily kApple11Family = static_cast<MTLGPUFamily>(1011);
constexpr MTLLanguageVersion kMetalLanguage4_1 =
    static_cast<MTLLanguageVersion>((4 << 16) + 1);
#if defined(__MAC_OS_X_VERSION_MAX_ALLOWED) && __MAC_OS_X_VERSION_MAX_ALLOWED >= 270000
static_assert(kApple11Family == MTLGPUFamilyApple11, "Metal GPU family ABI changed");
static_assert(kMetalLanguage4_1 == MTLLanguageVersion4_1, "Metal language ABI changed");
#endif

struct PowParams {
  uint32_t n_row_bases;
  uint32_t n_col_bases;
  uint32_t hits_cap;
  uint32_t flags;
};

struct Bytes32 {
  uint8_t b[32];
};

} // namespace

struct pm_ctx {
  id<MTLDevice> device;
  id<MTLCommandQueue> queue;
  id<MTLComputePipelineState> pipelines[kNumKernels];
  id<MTLBuffer> dummy; // bound to unused optional buffer slots
  id<NSObject> activity; // App Nap / timer-throttle suppression token
  pm_shape shape;
  bool compiled = false;
  bool matrices_available = false;
  uint32_t fast_kernel = 5;
  uint32_t tensor_simdgroups = 2;
  bool profile_enabled = false;
  uint64_t profile_count[16] = {};
  double profile_gpu[16] = {};
  double profile_wall[16] = {};
};

extern "C" {

pm_ctx *pm_create(char *err, size_t errlen) {
  @autoreleasepool {
    NSArray<id<MTLDevice>> *devices = MTLCopyAllDevices();
    if (devices.count == 0) {
      set_err(err, errlen, @"no Metal devices (MTLCopyAllDevices returned none)");
      return nullptr;
    }
    pm_ctx *ctx = new pm_ctx();
    ctx->device = devices[0];
    const char *profile = std::getenv("PEARL_METAL_PROFILE");
    ctx->profile_enabled = profile && std::strcmp(profile, "0") != 0;
    // Without this, macOS App Nap suspends an unattended background miner
    // after a minute — measured: full speed for ~30 s, then near-total stall.
    // Suppress App Nap and timer throttling while allowing idle system sleep.
    // The Rust session's keep_awake policy owns the sleep assertion.
    ctx->activity = [[NSProcessInfo processInfo]
        beginActivityWithOptions:(NSActivityUserInitiatedAllowingIdleSystemSleep |
                                  NSActivityLatencyCritical)
                          reason:@"pearl-metal-miner mining"];
    ctx->queue = [ctx->device newCommandQueue];
    ctx->dummy = [ctx->device newBufferWithLength:16
                                          options:MTLResourceStorageModeShared];
    if (!ctx->queue || !ctx->dummy) {
      set_err(err, errlen, @"failed to create command queue");
      pm_destroy(ctx);
      return nullptr;
    }
    return ctx;
  }
}

void pm_destroy(pm_ctx *ctx) {
  if (!ctx) return;
  // Symmetry with beginActivity above: the token outlives the context
  // otherwise, and a caller that creates and destroys a context without
  // exiting would keep App Nap suppressed for the rest of the process.
  if (ctx->activity) {
    [[NSProcessInfo processInfo] endActivity:ctx->activity];
    ctx->activity = nil;
  }
  delete ctx;
}

int pm_device_info(pm_ctx *ctx, char *name, size_t namelen,
                   uint64_t *max_threadgroup_mem, uint64_t *max_threads_per_tg) {
  @autoreleasepool {
    if (name && namelen) snprintf(name, namelen, "%s", ctx->device.name.UTF8String);
    if (max_threadgroup_mem) *max_threadgroup_mem = ctx->device.maxThreadgroupMemoryLength;
    if (max_threads_per_tg)
      *max_threads_per_tg = ctx->device.maxThreadsPerThreadgroup.width;
    return 0;
  }
}

const char *pm_sweep_name(pm_ctx *ctx) {
  return kKernelNames[ctx->fast_kernel];
}

int pm_profile(pm_ctx *ctx, uint64_t *counts, double *gpu_seconds,
               double *wall_seconds, uint32_t capacity, uint32_t reset) {
  uint32_t n = MIN(capacity, 16u);
  for (uint32_t i = 0; i < n; ++i) {
    if (counts) counts[i] = ctx->profile_count[i];
    if (gpu_seconds) gpu_seconds[i] = ctx->profile_gpu[i];
    if (wall_seconds) wall_seconds[i] = ctx->profile_wall[i];
  }
  if (reset) {
    std::memset(ctx->profile_count, 0, sizeof(ctx->profile_count));
    std::memset(ctx->profile_gpu, 0, sizeof(ctx->profile_gpu));
    std::memset(ctx->profile_wall, 0, sizeof(ctx->profile_wall));
  }
  return kNumKernels;
}

int pm_compile(pm_ctx *ctx, const pm_shape *shape, char *err, size_t errlen) {
  @autoreleasepool {
    if (ctx->compiled && memcmp(&ctx->shape, shape, sizeof(pm_shape)) == 0) return 0;

    NSString *src = [[NSString alloc] initWithBytes:PEARL_KERNELS_MSL
                                             length:PEARL_KERNELS_MSL_LEN
                                           encoding:NSUTF8StringEncoding];
    MTLCompileOptions *opts = [MTLCompileOptions new];
    const char *tensor_groups = std::getenv("PEARL_METAL_TENSOR_SIMDGROUPS");
    ctx->tensor_simdgroups = 2;
    if (tensor_groups) {
      if (std::strcmp(tensor_groups, "1") == 0) ctx->tensor_simdgroups = 1;
      else if (std::strcmp(tensor_groups, "2") == 0) ctx->tensor_simdgroups = 2;
      else if (std::strcmp(tensor_groups, "4") == 0) ctx->tensor_simdgroups = 4;
      else if (std::strcmp(tensor_groups, "8") == 0) ctx->tensor_simdgroups = 8;
      else {
        set_err(err, errlen, @"PEARL_METAL_TENSOR_SIMDGROUPS must be 1, 2, 4 or 8");
        return 1;
      }
    }
    const char *kernel_request = std::getenv("PEARL_METAL_KERNEL");
    const bool allow_matrix_fallback = !kernel_request ||
        std::strcmp(kernel_request, "auto") == 0 || std::strcmp(kernel_request, "scalar") == 0;
    const char *disable_m6 = std::getenv("PEARL_METAL_DISABLE_M6");
    const bool matrices_disabled = disable_m6 && std::strcmp(disable_m6, "0") != 0;
    if (@available(macOS 27.0, *)) {
      ctx->matrices_available = !matrices_disabled &&
          [ctx->device supportsFamily:kApple11Family];
      if (ctx->matrices_available) {
        opts.languageVersion = kMetalLanguage4_1;
        opts.preprocessorMacros = @{@"PEARL_M6_EXPERIMENTAL_MATRICES": @1,
            @"PEARL_M6_TENSOR_SIMDGROUPS": @(ctx->tensor_simdgroups)};
      }
    }
    // IEEE fp32 semantics are load-bearing: the fast-path GEMM accumulates
    // chunk partials (integers < 2^24) in fp32 FMA, exactly, only because
    // fast-math is off. The self-test enforces the resulting bit-exactness.
    // macOS 15 renamed this knob; MTLMathModeSafe IS `fastMathEnabled = NO`,
    // so both spellings ask for exactly the same semantics — the split is
    // only to keep the build warning-free on both SDKs.
#if defined(__MAC_OS_X_VERSION_MAX_ALLOWED) && __MAC_OS_X_VERSION_MAX_ALLOWED >= 150000
    if (@available(macOS 15.0, *)) {
      opts.mathMode = MTLMathModeSafe;
    } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
      opts.fastMathEnabled = NO;
#pragma clang diagnostic pop
    }
#else
    opts.fastMathEnabled = NO;
#endif
    NSError *nserr = nil;
    id<MTLLibrary> lib = [ctx->device newLibraryWithSource:src options:opts error:&nserr];
    // A new OS/compiler can reject an optional matrix source. Auto mode must
    // retain the original scalar kernels; an explicitly forced mode fails.
    if (!lib && ctx->matrices_available && allow_matrix_fallback) {
      ctx->matrices_available = false;
      opts.preprocessorMacros = @{};
      opts.languageVersion = MTLLanguageVersion3_1;
      lib = [ctx->device newLibraryWithSource:src options:opts error:&nserr];
    }
    if (!lib) {
      set_err(err, errlen, [NSString stringWithFormat:@"MSL compile failed: %@",
                                                      nserr.localizedDescription]);
      return 1;
    }

    MTLFunctionConstantValues *cv = [MTLFunctionConstantValues new];
    uint32_t vals[16] = {shape->k,       shape->r,       shape->h,       shape->w,
                         shape->rows[0], shape->rows[1], shape->rows[2], shape->rows[3],
                         shape->rows[4], shape->rows[5], shape->cols[0], shape->cols[1],
                         shape->cols[2], shape->cols[3], shape->cols[4], shape->cols[5]};
    for (uint32_t i = 0; i < 16; i++) {
      [cv setConstantValue:&vals[i] type:MTLDataTypeUInt atIndex:i];
    }

    // Compile every kernel up front so shader errors surface here, once,
    // rather than at first dispatch (ADR-0004).
    for (uint32_t i = 0; i < kNumKernels; i++) {
      if (i >= 6 && !ctx->matrices_available) continue;
      id<MTLFunction> fn =
          [lib newFunctionWithName:[NSString stringWithUTF8String:kKernelNames[i]]
                    constantValues:cv
                             error:&nserr];
      if (!fn) {
        if (i >= 6 && allow_matrix_fallback) {
          ctx->matrices_available = false;
          break;
        }
        set_err(err, errlen,
                [NSString stringWithFormat:@"function %s: %@", kKernelNames[i],
                                           nserr.localizedDescription]);
        return 1;
      }
      ctx->pipelines[i] = [ctx->device newComputePipelineStateWithFunction:fn error:&nserr];
      if (!ctx->pipelines[i]) {
        if (i >= 6 && allow_matrix_fallback) {
          ctx->matrices_available = false;
          break;
        }
        set_err(err, errlen,
                [NSString stringWithFormat:@"pipeline %s: %@", kKernelNames[i],
                                           nserr.localizedDescription]);
        return 1;
      }
      // The kernels' simd_xor/simd_shuffle_xor folds and per-simdgroup
      // scratch sizing assume 32-lane simdgroups — true of every Apple GPU
      // shipped to date (M1…M5). If a future GPU differs, refuse loudly here
      // rather than fail the self-test mysteriously.
      NSUInteger width = ctx->pipelines[i].threadExecutionWidth;
      if (width != 32) {
        set_err(err, errlen,
                [NSString stringWithFormat:
                              @"kernel %s: simdgroup width %lu (expected 32) — "
                              @"untested GPU generation; refusing to mine. "
                              @"Please open an issue with your chip and macOS "
                              @"version.",
                              kKernelNames[i], (unsigned long)width]);
        return 1;
      }
    }
    ctx->fast_kernel = 5;
    const char *requested = std::getenv("PEARL_METAL_KERNEL");
    const bool use_tensor = !requested || std::strcmp(requested, "auto") == 0 ||
                             std::strcmp(requested, "tensor") == 0;
    const bool use_simd = requested && std::strcmp(requested, "simd") == 0;
    const bool use_scalar = requested && std::strcmp(requested, "scalar") == 0;
    if (!use_tensor && !use_simd && !use_scalar) {
      set_err(err, errlen, @"PEARL_METAL_KERNEL must be auto, scalar, simd or tensor");
      return 1;
    }
    if ((use_simd || (requested && std::strcmp(requested, "tensor") == 0)) &&
        !ctx->matrices_available) {
      set_err(err, errlen, @"requested experimental matrix kernel is unavailable");
      return 1;
    }
    if (ctx->matrices_available && shape->r == 128 && use_tensor) ctx->fast_kernel = 7;
    if (ctx->matrices_available && shape->r <= 128 && shape->r % 8 == 0 && use_simd)
      ctx->fast_kernel = 6;
    ctx->shape = *shape;
    ctx->compiled = true;
    return 0;
  }
}

void *pm_alloc(pm_ctx *ctx, size_t bytes) {
  @autoreleasepool {
    id<MTLBuffer> buf = [ctx->device newBufferWithLength:bytes
                                                 options:MTLResourceStorageModeShared];
    return (__bridge_retained void *)buf;
  }
}

void *pm_contents(void *buf) { return [(__bridge id<MTLBuffer>)buf contents]; }

void pm_release(void *buf) {
  id<MTLBuffer> b = (__bridge_transfer id<MTLBuffer>)buf;
  (void)b;
}

namespace {

// Synchronously run one compute pass. `configure` sets buffers/bytes.
int run(pm_ctx *ctx, uint32_t kernel_idx, MTLSize grid, MTLSize tg,
        NSUInteger tgmem, bool uniform_grid,
        void (^configure)(id<MTLComputeCommandEncoder>), char *err, size_t errlen) {
  @autoreleasepool {
    const auto wall_start = std::chrono::steady_clock::now();
    if (!ctx->compiled) {
      set_err(err, errlen, @"pm_compile has not run");
      return 1;
    }
    id<MTLComputePipelineState> pipeline = ctx->pipelines[kernel_idx];
    if (tg.width * tg.height * tg.depth > pipeline.maxTotalThreadsPerThreadgroup ||
        tgmem + pipeline.staticThreadgroupMemoryLength > ctx->device.maxThreadgroupMemoryLength) {
      set_err(err, errlen, @"dispatch exceeds selected pipeline thread or threadgroup memory limits");
      return 1;
    }
    id<MTLCommandBuffer> cb = [ctx->queue commandBuffer];
    id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
    [enc setComputePipelineState:ctx->pipelines[kernel_idx]];
    if (tgmem) [enc setThreadgroupMemoryLength:tgmem atIndex:0];
    configure(enc);
    if (uniform_grid) {
      [enc dispatchThreadgroups:grid threadsPerThreadgroup:tg];
    } else {
      [enc dispatchThreads:grid threadsPerThreadgroup:tg];
    }
    [enc endEncoding];
    [cb commit];
    [cb waitUntilCompleted];
    if (ctx->profile_enabled) {
      ctx->profile_count[kernel_idx] += 1;
      ctx->profile_gpu[kernel_idx] += MAX(0.0, cb.GPUEndTime - cb.GPUStartTime);
      ctx->profile_wall[kernel_idx] +=
          std::chrono::duration<double>(std::chrono::steady_clock::now() - wall_start).count();
    }
    if (cb.status == MTLCommandBufferStatusError) {
      set_err(err, errlen,
              [NSString stringWithFormat:@"GPU error: %@", cb.error.localizedDescription]);
      return 1;
    }
    return 0;
  }
}

inline id<MTLBuffer> mb(void *p) { return (__bridge id<MTLBuffer>)p; }

} // namespace

int pm_blake3_64(pm_ctx *ctx, void *msgs, void *keys, void *out, uint32_t count,
                 char *err, size_t errlen) {
  return run(
      ctx, 0, MTLSizeMake(count, 1, 1), MTLSizeMake(MIN(count, 256u), 1, 1), 0, false,
      ^(id<MTLComputeCommandEncoder> enc) {
        [enc setBuffer:mb(msgs) offset:0 atIndex:0];
        [enc setBuffer:mb(keys) offset:0 atIndex:1];
        [enc setBuffer:mb(out) offset:0 atIndex:2];
        [enc setBytes:&count length:4 atIndex:3];
      },
      err, errlen);
}

int pm_noise_uniform(pm_ctx *ctx, const uint8_t *seed, const uint8_t *key, void *out,
                     uint32_t rows, char *err, size_t errlen) {
  uint32_t nblocks = rows * ctx->shape.r / 32;
  Bytes32 seed32, key32;
  memcpy(seed32.b, seed, 32);
  memcpy(key32.b, key, 32);
  return run(
      ctx, 1, MTLSizeMake(nblocks, 1, 1), MTLSizeMake(MIN(nblocks, 256u), 1, 1), 0, false,
      ^(id<MTLComputeCommandEncoder> enc) {
        [enc setBytes:seed32.b length:32 atIndex:0];
        [enc setBytes:key32.b length:32 atIndex:1];
        [enc setBuffer:mb(out) offset:0 atIndex:2];
        [enc setBytes:&nblocks length:4 atIndex:3];
      },
      err, errlen);
}

int pm_noise_pairs(pm_ctx *ctx, const uint8_t *seed, const uint8_t *key, void *out,
                   char *err, size_t errlen) {
  uint32_t ndigests = (ctx->shape.k + 7) / 8;
  Bytes32 seed32, key32;
  memcpy(seed32.b, seed, 32);
  memcpy(key32.b, key, 32);
  return run(
      ctx, 2, MTLSizeMake(ndigests, 1, 1), MTLSizeMake(MIN(ndigests, 256u), 1, 1), 0,
      false,
      ^(id<MTLComputeCommandEncoder> enc) {
        [enc setBytes:seed32.b length:32 atIndex:0];
        [enc setBytes:key32.b length:32 atIndex:1];
        [enc setBuffer:mb(out) offset:0 atIndex:2];
        [enc setBytes:&ndigests length:4 atIndex:3];
      },
      err, errlen);
}

int pm_noise_apply(pm_ctx *ctx, void *base, void *table, void *pairs, void *out,
                   uint32_t rows, char *err, size_t errlen) {
  return run(
      ctx, 3, MTLSizeMake(ctx->shape.k, rows, 1), MTLSizeMake(64, 4, 1), 0, false,
      ^(id<MTLComputeCommandEncoder> enc) {
        [enc setBuffer:mb(base) offset:0 atIndex:0];
        [enc setBuffer:mb(table) offset:0 atIndex:1];
        [enc setBuffer:mb(pairs) offset:0 atIndex:2];
        [enc setBuffer:mb(out) offset:0 atIndex:3];
        [enc setBytes:&rows length:4 atIndex:4];
      },
      err, errlen);
}

static int pow_sweep_impl(pm_ctx *ctx, void *an, void *bnt, void *row_bases,
                          uint32_t n_row_bases, void *col_bases, uint32_t n_col_bases,
                          const uint8_t *a_seed, const uint8_t *bound, void *hits,
                          uint32_t hits_capacity, void *digests_out, void *csums_out,
                          void *transcripts_out, char *err, size_t errlen) {
  const pm_shape &s = ctx->shape;
  uint32_t nthreads = s.h * s.w;
  NSUInteger tgmem = (NSUInteger)(s.h + s.w) * s.r + (8 + 16) * 4;
  uint64_t dev_max = ctx->device.maxThreadgroupMemoryLength;
  if (tgmem > dev_max) {
    set_err(err, errlen,
            [NSString stringWithFormat:@"tiling needs %lu B threadgroup memory, device has %llu"
                                       @" — refusing (query-don't-assume)",
                                       (unsigned long)tgmem, dev_max]);
    return 1;
  }
  if (nthreads > ctx->device.maxThreadsPerThreadgroup.width) {
    set_err(err, errlen, @"h*w exceeds device max threads per threadgroup");
    return 1;
  }
  PowParams P = {n_row_bases, n_col_bases, hits_capacity,
                 (digests_out ? 1u : 0u) | (csums_out ? 2u : 0u)};
  Bytes32 seed32, bound32;
  memcpy(seed32.b, a_seed, 32);
  memcpy(bound32.b, bound, 32);
  return run(
      ctx, 4, MTLSizeMake(n_col_bases, n_row_bases, 1), MTLSizeMake(nthreads, 1, 1),
      tgmem, true,
      ^(id<MTLComputeCommandEncoder> enc) {
        [enc setBuffer:mb(an) offset:0 atIndex:0];
        [enc setBuffer:mb(bnt) offset:0 atIndex:1];
        [enc setBuffer:mb(row_bases) offset:0 atIndex:2];
        [enc setBuffer:mb(col_bases) offset:0 atIndex:3];
        [enc setBytes:seed32.b length:32 atIndex:4];
        [enc setBytes:bound32.b length:32 atIndex:5];
        [enc setBuffer:mb(hits) offset:0 atIndex:6];
        [enc setBytes:&P length:sizeof(P) atIndex:7];
        [enc setBuffer:(digests_out ? mb(digests_out) : ctx->dummy) offset:0 atIndex:8];
        [enc setBuffer:(csums_out ? mb(csums_out) : ctx->dummy) offset:0 atIndex:9];
        [enc setBuffer:(transcripts_out ? mb(transcripts_out) : ctx->dummy)
                offset:0
               atIndex:10];
      },
      err, errlen);
}

int pm_pow_sweep(pm_ctx *ctx, void *an, void *bnt, void *row_bases, uint32_t n_row_bases,
                 void *col_bases, uint32_t n_col_bases, const uint8_t *a_seed,
                 const uint8_t *bound, void *hits, uint32_t hits_capacity,
                 void *digests_out, char *err, size_t errlen) {
  return pow_sweep_impl(ctx, an, bnt, row_bases, n_row_bases, col_bases, n_col_bases,
                        a_seed, bound, hits, hits_capacity, digests_out, nullptr, nullptr,
                        err, errlen);
}

/* Fast-path blocked sweep. Host must ensure rows_pattern == [0,32] (two-level),
 * cols_pattern == [0..63], r <= 128, m % 64 == 0, n % 64 == 0. */
static int pow_sweep2_impl(pm_ctx *ctx, void *an, void *bnt, uint32_t band_lo,
                  uint32_t n_bands, uint32_t n_col_bases, const uint8_t *a_seed,
                  const uint8_t *bound, void *hits, uint32_t hits_capacity,
                  void *digests_out, void *csums_out, void *transcripts_out,
                  char *err, size_t errlen) {
  const pm_shape &s = ctx->shape;
  if (!(s.rows[0] == 32 && s.rows[1] == 2 && s.rows[3] == 1 && s.rows[5] == 1 &&
        s.cols[0] == 1 && s.cols[1] == 64 && s.cols[3] == 1 && s.cols[5] == 1 &&
        s.r <= 128)) {
    set_err(err, errlen, @"shape not eligible for pow_sweep_v2 fast path");
    return 1;
  }
  PowParams P = {n_bands, n_col_bases, hits_capacity,
                 (digests_out ? 1u : 0u) | (csums_out ? 2u : 0u)};
  Bytes32 seed32, bound32;
  memcpy(seed32.b, a_seed, 32);
  memcpy(bound32.b, bound, 32);
  // Blocks cannot capture C arrays; capture scalars and rebuild inside.
  const uint32_t p2a = band_lo, p2b = n_col_bases;
  return run(
      ctx, ctx->fast_kernel, MTLSizeMake(n_col_bases, n_bands, 1),
      MTLSizeMake(ctx->fast_kernel == 7 ? ctx->tensor_simdgroups * 32u : 256, 1, 1), 0, true,
      ^(id<MTLComputeCommandEncoder> enc) {
        uint32_t pp[2] = {p2a, p2b};
        [enc setBuffer:mb(an) offset:0 atIndex:0];
        [enc setBuffer:mb(bnt) offset:0 atIndex:1];
        [enc setBytes:pp length:8 atIndex:2];
        [enc setBytes:seed32.b length:32 atIndex:4];
        [enc setBytes:bound32.b length:32 atIndex:5];
        [enc setBuffer:mb(hits) offset:0 atIndex:6];
        [enc setBytes:&P length:sizeof(P) atIndex:7];
        [enc setBuffer:(digests_out ? mb(digests_out) : ctx->dummy) offset:0 atIndex:8];
        [enc setBuffer:(csums_out ? mb(csums_out) : ctx->dummy) offset:0 atIndex:9];
        [enc setBuffer:(transcripts_out ? mb(transcripts_out) : ctx->dummy) offset:0 atIndex:10];
      },
      err, errlen);
}

int pm_pow_sweep2(pm_ctx *ctx, void *an, void *bnt, uint32_t band_lo,
                 uint32_t n_bands, uint32_t n_col_bases, const uint8_t *a_seed,
                 const uint8_t *bound, void *hits, uint32_t hits_capacity,
                 void *digests_out, char *err, size_t errlen) {
  return pow_sweep2_impl(ctx, an, bnt, band_lo, n_bands, n_col_bases, a_seed,
                         bound, hits, hits_capacity, digests_out, nullptr, nullptr,
                         err, errlen);
}

int pm_pow_sweep2_debug(pm_ctx *ctx, void *an, void *bnt, uint32_t band_lo,
                       uint32_t n_bands, uint32_t n_col_bases, const uint8_t *a_seed,
                       const uint8_t *bound, void *hits, uint32_t hits_capacity,
                       void *digests_out, void *csums_out, void *transcripts_out,
                       char *err, size_t errlen) {
  return pow_sweep2_impl(ctx, an, bnt, band_lo, n_bands, n_col_bases, a_seed,
                         bound, hits, hits_capacity, digests_out, csums_out,
                         transcripts_out, err, errlen);
}

int pm_pow_sweep_debug(pm_ctx *ctx, void *an, void *bnt, void *row_bases,
                       uint32_t n_row_bases, void *col_bases, uint32_t n_col_bases,
                       const uint8_t *a_seed, const uint8_t *bound, void *hits,
                       uint32_t hits_capacity, void *digests_out, void *csums_out,
                       void *transcripts_out, char *err, size_t errlen) {
  return pow_sweep_impl(ctx, an, bnt, row_bases, n_row_bases, col_bases, n_col_bases,
                        a_seed, bound, hits, hits_capacity, digests_out, csums_out,
                        transcripts_out, err, errlen);
}

} // extern "C"
