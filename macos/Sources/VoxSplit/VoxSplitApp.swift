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

/// Paints the window itself in the app's background, so no system grey shows: not in the
/// hidden title bar, and not in any space outside the content (e.g. while the window
/// resizes to fit a different tool's settings).
struct WindowBackground: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        // The view has no window until it's on screen.
        DispatchQueue.main.async { [weak view] in configure(view?.window) }
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        configure(view.window)
    }

    private func configure(_ window: NSWindow?) {
        guard let window else { return }
        window.backgroundColor = NSColor(Theme.background)
        window.titlebarAppearsTransparent = true
    }
}
