//! Model description (sidecar JSON) and the inference seam the separator drives.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

/// Architecture family. Each family defines how audio becomes a model input.
/// Only MDX-Net (spectrogram in, spectrogram out) is implemented; RoFormer
/// models (complex mask out) would be a second variant.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelFamily {
    Mdx,
}

/// Contents of `<model>.json` next to `<model>.onnx`.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub family: ModelFamily,
    pub sample_rate: u32,
    pub n_fft: usize,
    pub hop: usize,
    pub dim_f: usize,
    pub dim_t: usize,
    pub compensate: f32,
    pub primary_stem: String,
    /// Symbolic batch dimension in the ONNX graph, pinned so CoreML sees static shapes.
    #[serde(default = "default_batch_dim")]
    pub batch_dim: String,
}

fn default_batch_dim() -> String {
    "batch_size".into()
}

impl ModelConfig {
    /// Load and validate the sidecar for `onnx_path` (same stem, `.json`).
    pub fn for_model(onnx_path: &Path) -> Result<Self> {
        let sidecar = sidecar_path(onnx_path);
        let text = fs::read_to_string(&sidecar)
            .with_context(|| format!("missing model sidecar {}", sidecar.display()))?;
        let cfg: Self =
            serde_json::from_str(&text).with_context(|| format!("invalid model sidecar {}", sidecar.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.n_fft % 2 == 0 && self.n_fft >= 64, "n_fft must be even and >= 64");
        ensure!(self.hop > 0 && self.hop <= self.n_fft, "hop must be in 1..=n_fft");
        ensure!(self.dim_f >= 4 && self.dim_f <= self.n_fft / 2 + 1, "dim_f must be <= n_fft/2 + 1");
        ensure!(self.dim_t >= 2, "dim_t must be >= 2");
        ensure!(self.chunk_len() > self.n_fft, "chunk (hop * (dim_t - 1)) must exceed n_fft");
        ensure!(self.compensate.is_finite() && self.compensate > 0.0, "compensate must be positive");
        ensure!(self.sample_rate > 0, "sample_rate must be positive");
        Ok(())
    }

    /// Samples per inference chunk; the STFT of a chunk has exactly `dim_t` frames.
    pub fn chunk_len(&self) -> usize {
        self.hop * (self.dim_t - 1)
    }

    /// Floats in one `[4, dim_f, dim_t]` model input.
    pub fn input_len(&self) -> usize {
        4 * self.dim_f * self.dim_t
    }
}

pub fn sidecar_path(onnx_path: &Path) -> PathBuf {
    onnx_path.with_extension("json")
}

/// Anything that maps a batch of `[4, dim_f, dim_t]` spectrograms to the
/// spectrograms of the model's primary stem. Implemented by the ONNX Runtime
/// session and, in tests, by pass-through mocks.
pub trait SpectrogramModel {
    fn run(&mut self, input: &[f32], batch: usize) -> Result<Vec<f32>>;
}
