//! Device chain over one or two channels sharing one gain: high-pass (Butterworth, even order), upstream
//! low-pass, NS, normalise to `peak_dbfs`, `gain_db`, hard clip, quantise.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::config::ChainDevice;
use super::ns;
use crate::sim::scenario::Device;

#[derive(Clone, Copy)]
enum Kind {
    High,
    Low,
}

/// One RBJ biquad section with quality `q`.
fn biquad(buf: &mut [f64], fs: f64, fc: f64, q: f64, kind: Kind) {
    let w0 = 2.0 * PI * fc / fs;
    let alpha = w0.sin() / (2.0 * q);
    let cw = w0.cos();
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = match kind {
        Kind::High => ((1.0 + cw) / 2.0 / a0, -(1.0 + cw) / a0, (1.0 + cw) / 2.0 / a0),
        Kind::Low => ((1.0 - cw) / 2.0 / a0, (1.0 - cw) / a0, (1.0 - cw) / 2.0 / a0),
    };
    let (a1, a2) = (-2.0 * cw / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for x in buf.iter_mut() {
        let y = b0 * *x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        (x2, x1, y2, y1) = (x1, *x, y1, y);
        *x = y;
    }
}

/// Butterworth of even `order` as cascaded biquads with the standard pole Qs.
fn butterworth(buf: &mut [f64], fs: f64, fc: f64, order: u32, kind: Kind) {
    if order == 2 {
        return biquad(buf, fs, fc, FRAC_1_SQRT_2, kind);
    }
    for k in 0..order / 2 {
        let q = 1.0 / (2.0 * ((2 * k + 1) as f64 * PI / (2 * order) as f64).sin());
        biquad(buf, fs, fc, q, kind);
    }
}

/// Filters, NS and level; returns the output channels and the linear scale from physical mix to output.
pub fn apply(mut chans: Vec<Vec<f64>>, fs: f64, d: &Device, cd: &ChainDevice) -> (Vec<Vec<f32>>, f64) {
    for buf in &mut chans {
        if let Some(fc) = d.highpass_hz {
            butterworth(buf, fs, fc, cd.highpass_order, Kind::High);
        }
        if let Some(fc) = cd.lowpass_hz.filter(|f| *f < 0.5 * fs) {
            butterworth(buf, fs, fc, 4, Kind::Low);
        }
        if let Some(n) = &cd.ns {
            ns::apply(buf, fs, n);
        }
    }
    let peak = chans.iter().flatten().fold(0.0_f64, |m, x| m.max(x.abs()));
    let scale = if peak > 0.0 { 10f64.powf((d.peak_dbfs + d.gain_db) / 20.0) / peak } else { 1.0 };
    let q = cd.quantise_bits.map(|b| ((1u32 << (b - 1)) - 1) as f64);
    let out = chans
        .iter()
        .map(|c| {
            c.iter()
                .map(|x| {
                    let y = (x * scale).clamp(-1.0, 1.0);
                    q.map_or(y, |q| (y * q).round() / q) as f32
                })
                .collect()
        })
        .collect();
    (out, scale)
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{db, line_power};
    use super::*;

    fn tones(fs: f64, freqs: &[f64]) -> Vec<f64> {
        (0..(20.0 * fs) as usize)
            .map(|i| freqs.iter().map(|f| (std::f64::consts::TAU * f * i as f64 / fs).sin()).sum())
            .collect()
    }

    #[test]
    fn fourth_order_highpass_and_lowpass_shape_the_band() {
        let fs = 8000.0;
        let x = tones(fs, &[40.0, 80.0, 640.0, 3000.0]);
        let d = Device { highpass_hz: Some(160.0), ..Device::default() };
        let cd = ChainDevice { highpass_order: 4, lowpass_hz: Some(1500.0), ..ChainDevice::default() };
        let (y, scale) = apply(vec![x.clone()], fs, &d, &cd);
        let y: Vec<f64> = y[0].iter().map(|v| *v as f64 / scale).collect();
        let g = |f: f64| db(line_power(&y, fs, f, 1.0)) - db(line_power(&x, fs, f, 1.0));
        assert!(g(80.0) < -21.0 && g(80.0) > -27.0, "80 Hz {}", g(80.0));
        assert!(g(40.0) < -45.0, "40 Hz {}", g(40.0));
        assert!(g(640.0).abs() < 0.5, "640 Hz {}", g(640.0));
        assert!(g(3000.0) < -25.0, "3 kHz {}", g(3000.0));
    }

    #[test]
    fn low_gain_quantises_to_the_bit_grid_and_clipping_saturates() {
        let fs = 2000.0;
        let x = tones(fs, &[100.0]);
        let quiet = Device { peak_dbfs: -50.0, ..Device::default() };
        let cd = ChainDevice { quantise_bits: Some(16), ..ChainDevice::default() };
        let (y, _) = apply(vec![x.clone(), x.clone()], fs, &quiet, &cd);
        assert!(y[0].iter().all(|v| ((*v as f64) * 32767.0 - ((*v as f64) * 32767.0).round()).abs() < 1e-3));
        let levels: std::collections::BTreeSet<i32> = y[0].iter().map(|v| (*v * 32767.0).round() as i32).collect();
        assert!(levels.len() < 220, "−50 dBFS sine should use ~207 codes, got {}", levels.len());
        assert_eq!(y[0], y[1]);
        let hot = Device { gain_db: 12.0, ..Device::default() };
        let (z, _) = apply(vec![x], fs, &hot, &ChainDevice::default());
        let clipped = z[0].iter().filter(|v| v.abs() >= 1.0).count() as f64 / z[0].len() as f64;
        assert!(clipped > 0.3, "clipped fraction {clipped}");
    }
}
