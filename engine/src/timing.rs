//! How long a run will take. Each model has a load time and a time per chunk;
//! defaults come from the model sidecars (measured on an M2 Max), and every run
//! records what this machine actually did so later estimates fit it.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::ModelConfig;
use crate::separator::chunk_count;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct Timing {
    pub load_sec: f64,
    pub chunk_sec: f64,
}

impl Timing {
    /// Seconds for one pass of `cfg` over `samples` samples at its sample rate.
    pub fn stage_sec(
        &self,
        cfg: &ModelConfig,
        samples: usize,
        overlap: f32,
        with_load: bool,
    ) -> f64 {
        let chunks = chunk_count(cfg, samples, overlap) as f64;
        (if with_load { self.load_sec } else { 0.0 }) + chunks * self.chunk_sec
    }
}

/// Learned timings keyed by `model name|device`, persisted as JSON.
#[derive(Default)]
pub struct TimingStore {
    path: Option<PathBuf>,
    learned: BTreeMap<String, Timing>,
}

impl TimingStore {
    pub fn open(path: Option<PathBuf>) -> Self {
        let learned = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { path, learned }
    }

    fn key(cfg: &ModelConfig, device: &str) -> String {
        format!("{}|{device}", cfg.name)
    }

    pub fn get(&self, cfg: &ModelConfig, device: &str) -> Timing {
        if let Some(t) = self.learned.get(&Self::key(cfg, device)) {
            return *t;
        }
        if let Some(t) = cfg.timing.get(device) {
            return *t;
        }
        // An unknown model or device: a cautious guess the first run corrects.
        match device {
            "cpu" => Timing {
                load_sec: 2.0,
                chunk_sec: 3.0,
            },
            _ => Timing {
                load_sec: 10.0,
                chunk_sec: 0.5,
            },
        }
    }

    /// Blend a measurement into what's known; each new run counts for half.
    pub fn record(&mut self, cfg: &ModelConfig, device: &str, measured: Timing) {
        let key = Self::key(cfg, device);
        let blended = match self.learned.get(&key) {
            Some(old) => Timing {
                load_sec: 0.5 * (old.load_sec + measured.load_sec),
                chunk_sec: 0.5 * (old.chunk_sec + measured.chunk_sec),
            },
            None => measured,
        };
        self.learned.insert(key, blended);
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let Ok(json) = serde_json::to_string_pretty(&self.learned) else {
            return;
        };
        let partial = path.with_extension("json.partial");
        if fs::write(&partial, json).is_ok() {
            let _ = fs::rename(&partial, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Arch, ComputeUnits};

    fn cfg() -> ModelConfig {
        ModelConfig {
            name: "m".into(),
            sample_rate: 44_100,
            n_fft: 512,
            hop: 128,
            compensate: 1.0,
            primary_stem: "vocals".into(),
            batch_dim: "batch_size".into(),
            default_overlap: None,
            compute_units: ComputeUnits::All,
            timing: [(
                "coreml".to_string(),
                Timing {
                    load_sec: 1.0,
                    chunk_sec: 0.1,
                },
            )]
            .into(),
            arch: Arch::Mdx {
                dim_f: 257,
                dim_t: 64,
            },
        }
    }

    #[test]
    fn learned_timings_override_defaults_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timings.json");
        let mut store = TimingStore::open(Some(path.clone()));
        assert_eq!(store.get(&cfg(), "coreml").chunk_sec, 0.1);

        store.record(
            &cfg(),
            "coreml",
            Timing {
                load_sec: 3.0,
                chunk_sec: 0.3,
            },
        );
        store.record(
            &cfg(),
            "coreml",
            Timing {
                load_sec: 1.0,
                chunk_sec: 0.1,
            },
        );
        store.save();

        let reopened = TimingStore::open(Some(path));
        let t = reopened.get(&cfg(), "coreml");
        assert!((t.load_sec - 2.0).abs() < 1e-9 && (t.chunk_sec - 0.2).abs() < 1e-9);
        assert_eq!(
            reopened.get(&cfg(), "cpu").chunk_sec,
            3.0,
            "no data for cpu: generic guess"
        );
    }

    #[test]
    fn stage_estimate_scales_with_length() {
        let t = Timing {
            load_sec: 2.0,
            chunk_sec: 0.5,
        };
        let short = t.stage_sec(&cfg(), 44_100 * 10, 0.5, true);
        let long = t.stage_sec(&cfg(), 44_100 * 100, 0.5, true);
        assert!(long > short * 5.0);
        assert!((t.stage_sec(&cfg(), 44_100 * 10, 0.5, false) - (short - 2.0)).abs() < 1e-9);
    }
}
