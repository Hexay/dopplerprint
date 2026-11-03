//! M1b: does the per-frame IF bias reach the CV fit? Exact CV passes (closed-form IF, phase-integrated) go
//! through the real front end + discovery; the event ridge is compared to truth, re-estimated with the
//! Candan comparator at the same frames, and both are fitted with the production fit.

use super::super::discover::confident;
use super::super::{DiscoverConfig, Event, FitParams, Ridge, discover_refined, fit_cv_with};
use super::candan::Candan;
use super::*;
use super::super::candidate::peak_near;
use crate::audio::spectrogram::log_power_spectrogram;
use crate::audio::{Audio, FrontEnd, SpecParams};
use crate::physics::cv::{CvParams, frequency};

const SR: u32 = 2500;
const SEEDS: u64 = 6;

struct Pass {
    name: &'static str,
    f0: f64,
    v: f64,
    d: f64,
}

const PASSES: [Pass; 3] = [
    Pass { name: "steep d=300 v=60", f0: 100.0, v: 60.0, d: 300.0 },
    Pass { name: "mid d=1500 v=90", f0: 108.0, v: 90.0, d: 1500.0 },
    Pass { name: "far d=4000 v=50", f0: 80.0, v: 50.0, d: 4000.0 },
];

fn truth(p: &Pass, ta: f64) -> CvParams {
    CvParams { f0: p.f0, v: p.v, d_cpa: p.d, t_cpa_arrive: ta, c: C }
}

fn best_event(evs: &[Event], f0: f64) -> Option<&Event> {
    evs.iter()
        .filter(|e| (e.fit.best.f0 / e.ridge_harmonic.max(1) as f64 / f0 - 1.0).abs() < 0.05)
        .max_by_key(|e| e.ridge.t.len())
}

/// [f0 rel, v rel, d rel, t_cpa s] errors of a fit of `r` (harmonic `h`).
fn fit_err(r: &Ridge, tp: &CvParams, h: f64) -> ([f64; 4], f64) {
    let fit = fit_cv_with(&confident(r), C, &FitParams::default());
    let b = &fit.best;
    let curve_rms = rms(&r.t.iter().map(|&t| frequency(b, t) - h * frequency(tp, t)).collect::<Vec<_>>());
    ([b.f0 / (h * tp.f0) - 1.0, b.v / tp.v - 1.0, b.d_cpa / tp.d_cpa - 1.0, b.t_cpa_arrive - tp.t_cpa_arrive], curve_rms)
}

#[derive(Default)]
struct Acc {
    errs: Vec<[f64; 4]>,
    curve_rms: Vec<f64>,
    frame_rms_bins: Vec<f64>,
    rho1: Vec<f64>,
    pull: Vec<f64>,
}

impl Acc {
    fn push(&mut self, e: [f64; 4], curve_rms: f64, frame_err_bins: &[f64], off: &[f64]) {
        self.errs.push(e);
        self.curve_rms.push(curve_rms);
        self.frame_rms_bins.push(rms(frame_err_bins));
        self.rho1.push(autocorr(frame_err_bins, 1));
        self.pull.push(picket(off, frame_err_bins).pull);
    }
    fn line(&self, label: &str) -> String {
        let col = |i: usize| self.errs.iter().map(|e| e[i]).collect::<Vec<_>>();
        let pm = |x: Vec<f64>, s: f64| format!("{:+.2}±{:.2}", s * mean(&x), s * sd(&x));
        format!(
            "{label} | {} | {:.3} | {:.2} | {:+.2} | {} | {} | {} | {} | {:.3} |",
            self.errs.len(),
            mean(&self.frame_rms_bins),
            mean(&self.rho1),
            mean(&self.pull),
            pm(col(0), 1e4),
            pm(col(1), 100.0),
            pm(col(2), 100.0),
            pm(col(3), 1.0),
            mean(&self.curve_rms),
        )
    }
}

