import SwiftUI

struct MainView: View {
    @ObservedObject var vm: MinerViewModel

    @State private var showLogs = false
    @State private var intensityDraft: Double = 100
    @State private var isDraggingIntensity = false

    var body: some View {
        let l = vm.l10n
        ScrollView {
            VStack(spacing: 14) {
                headerBar(l)

                if let err = vm.errorMessage {
                    errorBanner(err)
                }

                heroControlCard(l)
                metricsGrid(l)
                poolAndSharesRow(l)
                intensityCard(l)
                activityLogDisclosure(l)
            }
            .padding(20)
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            intensityDraft = Double(vm.config.intensity)
        }
        .onChange(of: vm.config.intensity) { _, newVal in
            if !isDraggingIntensity {
                intensityDraft = Double(newVal)
            }
        }
        .sheet(isPresented: $vm.showWalletSheet) {
            WalletSheet(vm: vm)
        }
        .sheet(isPresented: $vm.showSettingsSheet) {
            SettingsSheet(vm: vm)
        }
    }

    @ViewBuilder
    private func headerBar(_ l: L10n) -> some View {
        HStack(spacing: 10) {
            PearlOrbLogo(size: 32)
            VStack(alignment: .leading, spacing: 1) {
                Text(l.appName)
                    .font(.headline)
                if let hw = vm.hardware {
                    Text("\(hw.deviceName) · \(hw.kernelName.replacingOccurrences(of: "pow_sweep_", with: ""))")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                } else {
                    Text(l.subtitle)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            LanguageToggle(language: Binding(
                get: { vm.language },
                set: { vm.setLanguage($0) }
            ))

            Button {
                vm.showWalletSheet = true
            } label: {
                Label(shortAddress(vm.wallet?.address), systemImage: "wallet.bifold")
                    .font(.caption.weight(.medium))
            }
            .buttonStyle(.bordered)
            .controlSize(.small)

            Button {
                vm.showSettingsSheet = true
            } label: {
                Image(systemName: "gearshape")
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
            .help(l.settingsTitle)
        }
    }

    @ViewBuilder
    private func errorBanner(_ message: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
            Text(message)
                .font(.caption)
                .lineLimit(2)
            Spacer()
            Button {
                vm.errorMessage = nil
            } label: {
                Image(systemName: "xmark")
                    .font(.caption2)
            }
            .buttonStyle(.plain)
        }
        .padding(10)
        .background(.red.opacity(0.1), in: RoundedRectangle(cornerRadius: 10))
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(.red.opacity(0.25), lineWidth: 1)
        )
    }

    @ViewBuilder
    private func heroControlCard(_ l: L10n) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .center, spacing: 12) {
                Circle()
                    .fill(statusColor)
                    .frame(width: 10, height: 10)
                    .shadow(color: statusColor.opacity(0.6), radius: vm.statusState == "mining" ? 5 : 0)

                VStack(alignment: .leading, spacing: 2) {
                    Text(statusTitle(l))
                        .font(.title3.weight(.semibold))
                    Text(heroSubtitle(l))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Spacer()

                if vm.isMining {
                    Button {
                        vm.stopMining()
                    } label: {
                        Label(l.stopMining, systemImage: "stop.fill")
                            .padding(.horizontal, 10)
                    }
                    .buttonStyle(.bordered)
                    .tint(.red)
                    .controlSize(.large)
                } else {
                    Button {
                        vm.startMining()
                    } label: {
                        Label(l.startMining, systemImage: "play.fill")
                            .padding(.horizontal, 10)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .disabled(vm.wallet == nil)
                }
            }

            Divider().opacity(0.5)

            HStack(spacing: 14) {
                Button {
                    vm.showWalletSheet = true
                } label: {
                    Label(shortAddress(vm.wallet?.address), systemImage: "creditcard")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Text("·").foregroundStyle(.tertiary)

                Button {
                    vm.showSettingsSheet = true
                } label: {
                    Label(poolEndpointText, systemImage: "server.rack")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Text("·").foregroundStyle(.tertiary)

                Label(vm.config.worker, systemImage: "desktopcomputer")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Spacer()

                if let h = vm.blockHeight {
                    Text("#\(h)")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(16)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 14))
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(Color.primary.opacity(0.08), lineWidth: 1)
        )
    }

    @ViewBuilder
    private func metricsGrid(_ l: L10n) -> some View {
        HStack(spacing: 12) {
            // 1. Hashrate
            MetricCard(title: l.hashrate, icon: "bolt.fill", accent: .teal) {
                if vm.equivThs > 0.001 {
                    HStack(alignment: .firstTextBaseline, spacing: 4) {
                        Text(String(format: "%.2f", vm.equivThs))
                            .font(.system(size: 26, weight: .semibold, design: .rounded))
                            .monospacedDigit()
                        Text("TH/s")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.secondary)
                    }
                    Text(String(format: "%.1fM tiles/s", vm.tilesPerS / 1_000_000.0))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                } else {
                    Text("—")
                        .font(.system(size: 26, weight: .semibold, design: .rounded))
                        .foregroundStyle(.secondary)
                    Text(vm.isMining ? l.statusConnecting : l.statusReady)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            // 2. Estimated Earnings
            MetricCard(title: l.estEarnings, icon: "dollarsign.circle.fill", accent: .green) {
                if let econ = vm.economics, vm.equivThs > 0.001 {
                    HStack(alignment: .firstTextBaseline, spacing: 3) {
                        Text(String(format: "$%.2f", econ.grossUsdPerDay))
                            .font(.system(size: 26, weight: .semibold, design: .rounded))
                            .monospacedDigit()
                        Text(l.perDay)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Text(String(format: "%.2f PRL · %@ $%.2f", econ.prlPerDay, l.netAfterPower, econ.netUsdPerDay))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .monospacedDigit()
                } else {
                    Text("—")
                        .font(.system(size: 26, weight: .semibold, design: .rounded))
                        .foregroundStyle(.secondary)
                    if let price = vm.market.prlPriceUsd {
                        Text(String(format: "PRL $%.2f · %@", price, l.liveMarket))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    } else {
                        Text(l.assumedMarket)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            // 3. System & GPU Power
            MetricCard(title: l.systemPower, icon: "gauge.with.needle.fill", accent: .orange) {
                if let sysW = vm.power.systemWatts {
                    HStack(alignment: .firstTextBaseline, spacing: 3) {
                        Text(String(format: "%.1f", sysW))
                            .font(.system(size: 26, weight: .semibold, design: .rounded))
                            .monospacedDigit()
                        Text("W")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.secondary)
                    }
                    let gpuPart = vm.power.gpuWatts.map { String(format: "%@ %.1f W · ", l.gpuPowerLabel, $0) } ?? ""
                    let sensorPart = vm.power.isEstimated ? l.estimatedSensor : l.liveSensor
                    Text("\(gpuPart)\(sensorPart)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .monospacedDigit()
                } else {
                    Text("—")
                        .font(.system(size: 26, weight: .semibold, design: .rounded))
                        .foregroundStyle(.secondary)
                    Text(l.estimatedSensor)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder
    private func poolAndSharesRow(_ l: L10n) -> some View {
        HStack(spacing: 12) {
            // Pool Balance card
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text(l.poolBalance)
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.secondary)
                        Button {
                            vm.refreshMarket()
                        } label: {
                            Image(systemName: "arrow.clockwise")
                                .font(.caption2)
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(.secondary)
                    }

                    let pending = vm.market.poolPendingPrl
                    let confirmed = vm.market.poolConfirmedPrl
                    if pending != nil || confirmed != nil {
                        HStack(spacing: 12) {
                            Text(String(format: "%@ %.4f PRL", l.pending, pending ?? 0))
                                .font(.subheadline.weight(.medium).monospacedDigit())
                            Text(String(format: "%@ %.4f", l.confirmed, confirmed ?? 0))
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    } else {
                        Text("—")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }
                Spacer()
            }
            .padding(14)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.primary.opacity(0.08), lineWidth: 1)
            )

            // Shares card
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text(l.shares)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                    HStack(spacing: 12) {
                        Label("\(vm.acceptedShares)", systemImage: "checkmark.circle.fill")
                            .font(.subheadline.weight(.medium).monospacedDigit())
                            .foregroundStyle(.green)
                        Label("\(vm.rejectedShares)", systemImage: "xmark.circle")
                            .font(.subheadline.monospacedDigit())
                            .foregroundStyle(vm.rejectedShares > 0 ? .red : .secondary)
                        if vm.pendingShares > 0 {
                            Text("(\(vm.pendingShares))")
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                Spacer()
            }
            .padding(14)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.primary.opacity(0.08), lineWidth: 1)
            )
            .frame(maxWidth: 210)
        }
    }

    @ViewBuilder
    private func intensityCard(_ l: L10n) -> some View {
        VStack(spacing: 8) {
            HStack {
                Label(l.intensity, systemImage: "slider.horizontal.3")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
                Spacer()
                Text("\(Int(intensityDraft))%")
                    .font(.caption.weight(.semibold).monospacedDigit())
                if vm.isMining && vm.intensityNow != Int(intensityDraft) {
                    Text("→ \(vm.intensityNow)%")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
            Slider(
                value: $intensityDraft,
                in: 25...100,
                step: 5,
                onEditingChanged: { editing in
                    isDraggingIntensity = editing
                    if !editing {
                        var next = vm.config
                        next.intensity = Int(intensityDraft)
                        vm.saveConfig(next)
                    }
                }
            )
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.primary.opacity(0.08), lineWidth: 1)
        )
    }

    @ViewBuilder
    private func activityLogDisclosure(_ l: L10n) -> some View {
        DisclosureGroup(isExpanded: $showLogs) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 4) {
                    ForEach(vm.logs.suffix(60)) { entry in
                        HStack(alignment: .top, spacing: 8) {
                            Text(entry.timestamp)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(.tertiary)
                            Text(entry.message)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(entry.level == "error" ? .red : (entry.level == "warn" ? .orange : .secondary))
                                .textSelection(.enabled)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 6)
            }
            .frame(height: 115)
        } label: {
            HStack {
                Text(l.activityLog)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
                if let last = vm.logs.last {
                    Text("· \(last.message)")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.primary.opacity(0.08), lineWidth: 1)
        )
    }

    private var statusColor: Color {
        switch vm.statusState {
        case "mining": return .green
        case "connecting", "reconnecting", "paused_battery": return .orange
        case "error": return .red
        default: return .gray
        }
    }

    private func statusTitle(_ l: L10n) -> String {
        switch vm.statusState {
        case "connecting": return l.statusConnecting
        case "mining": return l.statusMining
        case "paused_battery": return l.statusPausedBattery
        case "reconnecting": return l.statusReconnecting
        case "error": return l.statusError
        case "stopped": return vm.isMining ? l.statusStopped : l.statusReady
        default: return l.statusReady
        }
    }

    private func heroSubtitle(_ l: L10n) -> String {
        if vm.isMining && vm.uptimeSeconds > 0 {
            let h = vm.uptimeSeconds / 3600
            let m = (vm.uptimeSeconds % 3600) / 60
            let s = vm.uptimeSeconds % 60
            let clock = String(format: "%02d:%02d:%02d", h, m, s)
            return "\(vm.config.pool.displayName) · \(l.uptime) \(clock)"
        }
        if !vm.statusDetail.isEmpty {
            return vm.statusDetail
        }
        return vm.config.pool.displayName
    }

    private var poolEndpointText: String {
        if vm.config.pool == .custom {
            return "\(vm.config.customHost):\(vm.config.customPort)"
        }
        return vm.config.pool.endpointSubtitle
    }

    private func shortAddress(_ addr: String?) -> String {
        guard let addr, addr.count > 16 else { return addr ?? "—" }
        return "\(addr.prefix(9))…\(addr.suffix(6))"
    }
}

// MARK: - Shared UI Components

struct MetricCard<Content: View>: View {
    let title: String
    let icon: String
    let accent: Color
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 5) {
                Image(systemName: icon)
                    .font(.caption2)
                    .foregroundStyle(accent)
                Text(title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
            }
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.primary.opacity(0.08), lineWidth: 1)
        )
    }
}

struct LanguageToggle: View {
    @Binding var language: AppLanguage

    var body: some View {
        HStack(spacing: 2) {
            ForEach(AppLanguage.allCases) { lang in
                Button {
                    language = lang
                } label: {
                    Text(lang.shortLabel)
                        .font(.caption2.weight(.medium))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(
                            language == lang ? Color.accentColor.opacity(0.18) : Color.clear,
                            in: Capsule()
                        )
                        .foregroundStyle(language == lang ? Color.accentColor : .secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(2)
        .background(Color.primary.opacity(0.06), in: Capsule())
    }
}

struct PearlOrbLogo: View {
    let size: CGFloat

    var body: some View {
        ZStack {
            Circle()
                .fill(
                    RadialGradient(
                        colors: [
                            Color(red: 0.96, green: 0.98, blue: 1.0),
                            Color(red: 0.62, green: 0.85, blue: 0.88),
                            Color(red: 0.22, green: 0.49, blue: 0.60),
                        ],
                        center: .topLeading,
                        startRadius: size * 0.08,
                        endRadius: size * 0.85
                    )
                )
            Circle()
                .stroke(.white.opacity(0.45), lineWidth: 1)
        }
        .frame(width: size, height: size)
        .shadow(color: .teal.opacity(0.25), radius: 4, y: 2)
    }
}
