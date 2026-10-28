//! Audio I/O: decode anything symphonia understands into planar stereo f32,
//! resample with rubato, and write stems as WAV through `.partial` files that
//! are renamed into place only once complete.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rubato::audioadapter::Adapter;
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Planar stereo buffer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stereo {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl Stereo {
    pub fn zeros(len: usize) -> Self {
        Self {
            l: vec![0.0; len],
            r: vec![0.0; len],
        }
    }

    pub fn with_capacity(len: usize) -> Self {
        Self {
            l: Vec::with_capacity(len),
            r: Vec::with_capacity(len),
        }
    }

    pub fn len(&self) -> usize {
        self.l.len()
    }

    pub fn is_empty(&self) -> bool {
        self.l.is_empty()
    }

    pub fn extend(&mut self, l: &[f32], r: &[f32]) {
        self.l.extend_from_slice(l);
        self.r.extend_from_slice(r);
    }
}

pub struct Decoded {
    pub audio: Stereo,
    pub sample_rate: u32,
    pub source_channels: u16,
}

/// Decode a whole file. Mono is duplicated to both channels; more than two
/// channels is rejected rather than silently downmixed.
pub fn decode(path: &Path) -> Result<Decoded> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .context("unsupported or unrecognised audio format")?;
    let track = format
        .default_track(TrackType::Audio)
        .context("file has no audio track")?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .context("audio track has no codec parameters")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .context("no decoder for this codec")?;

    let mut audio = Stereo::default();
    let mut sample_rate = 0u32;
    let mut channels = 0usize;
    let mut interleaved: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(e).context("failed reading audio packets"),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            // A corrupt frame is skipped, matching every mainstream player.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e).context("decoder failed"),
        };
        if channels == 0 {
            sample_rate = buf.spec().rate();
            channels = buf.spec().channels().count();
            if channels == 0 || channels > 2 {
                bail!("{channels}-channel audio is not supported (mono or stereo only)");
            }
        }
        interleaved.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(&mut interleaved);
        if channels == 1 {
            audio.extend(&interleaved, &interleaved);
        } else {
            for frame in interleaved.chunks_exact(2) {
                audio.l.push(frame[0]);
                audio.r.push(frame[1]);
            }
        }
    }

    if audio.is_empty() {
        bail!("file decoded to zero samples");
    }
    Ok(Decoded {
        audio,
        sample_rate,
        source_channels: channels as u16,
    })
}

/// Resample a whole buffer; the result has exactly `round(len * to / from)` frames.
pub fn resample(audio: &Stereo, from: u32, to: u32) -> Result<Stereo> {
    if from == to {
        return Ok(audio.clone());
    }
    let mut resampler = Fft::<f32>::new(from as usize, to as usize, 1024, 2, FixedSync::Input)?;
    let channels = [audio.l.clone(), audio.r.clone()];
    let input = SequentialSliceOfVecs::new(&channels, 2, audio.len())?;
    let output = resampler.process_all(&input, audio.len(), None)?;
    let want = expected_len(audio.len(), from, to);
    let mut out = Stereo::with_capacity(want);
    for frame in 0..output.frames().min(want) {
        out.l.push(output.read_sample(0, frame).unwrap_or(0.0));
        out.r.push(output.read_sample(1, frame).unwrap_or(0.0));
    }
    out.l.resize(want, 0.0);
    out.r.resize(want, 0.0);
    Ok(out)
}

fn expected_len(len: usize, from: u32, to: u32) -> usize {
    ((len as f64) * to as f64 / from as f64).round() as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    F32,
    I24,
    I16,
}

/// Streams one stem to `<final>.partial` and renames it on `finish`. Dropping
/// an unfinished sink deletes the partial file. If the stem's working rate
/// differs from the output rate, samples are resampled on the fly so memory
/// stays bounded.
pub struct WavSink {
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    partial: PathBuf,
    final_path: PathBuf,
    depth: BitDepth,
    resampler: Option<StreamResampler>,
}

