//! `voxsplit` — the command-line tool, and the engine both frontends spawn
//! (as `voxsplit-engine`). See docs/02-engine-protocol.md.

use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum, error::ErrorKind};
use voxsplit::audio::{self, BitDepth};
use voxsplit::cancel::Cancel;
use voxsplit::device::{self, Device};
use voxsplit::error::EngineError;
use voxsplit::events::Stem;
use voxsplit::events::{self, ErrorCode, Event};
use voxsplit::human;
use voxsplit::model::ModelConfig;
use voxsplit::pipeline::{self, Job, Tool};
use voxsplit::preview;
use voxsplit::separator::SeparateOptions;
use voxsplit::timing::TimingStore;

const VOCAL_MODEL: &str = "Kim_Vocal_2.onnx";
const KARAOKE_FAST: &str = "UVR_MDXNET_KARA_2.onnx";
const KARAOKE_BEST: &str = "BS_Roformer_Karaoke.onnx";

#[derive(Parser)]
#[command(
    name = "voxsplit",
    version,
    about = "Split lead and backing vocals on this Mac, offline",
    after_help = "Examples:\n  voxsplit separate song.mp3\n  voxsplit separate song.wav --tool=vocal-remover --reveal\n  voxsplit separate song.flac --quality=fast --passes=3 --out-dir=stems\n  voxsplit play ~/Music/VoxSplit/song\n  voxsplit play ~/Music/VoxSplit/song --mute backing\n  voxsplit estimate song.mp3"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Output: readable progress in a terminal, JSON Lines (one event per line)
    /// when piped. See docs/02-engine-protocol.md.
    #[arg(long, value_enum, default_value_t = Format::Auto, global = true)]
    format: Format,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Auto,
    Human,
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Split a song into lead_vocals.wav + backing_vocals.wav
    /// (or vocals.wav + instrumental.wav with --tool=vocal-remover).
    Separate(SeparateArgs),
    /// Report which hardware accelerators this build can use.
    Probe,
    /// Estimate how long `separate` will take for a file, at both qualities.
    Estimate(EstimateArgs),
    /// Play a run's stems together (macOS), like the apps' preview.
    Play(PlayArgs),
}

#[derive(Args)]
struct PlayArgs {
    /// A results folder (e.g. ~/Music/VoxSplit/<song>), or the stem files to play together.
    #[arg(value_name = "FOLDER|FILES", required = true, num_args = 1..)]
    paths: Vec<PathBuf>,
    /// Leave a stem out, e.g. --mute backing to hear the lead alone. Repeatable.
    #[arg(long, value_enum)]
    mute: Vec<StemArg>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum StemArg {
    Lead,
    Backing,
    Vocals,
    Instrumental,
}

impl StemArg {
    fn stem(self) -> Stem {
        match self {
            Self::Lead => Stem::Lead,
            Self::Backing => Stem::Backing,
            Self::Vocals => Stem::Vocals,
            Self::Instrumental => Stem::Instrumental,
        }
    }
}

#[derive(Args)]
#[command(group(ArgGroup::new("source").required(true).args(["file", "input"])))]
struct SeparateArgs {
    /// Audio file to separate (wav, aiff, flac, mp3, m4a).
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
    /// Same as FILE.
    #[arg(long, value_name = "FILE")]
    input: Option<PathBuf>,
    /// lead-backing: lead_vocals.wav + backing_vocals.wav.
    /// vocal-remover: vocals.wav + instrumental.wav.
    #[arg(long, value_enum, default_value_t = ToolArg::LeadBacking)]
    tool: ToolArg,
    /// Output folder [default: ~/Music/VoxSplit/<input name>].
    #[arg(long)]
    out_dir: Option<PathBuf>,
    /// best: BS-RoFormer karaoke (cleaner, slower). fast: MDX-Net karaoke.
    #[arg(long, value_enum, default_value_t = Quality::Best)]
    quality: Quality,
    /// Folder holding the model files [default: the bundled models].
    #[arg(long)]
    models_dir: Option<PathBuf>,
    /// Stage 1 model (mix → vocals) [default: Kim_Vocal_2.onnx].
    #[arg(long)]
    vocal_model: Option<PathBuf>,
    /// Lead/backing model [default: chosen by --quality].
    #[arg(long)]
    karaoke_model: Option<PathBuf>,
    /// Vocal-extraction passes: 0 = the input is already vocals, 1 = usual,
    /// more = re-run on its own output to strip instrument bleed.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=5))]
    vocal_passes: u8,
    /// Same as --vocal-passes=0.
    #[arg(long)]
    input_is_vocals: bool,
    /// Also write instrumental.wav (mix − vocals).
    #[arg(long)]
    keep_instrumental: bool,
    /// Karaoke passes: the first splits the vocals, each further pass pulls
    /// remaining backing vocals out of the lead [default: 1 best, 3 fast].
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=5))]
    passes: Option<u8>,
    #[arg(long, value_enum, default_value_t = Device::Auto)]
    device: Device,
    /// Fraction of each chunk shared with the next (higher = smoother, slower)
    /// [default: per model, 0.75 RoFormer, 0.5 MDX].
    #[arg(long, value_parser = parse_overlap)]
    overlap: Option<f32>,
    /// Chunks per inference call.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=16))]
    batch: u8,
    /// Skip the denoise pass. By default each MDX model also runs on the phase-inverted
    /// input and the two results are averaged, which cancels the faint constant hiss
    /// MDX-Net adds to its outputs (about -70 dBFS, above 6 kHz). Costs 2x on MDX stages.
    #[arg(long)]
    no_denoise: bool,
    #[arg(long, value_enum, default_value_t = BitDepthArg::F32)]
    bit_depth: BitDepthArg,
    /// Open the output folder in Finder when done.
    #[arg(long)]
    reveal: bool,
}

