# 01 — Architecture & bundling

## System diagram

```
┌──────────────────────────────┐        ┌──────────────────────────────┐
│ FL Studio (or any DAW)       │        │ VoxSplit.app (SwiftUI)       │
│  └─ VoxSplit.vst3 / .component│        │  ContentView                 │
│     PluginEditor  (UI thread) │        │  ProcessManager  @MainActor  │
│     SeparationJob (juce::Thread)       │  EngineRunner (async Process) │
│     PluginProcessor: passthrough       └──────────────┬───────────────┘
│       audio thread never involved                     │
└──────────────┬───────────────┘                        │
               │  posix_spawn(argv[])  — no shell       │
               │  --input=… --out-dir=… --vocal-model=… │
               ▼                                        ▼
        ┌───────────────────────────────────────────────────────┐
        │ Contents/Helpers/voxsplit-engine   (Rust, one binary)  │
        │   symphonia decode → rubato 44.1k → STFT (realfft)     │
        │   ONNX Runtime (statically linked, `ort`)              │
        │     CoreML EP (ANE/GPU) → CPU fallback   [macOS]       │
        │     CUDA → DirectML → CPU                [Windows]     │
        │   stage 1  Kim_Vocal_2.onnx        mix    → vocals     │
        │   stage 2  UVR_MDXNET_KARA_2.onnx  vocals → backing    │
        │            lead = vocals − backing                     │
        │   stdout: JSON-Lines events   stderr: free-form logs   │
        └───────────────────────────┬───────────────────────────┘
                                    ▼
             ~/Music/VoxSplit/<song>/lead_vocals.wav
                                     backing_vocals.wav
```

### Why a child process instead of linking Rust into the plugin

- **Crash isolation.** ONNX Runtime, CoreML compilation and multi-GB tensor
  allocations happen in a separate address space. If the engine dies, the DAW
  sees a non-zero exit code. It doesn't crash.
- **No real-time hazards.** `processBlock` is a passthrough. The plugin's only
  work is spawning a process and reading a pipe, on a `juce::Thread`. Nothing
  touches the audio thread or blocks the message thread.
- **One engine, two frontends.** The plugin and the app speak the same argv +
  JSON-Lines contract ([02-engine-protocol.md](02-engine-protocol.md)), so the
  engine can be tested on its own from a terminal.
- **Orphan safety.** The engine polls its parent PID between chunks and exits
  if it gets re-parented to launchd (the DAW crashed or was force-quit).

### Reading the engine's output

- **Plugin:** `SeparationJob` reads the merged stdout and stderr one byte at a
  time through `ChildProcess::readProcessOutput`. That call uses `fread`, which
  blocks until the requested count arrives, and one-byte reads come back as
  soon as anything is written. The worker thread only writes a lock-protected
  snapshot plus a version counter. The editor polls it on each display frame,
  so nothing ever calls back into the UI.
- **App:** `EngineRunner` (VoxSplitKit) reads each pipe with a
  `readabilityHandler`. `FileHandle.bytes.lines` buffers pipe reads, so a
  progress stream of a few KB would arrive all at once at exit. The readers
  aren't children of the cancelled task: after a SIGTERM they keep draining
  until EOF, so the engine's final `cancelled` event still arrives. If the
  engine ignores SIGTERM, it gets SIGKILL after 5 s.

### Previewing stems

- **Plugin:** `StemPreview` plays a finished stem through the insert itself.
  While a preview runs, the stem replaces the input; otherwise the plugin is a
  pass-through. Disk reads happen on a read-ahead `TimeSliceThread`.
  `AudioTransportSource` resamples to the host rate, so the audio thread only
  copies buffered samples.
- **App:** `AVAudioPlayer`, one stem at a time.

## Bundle layouts (zero end-user dependencies)

Each shipped bundle is self-contained. It embeds the same engine binary and
the same model files, so no Python, Homebrew, ONNX Runtime install or terminal
step is needed.

### macOS (Apple Silicon)

