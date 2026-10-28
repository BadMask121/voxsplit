import AppKit
import SwiftUI
import VoxSplitKit

// MARK: - Buttons

/// Answers the press: scales to 0.97 while held (fast in, slightly slower out) and warms on hover.
struct PressableStyle: ButtonStyle {
    enum Kind { case primary, ghost }
    var kind: Kind

    func makeBody(configuration: Configuration) -> some View {
        PressableBody(configuration: configuration, kind: kind)
    }
}

private struct PressableBody: View {
    let configuration: ButtonStyleConfiguration
    let kind: PressableStyle.Kind
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovering = false

    var body: some View {
        configuration.label
            .font(.system(size: 13.5, weight: kind == .primary ? .semibold : .medium))
            .foregroundStyle(ink)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(RoundedRectangle(cornerRadius: 8).fill(fill))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(outline, lineWidth: 1))
            .contentShape(RoundedRectangle(cornerRadius: 8))
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.97 : 1)
            .animation(.easeOut(duration: configuration.isPressed ? Theme.press : Theme.release), value: configuration.isPressed)
            .animation(.easeOut(duration: hovering ? Theme.hover : Theme.hoverOut), value: hovering)
            .animation(Theme.easeOut(Theme.fade), value: kind)
            .onHover { hovering = $0 && isEnabled }
    }

    private var fill: Color {
        switch kind {
        case .primary:
            // A washed-out white reads as broken; disabled is a quiet surface instead.
            isEnabled ? (hovering ? .white : Theme.text) : Theme.surfaceHi
        case .ghost:
            hovering ? Theme.surfaceHi : .clear
        }
    }

    private var ink: Color {
        switch kind {
        case .primary: isEnabled ? Theme.background : Theme.textDim.opacity(0.55)
        case .ghost: hovering ? Theme.text : Theme.textDim
        }
    }

    private var outline: Color {
        kind == .ghost ? (hovering ? Theme.textDim.opacity(0.5) : Theme.border) : .clear
    }
}

/// Crossfade with a 2 px blur, so a label swap reads as one shape changing rather than two
/// overlapping.
struct BlurFade: ViewModifier {
    let active: Bool
    func body(content: Content) -> some View {
        content.blur(radius: active ? 2 : 0).opacity(active ? 0 : 1)
    }
}

extension AnyTransition {
    static var blurFade: AnyTransition {
        .modifier(active: BlurFade(active: true), identity: BlurFade(active: false))
    }
}

struct PlayButton: View {
    let playing: Bool
    let accent: Color
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: playing ? "pause.fill" : "play.fill")
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(Theme.text)
                .contentTransition(.symbolEffect(.replace))
                .frame(width: 32, height: 32)
                .background(Circle().fill(Theme.surfaceHi))
                .background(Circle().fill(accent.opacity(playing ? 0.22 : hovering ? 0.10 : 0)))
                .overlay(Circle().strokeBorder(playing ? accent.opacity(0.65) : Theme.border, lineWidth: 1))
                .contentShape(Circle())
        }
        .buttonStyle(ScaleOnPress())
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: hovering ? Theme.hover : Theme.hoverOut), value: hovering)
        .animation(Theme.easeOut(Theme.fade), value: playing)
        .accessibilityLabel(playing ? "Stop preview" : "Preview")
    }
}

private struct ScaleOnPress: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.96 : 1)
            .animation(.easeOut(duration: configuration.isPressed ? Theme.press : Theme.release), value: configuration.isPressed)
    }
}

// MARK: - Quality toggle

/// Mutually exclusive options; the highlight slides between them
/// (on-screen movement: ease-in-out).
struct SegmentedPicker<Value: Hashable>: View {
    @Binding var selection: Value
    let options: [(Value, String)]
    var accessibilityName: String

    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Namespace private var pill

