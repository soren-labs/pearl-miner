import Foundation
import SwiftUI

@MainActor
final class MinerViewModel: ObservableObject {
    @Published var config: MinerConfig = .default
    @Published var wallet: WalletSummary?
    @Published var revealedPrivateKey: String?
    @Published var hardware: HardwareInfo?
    @Published var market = MarketAndPoolSnapshot()
    @Published var power = PowerReading(systemWatts: nil, gpuWatts: nil, isEstimated: true, onBattery: false)
    @Published var economics: EconomicsSnapshot?

    @Published var isMining: Bool = false
    @Published var statusState: String = "stopped"
    @Published var statusDetail: String = ""
    @Published var jobId: String?
    @Published var blockHeight: UInt64?
    @Published var certVersion: UInt32?
    @Published var uptimeSeconds: UInt64 = 0

    @Published var equivThs: Double = 0
    @Published var tilesPerS: Double = 0
    @Published var acceptedShares: UInt64 = 0
    @Published var rejectedShares: UInt64 = 0
    @Published var pendingShares: Int = 0
    @Published var intensityNow: Int = 100

    @Published var logs: [LogEntry] = []
    @Published var errorMessage: String?
    @Published var isRunningSelfTest: Bool = false
    @Published var selfTestReport: SelfTestReport?
    @Published var isRunningBenchmark: Bool = false
    @Published var benchmarkReport: BenchmarkReport?

    @Published var language: AppLanguage = .en
    @Published var showWalletSheet: Bool = false
    @Published var showSettingsSheet: Bool = false
    @Published var hasLoadedInitialState: Bool = false

    private var process: Process?
    private var stdinPipe: Pipe?
    private let writeQueue = DispatchQueue(label: "com.pearlminer.stdin")

    var l10n: L10n { L10n(lang: language) }

    init() {
        launchDaemon()
    }

    func shutdown() {
        sendCommand(["cmd": "quit"])
        if let p = process, p.isRunning {
            p.terminate()
        }
        process = nil
    }

    private func resolveCoreBinary() -> URL? {
        if let aux = Bundle.main.url(forAuxiliaryExecutable: "pearl-miner-core"),
           FileManager.default.isExecutableFile(atPath: aux.path) {
            return aux
        }
        let execDir = Bundle.main.executableURL?.deletingLastPathComponent()
        if let sibling = execDir?.appendingPathComponent("pearl-miner-core"),
           FileManager.default.isExecutableFile(atPath: sibling.path) {
            return sibling
        }
        // Development fallback relative to repo
        let cwd = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        for candidate in [
            cwd.appendingPathComponent("target/release/pearl-miner-core"),
            cwd.appendingPathComponent("../target/release/pearl-miner-core"),
            cwd.appendingPathComponent("target/debug/pearl-miner-core"),
            cwd.appendingPathComponent("../target/debug/pearl-miner-core"),
        ] {
            if FileManager.default.isExecutableFile(atPath: candidate.path) {
                return candidate
            }
        }
        return nil
    }

