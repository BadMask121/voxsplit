import Foundation
import Testing
@testable import VoxSplitKit

struct EngineEventTests {
    @Test(arguments: [
        (#"{"event":"start","version":"0.1.0","stages":2,"duration_sec":213.4,"sample_rate":48000,"channels":2}"#,
         EngineEvent.start(stages: 2, durationSec: 213.4, sampleRate: 48000, estimateSec: nil)),
        (#"{"event":"start","stages":2,"duration_sec":10,"sample_rate":44100,"channels":2,"estimate_sec":42.5}"#,
         .start(stages: 2, durationSec: 10, sampleRate: 44100, estimateSec: 42.5)),
        (#"{"event":"estimate","duration_sec":190.0,"vocal_sec":12.0,"vocal_pass_sec":11.5,"best_sec":127.4,"best_pass_sec":91.0,"fast_sec":22.6,"fast_pass_sec":3.6}"#,
         .estimate(.init(durationSec: 190, vocalSec: 12, vocalPassSec: 11.5, bestSec: 127.4, bestPassSec: 91,
                         fastSec: 22.6, fastPassSec: 3.6))),
        (#"{"event":"stage","stage":"vocals_refine","index":2,"stages":3,"model":"Kim Vocal 2","device":"coreml"}"#,
         .stage(.vocalsRefine, index: 2, stages: 3, model: "Kim Vocal 2", device: "coreml")),
        (#"{"event":"stage","stage":"vocals","index":1,"stages":2,"model":"Kim Vocal 2","device":"coreml"}"#,
         .stage(.vocals, index: 1, stages: 2, model: "Kim Vocal 2", device: "coreml")),
        (#"{"event":"stage","stage":"refine","index":3,"stages":4,"model":"K","device":"coreml"}"#,
         .stage(.refine, index: 3, stages: 4, model: "K", device: "coreml")),
        (#"{"event":"progress","stage":"karaoke","fraction":0.5,"overall":0.75,"eta_sec":31.2}"#,
         .progress(.karaoke, fraction: 0.5, overall: 0.75, etaSec: 31.2)),
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
            modelsDir: URL(filePath: "/m dir/models", directoryHint: .isDirectory)
        )
        let input = URL(filePath: "/Music/-a \"song\" sång.wav")
        let args = location.separateArguments(
            input: input,
            outputFolder: URL(filePath: "/out dir", directoryHint: .isDirectory),
            quality: .best,
            passes: 2,
            vocalPasses: 0
        )
        #expect(args.first == "separate")
        #expect(args.contains("--input=/Music/-a \"song\" sång.wav"))
        #expect(args.contains { $0.hasPrefix("--out-dir=/out dir") })
        #expect(args.contains { $0.hasPrefix("--models-dir=/m dir/models") })
        #expect(args.contains("--quality=best"))
        #expect(args.contains("--passes=2"))
        #expect(args.contains("--vocal-passes=0"))
        #expect(args.contains("--tool=lead-backing"))
        #expect(location.estimateArguments(input: input).first == "estimate")
    }

    @Test func `estimate totals follow both sliders`() {
        let e = EngineEvent.Estimate(durationSec: 190, vocalSec: 12, vocalPassSec: 11, bestSec: 120, bestPassSec: 90,
                                     fastSec: 30, fastPassSec: 5)
        #expect(e.total(.best, passes: 1, vocalPasses: 1) == 120)
        #expect(e.total(.best, passes: 2, vocalPasses: 3) == 120 + 90 + 22)
        #expect(e.total(.fast, passes: 3, vocalPasses: 0) == 30 + 10 - 12)
        #expect(e.vocalRemoverTotal(vocalPasses: 2) == 12 + 11)
    }

    @Test func `output folder mirrors the plugin's`() {
        let folder = EngineLocator.defaultOutputFolder(for: URL(filePath: "/tmp/05 - Brother Brother.flac"))
        #expect(folder.lastPathComponent == "05 - Brother Brother")
        #expect(folder.deletingLastPathComponent().lastPathComponent == "VoxSplit")
    }
}
