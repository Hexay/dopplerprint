//! Deterministic RNG, calibrated coloured background, mains hum and device colouring.

use std::f64::consts::{PI, TAU};

use super::scenario::{Device, Noise};

/// SplitMix64: tiny, seedable, good enough for synthetic noise.
#[derive(Clone, Debug)]
pub struct Pcg(u64);

impl Pcg {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
    pub fn gauss(&mut self) -> f64 {
        let u1 = 1.0 - self.uniform();
        (-2.0 * u1.ln()).sqrt() * (TAU * self.uniform()).cos()
    }
}

/// White + one-pole-lowpassed ("red") Gaussian noise, scaled so the one-sided PSD at 100 Hz puts an
/// amplitude-1 tone (power ½) exactly `snr_db` above the noise in a 1 Hz band.
pub fn background(n: usize, fs: f64, p: &Noise, rng: &mut Pcg) -> Vec<f64> {
    let b = 1.0 - (-TAU * p.red_corner_hz / fs).exp();
    let a = 1.0 - b;
    let g = 10f64.powf(p.red_db / 20.0);
    let w = TAU * 100.0 / fs;
    let h2 = b * b / (1.0 - 2.0 * a * w.cos() + a * a);
    let psd_100 = 0.5 / 10f64.powf(p.snr_db / 10.0);
    let sigma = (psd_100 * fs / (2.0 * (1.0 + g * g * h2))).sqrt();
    let mut red = 0.0;
    (0..n)
        .map(|_| {
            red += b * (rng.gauss() - red);
            sigma * (rng.gauss() + g * red)
        })
        .collect()
}

/// Mains hum at `hum_hz` plus its 3rd harmonic 6 dB down.
pub fn add_hum(buf: &mut [f64], fs: f64, p: &Noise) {
    let Some(f) = p.hum_hz else { return };
    let a = 10f64.powf(p.hum_db / 20.0);
    for (i, x) in buf.iter_mut().enumerate() {
        let t = i as f64 / fs;
        *x += a * ((TAU * f * t).sin() + 0.5 * (TAU * 3.0 * f * t + 0.3).sin());
    }
}

/// 2nd-order Butterworth high-pass (RBJ biquad).
pub fn highpass(buf: &mut [f64], fs: f64, fc: f64) {
    let w0 = 2.0 * PI * fc / fs;
    let alpha = w0.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
    let cw = w0.cos();
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = ((1.0 + cw) / 2.0 / a0, -(1.0 + cw) / a0, (1.0 + cw) / 2.0 / a0);
    let (a1, a2) = (-2.0 * cw / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for x in buf.iter_mut() {
        let y = b0 * *x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        (x2, x1, y2, y1) = (x1, *x, y1, y);
        *x = y;
    }
}

/// Optional high-pass, normalise to `peak_dbfs`, apply `gain_db`, hard-clip. Returns samples and the
/// overall linear scale applied to the physical mix.
pub fn apply_device(mut buf: Vec<f64>, fs: f64, d: &Device) -> (Vec<f32>, f64) {
    if let Some(fc) = d.highpass_hz {
        highpass(&mut buf, fs, fc);
    }
    let peak = buf.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    let scale = if peak > 0.0 { 10f64.powf((d.peak_dbfs + d.gain_db) / 20.0) / peak } else { 1.0 };
    let out = buf.iter().map(|x| (x * scale).clamp(-1.0, 1.0) as f32).collect();
    (out, scale)
}
