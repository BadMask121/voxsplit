//! Terminal rendering of the event stream, for people running `voxsplit` by hand.
//! Progress and headings go to stderr (one line redrawn in place); the files
//! written go to stdout, so `voxsplit separate song.mp3 > files.txt` still works.

use std::io::Write;
use std::sync::Mutex;

use crate::events::{Event, Level, StageName, Stem};

struct State {
    /// A progress line is on screen without a newline after it.
    progress_open: bool,
    last_drawn: Option<(u32, u64)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    progress_open: false,
    last_drawn: None,
});

const BAR_WIDTH: usize = 28;

pub fn render(event: &Event) {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let mut err = std::io::stderr().lock();
    let style = Style::detect();

    // Anything but a progress update starts on a fresh line.
    if !matches!(event, Event::Progress { .. }) && state.progress_open {
        let _ = writeln!(err);
        state.progress_open = false;
    }

    match *event {
        Event::Probe {
            version,
            os,
            arch,
            ref devices,
        } => {
            let _ = writeln!(err, "VoxSplit {version} ({os} {arch})");
            for d in devices {
                let status = match (d.compiled, d.available) {
                    (true, true) => "available",
                    (true, false) => "not available on this machine",
                    (false, _) => "not in this build",
                };
                let _ = writeln!(err, "  {:<9}{}", device_label(d.name), style.dim(status));
            }
        }
        Event::Start {
            version,
            duration_sec,
            estimate_sec,
            ..
        } => {
            let _ = writeln!(
                err,
                "{} {}",
                style.bold(&format!("VoxSplit {version}")),
                style.dim(&format!(
                    "· {} of audio · takes about {}",
                    clock(duration_sec),
                    duration(estimate_sec)
                ))
            );
        }
        // The event names the model and device; what people read doesn't.
        Event::Stage {
            stage,
            index,
            stages,
            ..
        } => {
            state.last_drawn = None;
            let _ = writeln!(
                err,
                "{} {}",
                style.dim(&format!("[{index}/{stages}]")),
                stage_label(stage)
            );
        }
        Event::Progress {
            fraction, eta_sec, ..
        } => {
            let percent = (fraction.clamp(0.0, 1.0) * 100.0).floor() as u32;
            let eta = eta_sec.max(0.0).round() as u64;
            if state.last_drawn == Some((percent, eta)) {
                return;
            }
            state.last_drawn = Some((percent, eta));
            let filled =
                ((fraction.clamp(0.0, 1.0) * BAR_WIDTH as f32).round() as usize).min(BAR_WIDTH);
            let _ = write!(
                err,
                "\r\x1b[2K      {}{} {:>3}%  {}",
                "━".repeat(filled),
                style.dim(&"─".repeat(BAR_WIDTH - filled)),
                percent,
                style.dim(&time_left(eta_sec))
            );
            let _ = err.flush();
            state.progress_open = true;
        }
        Event::Log { level, message } => {
            let mark = match level {
                Level::Info => "·",
                Level::Warn => "!",
            };
            let _ = writeln!(err, "      {mark} {message}");
        }
        Event::Output { stem, path } => {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(
                out,
                "  {} {:<15}{}",
                style.green("✓"),
                stem_label(stem),
                path
            );
            let _ = out.flush();
        }
        Event::Done { elapsed_sec } => {
            let _ = writeln!(
                err,
                "{}",
                style.dim(&format!("Done in {}.", duration(elapsed_sec)))
            );
        }
        Event::Cancelled => {
            let _ = writeln!(err, "Cancelled. Nothing was saved.");
        }
        Event::Estimate {
            duration_sec,
            vocal_sec,
            best_sec,
            fast_sec,
            ..
        } => {
            let _ = writeln!(
                err,
                "{} of audio. Expected time on this Mac:",
                clock(duration_sec)
            );
            let _ = writeln!(err, "  Lead & Backing, best   about {}", duration(best_sec));
            let _ = writeln!(err, "  Lead & Backing, fast   about {}", duration(fast_sec));
            let _ = writeln!(
                err,
                "  Vocal Remover          about {}",
                duration(vocal_sec)
            );
        }
        // main prints the error itself; the line above only ended the progress bar.
        Event::Error { .. } => {}
    }
    let _ = err.flush();
}

fn stage_label(stage: StageName) -> &'static str {
    match stage {
        StageName::Vocals => "Isolating vocals",
        StageName::VocalsRefine => "Cleaning up the vocals",
        StageName::Karaoke => "Splitting lead from backing",
        StageName::Refine => "Cleaning up the lead",
    }
}

pub fn stem_label(stem: Stem) -> &'static str {
    match stem {
        Stem::Lead => "Lead vocals",
        Stem::Backing => "Backing vocals",
        Stem::Vocals => "Vocals",
        Stem::Instrumental => "Instrumental",
    }
}

fn device_label(name: &str) -> &str {
    match name {
        "coreml" => "CoreML",
        "cpu" => "CPU",
        "cuda" => "CUDA",
        "directml" => "DirectML",
        other => other,
    }
}

/// "42 s", "2 min 7 s".
pub fn duration(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    if s < 60 {
        format!("{s} s")
    } else if s.is_multiple_of(60) {
        format!("{} min", s / 60)
    } else {
        format!("{} min {} s", s / 60, s % 60)
    }
}

fn time_left(seconds: f64) -> String {
    if seconds < 1.0 {
        "almost done".into()
    } else {
        format!("about {} left", duration(seconds))
    }
}

/// "3:10".
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// ANSI styling, off when NO_COLOR is set (https://no-color.org).
struct Style {
    color: bool,
}

impl Style {
    fn detect() -> Self {
        Self {
            color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        }
    }

    fn wrap(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    fn dim(&self, text: &str) -> String {
        self.wrap("2", text)
    }

    fn bold(&self, text: &str) -> String {
        self.wrap("1", text)
    }

    fn green(&self, text: &str) -> String {
        self.wrap("32", text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(0.4), "0 s");
        assert_eq!(duration(42.4), "42 s");
        assert_eq!(duration(120.0), "2 min");
        assert_eq!(duration(127.2), "2 min 7 s");
        assert_eq!(duration(-3.0), "0 s");
        assert_eq!(clock(190.2), "3:10");
        assert_eq!(clock(5.0), "0:05");
        assert_eq!(time_left(0.4), "almost done");
        assert_eq!(time_left(41.0), "about 41 s left");
    }
}
