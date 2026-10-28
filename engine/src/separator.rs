//! Chunked spectrogram-model inference with click-free, memory-bounded overlap-add.
//!
//! Per chunk, MDX-Net models map a band-limited spectrogram to a spectrogram;
//! RoFormer models map the full spectrogram to a complex mask that the host
//! applies. Either way the chunk comes back as a waveform. The chunking mirrors
//! UVR's `SeperateMDX.demix`: the signal is padded with `n_fft/2`
//! samples of silence in front (so STFT edge artefacts land in discarded
//! padding), cut into `hop * (dim_t - 1)`-sample chunks every `step` samples,
//! and each chunk's output is weighted by a Hann window and normalised by the
//! running weight sum. Instead of keeping full-length accumulators, samples are
//! normalised and handed to the sink as soon as no later chunk can touch them,
//! so memory stays at about one chunk per stem.

use realfft::num_complex::Complex32;

use crate::audio::Stereo;
use crate::error::EngineError;
use crate::model::{Arch, ModelConfig, SpectrogramModel};
use crate::stft::{Stft, symmetric_hann};

#[derive(Clone, Copy, Debug)]
pub struct SeparateOptions {
    /// Fraction of each chunk shared with the next one (0.0..=0.95).
    pub overlap: f32,
    /// Chunks per model call.
    pub batch: usize,
    /// Average `f(x)` with `-f(-x)` to cancel model noise (2x cost). MDX only:
    /// a mask model's output doesn't flip sign with its input.
    pub denoise: bool,
}

impl SeparateOptions {
    /// Model calls per chunk: denoising runs MDX models twice.
    pub fn runs_per_chunk(&self, cfg: &ModelConfig) -> f64 {
        if self.denoise && matches!(cfg.arch, Arch::Mdx { .. }) {
            2.0
        } else {
            1.0
        }
    }
}

impl Default for SeparateOptions {
    fn default() -> Self {
        Self {
            overlap: 0.5,
            batch: 1,
            denoise: true,
        }
    }
}

/// Number of chunks `separate` will run for `len` input samples.
pub fn chunk_count(cfg: &ModelConfig, len: usize, overlap: f32) -> usize {
    let g = Geometry::new(cfg, len, overlap);
    g.padded_len.div_ceil(g.step)
}

struct Geometry {
    chunk: usize,
    trim: usize,
    step: usize,
    padded_len: usize,
}

impl Geometry {
    fn new(cfg: &ModelConfig, len: usize, overlap: f32) -> Self {
        let chunk = cfg.chunk_len();
        let trim = cfg.n_fft / 2;
        let gen_size = chunk - 2 * trim;
        let pad = gen_size + trim - (len % gen_size);
        // Chunks always share at least n_fft samples (UVR's default step).
        // Abutting chunks would expose each chunk's STFT edge artefacts as clicks.
        let step = (((1.0 - overlap) * chunk as f32) as usize).clamp(1, chunk - cfg.n_fft);
        Self {
            chunk,
            trim,
            step,
            padded_len: trim + len + pad,
        }
    }
}

/// Run `model` over `input` and stream the primary-stem estimate to `sink` as
/// `(start_sample, left, right)` slices, in order, covering exactly
/// `0..input.len()`. `on_chunk(done, total)` runs after every chunk and can
/// abort the run by returning an error (cancellation).
pub fn separate(
    model: &mut dyn SpectrogramModel,
    cfg: &ModelConfig,
    input: &Stereo,
    opts: SeparateOptions,
    mut on_chunk: impl FnMut(usize, usize) -> Result<(), EngineError>,
    mut sink: impl FnMut(usize, &[f32], &[f32]) -> Result<(), EngineError>,
) -> Result<(), EngineError> {
    let len = input.len();
    let g = Geometry::new(cfg, len, opts.overlap);
    let total = g.padded_len.div_ceil(g.step);
    let batch = opts.batch.max(1);
    let mut stft = Stft::new(cfg.n_fft, cfg.hop);
    let mut acc = Accumulator::new(g.chunk, g.trim, len);

    let starts: Vec<usize> = (0..g.padded_len).step_by(g.step).collect();
    let mut done = 0;
    let denoise = opts.runs_per_chunk(cfg) > 1.0;
    for group in starts.chunks(batch) {
        let mut model_in = Vec::with_capacity(group.len() * cfg.input_len());
        let mut chunk_specs = Vec::with_capacity(group.len());
        for &start in group {
            let part = padded_slice(input, start, g.chunk, g.trim);
            chunk_specs.push(encode(&mut stft, cfg, &part, &mut model_in));
        }

        let model_out = run_model(model, &model_in, group.len(), denoise)?;
        if model_out.len() != model_in.len() {
            return Err(EngineError::Inference(anyhow::anyhow!(
                "model returned {} values, expected {}",
                model_out.len(),
                model_in.len()
            )));
        }

        for (i, &start) in group.iter().enumerate() {
            let one = &model_out[i * cfg.input_len()..(i + 1) * cfg.input_len()];
            let wave = decode(&mut stft, cfg, &chunk_specs[i], one);
            let actual = (g.padded_len - start).min(g.chunk);
            acc.add(start, &wave, &symmetric_hann(actual));
            let next = start + g.step;
            acc.flush_until(next.min(g.padded_len), &mut sink)?;
            done += 1;
            on_chunk(done, total)?;
        }
    }
    acc.flush_until(g.padded_len, &mut sink)?;
    Ok(())
}

