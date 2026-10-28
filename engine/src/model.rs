//! Model description (sidecar JSON) and the inference seam the separator drives.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::timing::Timing;

/// Architecture family: how a chunk of audio becomes a model input and back.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "family", rename_all = "lowercase")]
pub enum Arch {
    /// MDX-Net: band-limited spectrogram in, spectrogram of the primary stem out.
    /// Input `[batch, 4, dim_f, dim_t]`, channels `[L.re, L.im, R.re, R.im]`.
    Mdx { dim_f: usize, dim_t: usize },
    /// BS-RoFormer with the STFT on the host (tools/roformer/export.py): full stereo
    /// spectrogram in as `[1, 2·bins, frames, 2]` (index `f·2 + channel`), complex
    /// mask for the primary stem out, same shape.
    Roformer {
        chunk_len: usize,
        #[serde(default = "yes")]
        zero_dc: bool,
    },
}

fn yes() -> bool {
    true
}

/// Which Core ML compute units a model runs best on (measured per model).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComputeUnits {
    #[default]
    All,
    Gpu,
    Ane,
    Cpu,
}

/// Contents of `<model>.json` next to `<model>.onnx`.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub sample_rate: u32,
    pub n_fft: usize,
    pub hop: usize,
    #[serde(default = "unity")]
    pub compensate: f32,
    /// What the model outputs: "vocals", "instrumental" (= backing, when fed vocals) or "lead".
    pub primary_stem: String,
    /// Symbolic batch dimension in the ONNX graph, pinned so CoreML sees static shapes.
    #[serde(default = "default_batch_dim")]
    pub batch_dim: String,
    /// Overlap to use when the CLI doesn't set one.
    #[serde(default)]
    pub default_overlap: Option<f32>,
    #[serde(default)]
    pub compute_units: ComputeUnits,
    /// Default load / per-chunk times by device ("coreml", "cpu"), for estimates
    /// before this machine has measured its own.
    #[serde(default)]
    pub timing: BTreeMap<String, Timing>,
    #[serde(flatten)]
    pub arch: Arch,
}

fn unity() -> f32 {
    1.0
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
        let cfg: Self = serde_json::from_str(&text)
            .with_context(|| format!("invalid model sidecar {}", sidecar.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.n_fft.is_multiple_of(2) && self.n_fft >= 64,
            "n_fft must be even and >= 64"
        );
        ensure!(
            self.hop > 0 && self.hop <= self.n_fft,
            "hop must be in 1..=n_fft"
        );
        match self.arch {
            Arch::Mdx { dim_f, dim_t } => {
                ensure!(
                    dim_f >= 4 && dim_f <= self.bins(),
                    "dim_f must be <= n_fft/2 + 1"
                );
                ensure!(dim_t >= 2, "dim_t must be >= 2");
            }
            Arch::Roformer { chunk_len, .. } => {
                ensure!(
                    chunk_len.is_multiple_of(self.hop),
                    "chunk_len must be a whole number of hops"
                );
            }
        }
        ensure!(
            self.chunk_len() > self.n_fft,
            "chunk length must exceed n_fft"
        );
        ensure!(
            self.compensate.is_finite() && self.compensate > 0.0,
            "compensate must be positive"
        );
        if let Some(o) = self.default_overlap {
            ensure!(
                (0.0..=0.95).contains(&o),
                "default_overlap must be in 0..=0.95"
            );
        }
        ensure!(self.sample_rate > 0, "sample_rate must be positive");
        Ok(())
    }

    pub fn bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    /// STFT frames per chunk.
    pub fn frames(&self) -> usize {
        match self.arch {
            Arch::Mdx { dim_t, .. } => dim_t,
            Arch::Roformer { chunk_len, .. } => chunk_len / self.hop + 1,
        }
    }

    /// Samples per inference chunk; its STFT has exactly `frames()` frames.
    pub fn chunk_len(&self) -> usize {
        self.hop * (self.frames() - 1)
    }

    pub fn input_shape(&self, batch: usize) -> [usize; 4] {
        match self.arch {
            Arch::Mdx { dim_f, dim_t } => [batch, 4, dim_f, dim_t],
            Arch::Roformer { .. } => [batch, 2 * self.bins(), self.frames(), 2],
        }
    }

    /// Floats in one chunk's model input (and output).
    pub fn input_len(&self) -> usize {
        self.input_shape(1).iter().product()
    }

    /// Whether the graph accepts a batch dimension other than 1.
    pub fn supports_batching(&self) -> bool {
        matches!(self.arch, Arch::Mdx { .. })
    }

    /// True when the model's output is the lead itself (RoFormer karaoke), false
    /// when it is the backing/instrumental side (MDX karaoke).
    pub fn primary_is_lead(&self) -> bool {
        self.primary_stem == "lead"
    }
}

pub fn sidecar_path(onnx_path: &Path) -> PathBuf {
    onnx_path.with_extension("json")
}

/// Anything that maps a batch of model inputs (`ModelConfig::input_shape`) to outputs
/// of the same shape. Implemented by the ONNX Runtime session and, in tests, by mocks.
pub trait SpectrogramModel {
    fn run(&mut self, input: &[f32], batch: usize) -> Result<Vec<f32>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecars_in_the_repo_parse() {
        let models = Path::new(env!("CARGO_MANIFEST_DIR")).join("../models");
        let kim = ModelConfig::for_model(&models.join("Kim_Vocal_2.onnx")).unwrap();
        assert_eq!(
            kim.arch,
            Arch::Mdx {
                dim_f: 3072,
                dim_t: 256
            }
        );
        assert_eq!(kim.chunk_len(), 261_120);

        let roformer = ModelConfig::for_model(&models.join("BS_Roformer_Karaoke.onnx")).unwrap();
        assert!(roformer.primary_is_lead());
        assert_eq!(roformer.compute_units, ComputeUnits::Gpu);
        assert_eq!(roformer.frames(), 518);
        assert_eq!(roformer.chunk_len(), 264_704);
        assert_eq!(roformer.input_shape(1), [1, 2050, 518, 2]);
    }
}