#[derive(Args)]
#[command(group(ArgGroup::new("source").required(true).args(["file", "input"])))]
struct EstimateArgs {
    /// Audio file to estimate for.
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
    /// Same as FILE.
    #[arg(long, value_name = "FILE")]
    input: Option<PathBuf>,
    #[arg(long)]
    models_dir: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = Device::Auto)]
    device: Device,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ToolArg {
    LeadBacking,
    VocalRemover,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Quality {
    Best,
    Fast,
}

#[derive(Clone, Copy, ValueEnum)]
enum BitDepthArg {
    #[value(name = "32f")]
    F32,
    #[value(name = "24")]
    I24,
    #[value(name = "16")]
    I16,
}

fn parse_overlap(s: &str) -> Result<f32, String> {
    let v: f32 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if (0.0..=0.95).contains(&v) {
        Ok(v)
    } else {
        Err("must be between 0.0 and 0.95".into())
    }
}

/// Decided before parsing, so even argument errors come out in the right form.
fn human_output() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let format = args.iter().enumerate().find_map(|(i, a)| {
        a.strip_prefix("--format=")
            .map(str::to_owned)
            .or_else(|| (a == "--format").then(|| args.get(i + 1).cloned().unwrap_or_default()))
    });
    match format.as_deref() {
        Some("human") => true,
        Some("json") => false,
        _ => std::io::stdout().is_terminal(),
    }
}

fn main() {
    events::set_human(human_output());
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        events::emit(&Event::Error {
            code: ErrorCode::Internal,
            message: &message,
        });
        eprintln!("{message}");
        std::process::exit(1);
    }));

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            e.exit()
        }
        Err(e) => {
            let rendered = e.to_string();
            let summary = rendered
                .lines()
                .next()
                .unwrap_or("invalid arguments")
                .trim_start_matches("error: ");
            events::emit(&Event::Error {
                code: ErrorCode::BadArgs,
                message: summary,
            });
            eprint!("{rendered}");
            std::process::exit(2);
        }
    };

    let code = match cli.command {
        Command::Probe => {
            events::emit(&Event::Probe {
                version: events::VERSION,
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                devices: device::probe(),
            });
            0
        }
        Command::Separate(args) => match separate(args) {
            Ok(()) => 0,
            Err(e) => report(e),
        },
        Command::Estimate(args) => match estimate(args) {
            Ok(()) => 0,
            Err(e) => report(e),
        },
        Command::Play(args) => match play(args) {
            Ok(()) => 0,
            Err(e) => report(e),
        },
    };
    std::process::exit(code);
}

