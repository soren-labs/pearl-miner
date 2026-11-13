import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct WalletSheet: View {
    @ObservedObject var vm: MinerViewModel
    @Environment(\.dismiss) private var dismiss

    @State private var showSwitchSection = false
    @State private var switchMode: SwitchMode = .create
    @State private var keyInput = ""
    @State private var addressInput = ""
    @State private var showFileImporter = false
    @State private var copiedAddr = false
    @State private var copiedPriv = false

    enum SwitchMode: String, CaseIterable, Identifiable {
        case create, importKey, importFile, addressOnly
        var id: String { rawValue }
    }

    var body: some View {
        let l = vm.l10n
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(l.walletTitle)
                    .font(.title3.weight(.semibold))
                Spacer()
                Button(l.done) {
                    vm.revealedPrivateKey = nil
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
            }

            if let error = vm.errorMessage { Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            if let notice = vm.walletNotice { Text(notice).font(.caption).foregroundStyle(.green) }
            if let wallet = vm.wallet {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text(l.currentAddress)
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.secondary)
                        Spacer()
                        Text(wallet.hasPrivateKey ? l.fullKeyBadge : l.watchOnlyBadge)
                            .font(.caption2.weight(.medium))
                            .padding(.horizontal, 8)
                            .padding(.vertical, 3)
                            .background(
                                (wallet.hasPrivateKey ? Color.green : Color.blue).opacity(0.12),
                                in: Capsule()
                            )
                            .foregroundStyle(wallet.hasPrivateKey ? .green : .blue)
                    }

                    HStack(spacing: 8) {
                        Text(wallet.address)
                            .lineLimit(1)
                            .minimumScaleFactor(0.8)
                            .truncationMode(.middle)
                            .help(wallet.address)
                            .font(.system(.caption, design: .monospaced))
                            .textSelection(.enabled)
                        Spacer()
                        Button(copiedAddr ? l.copied : l.copy) {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(wallet.address, forType: .string)
                            copiedAddr = true
                            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copiedAddr = false }
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }

                    HStack(spacing: 10) {
                        Button(l.exportBackup) { vm.exportWallet() }.controlSize(.small)
                        Button(l.showWalletFiles) { vm.showWalletBackups() }.controlSize(.small)
                    }
                    HStack(spacing: 10) {
                        if wallet.hasPrivateKey {
                            Button(vm.revealedPrivateKey == nil ? l.revealPrivateKey : l.hidePrivateKey) {
                                if vm.revealedPrivateKey == nil {
                                    vm.revealPrivateKey()
                                } else {
                                    vm.revealedPrivateKey = nil
                                }
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                        }

                        if let url = poolDashboardURL(address: wallet.address) {
                            Link(destination: url) {
                                Label(l.viewOnPool, systemImage: "arrow.up.right.square")
                                    .font(.caption)
                            }
                        }
                    }

                    if let priv = vm.revealedPrivateKey {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(l.backupWarning)
                                .font(.caption2)
                                .foregroundStyle(.orange)
                            HStack(spacing: 8) {
                                Text(priv)
                                    .font(.system(.caption2, design: .monospaced))
                                    .textSelection(.enabled)
                                Spacer()
                                Button(copiedPriv ? l.copied : l.copy) {
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(priv, forType: .string)
                                    copiedPriv = true
                                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copiedPriv = false }
                                }
                                .buttonStyle(.bordered)
                                .controlSize(.mini)
                            }
                        }
                        .padding(10)
                        .background(.orange.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
                    }
                }
                .padding(14)
                .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
            }

            DisclosureGroup(l.switchOrImport, isExpanded: $showSwitchSection) {
                VStack(alignment: .leading, spacing: 12) {
                    if vm.isMining {
                        Text(l.stopToEditPool)
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }

                    Picker("", selection: $switchMode) {
                        Text(l.createNewWallet).tag(SwitchMode.create)
                        Text(l.importPrivateKey).tag(SwitchMode.importKey)
                        Text(l.importWalletFile).tag(SwitchMode.importFile)
                        Text(l.connectAddressOnly).tag(SwitchMode.addressOnly)
                    }
                    .pickerStyle(.menu)
                    .labelsHidden()
                    .disabled(vm.isMining)

                    switch switchMode {
                    case .create:
                        HStack {
                            Text(l.createWalletDesc)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Spacer()
                            Button(l.createNewWallet) {
                                vm.createWallet()
                            }
                            .buttonStyle(.borderedProminent)
                            .disabled(vm.isMining)
                        }

                    case .importKey:
                        HStack {
                            SecureField(l.privateKeyPlaceholder, text: $keyInput)
                                .textFieldStyle(.roundedBorder)
                                .font(.system(.body, design: .monospaced))
                            Button(l.save) {
                                vm.importPrivateKey(keyInput.trimmingCharacters(in: .whitespacesAndNewlines))
                                keyInput = ""
                            }
                            .buttonStyle(.borderedProminent)
                            .disabled(vm.isMining || keyInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        }

                    case .importFile:
                        HStack {
                            Spacer()
                            Button(l.importWalletFile) {
                                showFileImporter = true
                            }
                            .buttonStyle(.borderedProminent)
                            .disabled(vm.isMining)
                        }

                    case .addressOnly:
                        HStack {
                            TextField(l.addressPlaceholder, text: $addressInput)
                                .textFieldStyle(.roundedBorder)
                                .font(.system(.body, design: .monospaced))
                            Button(l.save) {
                                vm.connectAddress(addressInput.trimmingCharacters(in: .whitespacesAndNewlines))
                                addressInput = ""
                            }
                            .buttonStyle(.borderedProminent)
                            .disabled(vm.isMining || addressInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        }
                    }
                }
                .padding(.top, 8)
            }
            .padding(14)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))

            Spacer(minLength: 0)
        }
        .padding(20)
        .frame(width: 500)
        .alert(l.replaceWalletTitle, isPresented: $vm.confirmWalletReplacement) {
            Button(l.cancel, role: .cancel) { vm.cancelWalletReplacement() }
            Button(l.replaceWallet, role: .destructive) { vm.replaceWalletConfirmed() }
        } message: { Text(l.replaceWalletMessage) }
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

    private func poolDashboardURL(address: String) -> URL? {
        switch vm.config.pool {
        case .luckypool:
            return URL(string: "https://pearl.luckypool.io/?wallet=\(address)")
        case .kryptexEu, .kryptexUs:
            return URL(string: "https://pool.kryptex.com/prl/miner/stats/\(address)")
        case .custom:
            return nil
        }
    }
}
