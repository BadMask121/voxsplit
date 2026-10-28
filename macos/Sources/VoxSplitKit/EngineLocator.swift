import Foundation

/// best: BS-RoFormer karaoke (cleanest, slower). fast: MDX-Net karaoke with 3 passes.
public enum Quality: String, Sendable, CaseIterable {
    case best, fast
}

/// leadBacking: lead_vocals.wav + backing_vocals.wav. vocalRemover: vocals.wav + instrumental.wav.
public enum Tool: String, Sendable, CaseIterable {
    case leadBacking = "lead-backing"
    case vocalRemover = "vocal-remover"
}

/// Where the engine binary and its models live (docs/01-architecture.md).
public struct EngineLocation: Sendable, Equatable {
    public let engine: URL
    public let modelsDir: URL

    public init(engine: URL, modelsDir: URL) {
        self.engine = engine
        self.modelsDir = modelsDir
    }

    /// argv for `separate`, one `--flag=value` element per option.
    public func separateArguments(input: URL, outputFolder: URL, quality: Quality, passes: Int,
                                  vocalPasses: Int = 1, tool: Tool = .leadBacking) -> [String] {
        [
            "separate",
            "--tool=\(tool.rawValue)",
            "--input=\(input.path(percentEncoded: false))",
            "--out-dir=\(outputFolder.path(percentEncoded: false))",
            "--models-dir=\(modelsDir.path(percentEncoded: false))",
            "--quality=\(quality.rawValue)",
            "--passes=\(min(max(passes, 1), 5))",
            "--vocal-passes=\(min(max(vocalPasses, 0), 5))",
        ]
    }

    /// argv for `estimate`: expected run time at both qualities.
    public func estimateArguments(input: URL) -> [String] {
        [
            "estimate",
            "--input=\(input.path(percentEncoded: false))",
            "--models-dir=\(modelsDir.path(percentEncoded: false))",
        ]
    }
}

public enum EngineLocator {
    public static let engineName = "voxsplit-engine"
    /// Every model the engine may load, for either quality (each with a `.json` sidecar).
    public static let requiredModels = ["Kim_Vocal_2", "UVR_MDXNET_KARA_2", "BS_Roformer_Karaoke"]

    /// Search order: `VOXSPLIT_ENGINE` / `VOXSPLIT_MODELS` (development, e.g. `swift run`),
    /// the app bundle (`Contents/Helpers`, `Contents/Resources/models`), then the shared
    /// installs: `~/Library/Application Support/VoxSplit` (`install.sh --cli`) and
    /// `/Library/Application Support/VoxSplit` (the .pkg).
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

        let shared = [URL.applicationSupportDirectory]
            + FileManager.default.urls(for: .applicationSupportDirectory, in: .localDomainMask)
        for support in shared.map({ $0.appending(path: "VoxSplit", directoryHint: .isDirectory) }) {
            candidates.append((support.appending(path: engineName), support.appending(path: "models", directoryHint: .isDirectory)))
        }

        let files = FileManager.default
        for candidate in candidates {
            let hasModels = requiredModels.allSatisfy { name in
                ["onnx", "json"].allSatisfy { ext in
                    files.fileExists(atPath: candidate.models.appending(path: "\(name).\(ext)").path(percentEncoded: false))
                }
            }
            if files.isExecutableFile(atPath: candidate.engine.path(percentEncoded: false)), hasModels {
                return EngineLocation(engine: candidate.engine, modelsDir: candidate.models)
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
