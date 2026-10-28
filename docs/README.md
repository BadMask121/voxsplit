# VoxSplit docs

| Doc | What's in it |
| --- | --- |
| [01-architecture.md](01-architecture.md) | System diagram, why the engine is a child process, bundle layouts, signing |
| [02-engine-protocol.md](02-engine-protocol.md) | CLI flags, JSON-Lines event schema, exit codes, cancellation |

## Reality check (read first)

**Lead vs. backing is a two-model job.** Kim Vocal 2 separates *vocals from
the instrumental*. It can't tell a lead singer from the harmonies behind them.
VoxSplit chains it with a karaoke model (UVR-MDX-NET Karaoke 2), the same
two-pass recipe Ultimate Vocal Remover users run by hand:

1. `mix → Kim Vocal 2 → vocals` (plus `instrumental = mix − vocals`)
2. `vocals → Karaoke 2 → backing`, and `lead = vocals − backing`

Because lead is computed by subtraction, `lead + backing` always reconstructs
stage 1's vocals exactly. Nothing is lost between the two stems, but bleed is
possible in either direction.

**Quality expectations.** Lead/backing is the hardest common stem task. Doubled
leads, unison harmonies and heavy stereo effects confuse every model, including
the paid web services. MDX-Net Karaoke 2 is a solid, well-tested baseline. The
current state of the art, Mel-Band RoFormer Karaoke, has no ready-made ONNX
export. Supporting it is a tracked follow-up, and the engine's model-family
seam (`ModelFamily` in `engine/src/model.rs`) is where it plugs in.

**Model licensing.** The weights are community models distributed through the
UVR project. They're fine for personal use. Check their terms before any
commercial distribution of bundles that embed them.

**Platform.** v1 targets Apple Silicon macOS (CoreML on the Neural Engine and
GPU). Windows code paths (DirectML, CUDA) are compiled behind `cfg` gates but
not built or tested yet.

**Measured performance** (M2 Max, 3:10 stereo 44.1 kHz FLAC, both stages, overlap 0.5):

| Device | Wall time | Peak RAM | Stems vs CPU reference |
| --- | --- | --- | --- |
| CoreML (`auto`) | 13.1 s | 0.46 GB | 56–67 dB SNR (inaudible) |
| CPU (`--device=cpu`) | 89.6 s | ~4.5 GB | reference |

Null test: `lead + backing + instrumental` reconstructs the input at 162 dB
SNR (float precision). Run `cargo run --release --example stem_check` to
repeat both checks on your own files.
