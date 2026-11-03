//! Deterministic RNG, calibrated coloured background and the legacy single-tone mains hum.
//! Realistic hum, lines, wind and the device chain live in `sim::chain`.

use std::f64::consts::TAU;

use super::scenario::Noise;

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

/// White noise with one-sided PSD `snr_db − level_db` below an amplitude-1 tone in 1 Hz (sensor self-noise).
pub fn white(n: usize, fs: f64, snr_db: f64, level_db: f64, rng: &mut Pcg) -> Vec<f64> {
    let psd = 0.5 / 10f64.powf((snr_db - level_db) / 10.0);
    let sigma = (psd * fs / 2.0).sqrt();
    (0..n).map(|_| sigma * rng.gauss()).collect()
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