    var body: some View {
        HStack(spacing: 0) {
            ForEach(options, id: \.0) { option, label in
                let selected = option == selection
                Text(label)
                    .font(.system(size: 12.5, weight: .semibold))
                    .foregroundStyle(selected ? Theme.text : Theme.textDim)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background {
                        if selected {
                            RoundedRectangle(cornerRadius: 6)
                                .fill(Theme.surfaceHi)
                                .brightness(0.06)
                                .matchedGeometryEffect(id: "pill", in: pill)
                        }
                    }
                    .contentShape(Rectangle())
                    .onTapGesture {
                        guard isEnabled, !selected else { return }
                        withAnimation(reduceMotion ? nil : .timingCurve(0.77, 0, 0.175, 1, duration: 0.2)) {
                            selection = option
                        }
                    }
                    .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
            }
        }
        .padding(3)
        .background(RoundedRectangle(cornerRadius: 8).fill(Theme.surface))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Theme.border, lineWidth: 1))
        .opacity(isEnabled ? 1 : 0.45)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(accessibilityName)
    }
}

/// Discrete 1...5 slider. The thumb follows the pointer while dragging, then
/// springs onto the nearest tick; arrow keys step instantly.
struct PassesSlider: View {
    @Binding var value: Int
    var range: ClosedRange<Int> = 1...5

    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var dragPosition: Double?
    @State private var hovering = false

    private let inset: CGFloat = 9

    var body: some View {
        GeometryReader { geo in
            let span = max(1, geo.size.width - inset * 2)
            let steps = Double(range.upperBound - range.lowerBound)
            let position = dragPosition ?? Double(value - range.lowerBound)
            let x = { (p: Double) in inset + span * CGFloat(p / steps) }

            ZStack(alignment: .leading) {
                Capsule().fill(Theme.surfaceHi).frame(height: 4).padding(.horizontal, inset)
                Capsule().fill(Theme.text.opacity(0.85))
                    .frame(width: max(0, x(position) - inset), height: 4)
                    .offset(x: inset)
                ForEach(range, id: \.self) { tick in
                    Circle()
                        .fill((tick <= value ? Theme.text : Theme.textDim).opacity(0.9))
                        .frame(width: 3, height: 3)
                        .position(x: x(Double(tick - range.lowerBound)), y: geo.size.height / 2 + 7.5)
                }
                Circle()
                    .fill(hovering ? .white : Theme.text)
                    .frame(width: 14, height: 14)
                    .scaleEffect(dragPosition != nil && !reduceMotion ? 1.2 : 1)
                    .position(x: x(position), y: geo.size.height / 2)
                    .animation(.easeOut(duration: dragPosition != nil ? Theme.press : Theme.release), value: dragPosition != nil)
            }
            .contentShape(Rectangle())
            // A value set from outside (switching quality) glides; dragging doesn't animate.
            .animation(dragPosition == nil ? Theme.easeOut(Theme.fade) : nil, value: value)
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        guard isEnabled else { return }
                        // Attached to the pointer: no animation while dragging.
                        let p = min(max(Double((drag.location.x - inset) / span) * steps, 0), steps)
                        dragPosition = p
                        let nearest = range.lowerBound + Int(p.rounded())
                        if nearest != value { value = nearest }
                    }
                    .onEnded { _ in
                        // Release: spring onto the tick (interruptible if grabbed again).
                        withAnimation(reduceMotion ? nil : .spring(duration: 0.25, bounce: 0.15)) {
                            dragPosition = nil
                        }
                    }
            )
        }
        .opacity(isEnabled ? 1 : 0.45)
        .onHover { hovering = $0 && isEnabled }
        .animation(.easeOut(duration: hovering ? Theme.hover : Theme.hoverOut), value: hovering)
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.leftArrow) { step(-1) }
        .onKeyPress(.downArrow) { step(-1) }
        .onKeyPress(.rightArrow) { step(1) }
        .onKeyPress(.upArrow) { step(1) }
        .accessibilityElement()
        .accessibilityLabel("Refinement passes")
        .accessibilityValue("\(value)")
        .accessibilityAdjustableAction { direction in
            _ = step(direction == .increment ? 1 : -1)
        }
    }

    private func step(_ delta: Int) -> KeyPress.Result {
        guard isEnabled else { return .ignored }
        var instant = Transaction()
        instant.disablesAnimations = true // keyboard: no animation
        withTransaction(instant) {
            value = min(max(value + delta, range.lowerBound), range.upperBound)
        }
        return .handled
    }
}

