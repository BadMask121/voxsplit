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
plugin/    JUCE 9 plugin (VST3, AU, Standalone): drop zone, Separate, preview, drag-out
macos/     SwiftUI app (SwiftPM): the same flow; VoxSplitKit holds the async Process() runner
models/    ONNX weights (downloaded, gitignored) + committed sidecar JSON hyperparameters
scripts/   fetch-models, build-engine, build-plugin, build-app, embed, sign, test-all
docs/      architecture, bundle layout, engine protocol, measured performance
```

## Build (Apple Silicon, macOS 14+)

Requirements: Xcode command-line tools, CMake ≥ 3.25, rustup. The Rust 1.92
toolchain is pinned in `engine/rust-toolchain.toml` and installs automatically;
JUCE 9.0.4 is fetched by CMake.

```bash
scripts/fetch-models.sh     # ~120 MB of ONNX weights, SHA-256 verified
scripts/build-engine.sh     # → engine/target/release/voxsplit-engine
scripts/build-plugin.sh     # → dist/VoxSplit.{vst3,component}, installed to ~/Library/Audio/Plug-Ins
                            #   (--no-install to skip; quit your DAW first if it has VoxSplit loaded)
scripts/build-app.sh        # → dist/VoxSplit.app
scripts/test-all.sh         # engine unit + CLI tests, headless plugin tests, Swift Testing suite
```

Bundles are assembled and ad-hoc signed in `~/Library/Caches/VoxSplit/stage`
and copied to `dist/`. Building them outside the checkout is deliberate. If the
repo lives in an iCloud-synced folder (like `~/Desktop`), macOS stamps Finder
metadata onto new bundle folders, and `codesign` refuses those. Run
`codesign --verify` on the staged or installed copies, not on `dist/`. For
distribution, set `VOXSPLIT_SIGN_IDENTITY` to a Developer ID and notarize.

Use the engine directly:

```bash
engine/target/release/voxsplit-engine probe
engine/target/release/voxsplit-engine separate --input="$HOME/Music/song.wav" --keep-instrumental
cd engine && cargo run --release --example stem_check -- null "$HOME/Music/song.wav" "$HOME/Music/VoxSplit/song"
```

## Using it

**FL Studio:** rescan plugins, then put **VoxSplit** on a mixer insert.

1. Drop a song onto the plugin (or click to browse) and press **Separate** (or Return).
2. When it's done, press ▶ on a stem to audition it. The stem replaces that
   insert's input while it plays, so you hear it where you're working. Space
   toggles the lead.
3. Drag a stem row onto the playlist, or press **Reveal**.

The run keeps going if you close the plugin window, and the result is saved
with the project.

**App:** the same flow in `VoxSplit.app`. You can also drop songs on its Dock
icon or use *Open With*. Stems are written to `~/Music/VoxSplit/<song>/`.

See [docs/](docs/) for the architecture, bundle layout, engine protocol and measured performance.
