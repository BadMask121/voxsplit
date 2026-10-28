//! ONNX Runtime implementation of [`SpectrogramModel`].

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow, ensure};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

use crate::device::{Candidate, Device, candidates};
use crate::error::EngineError;
use crate::model::{ModelConfig, SpectrogramModel};

pub struct OrtModel {
    session: Session,
    cfg: ModelConfig,
    batch: usize,
}

impl OrtModel {
    /// Load `path` on the first candidate device that accepts it. Returns the
    /// model and the name of the device it ended up on. `on_fallback(device,
    /// reason)` is called for each device skipped during `Device::Auto`.
    pub fn load(
        env: &Environment,
        path: &Path,
        cfg: &ModelConfig,
        device: Device,
        batch: usize,
        coreml_cache: Option<&Path>,
        mut on_fallback: impl FnMut(&str, &str),
    ) -> Result<(Self, &'static str), EngineError> {
        ensure_file(path).map_err(EngineError::ModelLoad)?;
        // Fixed-shape graphs (RoFormer exports) take one chunk per call.
        let batch = if cfg.supports_batching() { batch } else { 1 };
        let cache = coreml_cache.map(|base| model_cache_dir(base, path));
        let list = candidates(device, cfg.compute_units, cache.as_deref())
            .map_err(|e| EngineError::ModelLoad(anyhow!(e)))?;
        let last = list.len() - 1;
        for (i, candidate) in list.into_iter().enumerate() {
            let name = candidate.name;
            match build_session(env, path, cfg, batch, candidate) {
                Ok(session) => {
                    let model = Self {
                        session,
                        cfg: cfg.clone(),
                        batch,
                    };
                    model.check_signature().map_err(EngineError::ModelLoad)?;
                    if let Some(dir) = &cache {
                        prune_cache(dir);
                    }
                    return Ok((model, name));
                }
                Err(e) if i < last => on_fallback(name, &format!("{e:#}")),
                Err(e) => {
                    return Err(EngineError::ModelLoad(
                        e.context(format!("cannot load model on {name}")),
                    ));
                }
            }
        }
        unreachable!("candidate list is never empty")
    }

    fn check_signature(&self) -> Result<()> {
        ensure!(
            self.session.inputs().len() == 1,
            "expected a single model input"
        );
        ensure!(!self.session.outputs().is_empty(), "model has no outputs");
        if let Some(shape) = self.session.inputs()[0].dtype().tensor_shape() {
            let want = self.cfg.input_shape(self.batch).map(|d| d as i64);
            ensure!(
                shape.len() == 4,
                "model input has rank {}, expected 4",
                shape.len()
            );
            for (axis, (&got, &want)) in shape.iter().zip(&want).enumerate() {
                ensure!(
                    got <= 0 || got == want,
                    "model input axis {axis} is {got} but the sidecar implies {want} (wrong sidecar for this model?)"
                );
            }
        }
        Ok(())
    }
}

/// One Core ML cache folder per model (by file name and size). ONNX Runtime keys
/// compiled models by the model's path, so the same model run from the repo, the
/// app and each plugin bundle would otherwise pile up copies of several hundred MB.
fn model_cache_dir(base: &Path, model: &Path) -> PathBuf {
    let stem = model
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model");
    let size = fs::metadata(model).map(|m| m.len()).unwrap_or(0);
    let dir = base.join(format!("{stem}-{size}"));
    let _ = fs::create_dir_all(&dir);
    // Entries from before per-model folders are bare numeric hashes in `base`.
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
    dir
}

/// Keep only the most recently written compiled model in a model's cache folder.
fn prune_cache(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut compiled: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (modified, e.path())
        })
        .collect();
    compiled.sort();
    compiled.pop(); // the newest is the one this session uses
    for (_, stale) in compiled {
        let _ = fs::remove_dir_all(stale);
    }
}

fn ensure_file(path: &Path) -> Result<()> {
    ensure!(path.is_file(), "model file not found: {}", path.display());
    Ok(())
}

fn build_session(
    env: &Environment,
    path: &Path,
    cfg: &ModelConfig,
    batch: usize,
    candidate: Candidate,
) -> Result<Session> {
    let mut builder = Session::builder(env)?
        // Pin the free batch dimension so every node has a static shape (Core ML needs this).
        .with_dimension_override(&cfg.batch_dim, batch as i64)
        .and_then(|b| b.with_memory_pattern(candidate.name != "cpu"))
        .and_then(|b| b.with_execution_providers([candidate.dispatch]))
        .map_err(ort::Error::<()>::from)?;
    builder
        .commit_from_file(path)
        .with_context(|| format!("failed to initialise {}", path.display()))
}

impl SpectrogramModel for OrtModel {
    fn run(&mut self, input: &[f32], batch: usize) -> Result<Vec<f32>> {
        let one = self.cfg.input_len();
        ensure!(
            batch <= self.batch && input.len() == batch * one,
            "bad input batch"
        );
        // The session's batch size is fixed; pad a short final group with silence.
        let mut data = input.to_vec();
        data.resize(self.batch * one, 0.0);
        let shape = self.cfg.input_shape(self.batch);
        let tensor = Tensor::from_array((shape, data))?;
        let outputs = self.session.run(ort::inputs![tensor])?;
        let (_, out) = outputs[0].try_extract_tensor::<f32>()?;
        ensure!(
            out.len() >= batch * one,
            "model output has {} values, expected {}",
            out.len(),
            self.batch * one
        );
        Ok(out[..batch * one].to_vec())
    }
}