#[test]
#[ignore]
fn cv_fit_bias() {
    println!("f0 err in 1e-4 (100 ppm); v, d in %; t_cpa in s; mean±sd over {SEEDS} noise seeds.");
    println!("frame RMS in log bins vs truth; curve RMS = RMS(fitted curve − true curve) Hz over the ridge.");
    println!("| pass | SNR dB/1Hz | harmonics | ridge | n | frame RMS bins | ρ1 | pull | f0 | v % | d % | t_cpa s | curve RMS Hz |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let candan = Candan::new(SR, 1.0, 3);
    for p in &PASSES {
        let half = (4.0 * p.d / p.v).max(60.0);
        let (dur, ta) = (2.0 * half, half);
        let tp = truth(p, ta);
        for snr in [30.0, 15.0] {
            for harm in [&[1.0][..], &[1.0, 0.5, 0.3][..]] {
                let (mut real, mut cand, mut exact, mut rawdb) =
                    (Acc::default(), Acc::default(), Acc::default(), Acc::default());
                let mut missed = 0;
                for seed in 0..SEEDS {
                    let f = |t: f64| frequency(&tp, t);
                    let x = synth(SR, dur, &f, harm, amp_for_snr_1hz(SR, snr), 1.0, 100 + seed);
                    let audio = Audio { sample_rate: SR, samples: x };
                    let spec = FrontEnd::default().spectrogram(&audio);
                    let raw = log_power_spectrogram(&audio, &SpecParams::default());
                    let evs = discover_refined(&audio, &spec, &DiscoverConfig::default());
                    let Some(e) = best_event(&evs, p.f0) else {
                        missed += 1;
                        continue;
                    };
                    let h = e.ridge_harmonic.max(1) as f64;
                    let ft: Vec<f64> = e.ridge.t.iter().map(|&t| h * frequency(&tp, t)).collect();
                    let off: Vec<f64> = ft.iter().map(|&f| offset_of(spec.bin_of(f))).collect();
                    let bins = |fs: &[f64]| fs.iter().zip(&ft).map(|(a, b)| spec.bin_of(*a) - spec.bin_of(*b)).collect::<Vec<_>>();
                    let (er, cr) = fit_err(&e.ridge, &tp, h);
                    real.push(er, cr, &bins(&e.ridge.f_hz), &off);
                    let cf: Vec<f64> =
                        e.ridge.t.iter().zip(&e.ridge.f_hz).map(|(&t, &f)| candan.estimate(&audio.samples, t, f)).collect();
                    let cr_ridge = Ridge { f_hz: cf.clone(), ..e.ridge.clone() };
                    let (ec, cc) = fit_err(&cr_ridge, &tp, h);
                    cand.push(ec, cc, &bins(&cf), &off);
                    let raw_f: Vec<f64> = e
                        .ridge
                        .t
                        .iter()
                        .zip(&e.ridge.f_hz)
                        .map(|(&t, &f)| {
                            let k = raw.frame_of(t).round() as usize;
                            peak_near(raw.row(k), raw.bin_of(f)).map_or(f, |(pk, _)| raw.freq_of(pk))
                        })
                        .collect();
                    let (erw, crw) = fit_err(&Ridge { f_hz: raw_f.clone(), ..e.ridge.clone() }, &tp, h);
                    rawdb.push(erw, crw, &bins(&raw_f), &off);
                    let ex_ridge = Ridge { f_hz: ft.clone(), ..e.ridge.clone() };
                    let (ee, ce) = fit_err(&ex_ridge, &tp, h);
                    exact.push(ee, ce, &bins(&ft), &off);
                }
                let tag = format!("| {} | {snr:.0} | {} ", p.name, harm.len());
                println!("{}", real.line(&format!("{tag}| production (missed {missed})")));
                println!("{}", rawdb.line(&format!("{tag}| peak_near on raw dB 1 s")));
                println!("{}", cand.line(&format!("{tag}| Candan 1 s")));
                println!("{}", exact.line(&format!("{tag}| exact IF")));
            }
        }
    }
}
