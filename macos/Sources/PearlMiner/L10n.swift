import Foundation

enum AppLanguage: String, CaseIterable, Identifiable {
    case en = "en"
    case zhHans = "zh-Hans"

    var id: String { rawValue }

    var shortLabel: String {
        switch self {
        case .en: return "EN"
        case .zhHans: return "中文"
        }
    }

    var fullName: String {
        switch self {
        case .en: return "English"
        case .zhHans: return "简体中文"
        }
    }
}

struct L10n {
    let lang: AppLanguage

    private func t(_ en: String, _ zh: String) -> String {
        lang == .en ? en : zh
    }

    // Header & General
    var appName: String { "PearlMiner" }
    var subtitle: String { t("Apple Silicon Metal Miner", "Apple Silicon Metal 矿工") }
    var done: String { t("Done", "完成") }
    var cancel: String { t("Cancel", "取消") }
    var save: String { t("Save", "保存") }
    var copy: String { t("Copy", "复制") }
    var copied: String { t("Copied", "已复制") }
    var refresh: String { t("Refresh", "刷新") }

    // Onboarding
    var welcomeTitle: String { t("Start Mining Pearl (PRL)", "开始挖掘珍珠币 (PRL)") }
    var welcomeSubtitle: String {
        t(
            "Connect a Pearl Taproot wallet, choose a pool, and mine with your Mac's Metal GPU.",
            "连接或创建珍珠币 Taproot 钱包，选择矿池，即可一键使用 Mac GPU 挖矿。"
        )
    }
    var step1Title: String { t("1. Connect Your Wallet", "1. 连接或创建钱包") }
    var createNewWallet: String { t("Create New Wallet", "一键创建新钱包") }
    var createWalletDesc: String {
        t(
            "Generates a local prl1p… Taproot keypair stored securely on your Mac (0600).",
            "在本地生成 prl1p… Taproot 密钥对并加密级权限存储（0600）。"
        )
    }
    var importPrivateKey: String { t("Import Private Key", "导入私钥") }
    var importWalletFile: String { t("Import wallet.json", "导入 wallet.json") }
    var connectAddressOnly: String { t("Use Existing Address", "仅连接收款地址") }
    var privateKeyPlaceholder: String { t("64-character hex private key", "输入 64 位十六进制私钥") }
    var addressPlaceholder: String { t("prl1p… Pearl Taproot address", "输入 prl1p… 珍珠币 Taproot 地址") }
    var walletConnected: String { t("Wallet Connected", "钱包已连接") }
    var backupWarning: String {
        t(
            "Save your private key somewhere safe. It is the only way to spend mined PRL.",
            "请妥善备份你的私钥，它是支配挖出 PRL 资产的唯一凭证。"
        )
    }
    var step2Title: String { t("2. Pool & Start", "2. 选择矿池并启动") }

    // Status & Controls
    var startMining: String { t("Start Mining", "一键启动挖矿") }
    var stopMining: String { t("Stop Mining", "停止挖矿") }
    var statusReady: String { t("Ready", "就绪") }
    var statusConnecting: String { t("Connecting to pool…", "正在连接矿池…") }
    var statusMining: String { t("Mining", "挖矿中") }
    var statusPausedBattery: String { t("Paused on Battery", "使用电池已暂停") }
    var statusReconnecting: String { t("Reconnecting…", "正在重连…") }
    var statusError: String { t("Connection Error", "连接错误") }
    var statusStopped: String { t("Stopped", "已停止") }

    // Dashboard Cards
    var hashrate: String { t("Hashrate", "实时算力") }
    var estEarnings: String { t("Est. Earnings", "预计收益") }
    var systemPower: String { t("System Power", "系统功耗") }
    var perDay: String { t("/ day", "/ 天") }
    var perMonth: String { t("/ mo", "/ 月") }
    var netAfterPower: String { t("Net after power", "扣除电费净收益") }
    var gpuPowerLabel: String { t("GPU", "GPU") }
    var liveSensor: String { t("Live sensor", "实时传感器") }
    var estimatedSensor: String { t("Chip estimate", "芯片估算") }
    var liveMarket: String { t("Live price & diff", "实时币价与全网算力") }
    var assumedMarket: String { t("Assumed rate", "参考估算") }
    var poolBalance: String { t("Pool Balance", "矿池余额") }
    var pending: String { t("Pending", "待确认") }
    var confirmed: String { t("Confirmed", "已确认") }
    var shares: String { t("Shares", "份额") }
    var accepted: String { t("Accepted", "有效") }
    var rejected: String { t("Rejected", "拒绝") }
    var intensity: String { t("GPU Intensity", "GPU 强度") }
    var activityLog: String { t("Activity Log", "运行日志") }
    var walletLabel: String { t("Wallet", "钱包") }
    var poolLabel: String { t("Pool", "矿池") }
    var workerLabel: String { t("Worker", "矿工名") }
    var blockHeight: String { t("Height", "高度") }
    var uptime: String { t("Uptime", "运行时长") }

    // Wallet Sheet
    var walletTitle: String { t("Pearl Wallet", "珍珠币钱包") }
    var currentAddress: String { t("Payout Address", "收款地址") }
    var watchOnlyBadge: String { t("Address Only (External Key)", "仅收款地址（外部私钥）") }
    var fullKeyBadge: String { t("Local Keypair (0600)", "本地密钥对 (0600)") }
    var revealPrivateKey: String { t("Reveal Private Key", "查看私钥") }
    var hidePrivateKey: String { t("Hide Private Key", "隐藏私钥") }
    var switchOrImport: String { t("Switch / Import Wallet", "切换或导入钱包") }
    var viewOnPool: String { t("Open Pool Dashboard", "在浏览器查看矿池账本") }

    // Settings Sheet
    var settingsTitle: String { t("Settings", "设置") }
    var poolSection: String { t("Mining Pool", "矿池设置") }
    var customHost: String { t("Host", "主机地址") }
    var customPort: String { t("Port", "端口") }
    var dialectLabel: String { t("Protocol", "协议") }
    var perfSection: String { t("Performance & Power", "性能与电源") }
    var autoIntensity: String { t("Ramp to 100% when Mac is idle", "Mac 空闲时自动拉满 100%") }
    var keepAwake: String { t("Prevent system sleep while mining", "挖矿时阻止系统休眠") }
    var batteryPolicy: String { t("On Battery", "使用电池时") }
    var batteryPause: String { t("Pause", "自动暂停") }
    var batteryLow: String { t("Low (25%)", "低功耗 (25%)") }
    var batteryFull: String { t("Full Speed", "全速运行") }
    var economicsSection: String { t("Earnings & Electricity", "收益与电费参数") }
    var electricityRate: String { t("Electricity ($/kWh)", "电价 ($/度)") }
    var liveSyncToggle: String { t("Fetch live PRL price & pool balance", "自动获取实时币价与矿池余额") }
    var languageLabel: String { t("Language", "界面语言") }
    var verificationSection: String { t("Diagnostics", "诊断与自检") }
    var runSelfTest: String { t("Run GPU Self-Test (~3s)", "运行 GPU 数学自检 (~3秒)") }
    var runningSelfTest: String { t("Testing Metal & V3 Proofs…", "正在验证 Metal 与 V3 证明…") }
    var runBenchmark: String { t("Run 10s Speed Benchmark", "运行 10 秒离线测速") }
    var runningBenchmark: String { t("Benchmarking…", "测速中…") }
    var stopToEditPool: String {
        t("Stop mining before changing pool or wallet.", "请先停止挖矿再切换矿池或钱包。")
    }
}
