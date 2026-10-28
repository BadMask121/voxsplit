//! Short-time Fourier transform matching `torch.stft` / `torch.istft` with
//! `center=True`, `pad_mode="reflect"`, a periodic Hann window, no normalisation
//! and one-sided output. The MDX-Net ONNX graphs were trained on exactly this
//! front end, so any deviation here shows up as separation artefacts.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub struct Stft {
    n_fft: usize,
    hop: usize,
    window: Vec<f32>,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    time_buf: Vec<f32>,
    freq_buf: Vec<Complex32>,
    scratch_fwd: Vec<Complex32>,
    scratch_inv: Vec<Complex32>,
}

impl Stft {
    pub fn new(n_fft: usize, hop: usize) -> Self {
        assert!(n_fft % 2 == 0 && hop > 0 && hop <= n_fft);
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(n_fft);
        let inv = planner.plan_fft_inverse(n_fft);
        Self {
            n_fft,
            hop,
            window: periodic_hann(n_fft),
            time_buf: fwd.make_input_vec(),
            freq_buf: fwd.make_output_vec(),
            scratch_fwd: fwd.make_scratch_vec(),
            scratch_inv: inv.make_scratch_vec(),
            fwd,
            inv,
        }
    }

    pub fn bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    /// Number of frames `forward` produces for a signal of `len` samples.
    pub fn frames_for(&self, len: usize) -> usize {
        len / self.hop + 1
    }

    /// Forward transform. Output is frame-major: `out[t * bins + f]`.
    pub fn forward(&mut self, signal: &[f32]) -> Vec<Complex32> {
        let pad = self.n_fft / 2;
        assert!(signal.len() > pad, "signal shorter than n_fft/2 cannot be reflect-padded");
        let padded = reflect_pad(signal, pad);
        let frames = self.frames_for(signal.len());
        let bins = self.bins();
        let mut out = vec![Complex32::new(0.0, 0.0); frames * bins];

        for t in 0..frames {
            let start = t * self.hop;
            for (i, sample) in self.time_buf.iter_mut().enumerate() {
                *sample = padded[start + i] * self.window[i];
            }
            self.fwd
                .process_with_scratch(&mut self.time_buf, &mut self.freq_buf, &mut self.scratch_fwd)
                .expect("buffer sizes come from the planner");
            out[t * bins..(t + 1) * bins].copy_from_slice(&self.freq_buf);
        }
        out
    }

    /// Inverse transform of a frame-major spectrogram, returning
    /// `hop * (frames - 1)` samples (torch's default `length`).
    pub fn inverse(&mut self, spec: &[Complex32], frames: usize) -> Vec<f32> {
        let bins = self.bins();
        assert_eq!(spec.len(), frames * bins);
        let pad = self.n_fft / 2;
        let full_len = self.n_fft + self.hop * (frames - 1);
        let mut acc = vec![0.0f32; full_len];
        let mut wsum = vec![0.0f32; full_len];
        let norm = 1.0 / self.n_fft as f32;

        for t in 0..frames {
            self.freq_buf.copy_from_slice(&spec[t * bins..(t + 1) * bins]);
            // A real signal's DC and Nyquist bins are real; torch.istft ignores
            // their imaginary parts, and realfft requires them to be zero.
            self.freq_buf[0].im = 0.0;
            self.freq_buf[bins - 1].im = 0.0;
            self.inv
                .process_with_scratch(&mut self.freq_buf, &mut self.time_buf, &mut self.scratch_inv)
                .expect("buffer sizes come from the planner");
            let start = t * self.hop;
            for i in 0..self.n_fft {
                let w = self.window[i];
                acc[start + i] += self.time_buf[i] * norm * w;
                wsum[start + i] += w * w;
            }
        }

        let len = self.hop * (frames - 1);
        (0..len)
            .map(|i| {
                let w = wsum[pad + i];
                if w > 1e-11 { acc[pad + i] / w } else { 0.0 }
            })
            .collect()
    }
}

/// `torch.hann_window(n, periodic=True)`.
pub fn periodic_hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()) as f32)
        .collect()
}

/// `numpy.hanning(n)` (symmetric, zero at both ends) — the overlap-add weight UVR uses.
pub fn symmetric_hann(n: usize) -> Vec<f32> {
    if n == 1 {
        return vec![1.0];
    }
    (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos()) as f32)
        .collect()
}

/// numpy/torch "reflect" padding (the edge sample is not repeated).
fn reflect_pad(signal: &[f32], pad: usize) -> Vec<f32> {
    let n = signal.len();
    let mut out = Vec::with_capacity(n + 2 * pad);
    out.extend((1..=pad).rev().map(|i| signal[i]));
    out.extend_from_slice(signal);
    out.extend((0..pad).map(|i| signal[n - 2 - i]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_signal(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let t = i as f32 / 44_100.0;
                0.5 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                    + 0.25 * (2.0 * std::f32::consts::PI * 3_100.0 * t * (1.0 + t)).sin()
            })
            .collect()
    }

    #[test]
    fn reflect_pad_matches_numpy() {
        assert_eq!(reflect_pad(&[1.0, 2.0, 3.0, 4.0], 2), vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]);
    }

    #[test]
    fn frame_count_matches_mdx_geometry() {
        let stft = Stft::new(7680, 1024);
        assert_eq!(stft.frames_for(1024 * 255), 256);
        assert_eq!(stft.bins(), 3841);
    }

    #[test]
    fn roundtrip_is_identity() {
        for &(n_fft, hop) in &[(7680, 1024), (5120, 1024), (512, 128)] {
            let mut stft = Stft::new(n_fft, hop);
            let signal = test_signal(hop * 40);
            let frames = stft.frames_for(signal.len());
            let spec = stft.forward(&signal);
            let back = stft.inverse(&spec, frames);
            assert_eq!(back.len(), signal.len());
            let max_err = signal.iter().zip(&back).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(max_err < 1e-4, "n_fft={n_fft}: max error {max_err}");
        }
    }

    #[test]
    fn dc_bin_of_constant_signal() {
        let mut stft = Stft::new(512, 128);
        let spec = stft.forward(&vec![1.0; 2048]);
        // Sum of a periodic Hann window of length n is n/2.
        assert!((spec[5 * stft.bins()].re - 256.0).abs() < 1e-2);
    }
}
