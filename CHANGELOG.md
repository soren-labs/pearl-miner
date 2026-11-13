# Changelog

## 0.1.2 — Portable Metal runtime detection

- Keep M6 tensor-kernel detection in binaries built with older SDKs. Metal 4.1 is gated by runtime macOS/GPU support rather than preprocessed out by the build SDK; ABI values are checked against newer SDK headers. Older Macs retain the scalar fallback.
- Honor the sleep-prevention toggle: Metal activity tokens suppress App Nap without unconditionally blocking idle system sleep.
- Remove and disable GitHub-hosted build/test/release workflows. All verification and packaging run locally via `scripts/ci_local.sh`; build/runtime guards refuse GitHub Actions environments, and GitHub only receives prebuilt artifacts.
- Validate custom ports only when a custom pool is selected.
- v0.1.1 remains functionally correct but its CI-built binary falls back to the slower scalar kernel on M6. Use 0.1.2 for the M6 tensor path.

## 0.1.1 — Reliability preview

- Correct live network hashrate parsing (`net_hashrate`), fetch current block rewards and observed intervals, and remove unscaled difficulty conversions and silent historical market assumptions. Missing/stale data hides earnings; partial or failed balance responses stay unknown.
- Apply intensity, electricity and power-policy changes to running GPU sessions. Lock connection and wallet changes while mining.
- Back up previous wallets before replacement, serialize concurrent writers, use private atomic files, confirm replacement in the native UI, export wallet backups, and describe plaintext storage accurately.
- Handle authorization rejection, error-only share replies, invalid jobs and oversized pool messages. Shut down old socket readers before reconnecting, bound socket writes, and make connection cancellation responsive.
- Recover the UI after an engine exits; report launch/config/wallet errors, clear stopped/paused metrics and stale wallet balances, and prevent overlapping GPU diagnostics.
- Improve bilingual onboarding, validation and metric layout. Remove kernel internals and duplicate wallet controls from the dashboard.
- Add regression tests and a real-GPU local-pool smoke test. Add optional Developer ID/hardened-runtime signing and notarization; unsigned builds remain previews.

Validation: 14 Rust tests, 39/39 GPU checks, a real-GPU local-pool regression, native UI checks and one real LuckyPool CPU-port accepted share (zero rejects). See [the validation record](docs/validation-0.1.1.md). Public-pool credit, on-chain payout, long-duration/multiple-device stability and credential-dependent notarization remain unverified.