```
VoxSplit.vst3/                      VoxSplit.component/ (AU)       VoxSplit.app/
└── Contents/                       └── Contents/                  └── Contents/
    ├── Info.plist                      ├── Info.plist                 ├── Info.plist
    ├── MacOS/VoxSplit   (plugin)       ├── MacOS/VoxSplit             ├── MacOS/VoxSplit   (SwiftUI)
    ├── Helpers/                        ├── Helpers/                   ├── Helpers/
    │   └── voxsplit-engine             │   └── voxsplit-engine        │   └── voxsplit-engine
    ├── Resources/                      └── Resources/                 └── Resources/
    │   ├── moduleinfo.json                 └── models/…                   └── models/
    │   └── models/                                                          ├── Kim_Vocal_2.onnx
    │       ├── Kim_Vocal_2.onnx  (+ .json sidecar)                          ├── Kim_Vocal_2.json
    │       └── UVR_MDXNET_KARA_2.onnx (+ .json)                             ├── UVR_MDXNET_KARA_2.onnx
    └── _CodeSignature/                                                      └── UVR_MDXNET_KARA_2.json
```

Rules:

- **Executables go in `Contents/Helpers/`, never in `Resources/`.**
  `codesign` and notarization only accept nested code in the code
  directories (`MacOS`, `Helpers`, `Frameworks`, `PlugIns`, …). Executables
  under `Resources/` break the bundle seal.
- **Model weights are data**, so they go in `Contents/Resources/models/`. Each
  `.onnx` has a sidecar `.json` with its STFT hyperparameters and SHA-256. The
  engine never hardcodes model geometry.
- **ONNX Runtime is linked statically** into `voxsplit-engine` (the `ort`
  crate's `download-binaries` uses pyke's `aarch64-apple-darwin+coreml`
  static build). `otool -L voxsplit-engine` lists only system libraries and
  frameworks (CoreML, Foundation, Metal, Accelerate, libc++). If a future
  `ort` release switches to a dynamic build, the dylib goes in
  `Contents/Frameworks/` with an `@loader_path/../Frameworks` rpath.
- **Signing is inside-out:** first `Helpers/voxsplit-engine` (hardened runtime),
  then the bundle. For distribution, sign with a Developer ID and notarize.
  Without that, Gatekeeper quarantine stops the helper from being executed in a
  downloaded copy, and the user would need a terminal `xattr` fix. Dev builds
  are ad-hoc signed (`codesign -s -`), which works on the build machine.
- **Locating the engine.** Plugin: `File::getSpecialLocation(currentExecutableFile)`
  returns the *plugin* binary (not the host), then `../Helpers/voxsplit-engine`.
  App: `Bundle.main.bundleURL/Contents/Helpers/voxsplit-engine`.
  Both fall back to `~/Library/Application Support/VoxSplit/{voxsplit-engine,models/}`.
  That fallback is for dev builds, and for a future shared `.pkg` installer that
  would dedupe the ~120 MB of models across the three bundles.
- **Apple Silicon only (v1).** pyke publishes no `x86_64-apple-darwin`
  ONNX Runtime build. Intel support needs an ORT source build, tracked as a
  follow-up.

### Windows (designed for, not built in v1)

```
VoxSplit.vst3/
└── Contents/
    ├── x86_64-win/
    │   ├── VoxSplit.vst3              (plugin DLL)
    │   ├── voxsplit-engine.exe
    │   ├── onnxruntime.dll (+ providers_shared.dll)   if the ORT build is dynamic
    │   └── DirectML.dll               (redistributable, ~18 MB)
    └── Resources/models/…
```

DirectML is the default accelerator (any DX12 GPU). CUDA is opt-in at build
time (`--features cuda`). Its runtime DLLs are ~1 GB, so it's a separate
download. Device selection in the engine is already `cfg`-gated for both.

## Output location

Frontends pass `--out-dir=~/Music/VoxSplit/<input file stem>/`. Results are
written as `*.wav.partial` and atomically renamed on success, so a cancelled or
crashed run never leaves a half-written `lead_vocals.wav` behind.
