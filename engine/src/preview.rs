//! `voxsplit play`: hear a run's stems together, like the apps' preview. The
//! stems are summed into one buffer first, so they play sample-aligned.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::audio::{self, Stereo};
use crate::events::Stem;

/// The stems a results folder holds, as one tool's pair: lead + backing if both
/// are there, otherwise vocals + instrumental (both tools share the folder).
pub fn stems_in(dir: &Path) -> Vec<PathBuf> {
    let present = |stems: &[Stem]| -> Option<Vec<PathBuf>> {
        let files: Vec<PathBuf> = stems.iter().map(|s| dir.join(s.file_name())).collect();
        files.iter().all(|f| f.is_file()).then_some(files)
    };
    present(&[Stem::Lead, Stem::Backing])
        .or_else(|| present(&[Stem::Vocals, Stem::Instrumental]))
        .unwrap_or_default()
}

/// Which stem a file is, by its VoxSplit file name.
pub fn stem_of(path: &Path) -> Option<Stem> {
    let name = path.file_name()?.to_str()?;
    [Stem::Lead, Stem::Backing, Stem::Vocals, Stem::Instrumental]
        .into_iter()
        .find(|s| s.file_name() == name)
}

/// Sums the files sample-for-sample. They must share a sample rate (a run's
/// stems always do); shorter files are padded with silence.
pub fn mix(files: &[PathBuf]) -> Result<(Stereo, u32)> {
    let mut sum = Stereo::default();
    let mut rate = None;
    for file in files {
        let decoded = audio::decode(file)?;
        match rate {
            None => rate = Some(decoded.sample_rate),
            Some(r) if r != decoded.sample_rate => bail!(
                "{} is {} Hz but the other stems are {r} Hz",
                file.display(),
                decoded.sample_rate
            ),
            Some(_) => {}
        }
        let len = decoded.audio.len().max(sum.len());
        sum.l.resize(len, 0.0);
        sum.r.resize(len, 0.0);
        for (i, (l, r)) in decoded.audio.l.iter().zip(&decoded.audio.r).enumerate() {
            sum.l[i] += l;
            sum.r[i] += r;
        }
    }
    match rate {
        Some(rate) => Ok((sum, rate)),
        None => bail!("nothing to play"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, samples: &[f32]) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for s in samples {
            w.write_sample(*s).unwrap();
            w.write_sample(*s).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn folder_picks_one_tools_pair() {
        let dir = tempfile::tempdir().unwrap();
        assert!(stems_in(dir.path()).is_empty());
        write(&dir.path().join("vocals.wav"), &[0.1]);
        write(&dir.path().join("instrumental.wav"), &[0.1]);
        assert_eq!(stems_in(dir.path()).len(), 2);
        // Both tools ran on this song: lead + backing wins, never all four.
        write(&dir.path().join("lead_vocals.wav"), &[0.1]);
        write(&dir.path().join("backing_vocals.wav"), &[0.1]);
        let stems = stems_in(dir.path());
        assert_eq!(stem_of(&stems[0]), Some(Stem::Lead));
        assert_eq!(stem_of(&stems[1]), Some(Stem::Backing));
    }

    #[test]
    fn mix_sums_and_pads() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wav");
        let b = dir.path().join("b.wav");
        write(&a, &[0.25, 0.5, 0.25]);
        write(&b, &[0.25]);
        let (sum, rate) = mix(&[a, b]).unwrap();
        assert_eq!(rate, 44_100);
        assert_eq!(sum.l, vec![0.5, 0.5, 0.25]);
        assert_eq!(sum.r, sum.l);
    }
}
