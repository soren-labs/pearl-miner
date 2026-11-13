# 0.1.1 validation — 2026-11-13

Hardware: Apple M6, native arm64 macOS. This is one machine, not an M1–M6 compatibility or long-duration thermal certification.

## Automated checks

- Rust core and storage/protocol regression tests: 14 passed. Includes corrupted-config handling, symlink refusal, failed-backup preservation, concurrent wallet writes, persistent backup acknowledgement, malformed pool jobs, authorization errors and old-reader shutdown across reconnects.
- Built-in GPU self-test: 39/39 bit-exact Metal, V1/V2/V3 plain-proof checks passed.
- `scripts/smoke_test.py`: the actual Rust daemon and GPU connect to a local simulated pool; verify live 100% → 25% intensity changes, unavailable market-data handling, wallet export permissions, backup acknowledgement, wallet mutation rejection while mining, stop/quit, automatic backup and invalid-config rollback. No public mining or user wallets are involved in this test.
- Release SwiftUI build and app/DMG/ZIP packaging completed; `codesign --verify --deep --strict` passed for the ad-hoc bundle.

## Real pool proof submission

A separate generated validation wallet was used with `pearl-cpu-eu1.luckypool.io:3370` (LuckyPool low-difficulty CPU test endpoint), bounded to 90 seconds or one accepted share. The engine exited normally after receiving one accepted share, zero rejects. Measured short-window throughput settled around 12.49–12.96 equivalent TH/s. Relevant engine log:

```text
[status] connecting: pearl-cpu-eu1.luckypool.io:3370
[stats] 12.96 TH/s | acc=0 rej=0 | sys=50.6W gpu=25.9W | est=0.139 PRL/d ($0.20/d)
[stats] 12.49 TH/s | acc=0 rej=0 | sys=59.7W gpu=19.9W | est=0.134 PRL/d ($0.19/d)
[info] Share ACCEPTED (job 3709e986_26000) — total 1
[status] stopped: Ready
```

Earnings in that log are estimates at the observed market conditions, not credited balances or payout evidence. This test does not establish share acceptance on the default high-difficulty port, Kryptex credit, or on-chain payout. The test wallet is separate from the user's wallet and its key is retained locally, excluded from Git and distribution archives.

## Native UI checks

Onboarding/address connection, English and Chinese dashboards, wallet replacement confirmation/cancel, wallet panel layout, settings and engine failure/retry were inspected with computer use. The injected engine failure correctly disables mining and exposes Retry; reconnect restores the ready state. Wallet backups are unencrypted private files, accurately described in both languages.

## Distribution limitation

No Developer ID identity or Apple notarization credentials are installed/configured in this environment. Current artifacts are ad-hoc signed previews. The optional signing/notarization path is implemented but cannot be validated without those credentials. Public-pool balances/payouts and multi-device/extended thermal stability remain unverified.
