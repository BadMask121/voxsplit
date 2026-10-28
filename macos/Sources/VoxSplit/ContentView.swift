import SwiftUI
import UniformTypeIdentifiers
import VoxSplitKit

/// Drop a song, press Separate, preview or drag the stems into a project.
struct ContentView: View {
    @Environment(ProcessManager.self) private var manager
    @Environment(PreviewPlayer.self) private var player

    @State private var showingImporter = false
    @State private var showLog = false
    @State private var rejection: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
                .padding(.bottom, 16)

            DropZoneView(
                file: manager.input,
                details: manager.inputDetails,
                rejection: rejection,
                onChoose: { showingImporter = true },
                onDrop: choose
            )
            .frame(height: 128)
            .disabled(manager.phase.isActive)
            .padding(.bottom, 14)

            settingsBlock
                .padding(.bottom, 14)

            actionRow
                .frame(height: 36)
                .padding(.bottom, 20)

            ProgressStrip(mode: progressMode, fraction: manager.progress, label: progressLabel, detail: progressDetail)
                .frame(height: 26)
                .opacity(showProgress ? 1 : 0)
                .animation(Theme.easeOut(showProgress ? Theme.fade : Theme.hoverOut), value: showProgress)
                .padding(.bottom, 16)

            stemsArea
                .frame(height: 52 * 2 + 8)
                .padding(.bottom, 12)

            Button(showLog ? "Hide log" : "Show log") { showLog.toggle() }
                .buttonStyle(.plain)
                .font(.system(size: 12.5))
                .foregroundStyle(Theme.textDim)

