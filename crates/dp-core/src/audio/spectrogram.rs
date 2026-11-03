use super::Audio;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpecParams {
    pub win_s: f64,
    pub hop_s: f64,
    pub f_min: f64,
    pub f_max: f64,
    pub bins_per_octave: u32,
}

impl Default for SpecParams {
    fn default() -> Self {
        Self { win_s: 1.0, hop_s: 0.25, f_min: 20.0, f_max: 1000.0, bins_per_octave: 96 }
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
}

pub fn log_spectrogram(_audio: &Audio, _p: &SpecParams) -> LogSpectrogram {
    todo!("audio agent: STFT (realfft) -> log-freq resample -> 2-D running-median whitening")
}