/// A pass count: name and caption on the left, slider, value on the right.
struct PassesRow: View {
    let name: String
    let caption: String
    let range: ClosedRange<Int>
    @Binding var value: Int

    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 1) {
                Text(name)
                    .font(.system(size: 12.5, weight: .semibold))
                    .foregroundStyle(Theme.text)
                Text(caption)
                    .font(.system(size: 10.5))
                    .foregroundStyle(Theme.textDim)
                    .contentTransition(.opacity)
                    .animation(Theme.easeOut(Theme.fade), value: caption)
            }
            .frame(width: 150, alignment: .leading)
            PassesSlider(value: $value, range: range)
                .padding(.horizontal, 6)
            Text("\(value)")
                .font(.system(size: 13, weight: .semibold))
                .monospacedDigit()
                .foregroundStyle(Theme.text)
                .frame(width: 18, alignment: .trailing)
        }
        .frame(height: 34)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(name), \(caption)")
    }
}

/// "about 25 s", "about 2 min 10 s": rounded so the number doesn't flicker.
func aboutDuration(_ seconds: Double) -> String {
    if seconds < 60 { return "about \(max(5, Int((seconds / 5).rounded(.up)) * 5)) s" }
    let rounded = Int((seconds / 10).rounded()) * 10
    return "about \(rounded / 60) min" + (rounded % 60 > 0 ? " \(rounded % 60) s" : "")
}

/// Countdown while running: "1:05 left", "25 s left".
func timeLeft(_ seconds: Double) -> String {
    if seconds < 5 { return "a few seconds left" }
    if seconds < 60 { return "\(Int((seconds / 5).rounded(.up)) * 5) s left" }
    let s = Int(seconds.rounded())
    return "\(s / 60):" + String(format: "%02d", s % 60) + " left"
}

/// Speaker on/off: whether a stem is audible in the preview.
struct ListenToggle: View {
    let audible: Bool
    let accent: Color
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: audible ? "speaker.wave.2.fill" : "speaker.slash.fill")
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(audible ? Theme.text : Theme.textDim)
                .contentTransition(.symbolEffect(.replace))
                .frame(width: 32, height: 32)
                .background(Circle().fill(Theme.surfaceHi))
                .background(Circle().fill(accent.opacity(audible ? 0.18 : hovering ? 0.08 : 0)))
                .overlay(Circle().strokeBorder(audible ? accent.opacity(0.6) : Theme.border, lineWidth: 1))
                .contentShape(Circle())
        }
        .buttonStyle(ScaleOnPress())
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: hovering ? Theme.hover : Theme.hoverOut), value: hovering)
        .animation(Theme.easeOut(Theme.fade), value: audible)
        .accessibilityLabel(audible ? "Mute in preview" : "Unmute in preview")
    }
}

/// Play/pause for all stems together, the shared position, and a seek line.
struct TransportBarView: View {
    @Environment(PreviewPlayer.self) private var player
    @State private var dragFraction: Double?

    var body: some View {
        HStack(spacing: 10) {
            PlayButton(playing: player.isPlaying, accent: Theme.text) { player.togglePlay() }
            Text("\(clock(displayed * player.duration)) / \(clock(player.duration))")
                .font(.system(size: 12))
                .monospacedDigit()
                .foregroundStyle(Theme.textDim)
                .frame(width: 84, alignment: .leading)
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(Theme.surfaceHi).frame(height: 4)
                    Capsule().fill(Theme.text.opacity(0.85))
                        .frame(width: max(4, geo.size.width * displayed), height: 4)
                }
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag in dragFraction = fraction(drag.location.x, geo.size.width) }
                        .onEnded { drag in
                            player.seek(fraction(drag.location.x, geo.size.width))
                            dragFraction = nil
                        }
                )
            }
        }
        .frame(height: 28)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Preview all stems")
    }

    /// While scrubbing, the line follows the pointer; otherwise the playback position.
    private var displayed: Double { dragFraction ?? player.progress }

    private func fraction(_ x: CGFloat, _ width: CGFloat) -> Double {
        min(max(Double(x / max(width, 1)), 0), 1)
    }

    private func clock(_ seconds: Double) -> String {
        let s = max(0, Int(seconds))
        return "\(s / 60):" + String(format: "%02d", s % 60)
    }
}

