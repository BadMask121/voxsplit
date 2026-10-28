import AppKit
import SwiftUI

@main
struct VoxSplitApp: App {
    @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate
    @State private var manager = ProcessManager()
    @State private var player = PreviewPlayer()

    var body: some Scene {
        Window("VoxSplit", id: "main") {
            ContentView()
                .environment(manager)
                .environment(player)
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}
