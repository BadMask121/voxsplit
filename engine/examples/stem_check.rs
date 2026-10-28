//! Developer check for separation output.
//!
//!   cargo run --release --example stem_check -- null <input> <out-dir>
//!       Requires a run with --keep-instrumental. lead + backing + instrumental
//!       must reconstruct the input mix; prints the reconstruction SNR and each
//!       stem's level relative to the mix.
//!
//!   cargo run --release --example stem_check -- diff <out-dir-a> <out-dir-b>
//!       SNR of each stem in A against the same stem in B (e.g. CoreML vs CPU).
//!
//!   cargo run --release --example stem_check -- level <out-dir>
//!       Each stem's level and its level above 6 kHz (hiss shows up there).

use std::path::Path;

use voxsplit::audio::{self, Stereo};
use voxsplit::stft::Stft;

fn read_stem(dir: &Path, name: &str) -> Stereo {
    audio::decode(&dir.join(name))
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
        .audio
}

fn energy(s: &Stereo) -> f64 {
    s.l.iter().chain(&s.r).map(|&v| (v as f64).powi(2)).sum()
}

fn db(ratio: f64) -> f64 {
    10.0 * ratio.max(1e-30).log10()
}

fn snr(reference: &Stereo, estimate: &Stereo) -> f64 {
    let n = reference.len().min(estimate.len());
    let mut err = 0.0;
    for i in 0..n {
        err += ((reference.l[i] - estimate.l[i]) as f64).powi(2)
            + ((reference.r[i] - estimate.r[i]) as f64).powi(2);
    }
    db(energy(reference) / err.max(1e-30))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["null", input, dir] => {
            let mix = audio::decode(Path::new(input)).expect("decode input").audio;
            let dir = Path::new(dir);
            let stems = ["lead_vocals.wav", "backing_vocals.wav", "instrumental.wav"]
                .map(|n| (n, read_stem(dir, n)));
            for (name, s) in &stems {
                assert_eq!(
                    s.len(),
                    mix.len(),
                    "{name} length {} != input length {}",
                    s.len(),
                    mix.len()
                );
                println!(
                    "{name:<20} {:>7.1} dB relative to mix",
                    db(energy(s) / energy(&mix))
                );
            }
            let mut sum = Stereo::zeros(mix.len());
            for (_, s) in &stems {
                for i in 0..mix.len() {
                    sum.l[i] += s.l[i];
                    sum.r[i] += s.r[i];
                }
            }
            println!(
                "reconstruction SNR   {:>7.1} dB (lead + backing + instrumental vs input)",
                snr(&mix, &sum)
            );
        }
        ["diff", a, b] => {
            for name in ["lead_vocals.wav", "backing_vocals.wav", "instrumental.wav"] {
                if !Path::new(a).join(name).exists() {
                    continue;
                }
                let (sa, sb) = (read_stem(Path::new(a), name), read_stem(Path::new(b), name));
                println!("{name:<20} {:>7.1} dB SNR (a vs b)", snr(&sb, &sa));
            }
        }
        ["level", dir] => {
            for name in ["lead_vocals.wav", "backing_vocals.wav", "instrumental.wav"] {
                if !Path::new(dir).join(name).exists() {
                    continue;
                }
                let s = read_stem(Path::new(dir), name);
                let total = energy(&s) / (2 * s.len()).max(1) as f64;
                // Energy above 6 kHz, as a level (Parseval-scaled STFT power).
                let mut stft = Stft::new(2048, 512);
                let bins = stft.bins();
                let cutoff = 6_000 * 2048 / 44_100;
                let mut hf = 0.0;
                let mut frames = 0usize;
                for ch in [&s.l, &s.r] {
                    let spec = stft.forward(ch);
                    let n = spec.len() / bins;
                    frames += n;
                    for t in 0..n {
                        for f in cutoff..bins {
                            hf += spec[t * bins + f].norm_sqr() as f64;
                        }
                    }
                }
                // Periodic Hann at hop n/4: sum of window^2 per sample is 1.5 * n/4 ... normalise per frame.
                let hf = hf / (frames.max(1) as f64 * 2048.0 * 2048.0 * 0.375 / 2.0);
                println!(
                    "{name:<20} {:>7.1} dBFS overall {:>7.1} dBFS above 6 kHz",
                    db(total),
                    db(hf)
                );
            }
        }
        _ => {
            eprintln!(
                "usage: stem_check null <input> <out-dir> | diff <dir-a> <dir-b> | level <dir>"
            );
            std::process::exit(2);
        }
    }
}
