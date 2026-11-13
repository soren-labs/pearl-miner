import SwiftUI

struct SettingsSheet: View {
    @ObservedObject var vm: MinerViewModel
    @Environment(\.dismiss) private var dismiss

    @State private var draft: MinerConfig = .default
    @State private var portText: String = "3360"
    @State private var electricityText: String = "0.15"
    @State private var validationError: String?

    var body: some View {
        let l = vm.l10n
        VStack(spacing: 0) {
            HStack {
                Text(l.settingsTitle)
                    .font(.title3.weight(.semibold))
                Spacer()
                Button(l.cancel) {
                    dismiss()
                }
                Button(l.save) {
                    if commitChanges() { dismiss() }
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            }
            .padding(18)

            if let error = validationError ?? vm.errorMessage { Text(error).font(.caption).foregroundStyle(.red).padding(.horizontal, 18) }
            Divider()

            Form {
                Section(l.poolSection) {
                    if vm.isMining {
                        Text(l.stopToEditPool)
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }

                    Picker(l.poolLabel, selection: $draft.pool) {
                        ForEach(PoolKind.allCases) { p in
                            Text("\(p.displayName) — \(p.endpointSubtitle)").tag(p)
                        }
                    }
                    .disabled(vm.isMining)

                    if draft.pool == .custom {
                        TextField(l.customHost, text: $draft.customHost)
                            .disabled(vm.isMining)
                        TextField(l.customPort, text: $portText)
                            .disabled(vm.isMining)
                        Picker(l.dialectLabel, selection: $draft.customDialect) {
                            ForEach(DialectKind.allCases) { d in
                                Text(d.displayName).tag(d)
                            }
                        }
                        .disabled(vm.isMining)
                    }

                    TextField(l.workerLabel, text: $draft.worker)
                        .disabled(vm.isMining)
                }

                Section(l.perfSection) {
                    HStack {
                        Text(l.intensity)
                        Spacer()
                        Text("\(draft.intensity)%")
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                    Slider(
                        value: Binding(
                            get: { Double(draft.intensity) },
                            set: { draft.intensity = Int($0) }
                        ),
                        in: 25...100,
                        step: 5
                    )

                    Toggle(l.autoIntensity, isOn: $draft.autoIntensity)
                    Toggle(l.keepAwake, isOn: $draft.keepAwake)

                    Picker(l.batteryPolicy, selection: $draft.onBattery) {
                        Text(l.batteryPause).tag(BatteryPolicy.pause)
                        Text(l.batteryLow).tag(BatteryPolicy.low)
                        Text(l.batteryFull).tag(BatteryPolicy.full)
                    }
                }

                Section(l.economicsSection) {
                    Toggle(l.liveSyncToggle, isOn: $draft.liveMarketSync)
                    TextField(l.electricityRate, text: $electricityText)

                    Picker(l.languageLabel, selection: Binding(
                        get: { AppLanguage(rawValue: draft.language) ?? .en },
                        set: { draft.language = $0.rawValue }
                    )) {
                        ForEach(AppLanguage.allCases) { lang in
                            Text(lang.fullName).tag(lang)
                        }
                    }
                }

                Section(l.verificationSection) {
                    HStack {
                        Button {
                            vm.runSelfTest()
                        } label: {
                            Text(vm.isRunningSelfTest ? l.runningSelfTest : l.runSelfTest)
                        }
                        .disabled(vm.isRunningSelfTest || vm.isRunningBenchmark || vm.isMining || !vm.engineAvailable)

                        Spacer()

                        if vm.isRunningSelfTest {
                            ProgressView().controlSize(.small)
                        } else if let report = vm.selfTestReport {
                            Label(
                                "\(report.passed ? "✓" : "✗") \(report.checksPassed) · \(report.elapsedMs) ms",
                                systemImage: report.passed ? "checkmark.circle.fill" : "xmark.circle.fill"
                            )
                            .font(.caption)
                            .foregroundStyle(report.passed ? .green : .red)
                        }
                    }

                    HStack {
                        Button {
                            vm.runBenchmark()
                        } label: {
                            Text(vm.isRunningBenchmark ? l.runningBenchmark : l.runBenchmark)
                        }
                        .disabled(vm.isRunningBenchmark || vm.isRunningSelfTest || vm.isMining || !vm.engineAvailable)

                        Spacer()

                        if vm.isRunningBenchmark {
                            ProgressView().controlSize(.small)
                        } else if let bench = vm.benchmarkReport {
                            let sysW = bench.power.systemWatts.map { String(format: " · %.1f W", $0) } ?? ""
                            Text(String(format: "%.2f TH/s%@", bench.equivThs, sysW))
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .formStyle(.grouped)
        }
        .frame(width: 480, height: 540)
        .onAppear {
            draft = vm.config
            portText = String(vm.config.customPort)
            electricityText = String(format: "%.2f", vm.config.electricityUsdPerKwh)
        }
    }

    private func commitChanges() -> Bool {
        var next = draft
        guard let p = Int(portText), (1...65535).contains(p),
              let e = Double(electricityText), e.isFinite, e >= 0 else {
            validationError = vm.l10n.invalidSettings
            return false
        }
        next.customPort = p
        next.electricityUsdPerKwh = e
        let trimmedWorker = next.worker.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedWorker.isEmpty {
            next.worker = vm.config.worker
        } else {
            next.worker = trimmedWorker
        }
        if vm.saveConfig(next) { return true }
        validationError = vm.errorMessage
        vm.errorMessage = nil
        return false
    }
}