fn run_model(
    model: &mut dyn SpectrogramModel,
    input: &[f32],
    batch: usize,
    denoise: bool,
) -> Result<Vec<f32>, EngineError> {
    let mut out = model.run(input, batch).map_err(EngineError::Inference)?;
    if denoise {
        let negated: Vec<f32> = input.iter().map(|v| -v).collect();
        let neg_out = model.run(&negated, batch).map_err(EngineError::Inference)?;
        for (o, n) in out.iter_mut().zip(neg_out) {
            *o = 0.5 * *o - 0.5 * n;
        }
    }
    Ok(out)
}

/// `chunk` samples of the virtual signal `[trim zeros | input | zeros...]` starting at `start`.
fn padded_slice(input: &Stereo, start: usize, chunk: usize, trim: usize) -> Stereo {
    let mut part = Stereo::zeros(chunk);
    for i in 0..chunk {
        let pos = start + i;
        if pos >= trim && pos - trim < input.len() {
            part.l[i] = input.l[pos - trim];
            part.r[i] = input.r[pos - trim];
        }
    }
    part
}

/// A chunk's STFT, kept for decoding when the model returns a mask.
type ChunkSpec = Option<[Vec<Complex32>; 2]>;

/// Append one chunk's model input to `out`.
fn encode(stft: &mut Stft, cfg: &ModelConfig, part: &Stereo, out: &mut Vec<f32>) -> ChunkSpec {
    let base = out.len();
    out.resize(base + cfg.input_len(), 0.0);
    let bins = stft.bins();
    let frames = cfg.frames();
    let specs = [stft.forward(&part.l), stft.forward(&part.r)];
    match cfg.arch {
        // [4, dim_f, frames]: channels [L.re, L.im, R.re, R.im]; bins above dim_f
        // dropped, bins 0..3 zeroed (UVR does the same).
        Arch::Mdx { dim_f, .. } => {
            let plane = dim_f * frames;
            for (c, spec) in specs.iter().enumerate() {
                for t in 0..frames {
                    for f in 3..dim_f {
                        let v = spec[t * bins + f];
                        out[base + (2 * c) * plane + f * frames + t] = v.re;
                        out[base + (2 * c + 1) * plane + f * frames + t] = v.im;
                    }
                }
            }
            None
        }
        // [2·bins, frames, 2], row f·2 + channel (BSRoformer's `b s f t c -> b (f s) t c`).
        Arch::Roformer { .. } => {
            for (c, spec) in specs.iter().enumerate() {
                for t in 0..frames {
                    for f in 0..bins {
                        let v = spec[t * bins + f];
                        let i = base + ((f * 2 + c) * frames + t) * 2;
                        out[i] = v.re;
                        out[i + 1] = v.im;
                    }
                }
            }
            Some(specs)
        }
    }
}

/// Turn one chunk's model output back into audio.
fn decode(stft: &mut Stft, cfg: &ModelConfig, spec: &ChunkSpec, out: &[f32]) -> Stereo {
    let bins = stft.bins();
    let frames = cfg.frames();
    let mut channels = [Vec::new(), Vec::new()];
    for (c, channel) in channels.iter_mut().enumerate() {
        let mut full = vec![Complex32::new(0.0, 0.0); frames * bins];
        match (cfg.arch, spec) {
            // Spectrogram out: zero-fill the bins above dim_f.
            (Arch::Mdx { dim_f, .. }, _) => {
                let plane = dim_f * frames;
                for t in 0..frames {
                    for f in 0..dim_f {
                        full[t * bins + f] = Complex32::new(
                            out[(2 * c) * plane + f * frames + t],
                            out[(2 * c + 1) * plane + f * frames + t],
                        );
                    }
                }
            }
            // Complex mask out: multiply it into the chunk's own STFT.
            (Arch::Roformer { zero_dc, .. }, Some(specs)) => {
                for t in 0..frames {
                    for f in 0..bins {
                        let i = ((f * 2 + c) * frames + t) * 2;
                        let mask = Complex32::new(out[i], out[i + 1]);
                        full[t * bins + f] = specs[c][t * bins + f] * mask;
                    }
                    if zero_dc {
                        full[t * bins] = Complex32::new(0.0, 0.0);
                    }
                }
            }
            (Arch::Roformer { .. }, None) => unreachable!("encode keeps the STFT for mask models"),
        }
        *channel = stft.inverse(&full, frames);
    }
    let [l, r] = channels;
    Stereo { l, r }
}

