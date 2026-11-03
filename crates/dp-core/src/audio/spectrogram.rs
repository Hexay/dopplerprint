use super::Audio;
use super::resample::decimate_for;
use super::stft::{Grid, log_power, log_taps, nfft_for};
use super::whiten::{WhitenParams, median, whiten};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SpecParams {
    pub win_s: f64,
    pub hop_s: f64,
    pub f_min: f64,
    pub f_max: f64,
    pub bins_per_octave: u32,
}

impl Default for SpecParams {
    fn default() -> Self {
        Self {
            win_s: 1.0,
            hop_s: 0.25,
            f_min: 20.0,
            f_max: 1000.0,
            bins_per_octave: 96,
        }
    }
}

/// Whitened (robust z-score) power on a log-frequency axis, row-major `[frame][bin]`.
/// Log axis makes f0 and harmonic number pure vertical shifts.
#[derive(Clone, Debug)]
pub struct LogSpectrogram {
    /// Audio-relative time (s) of frame 0's centre.
    pub t0: f64,
    pub dt: f64,
    pub f_min: f64,
    pub bins_per_octave: u32,
    pub n_t: usize,
    pub n_f: usize,
    pub data: Vec<f32>,
}

impl LogSpectrogram {
    pub fn freq_of(&self, bin: f64) -> f64 {
        self.f_min * (bin / self.bins_per_octave as f64).exp2()
    }
    pub fn bin_of(&self, f_hz: f64) -> f64 {
        (f_hz / self.f_min).log2() * self.bins_per_octave as f64
    }
    pub fn frame_of(&self, t_audio: f64) -> f64 {
        (t_audio - self.t0) / self.dt
    }
    pub fn row(&self, frame: usize) -> &[f32] {
        &self.data[frame * self.n_f..(frame + 1) * self.n_f]
    }
    pub fn time_of(&self, frame: usize) -> f64 {
        self.t0 + frame as f64 * self.dt
    }
    pub fn freqs(&self) -> Vec<f64> {
        (0..self.n_f).map(|b| self.freq_of(b as f64)).collect()
    }
    pub fn times(&self) -> Vec<f64> {
        (0..self.n_t).map(|k| self.time_of(k)).collect()
    }
}

pub fn log_spectrogram(audio: &Audio, p: &SpecParams) -> LogSpectrogram {
    log_spectrogram_with(audio, p, &WhitenParams::default())
}

/// STFT → log-f resample → whitening. Audio above the band is decimated first (exact integer factor).
pub fn log_spectrogram_with(audio: &Audio, p: &SpecParams, w: &WhitenParams) -> LogSpectrogram {
    let (mut spec, power) = power_on_log_axis(audio, p);
    spec.data = whiten(&power, spec.n_t, spec.n_f, p.hop_s, &spec.freqs(), w);
    spec
}

/// Unwhitened power in dB (arbitrary reference) on the same grid: for display and level checks.
/// Stationary tones survive here; `log_spectrogram` removes them by design.
pub fn log_power_spectrogram(audio: &Audio, p: &SpecParams) -> LogSpectrogram {
    let (mut spec, power) = power_on_log_axis(audio, p);
    let floor = 1e-10 * power.iter().map(|&x| x as f64).sum::<f64>() / power.len().max(1) as f64;
    spec.data = power
        .iter()
        .map(|&x| (10.0 * (x as f64 + floor + 1e-30).log10()) as f32)
        .collect();
    spec
}

pub(super) fn power_on_log_axis(audio: &Audio, p: &SpecParams) -> (LogSpectrogram, Vec<f32>) {
    let a = decimate_for(audio, p.f_max);
    let sr = a.sample_rate;
    let bpo = p.bins_per_octave.max(1);
    let f_max = p.f_max.min(0.45 * sr as f64);
    let n_f = ((f_max / p.f_min).log2() * bpo as f64).floor().max(0.0) as usize + 1;
    let win_n = ((p.win_s * sr as f64).round() as usize).max(8);
    let n_t = (a.duration_s() / p.hop_s).floor() as usize + 1;
    let grid = Grid {
        hop_s: p.hop_s,
        n_t,
        win_n,
        nfft: nfft_for(win_n, sr),
    };
    let taps = log_taps(p.f_min, bpo, n_f, sr as f64 / grid.nfft as f64);
    let power = log_power(&a.samples, sr, &grid, &taps);
    (
        LogSpectrogram {
            t0: 0.0,
            dt: p.hop_s,
            f_min: p.f_min,
            bins_per_octave: bpo,
            n_t,
            n_f,
            data: Vec::new(),
        },
        power,
    )
}

/// One whitened spectrogram per window length, all on `p`'s time and frequency grid.
pub fn log_spectrogram_multi(audio: &Audio, p: &SpecParams, w: &WhitenParams, wins_s: &[f64]) -> Vec<LogSpectrogram> {
    let a = decimate_for(audio, p.f_max);
    wins_s
        .iter()
        .map(|&win_s| log_spectrogram_with(&a, &SpecParams { win_s, ..*p }, w))
        .collect()
}

/// Pixel-wise max over same-grid spectrograms, re-standardised so the null is again ~N(0,1).
pub fn combine_max(specs: &[LogSpectrogram]) -> Option<LogSpectrogram> {
    let first = specs.first()?;
    assert!(
        specs.iter().all(|s| s.n_t == first.n_t && s.n_f == first.n_f),
        "combine_max needs one grid"
    );
    let mut out = first.clone();
    for s in &specs[1..] {
        out.data.iter_mut().zip(&s.data).for_each(|(a, &b)| *a = a.max(b));
    }
    if specs.len() > 1 {
        let (lo, hi) = (-3.0f32, out.data.iter().copied().fold(f32::MIN, f32::max));
        let mut scratch = Vec::new();
        let sample: Vec<f32> = out.data.iter().step_by(7).copied().collect();
        let m = median(&sample, &mut scratch);
        let dev: Vec<f32> = sample.iter().map(|x| (x - m).abs()).collect();
        let s = (1.4826 * median(&dev, &mut scratch)).max(1e-6);
        out.data.iter_mut().for_each(|x| *x = ((*x - m) / s).clamp(lo, hi));
    }
    Some(out)
}
