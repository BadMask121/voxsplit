import Foundation

/// One line of the engine's JSON-Lines stdout (docs/02-engine-protocol.md).
public enum EngineEvent: Sendable, Equatable {
    public enum Stage: String, Sendable {
        case vocals, karaoke
    }

    public enum Stem: String, Sendable {
        case lead, backing, instrumental
    }

    case start(stages: Int, durationSec: Double, sampleRate: Int)
    case stage(Stage, index: Int, stages: Int, model: String, device: String)
    case progress(Stage, fraction: Double, overall: Double)
    case log(level: String, message: String)
    case output(Stem, url: URL)
    case done(elapsedSec: Double)
    case cancelled
    case error(code: String, message: String)
    /// A well-formed event this app doesn't know yet (newer engine).
    case other(name: String)

    /// Parses a stdout line; `nil` means the line isn't an event and should be shown as log text.
    public init?(line: some StringProtocol) {
        guard line.first == "{", let data = String(line).data(using: .utf8) else { return nil }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let raw = try? decoder.decode(Raw.self, from: data) else { return nil }

        switch raw.event {
        case "start":
            self = .start(stages: raw.stages ?? 2, durationSec: raw.durationSec ?? 0, sampleRate: raw.sampleRate ?? 0)
        case "stage":
            guard let stage = raw.stage.flatMap(Stage.init(rawValue:)) else { self = .other(name: raw.event); return }
            self = .stage(stage, index: raw.index ?? 1, stages: raw.stages ?? 2,
                          model: raw.model ?? "", device: raw.device ?? "")
        case "progress":
            guard let stage = raw.stage.flatMap(Stage.init(rawValue:)) else { self = .other(name: raw.event); return }
            self = .progress(stage, fraction: raw.fraction ?? 0, overall: raw.overall ?? 0)
        case "log":
            self = .log(level: raw.level ?? "info", message: raw.message ?? "")
        case "output":
            guard let stem = raw.stem.flatMap(Stem.init(rawValue:)), let path = raw.path else {
                self = .other(name: raw.event); return
            }
            self = .output(stem, url: URL(filePath: path))
        case "done":
            self = .done(elapsedSec: raw.elapsedSec ?? 0)
        case "cancelled":
            self = .cancelled
        case "error":
            self = .error(code: raw.code ?? "internal", message: raw.message ?? "Unknown error")
        default:
            self = .other(name: raw.event)
        }
    }

    private struct Raw: Decodable {
        var event: String
        var stages: Int?
        var durationSec: Double?
        var sampleRate: Int?
        var stage: String?
        var index: Int?
        var model: String?
        var device: String?
        var fraction: Double?
        var overall: Double?
        var level: String?
        var message: String?
        var stem: String?
        var path: String?
        var elapsedSec: Double?
        var code: String?
    }
}
