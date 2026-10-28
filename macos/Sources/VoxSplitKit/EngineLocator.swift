import Foundation

/// Where the engine binary and its two models live (docs/01-architecture.md).
public struct EngineLocation: Sendable, Equatable {
    public let engine: URL
    public let vocalModel: URL
    public let karaokeModel: URL

    public init(engine: URL, vocalModel: URL, karaokeModel: URL) {
        self.engine = engine
        self.vocalModel = vocalModel
        self.karaokeModel = karaokeModel
    }

    /// argv for `separate`, one `--flag=value` element per option.
    public func separateArguments(input: URL, outputFolder: URL) -> [String] {
        [
            "separate",
            "--input=\(input.path(percentEncoded: false))",
            "--out-dir=\(outputFolder.path(percentEncoded: false))",
            "--vocal-model=\(vocalModel.path(percentEncoded: false))",
            "--karaoke-model=\(karaokeModel.path(percentEncoded: false))",
        ]
    }
}

public enum EngineLocator {
    public static let engineName = "voxsplit-engine"
    public static let vocalModelName = "Kim_Vocal_2.onnx"
    public static let karaokeModelName = "UVR_MDXNET_KARA_2.onnx"

    /// Search order: `VOXSPLIT_ENGINE` / `VOXSPLIT_MODELS` (development, e.g. `swift run`),
    /// the app bundle (`Contents/Helpers`, `Contents/Resources/models`), then
    /// `~/Library/Application Support/VoxSplit`.
    public static func locate(
        bundleURL: URL = Bundle.main.bundleURL,
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> EngineLocation? {
        var candidates: [(engine: URL, models: URL)] = []

        if let engine = environment["VOXSPLIT_ENGINE"], let models = environment["VOXSPLIT_MODELS"] {
            candidates.append((URL(filePath: engine), URL(filePath: models, directoryHint: .isDirectory)))
        }

        let contents = bundleURL.appending(path: "Contents", directoryHint: .isDirectory)
        candidates.append((contents.appending(path: "Helpers/\(engineName)"),
                           contents.appending(path: "Resources/models", directoryHint: .isDirectory)))

        let support = URL.applicationSupportDirectory.appending(path: "VoxSplit", directoryHint: .isDirectory)
        candidates.append((support.appending(path: engineName), support.appending(path: "models", directoryHint: .isDirectory)))

        let files = FileManager.default
        for candidate in candidates {
            let location = EngineLocation(
                engine: candidate.engine,
                vocalModel: candidate.models.appending(path: vocalModelName),
                karaokeModel: candidate.models.appending(path: karaokeModelName)
            )
            if files.isExecutableFile(atPath: location.engine.path(percentEncoded: false)),
               files.fileExists(atPath: location.vocalModel.path(percentEncoded: false)),
               files.fileExists(atPath: location.karaokeModel.path(percentEncoded: false)) {
                return location
            }
        }
        return nil
    }

    /// `~/Music/VoxSplit/<input name>/`, the same place the plugin writes to.
    public static func defaultOutputFolder(for input: URL) -> URL {
        URL.musicDirectory
            .appending(path: "VoxSplit", directoryHint: .isDirectory)
            .appending(path: input.deletingPathExtension().lastPathComponent, directoryHint: .isDirectory)
    }
}
