//! The lead/backing pipeline:
//!
//! 1. `mix → vocal model → vocals` (and optionally `instrumental = mix − vocals`)
//! 2. `vocals → karaoke model → backing`, `lead = vocals − backing`
//! 3. Refinement passes: the karaoke model runs again on the current lead, and
//!    whatever backing it still finds moves from the lead into the backing stem.
//!    Each pass catches bleed the previous one missed.
//!
//! As in UVR, a model's primary output is scaled by its `compensate` factor and
//! the complementary stem is the input minus that scaled estimate, so
//! `lead + backing` reconstructs stage 1's vocals exactly after every pass.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, anyhow};
use ort::environment::Environment;

use crate::audio::{self, BitDepth, Stereo, WavSink};
use crate::cancel::Cancel;
use crate::device::{self, Device};
use crate::error::EngineError;
use crate::events::{self, Event, Level, StageName, Stem};
use crate::model::{ModelConfig, SpectrogramModel};
use crate::onnx::OrtModel;
use crate::separator::{self, SeparateOptions, chunk_count};
use crate::timing::{Timing, TimingStore};

pub struct Job {
    pub input: PathBuf,
    pub out_dir: PathBuf,
    pub vocal_model: PathBuf,
    pub karaoke_model: PathBuf,
    pub input_is_vocals: bool,
    pub keep_instrumental: bool,
    /// Karaoke passes (1 = a single split, more = refine the lead further).
    pub passes: u8,
    pub device: Device,
    /// Chunk overlap; `None` uses each model's own default.
    pub overlap: Option<f32>,
    pub separate: SeparateOptions,
    pub bit_depth: BitDepth,
    pub coreml_cache: Option<PathBuf>,
    /// Where learned per-model timings live (`None`: don't learn).
    pub timings: Option<PathBuf>,
}

impl Job {
    fn options_for(&self, cfg: &ModelConfig) -> SeparateOptions {
        SeparateOptions {
            overlap: overlap_for(self.overlap, cfg),
            ..self.separate
        }
    }
}

pub fn overlap_for(requested: Option<f32>, cfg: &ModelConfig) -> f32 {
    requested.or(cfg.default_overlap).unwrap_or(0.5)
}

/// Expected seconds per stage: the vocal stage (if any), then one entry per
/// karaoke pass. A model's load time is counted on its first stage only.
pub fn stage_estimates(
    timings: &TimingStore,
    device: &str,
    vocal: Option<&ModelConfig>,
    karaoke: &ModelConfig,
    passes: u8,
    overlap: Option<f32>,
    samples: usize,
) -> Vec<f64> {
    let mut estimates = Vec::new();
    if let Some(cfg) = vocal {
        let t = timings.get(cfg, device);
        estimates.push(t.stage_sec(cfg, samples, overlap_for(overlap, cfg), true));
    }
    let t = timings.get(karaoke, device);
    for pass in 0..passes.max(1) {
        estimates.push(t.stage_sec(karaoke, samples, overlap_for(overlap, karaoke), pass == 0));
    }
    estimates
}

