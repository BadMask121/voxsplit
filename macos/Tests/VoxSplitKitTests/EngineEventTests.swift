import Foundation
import Testing
@testable import VoxSplitKit

struct EngineEventTests {
    @Test(arguments: [
        (#"{"event":"start","version":"0.1.0","stages":2,"duration_sec":213.4,"sample_rate":48000,"channels":2}"#,
         EngineEvent.start(stages: 2, durationSec: 213.4, sampleRate: 48000)),
        (#"{"event":"stage","stage":"vocals","index":1,"stages":2,"model":"Kim Vocal 2","device":"coreml"}"#,
         .stage(.vocals, index: 1, stages: 2, model: "Kim Vocal 2", device: "coreml")),
        (#"{"event":"progress","stage":"karaoke","fraction":0.5,"overall":0.75}"#,
         .progress(.karaoke, fraction: 0.5, overall: 0.75)),
        (#"{"event":"log","level":"warn","message":"coreml unavailable"}"#,
         .log(level: "warn", message: "coreml unavailable")),
        (#"{"event":"output","stem":"lead","path":"/Users/me/Music/VoxSplit/a b/lead_vocals.wav"}"#,
         .output(.lead, url: URL(filePath: "/Users/me/Music/VoxSplit/a b/lead_vocals.wav"))),
        (#"{"event":"done","elapsed_sec":41.8}"#, .done(elapsedSec: 41.8)),
        (#"{"event":"cancelled"}"#, .cancelled),
        (#"{"event":"error","code":"decode","message":"bad file"}"#, .error(code: "decode", message: "bad file")),
        (#"{"event":"something_new","x":1}"#, .other(name: "something_new")),
    ])
    func `decodes every protocol event`(line: String, expected: EngineEvent) {
        #expect(EngineEvent(line: line) == expected)
    }

    @Test(arguments: ["", "2026-10-28 warning: CoreML fallback", "{not json", #"{"no_event":true}"#])
    func `non-event lines are log text`(line: String) {
        #expect(EngineEvent(line: line) == nil)
    }

    @Test func `arguments are one argv element per flag`() {
        let location = EngineLocation(
            engine: URL(filePath: "/x/voxsplit-engine"),
            vocalModel: URL(filePath: "/m/Kim_Vocal_2.onnx"),
            karaokeModel: URL(filePath: "/m/UVR_MDXNET_KARA_2.onnx")
        )
        let args = location.separateArguments(
            input: URL(filePath: "/Music/-a \"song\" sång.wav"),
            outputFolder: URL(filePath: "/out dir", directoryHint: .isDirectory)
        )
        #expect(args.first == "separate")
        #expect(args.contains("--input=/Music/-a \"song\" sång.wav"))
        #expect(args.contains { $0.hasPrefix("--out-dir=/out dir") })
    }

    @Test func `output folder mirrors the plugin's`() {
        let folder = EngineLocator.defaultOutputFolder(for: URL(filePath: "/tmp/05 - Brother Brother.flac"))
        #expect(folder.lastPathComponent == "05 - Brother Brother")
        #expect(folder.deletingLastPathComponent().lastPathComponent == "VoxSplit")
    }
}
