# PearlMiner（珍珠币 Mac GPU 挖矿工具）

[![Build](https://github.com/soren-labs/pearl-miner/actions/workflows/ci.yml/badge.svg)](https://github.com/soren-labs/pearl-miner/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/soren-labs/pearl-miner)](https://github.com/soren-labs/pearl-miner/releases/latest)
![Platform](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-black)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

**面向 [Pearl (PRL)](https://github.com/pearl-research-labs/pearl) 的 macOS 原生 Metal GPU 挖矿软件：Rust 核心引擎 + 极简 SwiftUI 原生界面。**

[English](README.md) | 简体中文

---

## 一键下载与快速上手

1. 前往 [**Releases 最新版本**](https://github.com/soren-labs/pearl-miner/releases/latest) 下载 **`PearlMiner-macOS-arm64.dmg`**（或 `.zip`）。
2. 打开 `.dmg`，将 **`PearlMiner.app`** 拖入 `Applications（应用程序）`。
3. **首次打开说明（开源自签名版本）：** 由于未购买苹果开发者签名证书，首次运行如遇 Gatekeeper 拦截，可**右键点击 → 打开**，或在终端执行：
   ```bash
   xattr -cr /Applications/PearlMiner.app
   ```
4. 首次打开后按两步引导即可开始：
   - **连接钱包：** 点击**一键创建新钱包**（在本地以 `0600` 权限生成 `prl1p…` Taproot 地址与密钥对，请妥善备份私钥），或导入 64 位私钥 / `wallet.json` / 仅输入收款地址。
   - **选择矿池并启动：** 默认已选中 **LuckyPool (EU)**，也可切换 **Kryptex (EU / US)** 或自定义矿池，点击**一键启动挖矿**即可。

---

## 核心功能

- **极简原生 macOS 界面** — 去掉多余信息，只保留核心状态与数据：一键启动/停止、实时算力 (`TH/s`)、预计日/月收益（含扣除电费净收益）、系统总功耗与 GPU 功耗、矿池待确认/已确认余额、有效份额及 GPU 强度调节（`25%–100%`）。
- **中英文实时切换** — 默认英文界面，顶部支持 **EN / 中文** 一键切换。
- **Rust + Metal GPU 核心 (`pearl-miner-core`)** — 自动匹配 Apple Silicon 最优 Metal 计算内核（M6 使用 `pow_sweep_m6_tensor`，M1–M5 使用 `pow_sweep_v2`），集成上游 `pearl-blake3` 与 `zk-pow` V1/V2/V3 证明计算，并通过 AppleSMC 与 IOReport 免 root 读取实时功耗。
- **笔记本电源策略** — 支持使用电池时自动暂停 / 低功耗 (`25%`) / 全速运行，并可在挖矿期间阻止系统自动休眠。
- **内置校验与测速** — 在设置中或 CLI 可随时运行 39 项逐位一致性 GPU 数学自检 (`self-test`) 与 10 秒离线测速 (`benchmark`)。

---

## 支持矿池

| 矿池 | 默认节点 | 协议 |
| :--- | :--- | :--- |
| **LuckyPool (EU)** *（默认）* | `pearl-eu1.luckypool.io:3360` | LuckyPool Stratum |
| **Kryptex (EU)** | `prl-eu.kryptex.network:7048` | Kryptex Stratum |
| **Kryptex (US)** | `prl-us.kryptex.network:7048` | Kryptex Stratum |
| **Custom（自定义）** | 自定义 `host:port` | LuckyPool 或 Kryptex |

> PearlMiner **不抽取任何开发者抽水（0% dev fee）**，矿池手续费以所选矿池规则为准。

---

## 从源码构建

环境要求：Apple Silicon Mac（macOS 14+）、Rust 工具链（`cargo`）、Swift / Command Line Tools。

```bash
git clone https://github.com/soren-labs/pearl-miner.git
cd pearl-miner

# 运行 Rust 单元测试
cargo test -p pearl-miner-core

# 一键打包 PearlMiner.app、.dmg 与 .zip（输出到 dist/）
./scripts/build_app.sh
```

---

## 开源协议与致谢

本项目基于 **Apache License 2.0**（[LICENSE](LICENSE)）开源，第三方组件版权归属详见 [NOTICE](NOTICE)（包含 `pearl-blake3` / `zk-pow` ISC、`plonky2` MIT/Apache-2.0 及 `pearl-metal-miner`）。

> *GPU 长时间满载挖矿会产生较高功耗与发热，使用无风扇或轻薄机型时建议适当调低 GPU 强度并注意散热。*