pub fn run(
    env: &Environment,
    job: &Job,
    cancel: &Cancel,
) -> Result<Vec<(Stem, PathBuf)>, EngineError> {
    let started = Instant::now();

    // Validate everything cheap before spending minutes on inference.
    let karaoke_cfg = ModelConfig::for_model(&job.karaoke_model).map_err(EngineError::ModelLoad)?;
    let vocal_cfg = (!job.input_is_vocals)
        .then(|| ModelConfig::for_model(&job.vocal_model))
        .transpose()
        .map_err(EngineError::ModelLoad)?;
    if let Some(cfg) = &vocal_cfg
        && cfg.sample_rate != karaoke_cfg.sample_rate
    {
        return Err(EngineError::ModelLoad(anyhow!(
            "vocal and karaoke models must share a sample rate"
        )));
    }
    fs::create_dir_all(&job.out_dir)
        .with_context(|| format!("cannot create output folder {}", job.out_dir.display()))
        .map_err(EngineError::Write)?;

    let decoded = audio::decode(&job.input).map_err(EngineError::Decode)?;
    let passes = job.passes.max(1);
    let karaoke_stage = if vocal_cfg.is_some() { 2 } else { 1 };
    let stages = karaoke_stage - 1 + passes;
    let source_rate = decoded.sample_rate;
    let work_rate = karaoke_cfg.sample_rate;
    let work_len = (decoded.audio.len() as f64 * work_rate as f64 / source_rate as f64) as usize;

    let mut timings = TimingStore::open(job.timings.clone());
    let mut estimates = stage_estimates(
        &timings,
        device::expected_name(job.device),
        vocal_cfg.as_ref(),
        &karaoke_cfg,
        passes,
        job.overlap,
        work_len,
    );
    events::emit(&Event::Start {
        version: events::VERSION,
        stages,
        duration_sec: decoded.audio.len() as f64 / source_rate as f64,
        sample_rate: source_rate,
        channels: decoded.source_channels,
        estimate_sec: estimates.iter().sum(),
    });
    cancel.check()?;

    let mix =
        audio::resample(&decoded.audio, source_rate, work_rate).map_err(EngineError::Internal)?;
    drop(decoded);

    let sink = |stem: Stem| {
        WavSink::create(
            &job.out_dir.join(stem.file_name()),
            work_rate,
            source_rate,
            job.bit_depth,
        )
        .map_err(EngineError::Write)
    };
    let mut lead = sink(Stem::Lead)?;
    let mut backing = sink(Stem::Backing)?;
    let mut instrumental = (job.keep_instrumental && vocal_cfg.is_some())
        .then(|| sink(Stem::Instrumental))
        .transpose()?;

    // Stage 1: mix → vocals (kept in memory; stage 2 needs all of it).
    let vocals = match &vocal_cfg {
        Some(cfg) => {
            let loading = Instant::now();
            let mut model = load(env, job, &job.vocal_model, cfg)?;
            let load_sec = loading.elapsed().as_secs_f64();
            events::emit(&Event::Stage {
                stage: StageName::Vocals,
                index: 1,
                stages,
                model: &cfg.name,
                device: model.1,
            });
            let opts = job.options_for(cfg);
            let chunks = chunk_count(cfg, mix.len(), opts.overlap);
            let mut progress = Progress::new(
                StageName::Vocals,
                0,
                stages,
                chunks as f64 * timings.get(cfg, model.1).chunk_sec,
                estimates[1..].iter().sum(),
            );
            let mut vocals = Stereo::with_capacity(mix.len());
            let comp = cfg.compensate;
            separator::separate(
                &mut model.0,
                cfg,
                &mix,
                opts,
                |done, total| {
                    cancel.check()?;
                    progress.update(done, total);
                    Ok(())
                },
                |start, l, r| {
                    let vl: Vec<f32> = l.iter().map(|v| v * comp).collect();
                    let vr: Vec<f32> = r.iter().map(|v| v * comp).collect();
                    if let Some(inst) = instrumental.as_mut() {
                        let il: Vec<f32> =
                            vl.iter().zip(&mix.l[start..]).map(|(v, m)| m - v).collect();
                        let ir: Vec<f32> =
                            vr.iter().zip(&mix.r[start..]).map(|(v, m)| m - v).collect();
                        inst.write(&il, &ir).map_err(EngineError::Write)?;
                    }
                    vocals.extend(&vl, &vr);
                    Ok(())
                },
            )?;
            timings.record(
                cfg,
                model.1,
                Timing {
                    load_sec,
                    chunk_sec: progress.elapsed() / chunks.max(1) as f64,
                },
            );
            vocals
        }
        None => mix,
    };
    cancel.check()?;

    // Stage 2 and refinement passes, with one karaoke session for all of them.
    // Stage 1's session was dropped above, so only one model is resident at a time.
    let loading = Instant::now();
    let mut model = load(env, job, &job.karaoke_model, &karaoke_cfg)?;
    let load_sec = loading.elapsed().as_secs_f64();
    let comp = karaoke_cfg.compensate;
    let karaoke_opts = job.options_for(&karaoke_cfg);
    let chunks = chunk_count(&karaoke_cfg, vocals.len(), karaoke_opts.overlap);
    let chunk_sec = timings.get(&karaoke_cfg, model.1).chunk_sec;
    // The remaining passes, re-estimated for the device that actually loaded.
    for e in estimates.iter_mut().skip(karaoke_stage as usize - 1) {
        *e = chunks as f64 * chunk_sec;
    }
    let mut inference_sec = 0.0;
    // MDX karaoke models output the backing side; RoFormer karaoke models output
    // the lead itself, so the backing they find is the current lead minus it.
    let primary_is_lead = karaoke_cfg.primary_is_lead();
    let mut lead_audio = vocals;
    let mut backing_audio = Stereo::zeros(lead_audio.len());
    for pass in 0..passes {
        let index = karaoke_stage + pass;
        let stage = if pass == 0 {
            StageName::Karaoke
        } else {
            StageName::Refine
        };
        events::emit(&Event::Stage {
            stage,
            index,
            stages,
            model: &karaoke_cfg.name,
            device: model.1,
        });
        let mut progress = Progress::new(
            stage,
            index - 1,
            stages,
            chunks as f64 * chunk_sec,
            estimates[index as usize..].iter().sum(),
        );
        let mut found = Stereo::with_capacity(lead_audio.len());
        separator::separate(
            &mut model.0,
            &karaoke_cfg,
            &lead_audio,
            karaoke_opts,
            |done, total| {
                cancel.check()?;
                progress.update(done, total);
                Ok(())
            },
            |start, l, r| {
                for (i, (&el, &er)) in l.iter().zip(r).enumerate() {
                    let (el, er) = (el * comp, er * comp);
                    if primary_is_lead {
                        found.l.push(lead_audio.l[start + i] - el);
                        found.r.push(lead_audio.r[start + i] - er);
                    } else {
                        found.l.push(el);
                        found.r.push(er);
                    }
                }
                Ok(())
            },
        )?;
        inference_sec += progress.elapsed();
        // Move what this pass found from the lead to the backing stem.
        for (channel, (lead_ch, backing_ch)) in [
            (&found.l, (&mut lead_audio.l, &mut backing_audio.l)),
            (&found.r, (&mut lead_audio.r, &mut backing_audio.r)),
        ] {
            for ((lead_s, backing_s), f) in
                lead_ch.iter_mut().zip(backing_ch.iter_mut()).zip(channel)
            {
                *lead_s -= f;
                *backing_s += f;
            }
        }
        cancel.check()?;
    }
    timings.record(
        &karaoke_cfg,
        model.1,
        Timing {
            load_sec,
            chunk_sec: inference_sec / (chunks.max(1) * passes as usize) as f64,
        },
    );
    lead.write(&lead_audio.l, &lead_audio.r)
        .map_err(EngineError::Write)?;
    backing
        .write(&backing_audio.l, &backing_audio.r)
        .map_err(EngineError::Write)?;

    let mut outputs = vec![
        (Stem::Lead, lead.finish().map_err(EngineError::Write)?),
        (Stem::Backing, backing.finish().map_err(EngineError::Write)?),
    ];
    if let Some(inst) = instrumental {
        outputs.push((
            Stem::Instrumental,
            inst.finish().map_err(EngineError::Write)?,
        ));
    }
    timings.save();
    for (stem, path) in &outputs {
        events::emit(&Event::Output {
            stem: *stem,
            path: &path.to_string_lossy(),
        });
    }
    events::emit(&Event::Done {
        elapsed_sec: started.elapsed().as_secs_f64(),
    });
    Ok(outputs)
}

