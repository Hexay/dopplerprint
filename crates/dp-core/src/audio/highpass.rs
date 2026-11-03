//! High-pass corner of a recording from its quiet-frame floor (corpus_signatures.md §6 method): the lower
//! edge of the lowest octave band whose 10th-percentile level is within `TOL_DB` of the 200–500 Hz band's.

use super::{Audio, SpecParams, log_power_spectrogram};

const TOL_DB: f64 = 10.0;
const REF_BAND_HZ: (f64, f64) = (200.0, 500.0);
const QUIET_QUANTILE: f64 = 0.1;

/// `sp.f_min` when nothing below the reference band is cut (or the recording is too short to tell).
pub fn corner_hz(audio: &Audio, sp: &SpecParams) -> f64 {
    let sp = SpecParams { f_max: sp.f_max.max(1.2 * REF_BAND_HZ.1), ..*sp };
    let spec = log_power_spectrogram(audio, &sp);
    let floor = |lo: f64, hi: f64| quiet_level(&spec, lo, hi);
    let Some(reference) = floor(REF_BAND_HZ.0, REF_BAND_HZ.1) else { return sp.f_min };
    let mut lo = sp.f_min;
    while lo < REF_BAND_HZ.0 {
        if floor(lo, 2.0 * lo).is_some_and(|l| l >= reference - TOL_DB) {
            return lo;
        }
        lo *= 2.0;
    }
    lo
}

/// 10th percentile over frames of the band-mean level (dB) in [lo, hi) Hz.
fn quiet_level(spec: &super::LogSpectrogram, lo: f64, hi: f64) -> Option<f64> {
    let (a, b) = (spec.bin_of(lo).ceil().max(0.0) as usize, (spec.bin_of(hi).ceil().max(0.0) as usize).min(spec.n_f));
    if a >= b || spec.n_t == 0 {
        return None;
    }
    let mut levels: Vec<f64> =
        (0..spec.n_t).map(|t| spec.row(t)[a..b].iter().map(|&x| x as f64).sum::<f64>() / (b - a) as f64).collect();
    levels.sort_by(f64::total_cmp);
    Some(levels[((levels.len() - 1) as f64 * QUIET_QUANTILE) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(sr: u32, secs: f64) -> Vec<f32> {
        let mut s = 0x2545_f491_4f6c_dd1du64;
        (0..(sr as f64 * secs) as usize)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 40) as f32 / (1u64 << 24) as f32 - 0.5
            })
            .collect()
    }

    /// Four cascaded one-pole high-passes at `fc` (24 dB/octave).
    fn high_pass(x: &[f32], sr: u32, fc: f64) -> Vec<f32> {
        let rc = 1.0 / (2.0 * std::f64::consts::PI * fc);
        let a = rc / (rc + 1.0 / sr as f64);
        let mut y = x.iter().map(|&v| v as f64).collect::<Vec<_>>();
        for _ in 0..4 {
            let (mut prev_x, mut prev_y) = (0.0, 0.0);
            for v in y.iter_mut() {
                let out = a * (prev_y + *v - prev_x);
                (prev_x, prev_y, *v) = (*v, out, out);
            }
        }
        y.into_iter().map(|v| v as f32).collect()
    }

    #[test]
    fn white_noise_has_no_corner_and_a_150_hz_filter_is_found() {
        let (sr, sp) = (4_000, SpecParams::default());
        let x = noise(sr, 20.0);
        assert_eq!(corner_hz(&Audio { sample_rate: sr, samples: x.clone() }, &sp), sp.f_min);
        let hp = corner_hz(&Audio { sample_rate: sr, samples: high_pass(&x, sr, 150.0) }, &sp);
        assert_eq!(hp, 160.0);
    }
}