    func launchDaemon() {
        guard let bin = resolveCoreBinary() else {
            errorMessage = "Could not find pearl-miner-core engine binary."
            return
        }
        let proc = Process()
        proc.executableURL = bin
        proc.arguments = ["daemon"]

        let inPipe = Pipe()
        let outPipe = Pipe()
        let errPipe = Pipe()
        proc.standardInput = inPipe
        proc.standardOutput = outPipe
        proc.standardError = errPipe

        self.stdinPipe = inPipe
        self.process = proc

        let handle = outPipe.fileHandleForReading
        Task.detached { [weak self] in
            var buffer = Data()
            while true {
                let chunk = handle.availableData
                if chunk.isEmpty { break }
                buffer.append(chunk)
                while let nlIndex = buffer.firstIndex(of: 0x0A) {
                    let lineData = buffer.subdata(in: 0..<nlIndex)
                    buffer.removeSubrange(0...nlIndex)
                    if let line = String(data: lineData, encoding: .utf8), !line.isEmpty {
                        await self?.handleDaemonLine(line)
                    }
                }
            }
        }

        let errHandle = errPipe.fileHandleForReading
        Task.detached { [weak self] in
            while true {
                let chunk = errHandle.availableData
                if chunk.isEmpty { break }
                if let text = String(data: chunk, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
                   !text.isEmpty {
                    await self?.appendLog(level: "stderr", message: text)
                }
            }
        }

        do {
            try proc.run()
        } catch {
            errorMessage = "Failed to start engine: \(error.localizedDescription)"
        }
    }

    private func sendCommand(_ dict: [String: Any]) {
        guard let pipe = stdinPipe else { return }
        guard let data = try? JSONSerialization.data(withJSONObject: dict) else { return }
        let payload = data + Data([0x0A])
        writeQueue.async {
            try? pipe.fileHandleForWriting.write(contentsOf: payload)
        }
    }

    func setLanguage(_ lang: AppLanguage) {
        self.language = lang
        var updated = config
        updated.language = lang.rawValue
        saveConfig(updated)
    }

    func saveConfig(_ newConfig: MinerConfig) {
        self.config = newConfig
        if let lang = AppLanguage(rawValue: newConfig.language) {
            self.language = lang
        }
        guard let data = try? JSONEncoder().encode(newConfig),
              let cfgObj = try? JSONSerialization.jsonObject(with: data) else { return }
        sendCommand(["cmd": "update_config", "config": cfgObj])
    }

    func startMining() {
        errorMessage = nil
        acceptedShares = 0
        rejectedShares = 0
        pendingShares = 0
        equivThs = 0
        tilesPerS = 0
        uptimeSeconds = 0
        statusState = "connecting"
        sendCommand(["cmd": "start"])
    }

    func stopMining() {
        sendCommand(["cmd": "stop"])
    }

    func createWallet() {
        errorMessage = nil
        sendCommand(["cmd": "wallet_create"])
    }

    func importPrivateKey(_ hex: String) {
        errorMessage = nil
        sendCommand(["cmd": "wallet_import_key", "private_key_hex": hex])
    }

    func importWalletFile(_ url: URL) {
        errorMessage = nil
        sendCommand(["cmd": "wallet_import_file", "path": url.path])
    }

    func connectAddress(_ address: String) {
        errorMessage = nil
        sendCommand(["cmd": "wallet_connect_address", "address": address])
    }

    func revealPrivateKey() {
        sendCommand(["cmd": "wallet_reveal_key"])
    }

    func refreshMarket() {
        sendCommand(["cmd": "refresh_market"])
    }

    func runSelfTest() {
        isRunningSelfTest = true
        selfTestReport = nil
        sendCommand(["cmd": "self_test"])
    }

    func runBenchmark() {
        isRunningBenchmark = true
        benchmarkReport = nil
        sendCommand(["cmd": "benchmark", "seconds": 10.0])
    }

    private func appendLog(level: String, message: String) {
        let df = DateFormatter()
        df.dateFormat = "HH:mm:ss"
        let entry = LogEntry(timestamp: df.string(from: Date()), level: level, message: message)
        logs.append(entry)
        if logs.count > 200 {
            logs.removeFirst(logs.count - 200)
        }
    }

    private func handleDaemonLine(_ line: String) {
        guard let data = line.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else { return }

        let decoder = JSONDecoder()

        switch type {
        case "state_snapshot":
            if let cfgObj = json["config"],
               let cfgData = try? JSONSerialization.data(withJSONObject: cfgObj),
               let cfg = try? decoder.decode(MinerConfig.self, from: cfgData) {
                self.config = cfg
                if let lang = AppLanguage(rawValue: cfg.language) {
                    self.language = lang
                }
            }
            if let wObj = json["wallet"], !(wObj is NSNull),
               let wData = try? JSONSerialization.data(withJSONObject: wObj),
               let w = try? decoder.decode(WalletSummary.self, from: wData) {
                self.wallet = w
            }
            if let hObj = json["hardware"], !(hObj is NSNull),
               let hData = try? JSONSerialization.data(withJSONObject: hObj),
               let h = try? decoder.decode(HardwareInfo.self, from: hData) {
                self.hardware = h
            }
            if let mObj = json["market"],
               let mData = try? JSONSerialization.data(withJSONObject: mObj),
               let m = try? decoder.decode(MarketAndPoolSnapshot.self, from: mData) {
                self.market = m
            }
            if let pObj = json["power"],
               let pData = try? JSONSerialization.data(withJSONObject: pObj),
               let p = try? decoder.decode(PowerReading.self, from: pData) {
                self.power = p
            }
            if let mFlag = json["mining"] as? Bool {
                self.isMining = mFlag
            }
            self.hasLoadedInitialState = true

        case "status":
            let state = json["state"] as? String ?? "stopped"
            self.statusState = state
            self.statusDetail = json["detail"] as? String ?? ""
            self.jobId = json["job_id"] as? String
            self.blockHeight = json["height"] as? UInt64
            self.certVersion = json["cert_version"] as? UInt32
            self.uptimeSeconds = json["uptime_s"] as? UInt64 ?? 0
            self.isMining = (state == "mining" || state == "connecting" || state == "reconnecting" || state == "paused_battery")
            if state == "error" && !statusDetail.isEmpty {
                self.errorMessage = statusDetail
            }

        case "metrics":
            self.tilesPerS = json["tiles_per_s"] as? Double ?? 0
            self.equivThs = json["equiv_ths"] as? Double ?? 0
            self.acceptedShares = json["accepted"] as? UInt64 ?? 0
            self.rejectedShares = json["rejected"] as? UInt64 ?? 0
            self.pendingShares = json["pending"] as? Int ?? 0
            self.intensityNow = json["intensity_now"] as? Int ?? config.intensity
            if let pObj = json["power"],
               let pData = try? JSONSerialization.data(withJSONObject: pObj),
               let p = try? decoder.decode(PowerReading.self, from: pData) {
                self.power = p
            }
            if let eObj = json["economics"],
               let eData = try? JSONSerialization.data(withJSONObject: eObj),
               let e = try? decoder.decode(EconomicsSnapshot.self, from: eData) {
                self.economics = e
            }

        case "market":
            if let mObj = json["market"],
               let mData = try? JSONSerialization.data(withJSONObject: mObj),
               let m = try? decoder.decode(MarketAndPoolSnapshot.self, from: mData) {
                self.market = m
            }

        case "power":
            if let pObj = json["power"],
               let pData = try? JSONSerialization.data(withJSONObject: pObj),
               let p = try? decoder.decode(PowerReading.self, from: pData) {
                self.power = p
            }

        case "wallet_updated":
            if let wObj = json["wallet"], !(wObj is NSNull),
               let wData = try? JSONSerialization.data(withJSONObject: wObj),
               let w = try? decoder.decode(WalletSummary.self, from: wData) {
                self.wallet = w
            }
            self.revealedPrivateKey = json["revealed_private_key"] as? String
            refreshMarket()

        case "self_test_result":
            self.isRunningSelfTest = false
            if let rObj = json["report"],
               let rData = try? JSONSerialization.data(withJSONObject: rObj),
               let r = try? decoder.decode(SelfTestReport.self, from: rData) {
                self.selfTestReport = r
            }

        case "benchmark_result":
            self.isRunningBenchmark = false
            if let rObj = json["report"],
               let rData = try? JSONSerialization.data(withJSONObject: rObj),
               let r = try? decoder.decode(BenchmarkReport.self, from: rData) {
                self.benchmarkReport = r
            }

        case "log":
            let lvl = json["level"] as? String ?? "info"
            let msg = json["message"] as? String ?? ""
            appendLog(level: lvl, message: msg)

        case "error":
            self.isRunningSelfTest = false
            self.isRunningBenchmark = false
            let msg = json["message"] as? String ?? "Unknown error"
            self.errorMessage = msg
            appendLog(level: "error", message: msg)

        default:
            break
        }
    }
}