fn load(
    env: &Environment,
    job: &Job,
    path: &Path,
    cfg: &ModelConfig,
) -> Result<(impl SpectrogramModel, &'static str), EngineError> {
    OrtModel::load(
        env,
        path,
        cfg,
        job.device,
        job.separate.batch,
        job.coreml_cache.as_deref(),
        |device, why| {
            events::log(
                Level::Warn,
                &format!("{device} unavailable, falling back: {why}"),
            );
        },
    )
}

/// Emits `progress` events (throttled to 0.5% steps) with a time-left estimate.
struct Progress {
    stage: StageName,
    index: u8,
    stages: u8,
    last: f32,
    started: Instant,
    /// Expected seconds for this stage's inference, before any is measured.
    estimate: f64,
    /// Expected seconds for every later stage.
    after: f64,
}

impl Progress {
    fn new(stage: StageName, index: u8, stages: u8, estimate: f64, after: f64) -> Self {
        Self {
            stage,
            index,
            stages,
            last: -1.0,
            started: Instant::now(),
            estimate,
            after,
        }
    }

    fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Seconds left: blends from the prior estimate to the measured rate over the
    /// first quarter of the stage, so early, noisy chunks don't swing it.
    fn eta(&self, fraction: f64) -> f64 {
        let elapsed = self.elapsed();
        let from_estimate = (self.estimate - elapsed).max(0.0);
        let current = if fraction > 0.0 {
            let measured = elapsed / fraction * (1.0 - fraction);
            let weight = (fraction / 0.25).min(1.0);
            weight * measured + (1.0 - weight) * from_estimate
        } else {
            from_estimate
        };
        current + self.after
    }

    fn update(&mut self, done: usize, total: usize) {
        let fraction = done as f32 / total.max(1) as f32;
        if fraction - self.last >= 0.005 || done == total {
            self.last = fraction;
            let overall = (self.index as f32 + fraction) / self.stages as f32;
            events::emit(&Event::Progress {
                stage: self.stage,
                fraction,
                overall,
                eta_sec: self.eta(fraction as f64),
            });
        }
    }
}