fn separate(mut args: SeparateArgs) -> Result<(), EngineError> {
    let input = args
        .file
        .take()
        .or(args.input.take())
        .expect("clap requires a file");
    let cancel = Cancel::install().map_err(EngineError::Internal)?;
    let env = ort::init().with_name("voxsplit").build().map_err(|e| {
        EngineError::ModelLoad(anyhow::anyhow!("ONNX Runtime failed to start: {e}"))
    })?;

    let out_dir = match args.out_dir {
        Some(dir) => dir,
        None => default_out_dir(&input)?,
    };
    let model = |file: &str| match &args.models_dir {
        Some(dir) => dir.join(file),
        None => bundled_model(file),
    };
    let (karaoke_file, default_passes) = match args.quality {
        Quality::Best => (KARAOKE_BEST, 1),
        Quality::Fast => (KARAOKE_FAST, 3),
    };
    let reveal = args.reveal.then(|| out_dir.clone());
    let job = Job {
        tool: match args.tool {
            ToolArg::LeadBacking => Tool::LeadBacking,
            ToolArg::VocalRemover => Tool::VocalRemover,
        },
        vocal_model: args
            .vocal_model
            .clone()
            .unwrap_or_else(|| model(VOCAL_MODEL)),
        karaoke_model: args
            .karaoke_model
            .clone()
            .unwrap_or_else(|| model(karaoke_file)),
        input,
        out_dir,
        vocal_passes: if args.input_is_vocals {
            0
        } else {
            args.vocal_passes
        },
        keep_instrumental: args.keep_instrumental,
        passes: args.passes.unwrap_or(default_passes),
        device: args.device,
        overlap: args.overlap,
        separate: SeparateOptions {
            overlap: 0.5, // replaced per model by Job::options_for
            batch: args.batch as usize,
            denoise: !args.no_denoise,
        },
        bit_depth: match args.bit_depth {
            BitDepthArg::F32 => BitDepth::F32,
            BitDepthArg::I24 => BitDepth::I24,
            BitDepthArg::I16 => BitDepth::I16,
        },
        coreml_cache: coreml_cache_dir(),
        timings: timings_path(),
    };
    pipeline::run(&env, &job, &cancel)?;
    if let Some(dir) = reveal {
        // Best effort: the stems are written either way.
        let _ = std::process::Command::new("open").arg(dir).status();
    }
    Ok(())
}

