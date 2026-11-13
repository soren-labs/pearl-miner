import Foundation

enum PoolKind: String, Codable, CaseIterable, Identifiable {
    case luckypool = "luckypool"
    case kryptexEu = "kryptex_eu"
    case kryptexUs = "kryptex_us"
    case custom = "custom"

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .luckypool: return "LuckyPool (EU)"
        case .kryptexEu: return "Kryptex (EU)"
        case .kryptexUs: return "Kryptex (US)"
        case .custom: return "Custom"
        }
    }

    var endpointSubtitle: String {
        switch self {
        case .luckypool: return "pearl-eu1.luckypool.io:3360"
        case .kryptexEu: return "prl-eu.kryptex.network:7048"
        case .kryptexUs: return "prl-us.kryptex.network:7048"
        case .custom: return "Custom Stratum"
        }
    }
}

enum DialectKind: String, Codable, CaseIterable, Identifiable {
    case luckypool = "luckypool"
    case kryptex = "kryptex"

    var id: String { rawValue }
    var displayName: String {
        switch self {
        case .luckypool: return "LuckyPool"
        case .kryptex: return "Kryptex"
        }
    }
}

enum BatteryPolicy: String, Codable, CaseIterable, Identifiable {
    case pause = "pause"
    case low = "low"
    case full = "full"

    var id: String { rawValue }
}

struct MinerConfig: Codable, Equatable {
    var language: String
    var pool: PoolKind
    var customDialect: DialectKind
    var customHost: String
    var customPort: Int
    var worker: String
    var intensity: Int
    var autoIntensity: Bool
    var onBattery: BatteryPolicy
    var keepAwake: Bool
    var cpuThreads: Int
    var electricityUsdPerKwh: Double
    var liveMarketSync: Bool
    var fallbackPrlPriceUsd: Double
    var fallbackNetworkHashrateEhs: Double

    enum CodingKeys: String, CodingKey {
        case language
        case pool
        case customDialect = "custom_dialect"
        case customHost = "custom_host"
        case customPort = "custom_port"
        case worker
        case intensity
        case autoIntensity = "auto_intensity"
        case onBattery = "on_battery"
        case keepAwake = "keep_awake"
        case cpuThreads = "cpu_threads"
        case electricityUsdPerKwh = "electricity_usd_per_kwh"
        case liveMarketSync = "live_market_sync"
        case fallbackPrlPriceUsd = "fallback_prl_price_usd"
        case fallbackNetworkHashrateEhs = "fallback_network_hashrate_ehs"
    }

    static let `default` = MinerConfig(
        language: "en",
        pool: .luckypool,
        customDialect: .luckypool,
        customHost: "pearl-eu1.luckypool.io",
        customPort: 3360,
        worker: Host.current().localizedName ?? "mac",
        intensity: 100,
        autoIntensity: false,
        onBattery: .pause,
        keepAwake: true,
        cpuThreads: 4,
        electricityUsdPerKwh: 0.15,
        liveMarketSync: true,
        fallbackPrlPriceUsd: 1.40,
        fallbackNetworkHashrateEhs: 35.0
    )
}

struct WalletSummary: Codable, Equatable {
    var address: String
    var mode: String
    var hasPrivateKey: Bool
    var createdUtc: String
    var path: String

    enum CodingKeys: String, CodingKey {
        case address
        case mode
        case hasPrivateKey = "has_private_key"
        case createdUtc = "created_utc"
        case path
    }
}

struct HardwareInfo: Codable, Equatable {
    var deviceName: String
    var kernelName: String
    var m: Int
    var n: Int
    var k: Int
    var r: Int
    var factor: UInt64

    enum CodingKeys: String, CodingKey {
        case deviceName = "device_name"
        case kernelName = "kernel_name"
        case m, n, k, r, factor
    }
}

struct PowerReading: Codable, Equatable {
    var systemWatts: Double?
    var gpuWatts: Double?
    var isEstimated: Bool
    var onBattery: Bool

    enum CodingKeys: String, CodingKey {
        case systemWatts = "system_watts"
        case gpuWatts = "gpu_watts"
        case isEstimated = "is_estimated"
        case onBattery = "on_battery"
    }
}

struct EconomicsSnapshot: Codable, Equatable {
    var prlPerDay: Double
    var prlPerMonth: Double
    var grossUsdPerDay: Double
    var grossUsdPerMonth: Double
    var powerCostUsdPerDay: Double
    var netUsdPerDay: Double
    var kwhPerDay: Double
    var prlPriceUsd: Double
    var networkHashrateEhs: Double
    var isLiveMarket: Bool

    enum CodingKeys: String, CodingKey {
        case prlPerDay = "prl_per_day"
        case prlPerMonth = "prl_per_month"
        case grossUsdPerDay = "gross_usd_per_day"
        case grossUsdPerMonth = "gross_usd_per_month"
        case powerCostUsdPerDay = "power_cost_usd_per_day"
        case netUsdPerDay = "net_usd_per_day"
        case kwhPerDay = "kwh_per_day"
        case prlPriceUsd = "prl_price_usd"
        case networkHashrateEhs = "network_hashrate_ehs"
        case isLiveMarket = "is_live_market"
    }
}

struct MarketAndPoolSnapshot: Codable, Equatable {
    var prlPriceUsd: Double?
    var networkHashrateEhs: Double?
    var poolConfirmedPrl: Double?
    var poolPendingPrl: Double?
    var poolPaidPrl: Double?
    var poolThresholdPrl: Double?
    var lastUpdatedUtc: String?

    enum CodingKeys: String, CodingKey {
        case prlPriceUsd = "prl_price_usd"
        case networkHashrateEhs = "network_hashrate_ehs"
        case poolConfirmedPrl = "pool_confirmed_prl"
        case poolPendingPrl = "pool_pending_prl"
        case poolPaidPrl = "pool_paid_prl"
        case poolThresholdPrl = "pool_threshold_prl"
        case lastUpdatedUtc = "last_updated_utc"
    }
}

struct SelfTestReport: Codable, Equatable {
    var passed: Bool
    var deviceName: String
    var kernelName: String
    var checksPassed: Int
    var elapsedMs: Int
    var message: String

    enum CodingKeys: String, CodingKey {
        case passed
        case deviceName = "device_name"
        case kernelName = "kernel_name"
        case checksPassed = "checks_passed"
        case elapsedMs = "elapsed_ms"
        case message
    }
}

struct BenchmarkReport: Codable, Equatable {
    var deviceName: String
    var kernelName: String
    var seconds: Double
    var tilesPerS: Double
    var equivThs: Double
    var power: PowerReading

    enum CodingKeys: String, CodingKey {
        case deviceName = "device_name"
        case kernelName = "kernel_name"
        case seconds
        case tilesPerS = "tiles_per_s"
        case equivThs = "equiv_ths"
        case power
    }
}

struct LogEntry: Identifiable, Equatable {
    let id = UUID()
    let timestamp: String
    let level: String
    let message: String
}
