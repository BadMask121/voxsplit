import AVFoundation
import Foundation
import Testing
@testable import VoxSplit

/// Plays silent stems, so a test run makes no sound.
@Suite(.serialized)
struct PreviewPlayerTests {
    let stems: [URL]

    init() throws {
        let dir = FileManager.default.temporaryDirectory.appending(path: "voxsplit-preview-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let format = AVAudioFormat(standardFormatWithSampleRate: 44_100, channels: 2)!
        stems = try ["lead_vocals.wav", "backing_vocals.wav"].map { name in
            let url = dir.appending(path: name)
            let file = try AVAudioFile(forWriting: url, settings: format.settings)
            let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 44_100 / 2)!
            buffer.frameLength = buffer.frameCapacity // 0.5 s of silence
            try file.write(from: buffer)
            return url
        }
    }

    @Test func `plays all stems together and stops at the end`() async throws {
        let player = PreviewPlayer()
        player.load(stems)
        #expect(player.loaded == stems)
        #expect(abs(player.duration - 0.5) < 0.01)

        player.play()
        #expect(player.isPlaying)
        for _ in 0..<60 where player.isPlaying {
            try await Task.sleep(for: .milliseconds(25))
        }
        #expect(!player.isPlaying, "stops by itself at the end")
        #expect(player.progress == 1)

        // Playing again after the end starts over.
        player.play()
        try await Task.sleep(for: .milliseconds(150))
        #expect(player.isPlaying)
        #expect(player.position < 0.4)
        player.pause()
        #expect(!player.isPlaying)
    }

    @Test func `mute and seek`() {
        let player = PreviewPlayer()
        player.load(stems)
        #expect(player.isAudible(stems[1]))
        player.toggleMute(stems[1])
        #expect(!player.isAudible(stems[1]))
        #expect(player.isAudible(stems[0]))

        player.seek(0.5)
        #expect(abs(player.position - 0.25) < 0.01)
        #expect(!player.isPlaying, "seeking while paused stays paused")

        // New stems reset mutes and position.
        player.load(stems.reversed())
        #expect(player.isAudible(stems[1]))
        #expect(player.position == 0)

        player.clear()
        #expect(player.loaded.isEmpty)
        player.play() // nothing loaded: no-op
        #expect(!player.isPlaying)
    }
}
