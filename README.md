# VoxSplit

Offline lead/backing vocal separation for macOS, as an **FL Studio-compatible
VST3/AU plugin**, a **native SwiftUI app** and a **command-line tool**. All three
are thin frontends over one Rust engine that runs ONNX models on the Apple Neural
Engine/GPU via CoreML. Nothing leaves your machine, and end users install nothing
else: no Python, no runtimes.

**Download:** https://voxsplit.jeffrey.build (Apple silicon, macOS 14+), or from Terminal:

```bash
curl -fsSL https://voxsplit.jeffrey.build/install.sh | bash               # app + plugins + voxsplit command
curl -fsSL https://voxsplit.jeffrey.build/install.sh | bash -s -- --cli   # only the command, no password
```

```
song.wav ─► Kim Vocal 2 ─► vocals ─► karaoke model ─► lead_vocals.wav + backing_vocals.wav
                                      Best: BS-RoFormer Karaoke (frazer & becruily)
                                      Fast: UVR-MDX-NET Karaoke 2, 3 refinement passes
```

| Quality | Model | 3:10 song on an M2 Max |
| --- | --- | --- |
| **Best** (default) | BS-RoFormer karaoke, Core ML GPU | ~2 min (first run ever: about +1 min to compile) |
| **Fast** | MDX-Net karaoke, 3 passes | ~1 min |
| Vocal Remover | Kim Vocal 2 | ~15 s |

Two tools, in both the plugin and the app:

- **Lead & Backing**: the pipeline above. *Vocal passes* (0–5) sets how many times
  Kim Vocal 2 runs (0 = the file is already vocals); *Refinement passes* (1–5) sets
  how many times the lead/backing model runs.
- **Vocal Remover**: `vocals.wav` + `instrumental.wav` from the vocal stage alone.

Both UIs show the expected time as soon as you choose a song, and a countdown
while it runs. Estimates learn from each run on your Mac.

## Layout

```
engine/    Rust CLI (voxsplit / voxsplit-engine): decode → STFT → ONNX Runtime → overlap-add → WAV
plugin/    JUCE 9 plugin (VST3, AU, Standalone): drop zone, Separate, preview, drag-out
macos/     SwiftUI app (SwiftPM): the same flow; VoxSplitKit holds the async Process() runner
models/    ONNX weights (downloaded, gitignored) + committed sidecar JSON hyperparameters
installer/ .pkg pieces: distribution.xml, welcome/conclusion pages, postinstall
site/      voxsplit.jeffrey.build: Cloudflare Worker + static page + install.sh; installers in R2
assets/    the VoxSplit mark (icon.svg → macos/AppIcon.icns via scripts/make-icon.sh)
scripts/   fetch-models, build-engine, build-plugin, build-app, build-pkg, release, embed, sign, test-all
docs/      architecture, bundle layout, engine protocol, measured performance
```

## Build (Apple Silicon, macOS 14+)

Requirements: Xcode command-line tools, CMake ≥ 3.25, rustup. The Rust 1.92
toolchain is pinned in `engine/rust-toolchain.toml` and installs automatically;
JUCE 9.0.4 is fetched by CMake.

```bash
scripts/fetch-models.sh     # MDX weights (~120 MB, SHA-256 verified); exports the RoFormer
                            #   via scripts/export-roformer.sh (needs uv; ~1 GB of PyTorch, once)
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

Release (maintainers):

```bash
scripts/build-pkg.sh        # → dist/VoxSplit-<version>.pkg: app + VST3 + AU + `voxsplit`, sharing one
                            #   engine + models in /Library/Application Support/VoxSplit
scripts/release.sh          # build-pkg, then upload to R2, update site/release.json, deploy the site,
                            #   and check the live download (needs `wrangler login`)
```

Use the engine directly from the checkout:

```bash
engine/target/release/voxsplit-engine probe
engine/target/release/voxsplit-engine separate "$HOME/Music/song.wav" --keep-instrumental
cd engine && cargo run --release --example stem_check -- null "$HOME/Music/song.wav" "$HOME/Music/VoxSplit/song"
```

## Using it

**FL Studio:** rescan plugins, then put **VoxSplit** on a mixer insert.

1. Drop a song onto the plugin (or click to browse) and press **Separate** (or Return).
2. When it's done, press ▶ above the stems to hear them all together; each stem's
   speaker button mutes it. The preview replaces that insert's input while it
   plays, so you hear it where you're working. Space plays and pauses.
3. Drag a stem row onto the playlist, or press **Reveal**.

The run keeps going if you close the plugin window, and the result is saved
with the project.

**App:** the same flow in `VoxSplit.app`. You can also drop songs on its Dock
icon or use *Open With*. Stems are written to `~/Music/VoxSplit/<song>/`.

**Terminal:** readable progress in a terminal, JSON Lines when piped (`--format json` forces it).

```bash
voxsplit separate song.mp3                                   # lead + backing → ~/Music/VoxSplit/song
voxsplit separate song.mp3 --quality fast --passes 3 --reveal
voxsplit separate song.mp3 --tool vocal-remover               # vocals + instrumental
voxsplit play ~/Music/VoxSplit/song --mute backing            # hear the stems together
voxsplit estimate song.mp3
```

See [docs/](docs/) for the architecture, bundle layout, engine protocol and measured performance.
