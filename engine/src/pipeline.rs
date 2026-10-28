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
use crate::device::Device;
use crate::error::EngineError;
use crate::events::{self, Event, Level, StageName, Stem};
use crate::model::{ModelConfig, SpectrogramModel};
use crate::onnx::OrtModel;
use crate::separator::{self, SeparateOptions};

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
    pub separate: SeparateOptions,
    pub bit_depth: BitDepth,
    pub coreml_cache: Option<PathBuf>,
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
    events::emit(&Event::Start {
        version: events::VERSION,
        stages,
        duration_sec: decoded.audio.len() as f64 / source_rate as f64,
        sample_rate: source_rate,
        channels: decoded.source_channels,
    });
    cancel.check()?;

    let work_rate = karaoke_cfg.sample_rate;
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
            let mut model = load(env, job, &job.vocal_model, cfg)?;
            events::emit(&Event::Stage {
                stage: StageName::Vocals,
                index: 1,
                stages,
                model: &cfg.name,
                device: model.1,
            });
            let mut vocals = Stereo::with_capacity(mix.len());
            let mut progress = Progress::new(StageName::Vocals, 0, stages);
            let comp = cfg.compensate;
            separator::separate(
                &mut model.0,
                cfg,
                &mix,
                job.separate,
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
            vocals
        }
        None => mix,
    };
    cancel.check()?;

    // Stage 2 and refinement passes, with one karaoke session for all of them.
    // Stage 1's session was dropped above, so only one model is resident at a time.
    let mut model = load(env, job, &job.karaoke_model, &karaoke_cfg)?;
    let comp = karaoke_cfg.compensate;
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
        let mut progress = Progress::new(stage, index - 1, stages);
        let mut found = Stereo::with_capacity(lead_audio.len());
        separator::separate(
            &mut model.0,
            &karaoke_cfg,
            &lead_audio,
            job.separate,
            |done, total| {
                cancel.check()?;
                progress.update(done, total);
                Ok(())
            },
            |_, l, r| {
                let bl: Vec<f32> = l.iter().map(|v| v * comp).collect();
                let br: Vec<f32> = r.iter().map(|v| v * comp).collect();
                found.extend(&bl, &br);
                Ok(())
            },
        )?;
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

/// Emits `progress` events, throttled to 0.5% steps.
struct Progress {
    stage: StageName,
    index: u8,
    stages: u8,
    last: f32,
}

impl Progress {
    fn new(stage: StageName, index: u8, stages: u8) -> Self {
        Self {
            stage,
            index,
            stages,
            last: -1.0,
        }
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
            });
        }
    }
}
