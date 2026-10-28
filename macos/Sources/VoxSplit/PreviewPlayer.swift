import AVFoundation
import Observation

/// Auditions one finished stem at a time.
@Observable
final class PreviewPlayer {
    private(set) var playing: URL?
    /// Playback position, 0…1, updated about 30 times a second while playing.
    private(set) var progress = 0.0

    @ObservationIgnored private var player: AVAudioPlayer?
    @ObservationIgnored private var ticker: Task<Void, Never>?

    func toggle(_ url: URL) {
        if playing == url { stop() } else { play(url) }
    }

    func play(_ url: URL) {
        stop()
        guard let player = try? AVAudioPlayer(contentsOf: url) else { return }
        player.prepareToPlay()
        player.play()
        self.player = player
        playing = url

        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(33))
                guard let self, let player = self.player else { return }
                if !player.isPlaying {
                    self.stop()
                    return
                }
                self.progress = player.duration > 0 ? player.currentTime / player.duration : 0
            }
        }
    }

    func stop() {
        ticker?.cancel()
        ticker = nil
        player?.stop()
        player = nil
        playing = nil
        progress = 0
    }
}
