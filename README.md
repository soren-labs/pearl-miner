# PearlMiner

**Build and validation run only on the local Mac.**
[![Release](https://img.shields.io/github/v/release/soren-labs/pearl-miner?include_prereleases)](https://github.com/soren-labs/pearl-miner/releases)
![Platform](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-black)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

**Early preview.** See [release notes](CHANGELOG.md) for validation and remaining limitations.

**Native macOS Metal GPU miner for [Pearl (PRL)](https://github.com/pearl-research-labs/pearl), powered by a Rust core and a lightweight SwiftUI interface.**

English | [简体中文](README.zh-CN.md)

---

## Download & Quick Start

1. Download **`PearlMiner-macOS-arm64.dmg`** (or `.zip`) from [**Latest Releases**](https://github.com/soren-labs/pearl-miner/releases).
2. Drag **`PearlMiner.app`** to `/Applications`.
3. **Preview builds are ad-hoc signed and not notarized.** macOS may block first launch. Developer ID signing and notarization require the maintainer's Apple credentials (see below).
4. Follow the two-step onboarding:
   - **Wallet:** Click **Create New Wallet** (generates a local `prl1p…` Taproot keypair saved with `0600` permissions — export a wallet backup before mining), import a 64-hex private key / `wallet.json`, or paste an existing `prl1p…` address.
   - **Pool:** Pick **LuckyPool (EU)** (default), **Kryptex (EU / US)**, or a custom Stratum host, and click **Start Mining**.

---

## Features

- **Minimal Native Mac UI** — SwiftUI dashboard focused only on what matters: one-click Start/Stop, real-time hashrate (`TH/s`), estimated earnings (`PRL` and `USD` gross / net after electricity), live system & GPU wattage, pool balance, accepted shares, and GPU intensity (`25%–100%`).
- **Bilingual (EN / 简体中文)** — English by default with a one-click language switch in the header.
- **Rust + Metal GPU Engine (`pearl-miner-core`)** — Uses Apple Silicon Metal compute shaders (`pow_sweep_m6_tensor` on M6 and `pow_sweep_v2` on M1–M5), vendored upstream `pearl-blake3` & `zk-pow` V1/V2/V3 verification/proofs, and zero-root AppleSMC + IOReport power telemetry.
- **Laptop-Friendly Controls** — Configurable intensity slider, optional idle auto-ramp, configurable battery policy (Pause / 25% Low / Full Speed), and sleep prevention while mining.
- **Built-In Diagnostics & CLI** — 39-check bit-exact Metal vs Rust reference self-test (`self-test`), offline speed benchmark (`benchmark`), and full CLI mode for headless runs.

---

## Supported Pools

| Pool | Default Endpoint | Protocol |
| :--- | :--- | :--- |
| **LuckyPool (EU)** *(default)* | `pearl-eu1.luckypool.io:3360` | LuckyPool Stratum |
| **Kryptex (EU)** | `prl-eu.kryptex.network:7048` | Kryptex Stratum |
| **Kryptex (US)** | `prl-us.kryptex.network:7048` | Kryptex Stratum |
| **Custom** | User-defined `host:port` | LuckyPool or Kryptex dialect |

> PearlMiner collects **0% developer fee**. Pool fees are determined solely by your chosen mining pool.

---

## Architecture

```text
pearl-miner/
├── core/                 # Rust miner engine & Stratum/Telemetry/Economics CLI & daemon (`pearl-miner-core`)
├── csrc/metal/           # Objective-C++ host bridge + Metal compute shaders (`pearl_kernels.metal`)
├── macos/                # Native SwiftUI macOS application (`PearlMiner`)
├── vendor/pearl/         # Vendored upstream `pearl-blake3`, `zk-pow`, and `plonky2`
└── scripts/              # Icon generation and `.app` / `.dmg` / `.zip` release bundler
```

State and wallets are stored under `~/Library/Application Support/PearlMiner/` (override with `PEARL_MINER_HOME`):
- `wallet.json` (`0600` file mode on Unix; **unencrypted**)
- `wallet-backups/` (previous wallets saved before replacement; private local files)
- `config.json`

---

## Build From Source

Requirements: macOS on Apple Silicon (macOS 14+), Rust (`cargo`), and Swift / Apple Command Line Tools (`swift`).

```bash
git clone https://github.com/soren-labs/pearl-miner.git
cd pearl-miner

# Run Rust tests
cargo test -p pearl-miner-core

# Build PearlMiner.app, .dmg, and .zip into dist/
./scripts/build_app.sh
```

### Headless CLI Usage

```bash
# Inspect detected Apple GPU and Metal kernel
./target/release/pearl-miner-core hardware

# Run the 39-check bit-exact Metal vs Rust reference self-test
./target/release/pearl-miner-core self-test

# Create or import a wallet
./target/release/pearl-miner-core wallet new

# After exporting/backing up a newly created wallet
./target/release/pearl-miner-core wallet acknowledge-backup

# Mine via CLI (e.g. LuckyPool)
./target/release/pearl-miner-core mine --pool luckypool
```

---

## License & Attributions

Licensed under the **Apache License, Version 2.0** ([LICENSE](LICENSE)). See [NOTICE](NOTICE) for third-party licenses (`pearl-blake3` & `zk-pow` under ISC, `plonky2` under MIT/Apache-2.0, and Metal kernels derived from `pearl-metal-miner`).

> *Mining cryptocurrency consumes substantial GPU power and generates heat. Monitor temperatures when running extended sessions on fanless or compact Macs.*

## Reliability, wallets, and earnings

Mining only needs a payout address; keep your spending wallet in your preferred wallet application. A newly created local wallet is a payout keypair, not a transaction/signing UI. Export its JSON backup from onboarding or the Wallet panel and store a private copy outside this Mac. The file is not encrypted. Replacing a wallet requires confirmation and automatically preserves the old file under `wallet-backups/`; an automatic local backup does not protect against disk loss. Import an old JSON backup to restore it. Spending requires a compatible Taproot `tr()` descriptor wallet.

Live earnings use the pool API's `net_hashrate`, current block reward, and observed block interval. Price comes from Gate's PEARL/USDT ticker. Estimates are hidden if required values are missing or older than three minutes, or if live sync is disabled. No assumed price, network hashrate or historical fixed reward is silently substituted. Pool fees are deducted when the selected pool API supplies them; otherwise the estimate is before pool fees. Estimated earnings are not credited balances or promised payouts. Custom Stratum servers do not inherit a public pool's balance dashboard. Power readings can include chip estimates and are not wall-socket measurements.

Intensity, electricity rate, idle/battery policy and sleep prevention apply during a running session. Stop mining before changing the pool, payout wallet, worker or CPU thread count.

Run the real GPU/daemon regression against a local simulated pool (no public mining or user wallets):

```bash
cargo build --release --locked -p pearl-miner-core
python3 scripts/smoke_test.py
```

## Developer ID signing and notarization

Local signing uses `PEARL_CODESIGN_IDENTITY` (a Developer ID Application identity in the keychain) and `PEARL_NOTARY_PROFILE` (credentials previously stored by `xcrun notarytool store-credentials`). With both configured, `./scripts/build_app.sh 0.1.3` signs both executables with hardened runtime, notarizes and staples the app and DMG, then creates final checksums. Notarization failures stop packaging.

GitHub-hosted build/test/release workflows are removed and disabled. Keep signing/notarization credentials in your local keychain. Run `./scripts/ci_local.sh` locally, then upload the resulting DMG, ZIP and `SHA256SUMS.txt` with `gh release create` or the GitHub release UI. Publishing uploads prebuilt files; it does not execute the miner on GitHub. Unsigned artifacts must be marked **prerelease**. See [Apple's notarization documentation](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).


### Verify the selected M6 kernel

On an M6 running macOS 27+, verify **the downloaded release binary**, not just a local source build:

```bash
PearlMiner.app/Contents/MacOS/pearl-miner-core self-test --expect-kernel pow_sweep_m6_tensor
```

This check fails if the math passes on the slower scalar fallback. Older Macs should run `self-test` without an expected-kernel constraint. Metal 4.1 availability is detected at runtime even when the app is built using older SDK headers.


### Local CI

```bash
./scripts/ci_local.sh
```

This runs formatting, Rust tests, strict Clippy, native packaging, GPU self-checks (including expected M6 tensor selection), the simulated localhost-pool regression, signature/integrity checks and release checksums. No public mining is run by default. All computation stays on the local Mac. Do not add GitHub-hosted miner workflows. See [AGENTS.md](AGENTS.md) for the repository execution policy.