// MARK: - Drop zone

struct DropZoneView: View {
    let file: URL?
    let details: String
    let rejection: String?
    let onChoose: () -> Void
    let onDrop: (URL) -> Void

    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovering = false
    @State private var targeted = false

    var body: some View {
        Button(action: onChoose) {
            VStack(spacing: 10) {
                Image(systemName: file == nil || targeted ? "arrow.down.to.line" : "waveform")
                    .font(.system(size: 24, weight: .regular))
                    .foregroundStyle(file == nil || targeted ? Theme.textDim : Theme.text)
                    .contentTransition(.symbolEffect(.replace))
                    .offset(y: targeted && !reduceMotion ? 3 : 0)
                    .frame(height: 30)
                    .padding(.bottom, 2)
                Text(headline)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.text)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(subline)
                    .font(.system(size: 12.5))
                    .foregroundStyle(rejection != nil && !targeted ? Theme.danger : Theme.textDim)
                    .lineLimit(1)
            }
            .padding(.horizontal, 20)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(RoundedRectangle(cornerRadius: 12).fill(fill))
            .overlay(border)
            .contentShape(RoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(.plain)
        .opacity(isEnabled ? 1 : 0.45)
        .onHover { hovering = $0 && isEnabled }
        .dropDestination(for: URL.self) { urls, _ in
            guard isEnabled, let url = urls.first else { return false }
            onDrop(url)
            return true
        } isTargeted: { targeted = $0 && isEnabled }
        .animation(.easeOut(duration: hovering || targeted ? Theme.hover : Theme.hoverOut), value: hovering)
        .animation(Theme.easeOut(Theme.hover), value: targeted)
        .accessibilityLabel(file == nil ? "Drop a song here, or click to choose one" : "Loaded \(file!.lastPathComponent). Click to replace.")
    }

    private var headline: String {
        if targeted { return "Release to load" }
        return file?.lastPathComponent ?? "Drop a song here"
    }

    private var subline: String {
        if targeted { return "VoxSplit will split its vocals" }
        if let rejection { return rejection }
        if file != nil { return "\(details)   ·   drop or click to replace" }
        return "or click to browse  ·  WAV, AIFF, FLAC, MP3, M4A"
    }

    private var fill: Color {
        targeted || hovering ? Theme.surfaceHi : Theme.surface
    }

    @ViewBuilder private var border: some View {
        let shape = RoundedRectangle(cornerRadius: 12)
        let colour = targeted ? Theme.text : (hovering ? Theme.textDim.opacity(0.6) : Theme.border)
        if file != nil && !targeted {
            shape.strokeBorder(colour, lineWidth: 1)
        } else {
            shape.strokeBorder(colour, style: StrokeStyle(lineWidth: 1.25, dash: [5, 4]))
        }
    }
}

// MARK: - Progress

struct ProgressStrip: View {
    enum Mode: Equatable { case indeterminate, determinate, done }
    let mode: Mode
    let fraction: Double
    let label: String
    let detail: String

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(label)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(mode == .done ? Theme.success : Theme.text)
                Spacer()
                Text(detail)
                    .font(.system(size: 12.5))
                    .monospacedDigit()
                    .foregroundStyle(Theme.textDim)
            }
            track.frame(height: 4)
        }
        .accessibilityElement(children: .combine)
        .accessibilityValue(mode == .determinate ? "\(Int(fraction * 100)) percent" : "")
    }

    private var track: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.surfaceHi)
                switch mode {
                case .indeterminate:
                    if reduceMotion {
                        Capsule().fill(Theme.textDim.opacity(0.5)).frame(width: geo.size.width * 0.3)
                    } else {
                        // Constant motion is linear: one sweep every 1.2 s.
                        TimelineView(.animation) { context in
                            let phase = context.date.timeIntervalSinceReferenceDate
                                .truncatingRemainder(dividingBy: 1.2) / 1.2
                            let segment = geo.size.width * 0.28
                            Capsule()
                                .fill(Theme.text.opacity(0.75))
                                .frame(width: segment)
                                .offset(x: -segment + (geo.size.width + segment) * phase)
                        }
                        .clipShape(Capsule())
                    }
                case .determinate:
                    Capsule()
                        .fill(Theme.text)
                        .frame(width: max(4, geo.size.width * fraction))
                        // Engine updates arrive per chunk; glide linearly between them.
                        .animation(.linear(duration: 0.25), value: fraction)
                case .done:
                    Capsule().fill(Theme.success)
                }
            }
        }
    }
}