impl WavSink {
    pub fn create(
        final_path: &Path,
        working_rate: u32,
        output_rate: u32,
        depth: BitDepth,
    ) -> Result<Self> {
        let partial = partial_path(final_path);
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: output_rate,
            bits_per_sample: match depth {
                BitDepth::F32 => 32,
                BitDepth::I24 => 24,
                BitDepth::I16 => 16,
            },
            sample_format: match depth {
                BitDepth::F32 => hound::SampleFormat::Float,
                _ => hound::SampleFormat::Int,
            },
        };
        let writer = hound::WavWriter::create(&partial, spec)
            .with_context(|| format!("cannot create {}", partial.display()))?;
        let resampler = (working_rate != output_rate)
            .then(|| StreamResampler::new(working_rate, output_rate))
            .transpose()?;
        Ok(Self {
            writer: Some(writer),
            partial,
            final_path: final_path.to_path_buf(),
            depth,
            resampler,
        })
    }

    pub fn write(&mut self, l: &[f32], r: &[f32]) -> Result<()> {
        let writer = self.writer.as_mut().expect("write after finish");
        match self.resampler.as_mut() {
            Some(rs) => rs.push(l, r, &mut |l, r| write_frames(writer, self.depth, l, r)),
            None => write_frames(writer, self.depth, l, r),
        }
    }

    pub fn finish(mut self) -> Result<PathBuf> {
        let mut writer = self.writer.take().expect("finish called twice");
        if let Some(rs) = self.resampler.as_mut() {
            rs.flush(&mut |l, r| write_frames(&mut writer, self.depth, l, r))?;
        }
        writer.finalize().context("failed to finalise WAV")?;
        fs::rename(&self.partial, &self.final_path).with_context(|| {
            format!(
                "cannot move output into place at {}",
                self.final_path.display()
            )
        })?;
        Ok(self.final_path.clone())
    }
}

impl Drop for WavSink {
    fn drop(&mut self) {
        if self.writer.take().is_some() {
            let _ = fs::remove_file(&self.partial);
        }
    }
}

pub fn partial_path(final_path: &Path) -> PathBuf {
    let mut name = final_path.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    final_path.with_file_name(name)
}

fn write_frames(
    writer: &mut hound::WavWriter<BufWriter<File>>,
    depth: BitDepth,
    l: &[f32],
    r: &[f32],
) -> Result<()> {
    for (&a, &b) in l.iter().zip(r) {
        for s in [a, b] {
            match depth {
                BitDepth::F32 => writer.write_sample(s)?,
                BitDepth::I24 => {
                    writer.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32)?
                }
                BitDepth::I16 => {
                    writer.write_sample((s.clamp(-1.0, 1.0) * 32_767.0).round() as i16)?
                }
            }
        }
    }
    Ok(())
}

/// Receives resampled `(left, right)` frames.
type Emit<'a> = dyn FnMut(&[f32], &[f32]) -> Result<()> + 'a;

/// Chunked resampler that trims rubato's start-up delay and pads the tail so
/// the output has exactly `round(input_len * ratio)` frames.
struct StreamResampler {
    rs: Fft<f32>,
    from: u32,
    to: u32,
    pending: [Vec<f32>; 2],
    in_chunk: [Vec<f32>; 2],
    out_chunk: [Vec<f32>; 2],
    delay_left: usize,
    total_in: usize,
    total_out: usize,
}

impl StreamResampler {
    fn new(from: u32, to: u32) -> Result<Self> {
        let rs = Fft::<f32>::new(from as usize, to as usize, 1024, 2, FixedSync::Input)?;
        let out_max = rs.output_frames_max();
        Ok(Self {
            delay_left: rs.output_delay(),
            from,
            to,
            pending: [Vec::new(), Vec::new()],
            in_chunk: [Vec::new(), Vec::new()],
            out_chunk: [vec![0.0; out_max], vec![0.0; out_max]],
            total_in: 0,
            total_out: 0,
            rs,
        })
    }

    fn push(&mut self, l: &[f32], r: &[f32], emit: &mut Emit<'_>) -> Result<()> {
        self.total_in += l.len();
        self.pending[0].extend_from_slice(l);
        self.pending[1].extend_from_slice(r);
        while self.pending[0].len() >= self.rs.input_frames_next() {
            self.process_one(usize::MAX, emit)?;
        }
        Ok(())
    }

