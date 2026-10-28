//! JSON-Lines events on stdout — the machine-readable half of the protocol in
//! docs/02-engine-protocol.md. Each line goes out in a single `write` and is
//! flushed immediately so frontends see progress in real time.

use std::io::Write;

use serde::Serialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_MESSAGE: usize = 300;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageName {
    Vocals,
    /// A further vocal pass over the previous pass's vocals.
    VocalsRefine,
    Karaoke,
    /// A further karaoke pass over the current lead.
    Refine,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stem {
    Lead,
    Backing,
    Instrumental,
}

impl Stem {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Lead => "lead_vocals.wav",
            Self::Backing => "backing_vocals.wav",
            Self::Instrumental => "instrumental.wav",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warn,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadArgs,
    Decode,
    ModelLoad,
    Inference,
    Write,
    Internal,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: &'static str,
    pub compiled: bool,
    pub available: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event<'a> {
    Probe {
        version: &'a str,
        os: &'a str,
        arch: &'a str,
        devices: Vec<DeviceInfo>,
    },
    Start {
        version: &'a str,
        stages: u8,
        duration_sec: f64,
        sample_rate: u32,
        channels: u16,
        /// Expected seconds for the whole run.
        estimate_sec: f64,
    },
    Stage {
        stage: StageName,
        index: u8,
        stages: u8,
        model: &'a str,
        device: &'a str,
    },
    Progress {
        stage: StageName,
        fraction: f32,
        overall: f32,
        /// Expected seconds until done.
        eta_sec: f64,
    },
    /// Expected seconds per quality with one karaoke pass, and per extra pass.
    Estimate {
        duration_sec: f64,
        /// The first vocal pass (incl. model load), and each extra vocal pass.
        vocal_sec: f64,
        vocal_pass_sec: f64,
        best_sec: f64,
        best_pass_sec: f64,
        fast_sec: f64,
        fast_pass_sec: f64,
    },
    Log {
        level: Level,
        message: &'a str,
    },
    Output {
        stem: Stem,
        path: &'a str,
    },
    Done {
        elapsed_sec: f64,
    },
    Cancelled,
    Error {
        code: ErrorCode,
        message: &'a str,
    },
}

pub fn to_line(event: &Event) -> String {
    let mut line = serde_json::to_string(&clamp(event)).expect("events always serialise");
    line.push('\n');
    line
}

pub fn emit(event: &Event) {
    let line = to_line(event);
    let mut out = std::io::stdout().lock();
    // A closed pipe (frontend gone) must not turn into a panic mid-run.
    let _ = out.write_all(line.as_bytes()).and_then(|()| out.flush());
}

pub fn log(level: Level, message: &str) {
    emit(&Event::Log { level, message });
}

/// Keep free-text fields short, and round floats so lines stay compact.
fn clamp<'a>(event: &'a Event<'a>) -> Event<'a> {
    let short = |s: &'a str| match s.char_indices().nth(MAX_MESSAGE) {
        Some((i, _)) => &s[..i],
        None => s,
    };
    let round = |v: f32| (v * 10_000.0).round() / 10_000.0;
    let secs = |v: f64| (v.max(0.0) * 10.0).round() / 10.0;
    match *event {
        Event::Log { level, message } => Event::Log {
            level,
            message: short(message),
        },
        Event::Error { code, message } => Event::Error {
            code,
            message: short(message),
        },
        Event::Progress {
            stage,
            fraction,
            overall,
            eta_sec,
        } => Event::Progress {
            stage,
            fraction: round(fraction),
            overall: round(overall),
            eta_sec: secs(eta_sec),
        },
        Event::Estimate {
            duration_sec,
            vocal_sec,
            vocal_pass_sec,
            best_sec,
            best_pass_sec,
            fast_sec,
            fast_pass_sec,
        } => Event::Estimate {
            duration_sec: secs(duration_sec),
            vocal_sec: secs(vocal_sec),
            vocal_pass_sec: secs(vocal_pass_sec),
            best_sec: secs(best_sec),
            best_pass_sec: secs(best_pass_sec),
            fast_sec: secs(fast_sec),
            fast_pass_sec: secs(fast_pass_sec),
        },
        Event::Probe {
            version,
            os,
            arch,
            ref devices,
        } => Event::Probe {
            version,
            os,
            arch,
            devices: devices.clone(),
        },
        Event::Start {
            version,
            stages,
            duration_sec,
            sample_rate,
            channels,
            estimate_sec,
        } => Event::Start {
            version,
            stages,
            duration_sec: (duration_sec * 100.0).round() / 100.0,
            sample_rate,
            channels,
            estimate_sec: secs(estimate_sec),
        },
        Event::Stage {
            stage,
            index,
            stages,
            model,
            device,
        } => Event::Stage {
            stage,
            index,
            stages,
            model,
            device,
        },
        Event::Output { stem, path } => Event::Output { stem, path },
        Event::Done { elapsed_sec } => Event::Done {
            elapsed_sec: (elapsed_sec * 100.0).round() / 100.0,
        },
        Event::Cancelled => Event::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_shapes_match_protocol() {
        assert_eq!(
            to_line(&Event::Progress {
                stage: StageName::Vocals,
                fraction: 0.123_456,
                overall: 0.061_728,
                eta_sec: 63.27,
            }),
            "{\"event\":\"progress\",\"stage\":\"vocals\",\"fraction\":0.1235,\"overall\":0.0617,\"eta_sec\":63.3}\n"
        );
        assert_eq!(
            to_line(&Event::Output {
                stem: Stem::Lead,
                path: "/tmp/a b/lead_vocals.wav"
            }),
            "{\"event\":\"output\",\"stem\":\"lead\",\"path\":\"/tmp/a b/lead_vocals.wav\"}\n"
        );
        assert_eq!(to_line(&Event::Cancelled), "{\"event\":\"cancelled\"}\n");
        assert!(
            to_line(&Event::Progress {
                stage: StageName::Refine,
                fraction: 0.5,
                overall: 0.875,
                eta_sec: 4.0,
            })
            .contains("\"stage\":\"refine\"")
        );
        assert_eq!(
            to_line(&Event::Stage {
                stage: StageName::Karaoke,
                index: 2,
                stages: 2,
                model: "K",
                device: "coreml"
            }),
            "{\"event\":\"stage\",\"stage\":\"karaoke\",\"index\":2,\"stages\":2,\"model\":\"K\",\"device\":\"coreml\"}\n"
        );
        assert_eq!(
            to_line(&Event::Error {
                code: ErrorCode::ModelLoad,
                message: "x"
            }),
            "{\"event\":\"error\",\"code\":\"model_load\",\"message\":\"x\"}\n"
        );
    }

    #[test]
    fn long_messages_are_truncated_on_char_boundaries() {
        let long = "é".repeat(1_000);
        let line = to_line(&Event::Log {
            level: Level::Warn,
            message: &long,
        });
        assert!(line.len() < 700);
        assert!(serde_json::from_str::<serde_json::Value>(line.trim_end()).is_ok());
    }
}
