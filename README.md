# PearlMiner

[![Build](https://github.com/soren-labs/pearl-miner/actions/workflows/ci.yml/badge.svg)](https://github.com/soren-labs/pearl-miner/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/soren-labs/pearl-miner)](https://github.com/soren-labs/pearl-miner/releases/latest)
![Platform](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-black)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

**Native macOS Metal GPU miner for [Pearl (PRL)](https://github.com/pearl-research-labs/pearl), powered by a Rust core and a lightweight SwiftUI interface.**

English | [简体中文](README.zh-CN.md)

---

## Download & Quick Start

1. Download **`PearlMiner-macOS-arm64.dmg`** (or `.zip`) from [**Latest Releases**](https://github.com/soren-labs/pearl-miner/releases/latest).
2. Drag **`PearlMiner.app`** to `/Applications`.
3. **First-launch note (ad-hoc signed open-source build):** if macOS Gatekeeper warns that the developer cannot be verified, either **Right-Click → Open** `PearlMiner.app`, or run:
   ```bash
   xattr -cr /Applications/PearlMiner.app
   ```
4. Follow the two-step onboarding:
   - **Wallet:** Click **Create New Wallet** (generates a local `prl1p…` Taproot keypair saved with `0600` permissions — remember to back up your private key), import a 64-hex private key / `wallet.json`, or paste an existing `prl1p…` address.
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
- `wallet.json` (`0600` file mode on Unix)
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

# Mine via CLI (e.g. LuckyPool)
./target/release/pearl-miner-core mine --pool luckypool
```

---

## License & Attributions

Licensed under the **Apache License, Version 2.0** ([LICENSE](LICENSE)). See [NOTICE](NOTICE) for third-party licenses (`pearl-blake3` & `zk-pow` under ISC, `plonky2` under MIT/Apache-2.0, and Metal kernels derived from `pearl-metal-miner`).

> *Mining cryptocurrency consumes substantial GPU power and generates heat. Monitor temperatures when running extended sessions on fanless or compact Macs.*
