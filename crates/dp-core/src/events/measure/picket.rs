//! M1a: sub-bin peak bias of `candidate::peak_near` vs fractional log-bin offset, per front-end stage, on
//! linear chirps (true IF exact at frame centres), against the Candan comparator on the complex STFT.

use super::super::candidate::peak_near;
use super::candan::Candan;
use super::*;
use crate::audio::spectrogram::{log_power_spectrogram, log_spectrogram_with};
use crate::audio::{Audio, FrontEnd, LogSpectrogram, SpecParams, WhitenParams};

const SR: u32 = 2500;

struct Stage {
    name: &'static str,
    off: Vec<f64>,
    err: Vec<f64>,
}

impl Stage {
    fn new(name: &'static str) -> Self {
        Self { name, off: Vec::new(), err: Vec::new() }
    }
}

fn spec_with(audio: &Audio, win_s: f64, w: Option<WhitenParams>) -> LogSpectrogram {
    let p = SpecParams { win_s, ..Default::default() };
    match w {
        Some(w) => log_spectrogram_with(audio, &p, &w),
        None => log_power_spectrogram(audio, &p),
    }
}

/// (stages, picket table rows) for one chirp at one SNR.
fn run(snr_db: f64, alpha: f64, seed: u64) -> Vec<Stage> {
    let dur = (600.0 / alpha).min(240.0);
    let f_start = 120.0 + alpha * dur;
    let f = move |t: f64| f_start - alpha * t;
    let audio = Audio { sample_rate: SR, samples: synth(SR, dur, &f, &[1.0], amp_for_snr_1hz(SR, snr_db), 1.0, seed) };
    let noclip = WhitenParams { clip_lo: -1e6, clip_hi: 1e6, ..Default::default() };
    let specs = [
        ("A raw dB, 1 s", spec_with(&audio, 1.0, None)),
        ("B whitened 1 s, no clip", spec_with(&audio, 1.0, Some(noclip))),
        ("B' whitened 1 s (clip 8)", spec_with(&audio, 1.0, Some(WhitenParams::default()))),
        ("C real: combine_max 0.5/1/2 s", FrontEnd::default().spectrogram(&audio)),
    ];
    let mut stages: Vec<Stage> = specs.iter().map(|(n, _)| Stage::new(n)).collect();
    let mut cands: Vec<(Candan, Stage)> =
        [(0.5, "E Candan 0.5 s"), (1.0, "E Candan 1 s"), (2.0, "E Candan 2 s")]
            .into_iter()
            .map(|(w, n)| (Candan::new(SR, w, 3), Stage::new(n)))
            .collect();
    let real = &specs[3].1;
    for k in 0..real.n_t {
        let t = real.time_of(k);
        if t < 6.0 || t > dur - 6.0 {
            continue;
        }
        let pos = real.bin_of(f(t));
        let off = offset_of(pos);
        for ((_, s), st) in specs.iter().zip(stages.iter_mut()) {
            if let Some((pk, _)) = peak_near(s.row(k), pos) {
                st.off.push(off);
                st.err.push(pk - pos);
            }
        }
        let seed_f = peak_near(real.row(k), pos).map(|(pk, _)| real.freq_of(pk)).unwrap_or(f(t));
        for (c, st) in cands.iter_mut() {
            st.off.push(off);
            st.err.push(real.bin_of(c.estimate(&audio.samples, t, seed_f)) - pos);
        }
    }
    stages.extend(cands.into_iter().map(|(_, s)| s));
    stages
}

fn row(snr: f64, alpha: f64, s: &Stage) -> (String, Picket) {
    let p = picket(&s.off, &s.err);
    let amp = p.curve.iter().copied().fold(f64::MIN, f64::max) - p.curve.iter().copied().fold(f64::MAX, f64::min);
    let line = format!(
        "| {snr:.0} | {alpha} | {} | {:+.3} | {:.3} | {:+.2} | {:.3} | {:.2} | {:.2} | {:.2} |",
        s.name,
        mean(&s.err),
        rms(&s.err),
        p.pull,
        0.5 * amp,
        p.share,
        autocorr(&s.err, 1),
        autocorr(&s.err, 8),
    );
    (line, p)
}

#[test]
#[ignore]
fn picket_fence() {
    println!("Errors in log bins (1 bin = 1/96 oct = 0.72 % of f). pull: 0 unbiased, 1 snapped to centre.");
    println!("| SNR dB/1Hz | α Hz/s | stage | mean | RMS | pull | ½ p-p curve | picket share | ρ1 | ρ8 (2 s) |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let mut curves = Vec::new();
    for snr in [40.0, 20.0, 10.0] {
        for alpha in [0.05, 0.5, 2.0, 5.0] {
            for s in run(snr, alpha, 7 + alpha as u64) {
                let (line, p) = row(snr, alpha, &s);
                println!("{line}");
                if alpha == 0.5 {
                    curves.push((snr, s.name, p.curve));
                }
            }
        }
    }
    println!("\nPicket curve, α = 0.5 Hz/s: mean error (bins) per true-offset decile [-0.5 … 0.5).");
    let head: Vec<String> = (0..N_OFFSET_BINS).map(|i| format!("{:+.2}", -0.45 + 0.1 * i as f64)).collect();
    println!("| SNR | stage | {} |", head.join(" | "));
    println!("|---|---|{}", "---|".repeat(N_OFFSET_BINS));
    for (snr, name, c) in curves {
        let v: Vec<String> = c.iter().map(|x| format!("{x:+.3}")).collect();
        println!("| {snr:.0} | {name} | {} |", v.join(" | "));
    }
}
