//! `voxsplit-engine` — the CLI both frontends spawn. See docs/02-engine-protocol.md.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum, error::ErrorKind};
use voxsplit::audio::BitDepth;
use voxsplit::cancel::Cancel;
use voxsplit::device::{self, Device};
use voxsplit::error::EngineError;
use voxsplit::events::{self, ErrorCode, Event};
use voxsplit::pipeline::{self, Job};
use voxsplit::separator::SeparateOptions;

const VOCAL_MODEL: &str = "Kim_Vocal_2.onnx";
const KARAOKE_FAST: &str = "UVR_MDXNET_KARA_2.onnx";
const KARAOKE_BEST: &str = "BS_Roformer_Karaoke.onnx";

#[derive(Parser)]
#[command(
    name = "voxsplit-engine",
    version,
    about = "Offline lead/backing vocal separation"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Split an audio file into lead_vocals.wav and backing_vocals.wav.
    Separate(SeparateArgs),
    /// Report which hardware accelerators this build can use.
    Probe,
}

#[derive(Args)]
struct SeparateArgs {
    /// Audio file to separate (wav, aiff, flac, mp3, m4a).
    #[arg(long)]
    input: PathBuf,
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
    /// The input is already an a cappella: skip stage 1.
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
    /// Average f(x) and −f(−x): cleaner stems, twice the work.
    #[arg(long)]
    denoise: bool,
    #[arg(long, value_enum, default_value_t = BitDepthArg::F32)]
    bit_depth: BitDepthArg,
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

fn main() {
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
    };
    std::process::exit(code);
}

fn separate(args: SeparateArgs) -> Result<(), EngineError> {
    let cancel = Cancel::install().map_err(EngineError::Internal)?;
    let env = ort::init().with_name("voxsplit").build().map_err(|e| {
        EngineError::ModelLoad(anyhow::anyhow!("ONNX Runtime failed to start: {e}"))
    })?;

    let out_dir = match args.out_dir {
        Some(dir) => dir,
        None => default_out_dir(&args.input)?,
    };
    let model = |file: &str| match &args.models_dir {
        Some(dir) => dir.join(file),
        None => bundled_model(file),
    };
    let (karaoke_file, default_passes) = match args.quality {
        Quality::Best => (KARAOKE_BEST, 1),
        Quality::Fast => (KARAOKE_FAST, 3),
    };
    let job = Job {
        vocal_model: args
            .vocal_model
            .clone()
            .unwrap_or_else(|| model(VOCAL_MODEL)),
        karaoke_model: args
            .karaoke_model
            .clone()
            .unwrap_or_else(|| model(karaoke_file)),
        input: args.input,
        out_dir,
        input_is_vocals: args.input_is_vocals,
        keep_instrumental: args.keep_instrumental,
        passes: args.passes.unwrap_or(default_passes),
        device: args.device,
        overlap: args.overlap,
        separate: SeparateOptions {
            overlap: 0.5, // replaced per model by Job::options_for
            batch: args.batch as usize,
            denoise: args.denoise,
        },
        bit_depth: match args.bit_depth {
            BitDepthArg::F32 => BitDepth::F32,
            BitDepthArg::I24 => BitDepth::I24,
            BitDepthArg::I16 => BitDepth::I16,
        },
        coreml_cache: coreml_cache_dir(),
    };
    pipeline::run(&env, &job, &cancel).map(|_| ())
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
/// a shared install, or the repo's `models/` when run from `engine/target/<profile>/`.
fn bundled_model(file: &str) -> PathBuf {
    let mut dirs = Vec::new();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        dirs.push(exe_dir.join("../Resources/models"));
        if let Some(home) = home() {
            dirs.push(home.join("Library/Application Support/VoxSplit/models"));
        }
        dirs.push(exe_dir.join("../../../models"));
    }
    dirs.iter()
        .map(|d| d.join(file))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(file))
}

fn coreml_cache_dir() -> Option<PathBuf> {
    let dir = home()?.join("Library/Caches/com.voxsplit.engine/coreml");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}