/// Weighted overlap-add over a sliding window of `chunk` samples. Positions are
/// in padded coordinates; the sink sees input coordinates (`pos - trim`).
struct Accumulator {
    base: usize,
    sum: Stereo,
    weight: Vec<f32>,
    trim: usize,
    len: usize,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
}

impl Accumulator {
    fn new(chunk: usize, trim: usize, len: usize) -> Self {
        Self {
            base: 0,
            sum: Stereo::zeros(chunk),
            weight: vec![0.0; chunk],
            trim,
            len,
            out_l: Vec::new(),
            out_r: Vec::new(),
        }
    }

    fn add(&mut self, start: usize, wave: &Stereo, weights: &[f32]) {
        debug_assert!(start >= self.base);
        let offset = start - self.base;
        let needed = offset + weights.len();
        if needed > self.weight.len() {
            self.sum.l.resize(needed, 0.0);
            self.sum.r.resize(needed, 0.0);
            self.weight.resize(needed, 0.0);
        }
        for (i, &w) in weights.iter().enumerate() {
            self.sum.l[offset + i] += wave.l[i] * w;
            self.sum.r[offset + i] += wave.r[i] * w;
            self.weight[offset + i] += w;
        }
    }

    /// Normalise and emit every position below `end`; no later chunk overlaps them.
    fn flush_until(
        &mut self,
        end: usize,
        sink: &mut impl FnMut(usize, &[f32], &[f32]) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        if end <= self.base {
            return Ok(());
        }
        let n = (end - self.base).min(self.weight.len());
        self.out_l.clear();
        self.out_r.clear();
        let mut first_out = None;
        for i in 0..n {
            let pos = self.base + i;
            if pos < self.trim || pos - self.trim >= self.len {
                continue;
            }
            first_out.get_or_insert(pos - self.trim);
            let w = self.weight[i];
            let (l, r) = if w > 1e-8 {
                (self.sum.l[i] / w, self.sum.r[i] / w)
            } else {
                (0.0, 0.0)
            };
            self.out_l.push(l);
            self.out_r.push(r);
        }
        if let Some(start) = first_out {
            sink(start, &self.out_l, &self.out_r)?;
        }
        self.sum.l.drain(..n);
        self.sum.r.drain(..n);
        self.weight.drain(..n);
        self.base += n;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ComputeUnits;

    /// Returns its input: separation should then reproduce the (band-limited) mix.
    struct Passthrough;
    impl SpectrogramModel for Passthrough {
        fn run(&mut self, input: &[f32], _batch: usize) -> anyhow::Result<Vec<f32>> {
            Ok(input.to_vec())
        }
    }

    /// Small geometry so tests run fast: chunk = 128 * 63 = 8064 samples.
    fn small_cfg() -> ModelConfig {
        ModelConfig {
            name: "test".into(),
            sample_rate: 44_100,
            n_fft: 512,
            hop: 128,
            compensate: 1.0,
            primary_stem: "vocals".into(),
            batch_dim: "batch_size".into(),
            default_overlap: None,
            compute_units: ComputeUnits::All,
            timing: Default::default(),
            arch: Arch::Mdx {
                dim_f: 257,
                dim_t: 64,
            },
        }
    }

    /// Content well inside bins 3..dim_f, with soft edges, survives the
    /// model's band limits intact (the passthrough zeroes bins 0..3).
    fn band_limited_signal(len: usize) -> Stereo {
        let fade = 1_500usize;
        let l: Vec<f32> = (0..len)
            .map(|i| {
                let t = i as f32 / 44_100.0;
                let edge = i.min(len - 1 - i);
                let env = if edge < fade {
                    0.5 - 0.5 * (std::f32::consts::PI * edge as f32 / fade as f32).cos()
                } else {
                    1.0
                };
                env * (0.4 * (2.0 * std::f32::consts::PI * 2_000.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * (1_500.0 + 2_000.0 * t) * t).sin())
            })
            .collect();
        Stereo {
            r: l.iter().map(|s| 0.5 * s).collect(),
            l,
        }
    }

    fn collect(cfg: &ModelConfig, input: &Stereo, opts: SeparateOptions) -> (Stereo, usize, usize) {
        let mut out = Stereo::default();
        let mut chunks = 0;
        let mut last_total = 0;
        separate(
            &mut Passthrough,
            cfg,
            input,
            opts,
            |done, total| {
                chunks = done;
                last_total = total;
                Ok(())
            },
            |start, l, r| {
                assert_eq!(
                    start,
                    out.len(),
                    "sink must receive contiguous, in-order slices"
                );
                out.extend(l, r);
                Ok(())
            },
        )
        .unwrap();
        (out, chunks, last_total)
    }

    #[test]
    fn passthrough_reconstructs_input_across_chunk_boundaries() {
        let cfg = small_cfg();
        for &overlap in &[0.0, 0.25, 0.5, 0.75] {
            for &len in &[20_000usize, 33_333, 8_064 * 3 + 1] {
                let input = band_limited_signal(len);
                let (out, chunks, total) = collect(
                    &cfg,
                    &input,
                    SeparateOptions {
                        overlap,
                        ..Default::default()
                    },
                );
                assert_eq!(out.len(), len, "overlap {overlap}, len {len}");
                assert_eq!(chunks, total);
                assert_eq!(total, chunk_count(&cfg, len, overlap));
                let max_err = input
                    .l
                    .iter()
                    .zip(&out.l)
                    .chain(input.r.iter().zip(&out.r))
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0, f32::max);
                assert!(
                    max_err < 1e-3,
                    "overlap {overlap}, len {len}: max error {max_err}"
                );
            }
        }
    }