// MARK: - Stem rows

struct StemRowView: View {
    let stem: ProcessManager.Stem
    let index: Int

    @Environment(PreviewPlayer.self) private var player
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var appeared = false
    @State private var hovering = false

    private var title: String {
        switch stem.kind {
        case .lead: "Lead vocals"
        case .backing: "Backing vocals"
        case .vocals: "Vocals"
        case .instrumental: "Instrumental"
        }
    }

    private var accent: Color {
        switch stem.kind {
        case .lead, .vocals: Theme.lead
        case .backing: Theme.backing
        case .instrumental: Theme.instrumental
        }
    }
    private var audible: Bool { player.isAudible(stem.url) }
    /// The hairline shows the shared position on stems you can hear.
    private var showsPosition: Bool { player.isPlaying && audible && player.loaded.contains(stem.url) }

    var body: some View {
        HStack(spacing: 12) {
            Circle().fill(accent).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Theme.text)
                Text("\(stem.url.lastPathComponent) · drag into your project")
                    .font(.system(size: 12))
                    .foregroundStyle(Theme.textDim)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 8)
            ListenToggle(audible: audible, accent: accent) { player.toggleMute(stem.url) }
            Button("Reveal") { NSWorkspace.shared.activateFileViewerSelecting([stem.url]) }
                .buttonStyle(PressableStyle(kind: .ghost))
                .frame(width: 76, height: 32)
        }
        .padding(.horizontal, 14)
        .frame(height: 52)
        .background(RoundedRectangle(cornerRadius: 10).fill(hovering ? Theme.surfaceHi : Theme.surface))
        .overlay(alignment: .bottomLeading) {
            // Playback position: a hairline along the bottom edge, moving linearly.
            GeometryReader { geo in
                Rectangle()
                    .fill(accent.opacity(0.9))
                    .frame(width: geo.size.width * (showsPosition ? player.progress : 0), height: 2)
                    .frame(maxHeight: .infinity, alignment: .bottom)
            }
            .opacity(showsPosition ? 1 : 0)
            .animation(.easeOut(duration: showsPosition ? Theme.fade : Theme.hoverOut), value: showsPosition)
        }
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .contentShape(RoundedRectangle(cornerRadius: 10))
        // A file promise of the WAV itself, so FL Studio's playlist, Finder or any DAW accept it.
        .onDrag { NSItemProvider(contentsOf: stem.url) ?? NSItemProvider() }
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: hovering ? Theme.hover : Theme.hoverOut), value: hovering)
        .opacity(appeared ? 1 : 0)
        .offset(y: appeared || reduceMotion ? 0 : 6)
        .onAppear {
            withAnimation(Theme.easeOut(Theme.enter).delay(Double(index) * Theme.stagger)) {
                appeared = true
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title)
    }
}

// MARK: - Device chip

struct DeviceChip: View {
    let device: String?

    var body: some View {
        HStack(spacing: 7) {
            Circle()
                .fill(device == nil || device == "cpu" ? Theme.textDim : Theme.success)
                .frame(width: 6, height: 6)
            Text(label)
                .font(.system(size: 11.5, weight: .semibold))
                .foregroundStyle(Theme.text)
        }
        .padding(.horizontal, 11)
        .frame(height: 24)
        .background(Capsule().fill(Theme.surface))
        .overlay(Capsule().strokeBorder(Theme.border, lineWidth: 1))
        .contentTransition(.opacity)
        .animation(Theme.easeOut(Theme.fade), value: device)
    }

    private var label: String {
        switch device {
        case "coreml": "CoreML"
        case "cuda": "CUDA"
        case "directml": "DirectML"
        case "cpu": "CPU"
        default: "Runs offline"
        }
    }
}