fn play(args: PlayArgs) -> Result<(), EngineError> {
    let mut files = match args.paths.as_slice() {
        [dir] if dir.is_dir() => preview::stems_in(dir),
        paths => paths.to_vec(),
    };
    files.retain(|f| {
        !args
            .mute
            .iter()
            .any(|m| preview::stem_of(f) == Some(m.stem()))
    });
    if files.is_empty() {
        return Err(EngineError::BadArgs(
            "nothing to play: pass a VoxSplit results folder or stem files (and don't mute them all)"
                .into(),
        ));
    }
    let names: Vec<String> = files
        .iter()
        .map(|f| match preview::stem_of(f) {
            Some(stem) => human::stem_label(stem).to_lowercase(),
            None => f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        })
        .collect();

    let (mixed, rate) = preview::mix(&files).map_err(EngineError::Decode)?;
    let length = mixed.len() as f64 / f64::from(rate);
    let temp = std::env::temp_dir().join(format!("voxsplit-play-{}.wav", std::process::id()));
    let mut sink =
        audio::WavSink::create(&temp, rate, rate, BitDepth::F32).map_err(EngineError::Write)?;
    sink.write(&mixed.l, &mixed.r).map_err(EngineError::Write)?;
    drop(mixed);
    let temp = sink.finish().map_err(EngineError::Write)?;

    let cancel = Cancel::install().map_err(EngineError::Internal)?;
    let result = (|| {
        let mut player = std::process::Command::new("afplay")
            .arg(&temp)
            .spawn()
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("cannot start afplay: {e}")))?;
        eprintln!(
            "Playing {} · {} · Ctrl-C to stop",
            names.join(" + "),
            human::clock(length)
        );
        let started = std::time::Instant::now();
        loop {
            if player
                .try_wait()
                .map_err(|e| EngineError::Internal(e.into()))?
                .is_some()
            {
                break;
            }
            if cancel.is_cancelled() {
                let _ = player.kill();
                let _ = player.wait();
                break;
            }
            let at = started.elapsed().as_secs_f64().min(length);
            eprint!("\r\x1b[2K  {} / {}", human::clock(at), human::clock(length));
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        eprintln!();
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

fn estimate(mut args: EstimateArgs) -> Result<(), EngineError> {
    let input = args
        .file
        .take()
        .or(args.input.take())
        .expect("clap requires a file");
    let model = |file: &str| match &args.models_dir {
        Some(dir) => dir.join(file),
        None => bundled_model(file),
    };
    let config = |file: &str| ModelConfig::for_model(&model(file)).map_err(EngineError::ModelLoad);
    let vocal = config(VOCAL_MODEL)?;
    let duration = audio::duration_sec(&input).map_err(EngineError::Decode)?;
    let samples = (duration * vocal.sample_rate as f64) as usize;
    let timings = TimingStore::open(timings_path());
    let device = device::expected_name(args.device);
    // Two vocal + two karaoke passes: [vocal first, vocal extra, karaoke first, karaoke extra].
    let split = |karaoke: &ModelConfig| -> [f64; 4] {
        let s = pipeline::stage_estimates(
            &timings,
            device,
            Some(&vocal),
            2,
            Some(karaoke),
            2,
            None,
            SeparateOptions::default(),
            samples,
        );
        [s[0], s[1], s[2], s[3]]
    };
    let best = split(&config(KARAOKE_BEST)?);
    let fast = split(&config(KARAOKE_FAST)?);
    events::emit(&Event::Estimate {
        duration_sec: duration,
        vocal_sec: best[0],
        vocal_pass_sec: best[1],
        best_sec: best[0] + best[2],
        best_pass_sec: best[3],
        fast_sec: fast[0] + fast[2],
        fast_pass_sec: fast[3],
    });
    Ok(())
}

fn report(e: EngineError) -> i32 {
    match &e {
        EngineError::Cancelled => events::emit(&Event::Cancelled),
        other => {
            let message = other.to_string();
            events::emit(&Event::Error {
                code: other.code(),
                message: &message,
            });
            eprintln!("error: {message}");
        }
    }
    e.exit_code()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn default_out_dir(input: &std::path::Path) -> Result<PathBuf, EngineError> {
    let stem = input
        .file_stem()
        .ok_or_else(|| EngineError::BadArgs(format!("{} has no file name", input.display())))?;
    let home =
        home().ok_or_else(|| EngineError::BadArgs("HOME is not set; pass --out-dir".into()))?;
    Ok(home.join("Music").join("VoxSplit").join(stem))
}

/// Bundled models: inside an app/plugin bundle (`Contents/Helpers/../Resources/models`),
/// next to the engine in a shared install (`/Library/Application Support/VoxSplit`, which
/// the installer links as `/usr/local/bin/voxsplit`), the per-user shared folder, or the
/// repo's `models/` when run from `engine/target/<profile>/`.
fn bundled_model(file: &str) -> PathBuf {
    let mut dirs = Vec::new();
    // Canonical, so a symlinked `voxsplit` on the PATH finds the real install.
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        dirs.push(exe_dir.join("../Resources/models"));
        dirs.push(exe_dir.join("models"));
        if let Some(home) = home() {
            dirs.push(home.join("Library/Application Support/VoxSplit/models"));
        }
        dirs.push(PathBuf::from(
            "/Library/Application Support/VoxSplit/models",
        ));
        dirs.push(exe_dir.join("../../../models"));
    }
    dirs.iter()
        .map(|d| d.join(file))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(file))
}

fn timings_path() -> Option<PathBuf> {
    let dir = home()?.join("Library/Caches/com.voxsplit.engine");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("timings.json"))
}

fn coreml_cache_dir() -> Option<PathBuf> {
    let dir = home()?.join("Library/Caches/com.voxsplit.engine/coreml");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}
