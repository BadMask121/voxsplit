import AVFoundation
import Observation

/// Plays the finished stems together, in sync, with a per-stem mute.
@Observable
final class PreviewPlayer {
    private(set) var loaded: [URL] = []
    private(set) var muted: Set<URL> = []
    private(set) var isPlaying = false
    /// Seconds into the stems, updated about 30 times a second while playing.
    private(set) var position = 0.0
    private(set) var duration = 0.0

    var progress: Double { duration > 0 ? min(position / duration, 1) : 0 }

    @ObservationIgnored private var players: [AVAudioPlayer] = []
    @ObservationIgnored private var ticker: Task<Void, Never>?
    @ObservationIgnored private var startedAt = Date.distantPast

    func isAudible(_ url: URL) -> Bool { !muted.contains(url) }

    /// Replaces the loaded stems (stopped, at the start, all audible).
    func load(_ urls: [URL]) {
        guard urls != loaded else { return }
        pause()
        players = urls.compactMap { try? AVAudioPlayer(contentsOf: $0) }
        players.forEach { $0.prepareToPlay() }
        loaded = players.count == urls.count ? urls : []
        if loaded.isEmpty { players = [] }
        muted = []
        position = 0
        duration = players.first?.duration ?? 0
    }

    func clear() { load([]) }

    func togglePlay() { isPlaying ? pause() : play() }

    func play() {
        guard let first = players.first else { return }
        if position >= duration - 0.01 { position = 0 } // finished last time: start over
        // Schedule every stem on the same device-clock instant so they stay aligned.
        let start = first.deviceCurrentTime + 0.05
        for (player, url) in zip(players, loaded) {
            player.currentTime = position
            player.volume = muted.contains(url) ? 0 : 1
            player.play(atTime: start)
        }
        isPlaying = true
        startedAt = .now
        startTicker()
    }

    func pause() {
        ticker?.cancel()
        ticker = nil
        players.forEach { $0.pause() }
        if let first = players.first, isPlaying { position = first.currentTime }
        isPlaying = false
    }

    /// Jumps all stems to `fraction` of the way through, keeping play/pause as it was.
    func seek(_ fraction: Double) {
        let wasPlaying = isPlaying
        pause()
        position = min(max(fraction, 0), 1) * duration
        players.forEach { $0.currentTime = position }
        if wasPlaying { play() }
    }

    func toggleMute(_ url: URL) {
        if muted.contains(url) { muted.remove(url) } else { muted.insert(url) }
        if let index = loaded.firstIndex(of: url) {
            // A short fade so muting never clicks.
            players[index].setVolume(muted.contains(url) ? 0 : 1, fadeDuration: 0.04)
        }
    }

    private func startTicker() {
        ticker?.cancel()
        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(33))
                guard let self, let first = self.players.first else { return }
                // Stems start 50 ms after play(); don't mistake that gap for the end.
                if !first.isPlaying, Date.now.timeIntervalSince(self.startedAt) > 0.2 {
                    // Reached the end.
                    self.isPlaying = false
                    self.position = self.duration
                    return
                }
                self.position = first.currentTime
            }
        }
    }
}
