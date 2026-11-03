//! Sub-bin instantaneous frequency on the complex, unwhitened Hann STFT: Candan's three-sample estimator
//! δ = c·Re{(X[k+1] − X[k−1]) / (2X[k] − X[k−1] − X[k+1])}, iterated about the running estimate.
//! Whitened images are for detection only: their floor and clip bias peak positions (docs/AUDIO.md).

use std::f64::consts::TAU;

use super::Audio;
use super::resample::decimate_for;

const ITERS: usize = 3;
/// Search and accept within ±3 log bins (1/96 octave) of the seed: whitened seeds of slow ridges are off by up to ~2.
const MAX_DEV_REL: f64 = 3.0 * 0.0072;
/// Longest window with |slope|·T² below this; the Hann estimate stays unbiased (< 0.005 bin) up to ~5.
const CHIRP_SPREAD: f64 = 4.0;

struct HannWin {
    w: Vec<f64>,
    /// Candan's bias-correction constant for this window, fitted on clean tones.
    c: f64,
}

impl HannWin {
    fn new(n: usize) -> Self {
        let w = (0..n).map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()).collect();
        let mut h = Self { w, c: 1.0 };
        h.c = h.calibrate();
        h
    }

    fn len_s(&self, sr: f64) -> f64 {
        self.w.len() as f64 / sr
    }

    /// DTFT of the windowed segment at `f` cycles/sample, phase referenced to the window centre (real Hann lobe).
    fn dtft(seg: &[f64], f: f64) -> (f64, f64) {
        let mid = (seg.len() as f64 - 1.0) / 2.0;
        let (sc, ss) = ((TAU * f).cos(), -(TAU * f).sin());
        let a0 = TAU * f * mid;
        let (mut zc, mut zs) = (a0.cos(), a0.sin());
        let (mut re, mut im) = (0.0, 0.0);
        for &v in seg {
            re += v * zc;
            im += v * zs;
            (zc, zs) = (zc * sc - zs * ss, zc * ss + zs * sc);
        }
        (re, im)
    }

    /// Uncorrected Candan ratio about `f`, in DFT bins.
    fn raw(&self, seg: &[f64], f: f64) -> f64 {
        let df = 1.0 / self.w.len() as f64;
        let (m, z, p) = (Self::dtft(seg, f - df), Self::dtft(seg, f), Self::dtft(seg, f + df));
        let num = (p.0 - m.0, p.1 - m.1);
        let den = (2.0 * z.0 - m.0 - p.0, 2.0 * z.1 - m.1 - p.1);
        (num.0 * den.0 + num.1 * den.1) / (den.0 * den.0 + den.1 * den.1).max(1e-300)
    }

    fn calibrate(&self) -> f64 {
        let n = self.w.len();
        let k = (n / 8) as f64;
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for i in 0..=18 {
            let delta = -0.45 + 0.05 * i as f64;
            let f = (k + delta) / n as f64;
            let seg: Vec<f64> = self.w.iter().enumerate().map(|(j, w)| w * (TAU * f * j as f64).sin()).collect();
            let r = self.raw(&seg, k / n as f64);
            sxy += delta * r;
            sxx += r * r;
        }
        sxy / sxx
    }

    fn estimate(&self, seg: &[f64], f_seed: f64) -> f64 {
        let df = 1.0 / self.w.len() as f64;
        let reach = ((MAX_DEV_REL * f_seed / df).ceil() as i64).clamp(1, 8);
        let mag = |f: f64| {
            let z = Self::dtft(seg, f);
            z.0 * z.0 + z.1 * z.1
        };
        let mut f = (-reach..=reach)
            .map(|j| f_seed + j as f64 * df)
            .max_by(|a, b| mag(*a).total_cmp(&mag(*b)))
            .unwrap_or(f_seed);
        for _ in 0..ITERS {
            f += (self.c * self.raw(seg, f)).clamp(-0.6, 0.6) * df;
        }
        f
    }
}

/// Re-measures line frequencies on the recording's own samples (decimated to the analysis band).
pub struct IfRefiner {
    x: Vec<f32>,
    sr: f64,
    /// Ascending length.
    wins: Vec<HannWin>,
}

impl IfRefiner {
    pub fn new(audio: &Audio, f_max: f64, wins_s: &[f64]) -> Self {
        let a = decimate_for(audio, f_max);
        let sr = a.sample_rate as f64;
        let mut lens: Vec<usize> = wins_s.iter().map(|w| ((w * sr).round() as usize).max(8)).collect();
        lens.sort_unstable();
        lens.dedup();
        Self { x: a.samples.clone(), sr, wins: lens.into_iter().map(HannWin::new).collect() }
    }

    /// Longest window over which a linear chirp of `slope` Hz/s spreads at most `CHIRP_SPREAD` DFT bins.
    fn window_for(&self, slope: f64) -> Option<&HannWin> {
        let fits = |w: &&HannWin| slope.abs() * w.len_s(self.sr).powi(2) <= CHIRP_SPREAD;
        self.wins.iter().rev().find(fits).or(self.wins.first())
    }

    /// Frequency (Hz) of the line near `f_seed` in the frame centred at `t` (s). None when the window leaves the
    /// recording or the estimate strays from the seed (noise won the argmax).
    pub fn refine(&self, t: f64, f_seed: f64, slope_hz_s: f64) -> Option<f64> {
        let win = self.window_for(slope_hz_s)?;
        let n = win.w.len();
        let start = usize::try_from((t * self.sr).round() as i64 - (n / 2) as i64).ok()?;
        let seg: Vec<f64> = win.w.iter().zip(self.x.get(start..start + n)?).map(|(w, &x)| w * x as f64).collect();
        let f = win.estimate(&seg, f_seed / self.sr) * self.sr;
        ((f / f_seed - 1.0).abs() <= MAX_DEV_REL).then_some(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chirp(sr: u32, dur: f64, f0: f64, alpha: f64) -> Audio {
        let samples = (0..(dur * sr as f64) as usize)
            .map(|i| {
                let t = i as f64 / sr as f64;
                (TAU * (f0 * t + 0.5 * alpha * t * t)).sin() as f32
            })
            .collect();
        Audio { sample_rate: sr, samples }
    }

    #[test]
    fn clean_tones_and_chirps_within_a_hundredth_of_a_bin() {
        for (f0, alpha) in [(100.3, 0.0), (237.71, 0.0), (120.0, -0.5), (300.0, -4.0)] {
            let a = chirp(2000, 20.0, f0, alpha);
            let r = IfRefiner::new(&a, 1000.0, &[0.5, 1.0, 2.0]);
            for t in [5.0, 10.0, 12.37] {
                let truth = f0 + alpha * t;
                let f = r.refine(t, truth * 1.006, alpha).expect("inside the recording");
                let df = 1.0 / r.window_for(alpha).expect("window").len_s(r.sr);
                assert!(((f - truth) / df).abs() < 1e-2, "f0 {f0} α {alpha} t {t}: {f} vs {truth}");
            }
        }
    }

    #[test]
    fn windows_off_the_recording_are_refused() {
        let a = chirp(2000, 4.0, 100.0, 0.0);
        let r = IfRefiner::new(&a, 1000.0, &[2.0]);
        assert!(r.refine(0.5, 100.0, 0.0).is_none());
        assert!(r.refine(2.0, 100.0, 0.0).is_some());
    }
}
