//! WebRTC-like noise suppression proxy (codec_measurements.md §4): a ~32 ms STFT whose per-bin noise
//! estimate climbs towards the bin's power over ~`time_s`, so anything steady (noise, hum, a slow tone)
//! ends at `depth_db` while onsets pass for a few seconds. Over-subtracted Wiener gain, floored at depth.

use realfft::RealFftPlanner;

use super::config::Ns;

const FRAME_S: f64 = 0.032;
const POWER_SMOOTH: f64 = 0.6;
const GAIN_SMOOTH: f64 = 0.5;
const OVERSUB: f64 = 3.0;
const FALL_RATE: f64 = 0.1;

pub fn apply(buf: &mut [f64], fs: f64, ns: &Ns) {
    let n = ((FRAME_S * fs) as usize).next_power_of_two().max(64);
    let hop = n / 2;
    let mut planner = RealFftPlanner::<f64>::new();
    let (fwd, inv) = (planner.plan_fft_forward(n), planner.plan_fft_inverse(n));
    // sqrt-periodic-Hann analysis and synthesis at 50 % overlap reconstructs exactly.
    let w: Vec<f64> = (0..n).map(|i| (std::f64::consts::PI * i as f64 / n as f64).sin()).collect();
    let bins = n / 2 + 1;
    let depth = 10f64.powf(ns.depth_db / 20.0);
    let rise = hop as f64 / fs / ns.time_s;
    let (mut ps, mut ln_noise, mut gain) = (vec![0.0; bins], vec![f64::NEG_INFINITY; bins], vec![1.0; bins]);
    let mut padded = vec![0.0; hop];
    padded.extend_from_slice(buf);
    padded.resize(padded.len() + n, 0.0);
    let mut out = vec![0.0; padded.len()];
    let mut start = 0;
    while start + n <= padded.len() {
        let mut frame: Vec<f64> = (0..n).map(|i| padded[start + i] * w[i]).collect();
        let mut spec = fwd.make_output_vec();
        fwd.process(&mut frame, &mut spec).unwrap();
        for (b, c) in spec.iter_mut().enumerate() {
            let p = c.norm_sqr() + 1e-30;
            ps[b] = if ln_noise[b].is_finite() { POWER_SMOOTH * ps[b] + (1.0 - POWER_SMOOTH) * p } else { p };
            let lp = ps[b].ln();
            ln_noise[b] = match ln_noise[b] {
                x if !x.is_finite() => lp,
                x if lp > x => x + rise * (lp - x),
                x => x + FALL_RATE * (lp - x),
            };
            let g = (1.0 - OVERSUB * (ln_noise[b] - lp).exp()).max(depth);
            gain[b] = GAIN_SMOOTH * gain[b] + (1.0 - GAIN_SMOOTH) * g;
            *c *= gain[b];
        }
        inv.process(&mut spec, &mut frame).unwrap();
        for i in 0..n {
            out[start + i] += frame[i] * w[i] / n as f64;
        }
        start += hop;
    }
    buf.copy_from_slice(&out[hop..hop + buf.len()]);
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{db, line_power};
    use super::*;
    use crate::sim::noise::Pcg;

    fn tone_plus_noise(fs: f64, secs: f64, on_s: f64) -> Vec<f64> {
        let mut rng = Pcg::new(8);
        (0..(secs * fs) as usize)
            .map(|i| {
                let t = i as f64 / fs;
                let tone = if t >= on_s { (std::f64::consts::TAU * 500.0 * t).sin() } else { 0.0 };
                tone + 0.05 * rng.gauss()
            })
            .collect()
    }

    #[test]
    fn steady_tone_and_noise_both_end_near_depth() {
        let fs = 8000.0;
        let x = tone_plus_noise(fs, 40.0, 0.0);
        let mut y = x.clone();
        apply(&mut y, fs, &Ns { depth_db: -15.0, time_s: 2.0 });
        let tail = (20.0 * fs) as usize;
        let tone = db(line_power(&y[tail..], fs, 500.0, 1.0)) - db(line_power(&x[tail..], fs, 500.0, 1.0));
        let noise = db(line_power(&y[tail..], fs, 2000.0, 300.0)) - db(line_power(&x[tail..], fs, 2000.0, 300.0));
        assert!((tone + 15.0).abs() < 3.0, "steady tone gain {tone} dB");
        assert!(noise < -10.0, "noise gain {noise} dB");
    }

    #[test]
    fn tone_onset_passes_then_fades_within_seconds() {
        let fs = 8000.0;
        let x = tone_plus_noise(fs, 30.0, 10.0);
        let mut y = x.clone();
        apply(&mut y, fs, &Ns { depth_db: -20.0, time_s: 2.0 });
        let seg = |a: f64, b: f64, v: &[f64]| line_power(&v[(a * fs) as usize..(b * fs) as usize], fs, 500.0, 6.0);
        let first = db(seg(10.0, 10.5, &y)) - db(seg(10.0, 10.5, &x));
        let later = db(seg(20.0, 30.0, &y)) - db(seg(20.0, 30.0, &x));
        assert!(first > -3.0, "onset gain {first} dB");
        assert!(later < -15.0, "settled gain {later} dB");
    }

    #[test]
    fn unity_reconstruction_when_depth_is_zero_db() {
        let fs = 4000.0;
        let x = tone_plus_noise(fs, 5.0, 0.0);
        let mut y = x.clone();
        apply(&mut y, fs, &Ns { depth_db: 0.0, time_s: 1.0 });
        let err = x.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(err < 1e-9, "max reconstruction error {err}");
    }
}
