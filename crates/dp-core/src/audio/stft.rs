//! Hann STFT power resampled onto a log-frequency axis.

use rayon::prelude::*;
use realfft::RealFftPlanner;

/// How one log bin reads the linear power spectrum.
#[derive(Clone, Copy, Debug)]
pub(super) enum Tap {
    /// Log bin narrower than a linear bin: interpolate between linear bins `i` and `i + 1`.
    Interp { i: usize, w: f32 },
    /// Log bin spans several linear bins: max-pool them, so a narrow tone is never lost between samples.
    Max { lo: usize, hi: usize },
}

impl Tap {
    fn last(&self) -> usize {
        match *self {
            Tap::Interp { i, .. } => i + 1,
            Tap::Max { hi, .. } => hi,
        }
    }
}

/// Log bins tile `[f_min, f_max]` edge to edge, so every linear bin lands in exactly one pooled log bin.
pub(super) fn log_taps(f_min: f64, bpo: u32, n_f: usize, df: f64) -> Vec<Tap> {
    let half = 0.5 / bpo as f64;
    (0..n_f)
        .map(|k| {
            let u = k as f64 / bpo as f64;
            let fc = f_min * u.exp2();
            let lo = (f_min * (u - half).exp2() / df).ceil() as usize;
            let hi = (f_min * (u + half).exp2() / df).floor() as usize;
            if hi > lo {
                Tap::Max { lo, hi }
            } else {
                let x = fc / df;
                Tap::Interp {
                    i: x.floor() as usize,
                    w: x.fract() as f32,
                }
            }
        })
        .collect()
}

pub(super) fn nfft_for(win_n: usize, sr: u32) -> usize {
    win_n.max((sr as f64 / 0.25).ceil() as usize).next_power_of_two()
}

pub(super) struct Grid {
    pub hop_s: f64,
    pub n_t: usize,
    pub win_n: usize,
    pub nfft: usize,
}

/// Power `[frame][log bin]`; frame k is centred on t = k·hop (zero-padded at the edges).
pub(super) fn log_power(samples: &[f32], sr: u32, g: &Grid, taps: &[Tap]) -> Vec<f32> {
    let n_f = taps.len();
    let n_lin = taps.iter().map(Tap::last).max().unwrap_or(0) + 1;
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(g.nfft);
    let win: Vec<f32> = (0..g.win_n)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / g.win_n as f64).cos()) as f32)
        .collect();
    let mut out = vec![0f32; g.n_t * n_f];
    out.par_chunks_mut(n_f).enumerate().for_each_init(
        || {
            (
                fft.make_input_vec(),
                fft.make_output_vec(),
                fft.make_scratch_vec(),
                vec![0f32; n_lin],
            )
        },
        |(inp, spec, scratch, pw), (k, row)| {
            let start = (k as f64 * g.hop_s * sr as f64).round() as i64 - (g.win_n / 2) as i64;
            inp.iter_mut().for_each(|x| *x = 0.0);
            for (i, w) in win.iter().enumerate() {
                let j = start + i as i64;
                if j >= 0 && (j as usize) < samples.len() {
                    inp[i] = samples[j as usize] * w;
                }
            }
            fft.process_with_scratch(inp, spec, scratch)
                .expect("fft sizes fixed by plan");
            for (p, c) in pw.iter_mut().zip(spec.iter()) {
                *p = c.norm_sqr();
            }
            for (r, tap) in row.iter_mut().zip(taps) {
                *r = match *tap {
                    Tap::Interp { i, w } => pw[i] * (1.0 - w) + pw[i + 1] * w,
                    Tap::Max { lo, hi } => pw[lo..=hi].iter().copied().fold(0.0, f32::max),
                };
            }
        },
    );
    out
}
