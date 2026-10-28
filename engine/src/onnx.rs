//! ONNX Runtime implementation of [`SpectrogramModel`].

use std::path::Path;

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
        let list =
            candidates(device, coreml_cache).map_err(|e| EngineError::ModelLoad(anyhow!(e)))?;
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
            let want = [
                self.batch as i64,
                4,
                self.cfg.dim_f as i64,
                self.cfg.dim_t as i64,
            ];
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
        let shape = [self.batch, 4, self.cfg.dim_f, self.cfg.dim_t];
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
