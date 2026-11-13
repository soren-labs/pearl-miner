import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
    var onTerminate: (() -> Void)?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationWillTerminate(_ notification: Notification) {
        onTerminate?()
    }
}

@main
struct PearlMinerApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var vm = MinerViewModel()
    @State private var showOnboarding = false
    @State private var initializedRoute = false
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        Window("PearlMiner", id: "main") {
            Group {
                if !vm.hasLoadedInitialState && vm.errorMessage == nil {
                    VStack(spacing: 12) {
                        PearlOrbLogo(size: 48)
                        ProgressView()
                            .controlSize(.small)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if showOnboarding || vm.wallet == nil {
                    OnboardingView(vm: vm) {
                        showOnboarding = false
                    }
                } else {
                    MainView(vm: vm)
                }
            }
            .frame(minWidth: 540, maxWidth: 620, minHeight: 520, maxHeight: 740)
            .onAppear { [vm] in
                appDelegate.onTerminate = {
                    vm.shutdown()
                }
            }
            .onChange(of: vm.hasLoadedInitialState) { _, ready in
                if ready && !initializedRoute {
                    initializedRoute = true
                    showOnboarding = (vm.wallet == nil)
                }
            }
        }
        .windowResizability(.contentSize)

        MenuBarExtra {
            menuBarContent
        } label: {
            HStack(spacing: 4) {
                Image(systemName: vm.isMining ? "circle.inset.filled" : "circle")
                if vm.isMining && vm.equivThs > 0.01 {
                    Text(String(format: "%.1f TH/s", vm.equivThs))
                }
            }
        }
    }

    @ViewBuilder
    private var menuBarContent: some View {
        let l = vm.l10n
        if vm.isMining {
            Text("\(l.statusMining): \(String(format: "%.2f TH/s", vm.equivThs))")
            Button(l.stopMining) {
                vm.stopMining()
            }
        } else {
            Text(l.statusReady)
            Button(l.startMining) {
                vm.startMining()
            }
            .disabled(vm.wallet == nil)
        }
        Divider()
        Button(vm.language == .en ? "Open PearlMiner" : "打开 PearlMiner") {
            NSApp.activate(ignoringOtherApps: true)
            openWindow(id: "main")
        }
        Divider()
        Button(vm.language == .en ? "Quit PearlMiner" : "退出 PearlMiner") {
            vm.shutdown()
            NSApp.terminate(nil)
        }
        .keyboardShortcut("q")
    }
}
