# VoxSplit

Offline lead/backing vocal separation for macOS, as an **FL Studio-compatible
VST3/AU plugin** and a **native SwiftUI app**. Both are thin frontends over one
Rust engine that runs ONNX models on the Apple Neural Engine/GPU via CoreML.
Nothing leaves your machine, and end users install nothing else: no Python,
no runtimes, no terminal.

```
song.wav ─► Kim Vocal 2 ─► vocals ─► Karaoke 2 ─► backing_vocals.wav
                                       └─ vocals − backing ─► lead_vocals.wav
```

## Layout

```
engine/    Rust CLI (voxsplit-engine): decode → STFT → ONNX Runtime → overlap-add → WAV
plugin/    JUCE 9 plugin (VST3, AU, Standalone): drop zone + Process, spawns the engine
macos/     SwiftUI app (SwiftPM): same UI flow, async Process() runner
models/    ONNX weights (downloaded, gitignored) + committed sidecar JSON hyperparameters
scripts/   fetch-models, build-engine, build-plugin, build-app, sign
docs/      architecture, bundle layout, engine protocol
```

## Build (Apple Silicon, macOS 14+)

Requirements: Xcode command-line tools, CMake ≥ 3.25, rustup. The Rust 1.92
toolchain is pinned in `engine/rust-toolchain.toml` and installs automatically.

```bash
scripts/fetch-models.sh     # ~120 MB of ONNX weights, SHA-256 verified
scripts/build-engine.sh     # → engine/target/release/voxsplit-engine
scripts/build-plugin.sh     # → dist/VoxSplit.{vst3,component} (+ installs to ~/Library/Audio/Plug-Ins)
scripts/build-app.sh        # → dist/VoxSplit.app
```

Use the engine directly:

```bash
engine/target/release/voxsplit-engine probe
engine/target/release/voxsplit-engine separate --input="$HOME/Music/song.wav"
```

In FL Studio, rescan plugins and load **VoxSplit** on any mixer insert. Drop a
file, press Process, then drag the finished stems straight onto the playlist.

See [docs/](docs/) for the architecture, bundle layout and engine protocol.
