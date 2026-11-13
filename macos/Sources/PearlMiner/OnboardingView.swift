import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct OnboardingView: View {
    @ObservedObject var vm: MinerViewModel
    let onContinueToDashboard: () -> Void

    @State private var walletTab: WalletSetupMode = .create
    @State private var privateKeyInput: String = ""
    @State private var addressInput: String = ""
    @State private var workerInput: String = ""
    @State private var selectedPool: PoolKind = .luckypool
    @State private var customHost: String = ""
    @State private var customPort: String = "3360"
    @State private var customDialect: DialectKind = .luckypool
    @State private var showFileImporter = false
    @State private var copiedAddress = false
    @State private var copiedKey = false

    enum WalletSetupMode: String, CaseIterable, Identifiable {
        case create, importKey, importFile, addressOnly
        var id: String { rawValue }
    }

    var body: some View {
        let l = vm.l10n
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header(l)

                if let err = vm.errorMessage {
                    errorBanner(err)
                }

                walletSection(l)
                poolSection(l)
            }
            .padding(24)
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            syncFromConfig()
        }
        .onChange(of: vm.config) { _, _ in
            syncFromConfig()
        }
        .fileImporter(
            isPresented: $showFileImporter,
            allowedContentTypes: [.json],
            allowsMultipleSelection: false
        ) { result in
            if case .success(let urls) = result, let url = urls.first {
                vm.importWalletFile(url)
            }
        }
    }

    private func syncFromConfig() {
        selectedPool = vm.config.pool
        workerInput = vm.config.worker
        customHost = vm.config.customHost
        customPort = String(vm.config.customPort)
        customDialect = vm.config.customDialect
    }

    @ViewBuilder
    private func header(_ l: L10n) -> some View {
        HStack(alignment: .center, spacing: 14) {
            PearlOrbLogo(size: 46)
            VStack(alignment: .leading, spacing: 3) {
                Text(l.welcomeTitle)
                    .font(.title2.weight(.semibold))
                Text(l.welcomeSubtitle)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            LanguageToggle(language: Binding(
                get: { vm.language },
                set: { vm.setLanguage($0) }
            ))
        }
    }

    @ViewBuilder
    private func errorBanner(_ message: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
            Text(message)
                .font(.caption)
                .foregroundStyle(.primary)
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
    private func walletSection(_ l: L10n) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(l.step1Title)
                    .font(.headline)
                Spacer()
                if vm.wallet != nil {
                    Label(l.walletConnected, systemImage: "checkmark.circle.fill")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.green)
                }
            }

            if let wallet = vm.wallet {
                connectedWalletCard(wallet, l: l)
            } else {
                Picker("", selection: $walletTab) {
                    Text(l.createNewWallet).tag(WalletSetupMode.create)
                    Text(l.importPrivateKey).tag(WalletSetupMode.importKey)
                    Text(l.importWalletFile).tag(WalletSetupMode.importFile)
                    Text(l.connectAddressOnly).tag(WalletSetupMode.addressOnly)
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                switch walletTab {
                case .create:
                    HStack(spacing: 12) {
                        Text(l.createWalletDesc)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button {
                            vm.createWallet()
                        } label: {
                            Label(l.createNewWallet, systemImage: "plus.circle.fill")
                        }
                        .buttonStyle(.borderedProminent)
                    }

                case .importKey:
                    HStack(spacing: 10) {
                        SecureField(l.privateKeyPlaceholder, text: $privateKeyInput)
                            .textFieldStyle(.roundedBorder)
                            .font(.system(.body, design: .monospaced))
                        Button(l.save) {
                            vm.importPrivateKey(privateKeyInput.trimmingCharacters(in: .whitespacesAndNewlines))
                            privateKeyInput = ""
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(privateKeyInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }

                case .importFile:
                    HStack(spacing: 12) {
                        Text("Import an existing Pearl wallet.json file.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button(l.importWalletFile) {
                            showFileImporter = true
                        }
                        .buttonStyle(.borderedProminent)
                    }

                case .addressOnly:
                    HStack(spacing: 10) {
                        TextField(l.addressPlaceholder, text: $addressInput)
                            .textFieldStyle(.roundedBorder)
                            .font(.system(.body, design: .monospaced))
                        Button(l.save) {
                            vm.connectAddress(addressInput.trimmingCharacters(in: .whitespacesAndNewlines))
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(addressInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
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
    private func connectedWalletCard(_ wallet: WalletSummary, l: L10n) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text(wallet.address)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer()
                Button(copiedAddress ? l.copied : l.copy) {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(wallet.address, forType: .string)
                    copiedAddress = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copiedAddress = false }
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }

            if let priv = vm.revealedPrivateKey, wallet.hasPrivateKey {
                VStack(alignment: .leading, spacing: 6) {
                    Text(l.backupWarning)
                        .font(.caption2.weight(.medium))
                        .foregroundStyle(.orange)
                    HStack(spacing: 8) {
                        Text(priv)
                            .font(.system(.caption2, design: .monospaced))
                            .textSelection(.enabled)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Spacer()
                        Button(copiedKey ? l.copied : l.copy) {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(priv, forType: .string)
                            copiedKey = true
                            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copiedKey = false }
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.mini)
                    }
                }
                .padding(10)
                .background(.orange.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
            }
        }
    }

    @ViewBuilder
    private func poolSection(_ l: L10n) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(l.step2Title)
                .font(.headline)

            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                ForEach(PoolKind.allCases) { pool in
                    Button {
                        selectedPool = pool
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(pool.displayName)
                                    .font(.subheadline.weight(.medium))
                                    .foregroundStyle(.primary)
                                Text(pool.endpointSubtitle)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            if selectedPool == pool {
                                Image(systemName: "checkmark.circle.fill")
                                    .foregroundStyle(Color.accentColor)
                            }
                        }
                        .padding(10)
                        .background(
                            selectedPool == pool
                                ? Color.accentColor.opacity(0.12)
                                : Color(nsColor: .textBackgroundColor).opacity(0.5),
                            in: RoundedRectangle(cornerRadius: 10)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 10)
                                .stroke(
                                    selectedPool == pool ? Color.accentColor.opacity(0.5) : Color.primary.opacity(0.08),
                                    lineWidth: 1
                                )
                        )
                    }
                    .buttonStyle(.plain)
                }
            }

            if selectedPool == .custom {
                HStack(spacing: 10) {
                    TextField(l.customHost, text: $customHost)
                        .textFieldStyle(.roundedBorder)
                    TextField(l.customPort, text: $customPort)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 80)
                    Picker(l.dialectLabel, selection: $customDialect) {
                        ForEach(DialectKind.allCases) { d in
                            Text(d.displayName).tag(d)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 120)
                }
            }

            HStack(spacing: 10) {
                Text(l.workerLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                TextField("mac", text: $workerInput)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 200)
                Spacer()

                if vm.wallet != nil {
                    Button {
                        applyPoolConfig()
                        vm.revealedPrivateKey = nil
                        onContinueToDashboard()
                    } label: {
                        Text(vm.language == .en ? "Dashboard" : "进入面板")
                    }
                    .buttonStyle(.bordered)
                }

                Button {
                    applyPoolConfig()
                    vm.revealedPrivateKey = nil
                    vm.startMining()
                    onContinueToDashboard()
                } label: {
                    Label(l.startMining, systemImage: "play.fill")
                        .padding(.horizontal, 8)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(vm.wallet == nil)
            }
        }
        .padding(16)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 14))
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(Color.primary.opacity(0.08), lineWidth: 1)
        )
        .opacity(vm.wallet == nil ? 0.65 : 1.0)
    }

    private func applyPoolConfig() {
        var next = vm.config
        next.pool = selectedPool
        let trimmedWorker = workerInput.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmedWorker.isEmpty {
            next.worker = trimmedWorker
        }
        if selectedPool == .custom {
            let trimmedHost = customHost.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmedHost.isEmpty { next.customHost = trimmedHost }
            if let port = Int(customPort), port > 0, port <= 65535 { next.customPort = port }
            next.customDialect = customDialect
        }
        vm.saveConfig(next)
    }
}
