import AVFoundation
import Foundation
import Testing
import os
@testable import VoxSplitKit

/// Runs the real engine from the repo (scripts/build-engine.sh + scripts/fetch-models.sh).
private let repo = URL(filePath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

private let repoEngine = EngineLocation(
    engine: repo.appending(path: "engine/target/release/voxsplit-engine"),
    modelsDir: repo.appending(path: "models", directoryHint: .isDirectory)
)

private let engineIsBuilt = FileManager.default.isExecutableFile(atPath: repoEngine.engine.path(percentEncoded: false))
    && FileManager.default.fileExists(atPath: repoEngine.modelsDir.appending(path: "BS_Roformer_Karaoke.onnx").path(percentEncoded: false))

/// Collects everything the runner reports, from whichever thread it arrives on.
private final class Recorder: Sendable {
    private let outputs = OSAllocatedUnfairLock<[EngineOutput]>(initialState: [])
    func record(_ output: EngineOutput) { outputs.withLock { $0.append(output) } }
    var events: [EngineEvent] {
        outputs.withLock { $0.compactMap { if case .event(let e) = $0 { e } else { nil } } }
    }
}

private func writeTestWav(to url: URL, seconds: Double) throws {
    let format = AVAudioFormat(standardFormatWithSampleRate: 44_100, channels: 2)!
    let file = try AVAudioFile(forWriting: url, settings: format.settings)
    let frames = AVAudioFrameCount(seconds * 44_100)
    let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
    buffer.frameLength = frames
    for i in 0..<Int(frames) {
        let v = 0.3 * sin(2 * Double.pi * 440 * Double(i) / 44_100)
        buffer.floatChannelData![0][i] = Float(v)
        buffer.floatChannelData![1][i] = Float(v * 0.8)
    }
    try file.write(from: buffer)
}

@Suite(.enabled(if: engineIsBuilt, "build the engine and fetch models first"), .serialized)
struct EngineRunnerTests {
    let dir: URL

    init() throws {
        dir = FileManager.default.temporaryDirectory.appending(path: "voxsplit-tests-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    @Test func `separates a file whose path needs no escaping`() async throws {
        let input = dir.appending(path: "-it's a \"test\" sång.wav")
        try writeTestWav(to: input, seconds: 8)
        let out = dir.appending(path: "out dir", directoryHint: .isDirectory)
        let recorder = Recorder()

        let status = try await EngineRunner(executable: repoEngine.engine)
            .run(arguments: repoEngine.separateArguments(input: input, outputFolder: out, quality: .fast, passes: 1)) { recorder.record($0) }

        #expect(status == 0)
        let events = recorder.events
        #expect(events.contains { if case .done = $0 { true } else { false } })
        let stems = events.compactMap { if case .output(let stem, let url) = $0 { (stem, url) } else { nil } }
        #expect(Set(stems.map(\.0)) == [.lead, .backing])
        for (_, url) in stems {
            #expect(FileManager.default.fileExists(atPath: url.path(percentEncoded: false)))
        }
    }

    @Test func `cancelling the task SIGTERMs the engine, which cleans up and exits 130`() async throws {
        let input = dir.appending(path: "long.wav")
        try writeTestWav(to: input, seconds: 90)
        let out = dir.appending(path: "cancel", directoryHint: .isDirectory)
        let recorder = Recorder()

        let run = Task {
            try await EngineRunner(executable: repoEngine.engine)
                .run(arguments: repoEngine.separateArguments(input: input, outputFolder: out, quality: .fast, passes: 1)) { recorder.record($0) }
        }
        // Wait until inference is underway, then cancel.
        for _ in 0..<600 where !recorder.events.contains(where: { if case .progress = $0 { true } else { false } }) {
            try await Task.sleep(for: .milliseconds(50))
        }
        run.cancel()
        let status = try await run.value

        #expect(status == 130, "status \(status); last events \(recorder.events.suffix(3))")
        #expect(recorder.events.contains(.cancelled))
        let leftovers = (try? FileManager.default.contentsOfDirectory(atPath: out.path(percentEncoded: false))) ?? []
        #expect(!leftovers.contains { $0.hasSuffix(".partial") })
    }

    @Test func `estimate answers quickly with both qualities`() async throws {
        let input = dir.appending(path: "estimate me.wav")
        try writeTestWav(to: input, seconds: 20)
        let recorder = Recorder()
        let status = try await EngineRunner(executable: repoEngine.engine)
            .run(arguments: repoEngine.estimateArguments(input: input)) { recorder.record($0) }
        #expect(status == 0)
        let estimate = recorder.events.compactMap {
            if case .estimate(let e) = $0 { (e.durationSec, e.bestSec, e.fastSec) } else { nil }
        }.first
        let (duration, best, fast) = try #require(estimate)
        #expect(abs(duration - 20) < 0.1)
        #expect(best > fast && fast > 0)
    }

    @Test func `bad arguments exit 2 with an error event`() async throws {
        let recorder = Recorder()
        let status = try await EngineRunner(executable: repoEngine.engine)
            .run(arguments: ["separate", "--overlap=7"]) { recorder.record($0) }
        #expect(status == 2)
        #expect(recorder.events.contains { if case .error("bad_args", _) = $0 { true } else { false } })
    }

    @Test func `a missing executable throws instead of hanging`() async {
        await #expect(throws: (any Error).self) {
            _ = try await EngineRunner(executable: URL(filePath: "/nonexistent/voxsplit-engine"))
                .run(arguments: ["probe"]) { _ in }
        }
    }
}
