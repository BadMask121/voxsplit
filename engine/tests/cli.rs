//! Exercises the binary's argv / exit-code / event contract (docs/02-engine-protocol.md).
//! None of these need model weights.

use std::process::{Command, Output};

fn engine(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_voxsplit-engine"))
        .args(args)
        .output()
        .expect("spawn engine")
}

fn events(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| {
            serde_json::from_str(l).unwrap_or_else(|e| panic!("non-JSON stdout line {l:?}: {e}"))
        })
        .collect()
}

fn last_error(out: &Output) -> (String, String) {
    let ev = events(out).pop().expect("at least one event");
    assert_eq!(ev["event"], "error", "{ev}");
    (
        ev["code"].as_str().unwrap().into(),
        ev["message"].as_str().unwrap().into(),
    )
}

#[test]
fn probe_reports_cpu() {
    let out = engine(&["probe"]);
    assert!(out.status.success());
    let ev = &events(&out)[0];
    assert_eq!(ev["event"], "probe");
    let devices = ev["devices"].as_array().unwrap();
    assert!(
        devices
            .iter()
            .any(|d| d["name"] == "cpu" && d["available"] == true)
    );
}

#[test]
fn bad_arguments_exit_2_with_event() {
    let out = engine(&["separate", "--input=x.wav", "--overlap=2"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(last_error(&out).0, "bad_args");

    let out = engine(&["separate"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(last_error(&out).0, "bad_args");
}

#[test]
fn missing_model_exits_4_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let out = engine(&[
        "separate",
        "--input=/nonexistent/song.wav",
        &format!("--out-dir={}", dir.path().display()),
        "--karaoke-model=/nonexistent/model.onnx",
    ]);
    assert_eq!(out.status.code(), Some(4));
    let (code, message) = last_error(&out);
    assert_eq!(code, "model_load");
    assert!(message.contains("sidecar"), "{message}");
}

#[test]
fn unreadable_input_exits_3() {
    let dir = tempfile::tempdir().unwrap();
    let models = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
    let input = dir.path().join("not audio.wav");
    std::fs::write(&input, b"definitely not a wav file").unwrap();
    let out = engine(&[
        "separate",
        &format!("--input={}", input.display()),
        &format!("--out-dir={}", dir.path().join("out").display()),
        &format!("--vocal-model={models}/Kim_Vocal_2.onnx"),
        &format!("--karaoke-model={models}/UVR_MDXNET_KARA_2.onnx"),
    ]);
    assert_eq!(
        out.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(last_error(&out).0, "decode");
}

#[test]
fn paths_starting_with_dash_are_values_not_flags() {
    let out = engine(&[
        "separate",
        "--input=-not-a-flag.wav",
        "--karaoke-model=/nonexistent.onnx",
    ]);
    // Parsed fine (would be exit 2 otherwise); fails later on the missing model.
    assert_eq!(out.status.code(), Some(4));
}

#[test]
fn quality_picks_the_karaoke_model() {
    for (quality, model) in [
        ("best", "BS_Roformer_Karaoke"),
        ("fast", "UVR_MDXNET_KARA_2"),
    ] {
        let out = engine(&[
            "separate",
            "--input=/nonexistent/song.wav",
            "--models-dir=/nonexistent/models",
            &format!("--quality={quality}"),
        ]);
        assert_eq!(out.status.code(), Some(4));
        let (_, message) = last_error(&out);
        assert!(message.contains(model), "{quality}: {message}");
    }
}

#[test]
fn estimate_reports_both_qualities_from_the_header() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("ten seconds.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&input, spec).unwrap();
    for _ in 0..48_000 * 10 * 2 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();

    let models = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
    let out = engine(&[
        "estimate",
        &format!("--input={}", input.display()),
        &format!("--models-dir={models}"),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ev = &events(&out)[0];
    assert_eq!(ev["event"], "estimate");
    assert!((ev["duration_sec"].as_f64().unwrap() - 10.0).abs() < 0.05);
    let (best, fast) = (
        ev["best_sec"].as_f64().unwrap(),
        ev["fast_sec"].as_f64().unwrap(),
    );
    assert!(best > fast && fast > 0.0, "best {best} fast {fast}");
}

#[test]
fn vocal_remover_writes_vocals_and_instrumental() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("tone.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&input, spec).unwrap();
    for i in 0..44_100 * 4 {
        let v = ((i as f32 * 0.05).sin() * 8_000.0) as i16;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
    let models = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
    let out_dir = dir.path().join("out");
    let out = engine(&[
        "separate",
        "--tool=vocal-remover",
        "--vocal-passes=0", // clamped to 1: the vocal remover is the vocal stage
        &format!("--input={}", input.display()),
        &format!("--out-dir={}", out_dir.display()),
        &format!("--models-dir={models}"),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stems: Vec<String> = events(&out)
        .iter()
        .filter(|e| e["event"] == "output")
        .map(|e| e["stem"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(stems, ["vocals", "instrumental"]);
    assert!(out_dir.join("vocals.wav").is_file() && out_dir.join("instrumental.wav").is_file());
    assert!(!out_dir.join("lead_vocals.wav").exists());
}

#[test]
fn positional_file_works_like_input() {
    let dir = tempfile::tempdir().unwrap();
    let out = engine(&[
        "separate",
        dir.path().join("missing.wav").to_str().unwrap(),
        "--models-dir=/nonexistent",
    ]);
    // Gets past argument parsing to the model check (exit 4), not a usage error (exit 2).
    assert_eq!(
        out.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = engine(&["separate"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn play_with_nothing_to_play_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = engine(&["play", dir.path().to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let (code, message) = last_error(&out);
    assert_eq!(code, "bad_args");
    assert!(message.contains("nothing to play"), "{message}");
}

#[test]
fn format_human_prints_no_json() {
    let out = engine(&["probe", "--format=human"]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "human output goes to stderr");
    assert!(String::from_utf8_lossy(&out.stderr).contains("VoxSplit"));
}
