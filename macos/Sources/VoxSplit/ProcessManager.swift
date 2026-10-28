import AVFoundation
import Foundation
import Observation
import os
import VoxSplitKit

/// Owns one separation at a time: launches the engine through `EngineRunner`, turns its
/// event stream into observable state for the UI, and maps the exit into a final phase.
///
/// Main-actor isolated (the module default). The engine runs in a child process and its
/// output is read off the main actor; only the small state updates hop back here.
@Observable
final class ProcessManager {
    enum Phase: Equatable {
        case idle, preparing, running, finished, cancelled
        case failed(String)

        var isActive: Bool { self == .preparing || self == .running }
    }

    struct Stem: Identifiable, Equatable {
        let kind: EngineEvent.Stem
        let url: URL
        var id: URL { url }
    }

    struct LogLine: Identifiable {
        let id: Int
        let text: String
        let isError: Bool
    }

    static let supportedExtensions: Set<String> = ["wav", "wave", "aif", "aiff", "flac", "mp3", "m4a", "aac", "ogg"]
    private static let maxLogLines = 500

    private(set) var input: URL?
    private(set) var inputDetails = ""
    private(set) var phase: Phase = .idle
    private(set) var stage: EngineEvent.Stage?
    private(set) var stageIndex = 0
    private(set) var stageCount = 2
    private(set) var progress = 0.0
    private(set) var device: String?
    private(set) var stems: [Stem] = []
    private(set) var elapsed: Double?
    private(set) var log: [LogLine] = []
    /// Expected seconds per quality for the chosen file (`nil` while estimating).
    private(set) var estimates: (best: Double, fast: Double)?
    /// Seconds left in the current run, as the engine last reported.
    private(set) var eta: Double?

    /// Remembered across launches.
    var quality: Quality = Quality(rawValue: UserDefaults.standard.string(forKey: "quality") ?? "") ?? .best {
        didSet { UserDefaults.standard.set(quality.rawValue, forKey: "quality") }
    }

    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var estimateTask: Task<Void, Never>?
    @ObservationIgnored private var nextLogID = 0
    @ObservationIgnored private var sawDone = false
    @ObservationIgnored private var sawCancelled = false
    @ObservationIgnored private var engineError: String?

    static func isSupported(_ url: URL) -> Bool {
        supportedExtensions.contains(url.pathExtension.lowercased())
    }

    /// Selects a new song. Returns false (and changes nothing) for unsupported files or
    /// while a run is active.
    @discardableResult
    func choose(_ url: URL) -> Bool {
        guard !phase.isActive, url.isFileURL, Self.isSupported(url) else { return false }
        input = url
        inputDetails = Self.describe(url)
        phase = .idle
        stems = []
        progress = 0
        elapsed = nil
        requestEstimate(for: url)
        return true
    }

    /// Asks the engine how long each quality will take; quick (header only).
    private func requestEstimate(for url: URL) {
        estimates = nil
        estimateTask?.cancel()
        guard let location = EngineLocator.locate() else { return }
        let runner = EngineRunner(executable: location.engine)
        let arguments = location.estimateArguments(input: url)
        estimateTask = Task {
            let result = OSAllocatedUnfairLock<(Double, Double)?>(initialState: nil)
            _ = try? await runner.run(arguments: arguments) { output in
                if case .event(.estimate(_, let best, let fast)) = output {
                    result.withLock { $0 = (best, fast) }
                }
            }
            guard !Task.isCancelled, input == url, let (best, fast) = result.withLock({ $0 }) else { return }
            estimates = (best, fast)
        }
    }

    func start() {
        guard let input, !phase.isActive else { return }
        guard let location = EngineLocator.locate() else {
            phase = .failed("VoxSplit's engine is missing from the app bundle. Reinstall VoxSplit.")
            return
        }

        phase = .preparing
        stage = nil
        stageIndex = 0
        progress = 0
        stems = []
        elapsed = nil
        sawDone = false
        sawCancelled = false
        engineError = nil
        appendLog("Separating \(input.lastPathComponent)", isError: false)

        let runner = EngineRunner(executable: location.engine)
        let arguments = location.separateArguments(
            input: input,
            outputFolder: EngineLocator.defaultOutputFolder(for: input),
            quality: quality
        )
        eta = nil

        task = Task {
            do {
                // The task already holds `self` until the run ends; the cycle through `task`
                // is broken when it's cleared below.
                let status = try await runner.run(arguments: arguments) { output in
                    await self.handle(output)
                }
                finish(status: status)
            } catch {
                phase = .failed("Couldn't launch the VoxSplit engine: \(error.localizedDescription)")
            }
            task = nil
        }
    }

    /// SIGTERMs the engine; it finishes the current chunk, removes partial files and exits.
    func cancel() {
        task?.cancel()
    }

    private func handle(_ output: EngineOutput) {
        switch output {
        case .log(let line):
            appendLog(line, isError: false)
        case .event(let event):
            apply(event)
        }
    }

    private func apply(_ event: EngineEvent) {
        switch event {
        case .start(let stages, _, _, let estimate):
            stageCount = stages
            eta = estimate
        case .stage(let name, let index, let stages, let model, let device):
            phase = .running
            stage = name
            stageIndex = index
            stageCount = stages
            self.device = device
            appendLog("Stage \(index): \(model) on \(device)", isError: false)
        case .progress(_, _, let overall, let secondsLeft):
            progress = overall
            if let secondsLeft { eta = secondsLeft }
        case .log(_, let message):
            appendLog(message, isError: false)
        case .output(let kind, let url):
            stems.append(Stem(kind: kind, url: url))
            appendLog("Wrote \(url.path(percentEncoded: false))", isError: false)
        case .done(let seconds):
            sawDone = true
            elapsed = seconds
        case .cancelled:
            sawCancelled = true
        case .error(_, let message):
            engineError = message
            appendLog("Error: \(message)", isError: true)
        case .estimate, .other:
            break
        }
    }

    private func finish(status: Int32) {
        if Task.isCancelled || sawCancelled || status == 130 {
            phase = .cancelled
            stems = []
        } else if sawDone && status == 0 {
            phase = .finished
            progress = 1
        } else {
            phase = .failed(engineError ?? "The engine stopped unexpectedly (exit code \(status)).")
            stems = []
        }
    }

    private func appendLog(_ text: String, isError: Bool) {
        log.append(LogLine(id: nextLogID, text: text, isError: isError))
        nextLogID += 1
        if log.count > Self.maxLogLines {
            log.removeFirst(log.count - Self.maxLogLines)
        }
    }

    /// "3:10  ·  44.1 kHz  ·  FLAC", read from the file header.
    private static func describe(_ url: URL) -> String {
        let format = url.pathExtension.uppercased()
        guard let file = try? AVAudioFile(forReading: url) else { return format }
        let rate = file.fileFormat.sampleRate
        guard rate > 0 else { return format }
        let seconds = Int((Double(file.length) / rate).rounded())
        let duration = "\(seconds / 60):" + String(format: "%02d", seconds % 60)
        let kHz = rate.truncatingRemainder(dividingBy: 1000) == 0
            ? "\(Int(rate / 1000)) kHz"
            : String(format: "%.1f kHz", rate / 1000)
        return [duration, kHz, format].joined(separator: "  ·  ")
    }
}
