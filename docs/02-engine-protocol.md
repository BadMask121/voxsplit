# 02 — Engine protocol (`voxsplit-engine`)

The contract the JUCE plugin and the SwiftUI app both code against. Any change
here must update `plugin/Source/SeparationJob.cpp` and
`macos/Sources/VoxSplit/EngineEvent.swift`.

## Invocation

Frontends spawn the binary with an **argv array** (never a shell string) and
pass every option as a single `--flag=value` element. A path that happens to
start with `-` can't be misread as a flag.

```
voxsplit-engine separate --input=<audio file> [options]
voxsplit-engine estimate --input=<audio file> [--models-dir=<dir>] [--device=…]
voxsplit-engine probe
```

`estimate` reads the file's length from its header, without decoding it, and
prints one `estimate` event with the expected seconds for both qualities. It
takes well under a second, so frontends call it as soon as a file is chosen.

**How estimates work.** Every model has a load time and a time per chunk. The
defaults live in its sidecar's `timing` block, measured on an M2 Max. After
each run, the engine blends what it measured into
`~/Library/Caches/com.voxsplit.engine/timings.json`, keyed by model and device,
so estimates adapt to the machine. The first Best run also compiles the
RoFormer for Core ML, which is about a minute slower once.

### `separate` options

| Flag | Default | Notes |
| --- | --- | --- |
| `--input=<path>` | required | wav, aiff, flac, mp3, m4a/aac (decoded by symphonia). Mono is upmixed; >2 channels rejected. |
| `--out-dir=<path>` | `~/Music/VoxSplit/<input stem>` | Created if missing. Existing stems are replaced atomically. |
| `--quality=best\|fast` | `best` | **best**: BS-RoFormer karaoke (frazer & becruily), cleaner and about 6× slower. **fast**: UVR-MDX-NET Karaoke 2. |
| `--models-dir=<dir>` | bundled models | Folder holding all model files and sidecars. Frontends pass their bundle's `Resources/models`. |
| `--vocal-model=<path.onnx>` | `Kim_Vocal_2.onnx` | Stage 1, mix → vocals. Sidecar `<same name>.json` required. |
| `--karaoke-model=<path.onnx>` | chosen by `--quality` | Lead/backing model. MDX karaoke models output the backing side; RoFormer karaoke models output the lead (the sidecar's `primary_stem` says which). |
| `--input-is-vocals` | off | Skip stage 1 (input is already an a cappella). |
| `--keep-instrumental` | off | Also write `instrumental.wav` (mix − vocals). |
| `--passes=<1–5>` | `1` best, `3` fast | Karaoke passes. The first splits the vocals; each further pass re-runs the karaoke model on the current lead and moves the backing it still finds into the backing stem. `lead + backing` stays equal to the vocals. |
| `--device=auto\|coreml\|cuda\|directml\|cpu` | `auto` | An explicit device that fails to initialise is an error (`model_load`). `auto` falls back to CPU. |
| `--overlap=<0.0–0.95>` | per model: `0.75` RoFormer, `0.5` MDX | Chunk overlap. Higher means smoother and slower. Chunks always share at least `n_fft` samples, so `0` means "minimum overlap", never abutting chunks. |
| `--batch=<1–16>` | `1` | Chunks per inference call. |
| `--denoise` | off | Averages `f(x)` and `−f(−x)`. Cleaner, 2× slower. MDX models only. |
| `--bit-depth=32f\|24\|16` | `32f` | Output WAV sample format. |

Bundled-model lookup order: `<exe dir>/../Resources/models/`, then
`~/Library/Application Support/VoxSplit/models/`, then `<exe dir>/../../../models/`
(the repo's `models/` when running from `engine/target/release`).

Outputs (in `--out-dir`): `lead_vocals.wav`, `backing_vocals.wav`, and
optionally `instrumental.wav`. They're always stereo, at the **input's sample
rate**, and the same length as the input, so they line up sample-for-sample
when dropped on the DAW timeline.

## Events (stdout, JSON Lines)

One JSON object per line, written with a single `write` and flushed
immediately. Free-text `message` fields are truncated to 300 characters to
keep lines short. If a frontend merges stdout and stderr into one pipe (JUCE
does), any line that doesn't start with `{` is log text and must never be
treated as an error. stderr is free-form diagnostic text (ONNX Runtime
warnings, panics).

| `event` | Fields | When |
| --- | --- | --- |
| `probe` | `version`, `os`, `arch`, `devices: [{name, compiled, available}]` | `probe` subcommand only |
| `estimate` | `duration_sec`, `best_sec`, `fast_sec` | `estimate` subcommand only |
| `start` | `version`, `stages`, `duration_sec`, `sample_rate`, `channels` (all of the *input*), `estimate_sec` (expected total) | input decoded |
| `stage` | `stage` (`vocals`\|`karaoke`\|`refine`), `index` (1-based), `stages`, `model`, `device` (`coreml`\|`cuda`\|`directml`\|`cpu`) | a stage's model has loaded and inference begins. Between `start` and the first `stage`, the engine is loading (and, the first time, compiling) the model. |
| `progress` | `stage`, `fraction` (0–1 within stage), `overall` (0–1, stages weighted equally), `eta_sec` (seconds left: the measured rate of the current stage plus estimates for later ones) | after each chunk (throttled to ≥0.5% steps) |
| `log` | `level` (`info`\|`warn`), `message` | notable milestones |
| `output` | `stem` (`lead`\|`backing`\|`instrumental`), `path` | a stem has been finalised on disk |
| `done` | `elapsed_sec` | success, just before exit 0 |
| `cancelled` | — | SIGTERM/SIGINT or parent died, just before exit 130 |
| `error` | `code`, `message` | fatal, just before a non-zero exit |

Example run:

```json
{"event":"start","version":"0.1.0","stages":2,"duration_sec":213.4,"sample_rate":48000,"channels":2}
{"event":"stage","stage":"vocals","index":1,"stages":2,"model":"Kim Vocal 2","device":"coreml"}
{"event":"progress","stage":"vocals","fraction":0.37,"overall":0.185}
{"event":"stage","stage":"karaoke","index":2,"stages":2,"model":"UVR-MDX-NET Karaoke 2","device":"coreml"}
{"event":"output","stem":"lead","path":"/Users/me/Music/VoxSplit/song/lead_vocals.wav"}
{"event":"output","stem":"backing","path":"/Users/me/Music/VoxSplit/song/backing_vocals.wav"}
{"event":"done","elapsed_sec":41.8}
```

## Exit codes

| Code | `error.code` | Meaning |
| --- | --- | --- |
| 0 | — | success |
| 1 | `internal` | unexpected failure / panic |
| 2 | `bad_args` | invalid flags or values (clap usage errors also exit 2) |
| 3 | `decode` | input missing, unreadable, or unsupported |
| 4 | `model_load` | model or sidecar missing/invalid, or the requested device failed |
| 5 | `inference` | ONNX Runtime failed during a run |
| 6 | `write` | output directory or file couldn't be written |
| 130 | — (`cancelled` event) | SIGTERM/SIGINT, or the parent process died |

When `--device` is `auto` and an accelerator fails to initialise, the engine
emits a `log` event (`level: "warn"`) and tries the next one. The `stage`
event's `device` says what actually ran.

## Cancellation

Send **SIGTERM** (SwiftUI `Process.terminate()`). The engine finishes the
current chunk, deletes its `*.partial` files, emits `cancelled` and exits 130.
**SIGKILL** (JUCE `ChildProcess::kill()`) is also safe: only `*.partial` files
can be left behind, and the frontend deletes them.
