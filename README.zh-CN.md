# PearlMiner

**早期预览版。** 验证结果及已知限制见 [更新记录](CHANGELOG.md)。（珍珠币 Mac GPU 挖矿工具）

**编译与验证仅在本机 Mac 执行。**
[![Release](https://img.shields.io/github/v/release/soren-labs/pearl-miner?include_prereleases)](https://github.com/soren-labs/pearl-miner/releases)
![Platform](https://img.shields.io/badge/platform-macOS%20Apple%20Silicon-black)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

**面向 [Pearl (PRL)](https://github.com/pearl-research-labs/pearl) 的 macOS 原生 Metal GPU 挖矿软件：Rust 核心引擎 + 极简 SwiftUI 原生界面。**

[English](README.md) | 简体中文

---

## 一键下载与快速上手

1. 前往 [**Releases 最新版本**](https://github.com/soren-labs/pearl-miner/releases) 下载 **`PearlMiner-macOS-arm64.dmg`**（或 `.zip`）。
2. 打开 `.dmg`，将 **`PearlMiner.app`** 拖入 `Applications（应用程序）`。
3. **预览版使用 ad-hoc 签名，尚未公证。** macOS 可能阻止首次打开；正式 Developer ID 签名和公证需要维护者的 Apple 凭据（见下方说明）。
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

## 钱包保护与估算边界

挖矿只需收款地址。内置钱包仅保存收款密钥对，没有转账界面。创建后请导出 JSON 备份，并在本机之外另存保密副本；钱包文件未加密。替换钱包需要确认，旧文件会自动保存到 `wallet-backups/`。自动本地备份不能防止磁盘损坏；恢复时导入旧 JSON 文件。支出需使用兼容 Taproot `tr()` 描述符的钱包。

实时收益使用 API 的 `net_hashrate`、当前区块奖励和观测出块间隔。数据缺失、超过三分钟或关闭实时同步时隐藏收益估算，不再静默使用旧币价、旧奖励或假定全网算力。矿池提供费率时扣除矿池费，否则为扣除矿池费之前的估算。预计收益不等于到账余额；自定义 Stratum 不会显示其他公共矿池的账本。功耗可能包含芯片估算值，不代表插座实测。

强度、电价、空闲及电池策略、防休眠配置会即时应用。修改矿池、钱包、矿工名或 CPU 线程数前需停止挖矿。

正式签名、公证和 GitHub 凭据配置，以及本地 GPU 回归测试的运行方法，见 [英文文档](README.md#developer-id-signing-and-notarization)。未配置签名证书的发布将标记为预发布版。


## 本地 CI 与发布

运行 `./scripts/ci_local.sh`，在本机完成 Rust 测试、严格 lint、原生打包、GPU 自检、M6 内核选择检查、本地模拟矿池回归以及安装包完整性检查。默认不连接公共矿池挖矿。

GitHub 托管的构建、测试和发布工作流已移除并停用。GitHub 仅托管代码与本机生成后上传的安装包，不在其托管 Mac 中运行内核。签名及公证凭据保留在本机钥匙串；上传未公证成品时标记为预发布。详见 [AGENTS.md](AGENTS.md)。
