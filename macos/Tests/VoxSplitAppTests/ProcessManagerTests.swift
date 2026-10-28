import AVFoundation
import Foundation
import Testing
@testable import VoxSplit
import VoxSplitKit

/// Drives the app's state machine against the real engine, without any UI.
private nonisolated let repo = URL(filePath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
private nonisolated let engine = repo.appending(path: "engine/target/release/voxsplit-engine")
private nonisolated let models = repo.appending(path: "models", directoryHint: .isDirectory)
private nonisolated let engineIsBuilt = FileManager.default.isExecutableFile(atPath: engine.path(percentEncoded: false))
    && FileManager.default.fileExists(atPath: models.appending(path: "BS_Roformer_Karaoke.onnx").path(percentEncoded: false))

@Suite(.enabled(if: engineIsBuilt, "build the engine and fetch models first"), .serialized)
struct ProcessManagerTests {
    let input: URL

    init() throws {
        setenv("VOXSPLIT_ENGINE", engine.path(percentEncoded: false), 1)
        setenv("VOXSPLIT_MODELS", models.path(percentEncoded: false), 1)
        // A uniquely named input keeps each run's output folder separate.
        let dir = FileManager.default.temporaryDirectory.appending(path: "voxsplit-app-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        input = dir.appending(path: "voxsplit-test-\(UUID().uuidString.prefix(8)).wav")
        let format = AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 1)!
        let file = try AVAudioFile(forWriting: input, settings: format.settings)
        let frames = AVAudioFrameCount(48_000 * 30)
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
        buffer.frameLength = frames
        for i in 0..<Int(frames) { buffer.floatChannelData![0][i] = Float(0.3 * sin(Double(i) * 0.0576)) }
        try file.write(from: buffer)
    }

    private func cleanUp() {
        try? FileManager.default.removeItem(at: EngineLocator.defaultOutputFolder(for: input))
    }

    @Test func `rejects unsupported files and describes supported ones`() {
        let manager = ProcessManager()
        #expect(!manager.choose(URL(filePath: "/tmp/notes.txt")))
        #expect(manager.input == nil)
        #expect(manager.choose(input))
        #expect(manager.inputDetails.hasPrefix("0:30"))
        #expect(manager.inputDetails.contains("48 kHz"))
    }

    @Test func `choosing a file estimates both qualities`() async throws {
        let manager = ProcessManager()
        manager.choose(input)
        #expect(manager.estimates == nil)
        for _ in 0..<200 where manager.estimates == nil {
            try await Task.sleep(for: .milliseconds(25))
        }
        let estimates = try #require(manager.estimates)
        #expect(estimates.bestSec > estimates.fastSec && estimates.fastSec > 0)
        #expect(estimates.total(.fast, passes: 3, vocalPasses: 1) > estimates.total(.fast, passes: 1, vocalPasses: 1),
                "extra passes add time")
    }

    @Test func `a run reports live progress, then two stems`() async throws {
        defer { cleanUp() }
        let manager = ProcessManager()
        manager.quality = .fast
        manager.passes = 1
        manager.choose(input)
        manager.start()
        #expect(manager.phase == .preparing)

        var sawMidRunProgress = false
        var sawCountdown = false
        for _ in 0..<1200 where manager.phase.isActive {
            if manager.phase == .running, manager.progress > 0, manager.progress < 1 { sawMidRunProgress = true }
            if manager.phase == .running, let eta = manager.eta, eta > 0 { sawCountdown = true }
            try await Task.sleep(for: .milliseconds(25))
        }

        #expect(manager.phase == .finished)
        #expect(sawMidRunProgress, "progress must update while the engine runs, not only at the end")
        #expect(sawCountdown, "a time-left estimate is shown while running")
        #expect(manager.progress == 1)
        #expect(manager.device != nil)
        #expect(Set(manager.stems.map(\.kind)) == [.lead, .backing])
        #expect((manager.elapsed ?? 0) > 0)
    }

    @Test func `vocal remover returns vocals and instrumental`() async throws {
        defer { cleanUp() }
        let manager = ProcessManager()
        manager.tool = .vocalRemover
        defer { manager.tool = .leadBacking }
        manager.vocalPasses = 0 // the remover still runs the vocal stage once
        defer { manager.vocalPasses = 1 }
        #expect(manager.effectiveVocalPasses == 1)
        manager.choose(input)
        manager.start()
        for _ in 0..<1200 where manager.phase.isActive {
            try await Task.sleep(for: .milliseconds(25))
        }
        #expect(manager.phase == .finished)
        #expect(Set(manager.stems.map(\.kind)) == [.vocals, .instrumental])
    }

    @Test func `passes are remembered per quality`() {
        let manager = ProcessManager()
        manager.quality = .best
        manager.passes = 2
        manager.quality = .fast
        manager.passes = 4
        manager.quality = .best
        #expect(manager.passes == 2)
        manager.quality = .fast
        #expect(manager.passes == 4)
        manager.passes = 9
        #expect(manager.passes == 5, "clamped to 1...5")
        manager.vocalPasses = 0
        #expect(manager.vocalPasses == 0, "0 = file is already vocals")
        manager.vocalPasses = 1
    }

    @Test func `cancel ends in cancelled with no stems`() async throws {
        defer { cleanUp() }
        let manager = ProcessManager()
        manager.quality = .fast
        manager.choose(input)
        manager.start()
        for _ in 0..<1200 where manager.phase != .running {
            try await Task.sleep(for: .milliseconds(25))
        }
        manager.cancel()
        for _ in 0..<400 where manager.phase.isActive {
            try await Task.sleep(for: .milliseconds(25))
        }
        #expect(manager.phase == .cancelled)
        #expect(manager.stems.isEmpty)
    }
}