    fn flush(&mut self, emit: &mut Emit<'_>) -> Result<()> {
        let want = expected_len(self.total_in, self.from, self.to);
        while self.total_out < want {
            let need = self.rs.input_frames_next();
            for ch in &mut self.pending {
                ch.resize(ch.len().max(need), 0.0);
            }
            self.process_one(want, emit)?;
        }
        Ok(())
    }

    fn process_one(&mut self, limit: usize, emit: &mut Emit<'_>) -> Result<()> {
        let need = self.rs.input_frames_next();
        for ch in 0..2 {
            self.in_chunk[ch].clear();
            self.in_chunk[ch].extend(self.pending[ch].drain(..need));
        }
        let out_max = self.out_chunk[0].len();
        let input = SequentialSliceOfVecs::new(&self.in_chunk, 2, need)?;
        let mut output = SequentialSliceOfVecs::new_mut(&mut self.out_chunk, 2, out_max)?;
        let (_, produced) = self.rs.process_into_buffer(&input, &mut output, None)?;
        let skip = self.delay_left.min(produced);
        self.delay_left -= skip;
        let available = produced - skip;
        let take = available.min(limit.saturating_sub(self.total_out));
        if take > 0 {
            emit(
                &self.out_chunk[0][skip..skip + take],
                &self.out_chunk[1][skip..skip + take],
            )?;
            self.total_out += take;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(len: usize, rate: u32, hz: f32) -> Stereo {
        let l: Vec<f32> = (0..len)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect();
        Stereo {
            r: l.iter().map(|s| -s).collect(),
            l,
        }
    }

    #[test]
    fn whole_buffer_resample_roundtrip() {
        let input = sine(48_000, 48_000, 440.0);
        let down = resample(&input, 48_000, 44_100).unwrap();
        assert_eq!(down.len(), 44_100);
        let up = resample(&down, 44_100, 48_000).unwrap();
        assert_eq!(up.len(), 48_000);
        // Ignore the edges, where the band-limited filter rings.
        let err = (2_000..46_000)
            .map(|i| (input.l[i] - up.l[i]).abs())
            .fold(0.0, f32::max);
        assert!(err < 1e-2, "roundtrip error {err}");
    }

    #[test]
    fn wav_sink_streams_resampled_output_with_exact_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lead_vocals.wav");
        let input = sine(44_100 * 2 + 123, 44_100, 330.0);
        let mut sink = WavSink::create(&path, 44_100, 48_000, BitDepth::F32).unwrap();
        for (l, r) in input.l.chunks(7_000).zip(input.r.chunks(7_000)) {
            sink.write(l, r).unwrap();
        }
        assert!(partial_path(&path).exists());
        sink.finish().unwrap();
        assert!(!partial_path(&path).exists());

        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(
            reader.duration() as usize,
            expected_len(input.len(), 44_100, 48_000)
        );

        // Compare the streamed result with the whole-buffer resampler.
        let samples: Vec<f32> = reader.into_samples::<f32>().map(Result::unwrap).collect();
        let reference = resample(&input, 44_100, 48_000).unwrap();
        let err = (2_000..90_000)
            .map(|i| (samples[2 * i] - reference.l[i]).abs())
            .fold(0.0, f32::max);
        assert!(err < 1e-3, "streamed vs whole-buffer error {err}");
    }

    #[test]
    fn dropped_sink_leaves_no_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backing_vocals.wav");
        {
            let mut sink = WavSink::create(&path, 44_100, 44_100, BitDepth::I24).unwrap();
            sink.write(&[0.1; 64], &[0.2; 64]).unwrap();
        }
        assert!(!partial_path(&path).exists());
        assert!(!path.exists());
    }

    #[test]
    fn decode_wav_roundtrip_and_mono_upmix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mono.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..1_000 {
            w.write_sample(((i % 100) as i16 - 50) * 300).unwrap();
        }
        w.finalize().unwrap();

        let d = decode(&path).unwrap();
        assert_eq!(d.sample_rate, 22_050);
        assert_eq!(d.source_channels, 1);
        assert_eq!(d.audio.len(), 1_000);
        assert_eq!(d.audio.l, d.audio.r);
        assert!((d.audio.l[0] - (-50.0 * 300.0 / 32_768.0)).abs() < 1e-4);
    }
}
