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