    #[test]
    fn no_discontinuity_at_chunk_seams() {
        // A model that scales chunks differently would click at seams; here every
        // chunk sees the same passthrough, so the first difference must stay smooth.
        let cfg = small_cfg();
        let input = band_limited_signal(40_000);
        let (out, ..) = collect(
            &cfg,
            &input,
            SeparateOptions {
                overlap: 0.5,
                ..Default::default()
            },
        );
        let max_in_step = input
            .l
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        let max_out_step = out
            .l
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(
            max_out_step <= max_in_step * 1.01 + 1e-4,
            "out step {max_out_step} vs in {max_in_step}"
        );
    }

    #[test]
    fn batching_and_denoise_do_not_change_passthrough_result() {
        let cfg = small_cfg();
        let input = band_limited_signal(30_000);
        let (a, ..) = collect(&cfg, &input, SeparateOptions::default());
        let (b, ..) = collect(
            &cfg,
            &input,
            SeparateOptions {
                batch: 3,
                denoise: true,
                ..Default::default()
            },
        );
        let max_diff =
            a.l.iter()
                .zip(&b.l)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0, f32::max);
        assert!(
            max_diff < 1e-5,
            "batch/denoise changed output by {max_diff}"
        );
    }

    #[test]
    fn cancellation_stops_between_chunks() {
        let cfg = small_cfg();
        let input = band_limited_signal(50_000);
        let mut seen = 0;
        let result = separate(
            &mut Passthrough,
            &cfg,
            &input,
            SeparateOptions::default(),
            |done, _| {
                seen = done;
                if done == 2 {
                    Err(EngineError::Cancelled)
                } else {
                    Ok(())
                }
            },
            |_, _, _| Ok(()),
        );
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert_eq!(seen, 2);
    }

    /// A RoFormer whose mask is 1 everywhere: the chunk's own STFT comes back unchanged.
    struct UnitMask;
    impl SpectrogramModel for UnitMask {
        fn run(&mut self, input: &[f32], _batch: usize) -> anyhow::Result<Vec<f32>> {
            Ok(input.chunks(2).flat_map(|_| [1.0, 0.0]).collect())
        }
    }

    #[test]
    fn roformer_unit_mask_reconstructs_input() {
        let cfg = ModelConfig {
            primary_stem: "lead".into(),
            arch: Arch::Roformer {
                chunk_len: 128 * 63,
                zero_dc: true,
            },
            ..small_cfg()
        };
        for &overlap in &[0.5, 0.75] {
            let input = band_limited_signal(30_000);
            let mut out = Stereo::default();
            separate(
                &mut UnitMask,
                &cfg,
                &input,
                SeparateOptions {
                    overlap,
                    ..Default::default()
                },
                |_, _| Ok(()),
                |start, l, r| {
                    assert_eq!(start, out.len());
                    out.extend(l, r);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(out.len(), input.len());
            let max_err = input
                .l
                .iter()
                .zip(&out.l)
                .chain(input.r.iter().zip(&out.r))
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(max_err < 1e-3, "overlap {overlap}: max error {max_err}");
        }
    }

    #[test]
    fn mdx_geometry_chunk_counts() {
        let kim = ModelConfig {
            n_fft: 7680,
            hop: 1024,
            arch: Arch::Mdx {
                dim_f: 3072,
                dim_t: 256,
            },
            ..small_cfg()
        };
        assert_eq!(kim.chunk_len(), 261_120);
        // 3:30 at 44.1 kHz with 50% overlap.
        let n = chunk_count(&kim, 44_100 * 210, 0.5);
        assert!((70..=75).contains(&n), "{n} chunks");
    }
}
