# 0.1.2 validation — portable M6 selection

The final public-download check for 0.1.1 found a build parity defect: the GitHub binary selected `pow_sweep_v2` on the same Apple M6 where a local build selected `pow_sweep_m6_tensor`. Both passed 39 checks, so mathematical correctness alone did not catch the performance regression. The Metal host had compiled out runtime detection when its build SDK was older than macOS 27.

0.1.2 uses Metal's stable enum ABI values behind runtime macOS 27/GPU-family checks. New SDK builds assert those values against the SDK declarations. No new host API symbols are linked. Older OS/GPU combinations keep the existing scalar fallback.

Local regression checks:
- Build using `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk`.
- On Apple M6, `self-test --expect-kernel pow_sweep_m6_tensor`: 39/39 passed.
- With `PEARL_METAL_DISABLE_M6=1`, scalar self-test: 39/39 passed. The expected-tensor check correctly exits with a selection-regression error.
- Sequential short offline diagnostics (3 seconds warmup + 3 seconds measured, one run each): tensor build 11.985 equivalent TH/s; the 0.1.1 public scalar binary 1.109 equivalent TH/s. These are short diagnostic measurements, not thermal-stability results or a multi-device benchmark.
- Metal activity no longer imposes an unconditional idle-sleep assertion. During a standalone benchmark, `pmset -g assertions` reported no PreventUserIdleSystemSleep/PreventSystemSleep assertion owned by the test process; session sleep prevention remains controlled by the Rust policy.

The release must also be downloaded after publishing and checked with `self-test --expect-kernel pow_sweep_m6_tensor` on M6. This catches SDK-dependent kernel removal that a source-only test cannot detect. The 0.1.1 storage/protocol and real-GPU regression suite remains applicable; notarization, payout and long-duration/multiple-device limitations are unchanged.


Execution policy: the former GitHub build/test/release workflows are disabled and removed. All 0.1.2 verification and packaging run on the local Mac via `scripts/ci_local.sh`; only prebuilt assets are uploaded to GitHub. No hosted runner executes, tests, benchmarks or builds the miner. The independent BRC miner was temporarily paused for local validation and is restored afterward.


The local CI also starts a child CLI with a synthetic `GITHUB_ACTIONS=true` environment and confirms it exits before dispatching any core command. This guard test is itself performed locally; no GitHub runner is used. The Rust build script refuses the same environment before compiling the core's Metal host.