            if showLog {
                console
                    .frame(height: 140)
                    .padding(.top, 8)
            }
        }
        .padding(.horizontal, 24)
        .padding(.top, 34) // clear the traffic lights in the hidden title bar
        .padding(.bottom, 22)
        .frame(width: 540)
        .background(Theme.background)
        .preferredColorScheme(.dark)
        .fileImporter(isPresented: $showingImporter, allowedContentTypes: [.audio]) { result in
            if case .success(let url) = result { choose(url) }
        }
        .onOpenURL(perform: choose)
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.space) {
            // Space previews the lead, or stops whatever is playing. Keyboard: no animation.
            guard let lead = manager.stems.first(where: { $0.kind == .lead || $0.kind == .vocals }) else { return .ignored }
            if player.playing != nil { player.stop() } else { player.play(lead.url) }
            return .handled
        }
        .background {
            Button("Open…") { showingImporter = true }
                .keyboardShortcut("o", modifiers: .command)
                .opacity(0)
                .allowsHitTesting(false)
        }
    }

    // MARK: Sections

    private var header: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 3) {
                Text("VoxSplit")
                    .font(.system(size: 18, weight: .bold))
                    .foregroundStyle(Theme.text)
                Text("Lead and backing vocal separation, on this Mac")
                    .font(.system(size: 12.5))
                    .foregroundStyle(Theme.textDim)
            }
            Spacer()
            DeviceChip(device: manager.device)
        }
    }

    private var actionRow: some View {
        HStack(spacing: 16) {
            Button(action: primaryAction) {
                // The label crossfades (with a light blur) between Separate and Cancel.
                ZStack {
                    Text(primaryLabel)
                        .id(primaryLabel)
                        .transition(.blurFade)
                }
            }
            .buttonStyle(PressableStyle(kind: manager.phase.isActive ? .ghost : .primary))
            .frame(width: 128)
            .disabled(manager.input == nil && !manager.phase.isActive)
            .keyboardShortcut(manager.phase.isActive ? .cancelAction : .defaultAction)
            .animation(Theme.easeOut(Theme.fade), value: primaryLabel)

            Text(status)
                .font(.system(size: 12.5))
                .foregroundStyle(statusColour)
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentTransition(.opacity)
                .animation(Theme.easeOut(Theme.fade), value: status)
        }
    }

    /// Quality, then two sliders: how many times vocal extraction runs, and how
    /// many times the lead/backing model (whichever the quality uses) runs.
    private var settingsBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            settingLabel("Tool") {
                SegmentedPicker(selection: Bindable(manager).tool,
                                options: [(.leadBacking, "Lead & Backing"), (.vocalRemover, "Vocal Remover")],
                                accessibilityName: "Tool")
                    .frame(width: 248, height: 32)
            }
            if manager.tool == .leadBacking {
                settingLabel("Quality") {
                    SegmentedPicker(selection: Bindable(manager).quality,
                                    options: [(.best, "Best"), (.fast, "Fast")],
                                    accessibilityName: "Quality")
                        .frame(width: 124, height: 32)
                }
                PassesRow(name: "Vocal passes", caption: "Kim Vocal 2 · 0 = file is vocals", range: 0...5,
                          value: Bindable(manager).vocalPasses)
                PassesRow(name: "Refinement passes",
                          caption: manager.quality == .best ? "BS-RoFormer (Best)" : "MDX Karaoke 2 (Fast)",
                          range: 1...5, value: Bindable(manager).passes)
            } else {
                // The vocal remover is the vocal stage on its own: at least one pass.
                PassesRow(name: "Vocal passes", caption: "Kim Vocal 2", range: 1...5,
                          value: Binding(get: { manager.effectiveVocalPasses },
                                         set: { manager.vocalPasses = $0 }))
            }
        }
        .disabled(manager.phase.isActive)
    }

    private func settingLabel(_ title: String, @ViewBuilder control: () -> some View) -> some View {
        HStack(spacing: 0) {
            Text(title)
                .font(.system(size: 12.5))
                .foregroundStyle(Theme.textDim)
                .frame(width: 150, alignment: .leading)
            control()
        }
    }

    @ViewBuilder private var stemsArea: some View {
        // Each tool shows its own pair, in a fixed order.
        let order: [EngineEvent.Stem] = manager.tool == .vocalRemover ? [.vocals, .instrumental] : [.lead, .backing]
        let shown = order.compactMap { kind in manager.stems.first { $0.kind == kind } }
        if manager.phase == .finished && !shown.isEmpty {
            VStack(spacing: 8) {
                ForEach(Array(shown.enumerated()), id: \.element.id) { index, stem in
                    StemRowView(stem: stem, index: index)
                }
            }
        } else {
            RoundedRectangle(cornerRadius: 10)
                .strokeBorder(Theme.border, style: StrokeStyle(lineWidth: 1, dash: [3, 4]))
                .overlay {
                    Text(manager.tool == .vocalRemover ? "Vocals and instrumental will appear here"
                                                       : "Lead and backing stems will appear here")
                        .font(.system(size: 12.5))
                        .foregroundStyle(Theme.textDim.opacity(0.7))
                }
        }
    }

    private var console: some View {
        ScrollViewReader { proxy in
            ScrollView {
                // Log lines are frequent: they never animate.
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(manager.log) { line in
                        Text(line.text)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(line.isError ? Theme.danger : Theme.textDim)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .id(line.id)
                    }
                }
                .padding(10)
            }
            .background(RoundedRectangle(cornerRadius: 10).fill(Theme.surface))
            .onChange(of: manager.log.last?.id) { _, id in
                if let id { proxy.scrollTo(id, anchor: .bottom) }
            }
        }
    }

    // MARK: Actions

    private func choose(_ url: URL) {
        if manager.phase.isActive { return }
        if manager.choose(url) {
            player.stop()
            rejection = nil
        } else {
            rejection = "VoxSplit can't read \(url.pathExtension.isEmpty ? "that file" : "." + url.pathExtension + " files")"
            Task {
                try? await Task.sleep(for: .seconds(3))
                rejection = nil
            }
        }
    }

    private func primaryAction() {
        if manager.phase.isActive {
            manager.cancel()
        } else if manager.input == nil {
            showingImporter = true
        } else {
            player.stop() // the stems being previewed are about to be replaced
            manager.start()
        }
    }

    // MARK: Derived state

    private var primaryLabel: String {
        switch manager.phase {
        case .preparing, .running: "Cancel"
        case .failed: "Try again"
        default: "Separate"
        }
    }

    private var showProgress: Bool {
        manager.phase.isActive || manager.phase == .finished
    }

    private var progressMode: ProgressStrip.Mode {
        switch manager.phase {
        case .running: .determinate
        case .finished: .done
        default: .indeterminate
        }
    }

    private var progressLabel: String {
        switch manager.phase {
        case .preparing: "Loading models"
        case .running:
            switch manager.stage {
            case .vocals: "Isolating vocals"
            case .vocalsRefine: "Cleaning up the vocals"
            case .refine: "Cleaning up the lead"
            default: "Splitting lead from backing"
            }
        case .finished: "Done"
        default: ""
        }
    }

    private var progressDetail: String {
        switch manager.phase {
        case .preparing:
            manager.eta.map { "\(aboutDuration($0)) in total" } ?? ""
        case .running:
            "Stage \(manager.stageIndex) of \(manager.stageCount)   ·   \(Int((manager.progress * 100).rounded()))%"
                + (manager.eta.map { "   ·   \(timeLeft($0))" } ?? "")
        case .finished:
            manager.elapsed.map { String(format: "%.1f s on %@", $0, manager.device == "cpu" ? "CPU" : "CoreML") } ?? ""
        default:
            ""
        }
    }

    private var status: String {
        switch manager.phase {
        case .failed(let message): message
        case .cancelled: "Cancelled. Nothing was saved."
        case .finished: "Saved to Music / VoxSplit / \(manager.input?.deletingPathExtension().lastPathComponent ?? "")"
        case .idle:
            if manager.input == nil {
                ""
            } else if let seconds = manager.estimatedSeconds {
                manager.tool == .leadBacking && manager.vocalPasses == 0
                    ? "Takes \(aboutDuration(seconds)). Vocal passes is 0: the file must be vocals only."
                    : "Takes \(aboutDuration(seconds)) with these settings"
            } else {
                "Estimating time…"
            }
        case .preparing, .running: ""
        }
    }

    private var statusColour: Color {
        if case .failed = manager.phase { return Theme.danger }
        return Theme.textDim
    }
}
