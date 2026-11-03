//! Per-event dump of discovery on posed eval cases, labelled as the discovered eval labels them, with the
//! ADS-B-predicted Doppler factor of the matched flight at the ridge times and diagnostic refits.
//! MEASURE_EXPERIMENT=<toml> [MEASURE_SIM_DIR=<dir>] MEASURE_OUT=<jsonl>.

use std::io::Write;

use serde_json::json;

use super::super::{Event, Ridge, discover_refined, fit_cv_with};
use crate::audio::resample::decimate_for;
use crate::config::Config;
use crate::dem::geoid_undulation;
use crate::eval::case;
use crate::eval::discovered::truth::{self, Found, Labelled};
use crate::eval::metrics::same_flight;
use crate::geo::{Geodetic, geodetic_to_ecef};
use crate::physics::{self, Atmosphere};

fn with_f(r: &Ridge, f: Vec<f64>) -> Ridge {
    Ridge { t: r.t.clone(), f_hz: f, snr: r.snr.clone() }
}

fn keep(r: &Ridge, m: &[bool]) -> Ridge {
    let pick = |v: &[f64]| v.iter().zip(m).filter(|x| *x.1).map(|x| *x.0).collect::<Vec<f64>>();
    Ridge { t: pick(&r.t), f_hz: pick(&r.f_hz), snr: r.snr.iter().zip(m).filter(|x| *x.1).map(|x| *x.0).collect() }
}

fn line(t: &[f64], y: &[f64]) -> (f64, f64) {
    let n = t.len() as f64;
    let (mt, my) = (t.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let sxy: f64 = t.iter().zip(y).map(|(a, b)| (a - mt) * (b - my)).sum();
    let sxx: f64 = t.iter().map(|a| (a - mt).powi(2)).sum();
    let b = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    (my - b * mt, b)
}

fn fit_json(r: &Ridge, c: f64) -> serde_json::Value {
    let f = fit_cv_with(r, c, &Default::default());
    json!({"f0": f.best.f0, "v": f.best.v, "d": f.best.d_cpa, "t": f.best.t_cpa_arrive, "rmse": f.rmse_hz,
           "d_range": f.d_range, "v_range": f.v_range})
}

/// Refits isolating causes: the truth curve itself (model error), the ridge with the ADS-B-relative linear drift
/// removed, and the ridge restricted to frames within 0.5 % of the drift-corrected truth (stitch outliers).
fn diagnose(e: &Event, dfac: &[f64], c: f64) -> serde_json::Value {
    let r = &e.ridge;
    let ok: Vec<bool> = dfac.iter().map(|d| d.is_finite()).collect();
    if ok.iter().filter(|x| **x).count() < 8 {
        return json!(null);
    }
    let rr = keep(r, &ok);
    let dd: Vec<f64> = dfac.iter().copied().filter(|d| d.is_finite()).collect();
    let lr: Vec<f64> = rr.f_hz.iter().zip(&dd).map(|(f, d)| (f / d).ln()).collect();
    let mut s = lr.clone();
    s.sort_by(f64::total_cmp);
    let med = s[s.len() / 2];
    let inl: Vec<bool> = lr.iter().map(|x| (x - med).abs() < 0.01).collect();
    let (ti, li): (Vec<f64>, Vec<f64>) = rr.t.iter().zip(&lr).zip(&inl).filter(|x| *x.1).map(|x| (*x.0.0, *x.0.1)).unzip();
    let (a, b) = if ti.len() >= 8 { line(&ti, &li) } else { (med, 0.0) };
    let detr: Vec<f64> = rr.t.iter().zip(&rr.f_hz).map(|(t, f)| f * (-(b * t)).exp()).collect();
    let clean: Vec<bool> = rr.t.iter().zip(&lr).map(|(t, l)| (l - a - b * t).abs() < 0.005).collect();
    let f0 = (a + b * 0.5 * (rr.t[0] + rr.t[rr.t.len() - 1])).exp();
    let truth_curve: Vec<f64> = dd.iter().map(|d| f0 * d).collect();
    json!({
        "beta_per_s": b,
        "frac_off_1pct": 1.0 - inl.iter().filter(|x| **x).count() as f64 / inl.len() as f64,
        "frac_off_half_pct": 1.0 - clean.iter().filter(|x| **x).count() as f64 / clean.len() as f64,
        "fit_truthcurve": fit_json(&with_f(&rr, truth_curve), c),
        "fit_detrended": fit_json(&with_f(&rr, detr), c),
        "fit_clean": fit_json(&keep(&rr, &clean), c),
        "dfac": dd,
        "t_cov": [rr.t[0], rr.t[rr.t.len() - 1]],
    })
}

/// Track geometry over the emission window of the ridge (times shifted back by slant/c): heading change between
/// first and last third, ground speed range, mean vertical rate, and the max angle of velocity from its mean.
fn geometry(tr: &crate::track::Track, obs: crate::geo::Vec3, t_abs: &[f64], lag: f64) -> serde_json::Value {
    let up = obs.normalized();
    let st: Vec<_> = t_abs.iter().filter_map(|t| tr.state_at(t - lag)).collect();
    if st.len() < 4 {
        return json!(null);
    }
    let horiz = |v: crate::geo::Vec3| v - up * v.dot(up);
    let n = st.len();
    let mean_h = |s: &[crate::track::State]| s.iter().fold(crate::geo::Vec3::default(), |a, x| a + horiz(x.vel));
    let (a, b) = (mean_h(&st[..n / 3]), mean_h(&st[2 * n / 3..]));
    let turn = (a.normalized().dot(b.normalized())).clamp(-1.0, 1.0).acos().to_degrees();
    let gs: Vec<f64> = st.iter().map(|x| horiz(x.vel).norm()).collect();
    let vz = st.iter().map(|x| x.vel.dot(up)).sum::<f64>() / n as f64;
    let vm = st.iter().fold(crate::geo::Vec3::default(), |acc, x| acc + x.vel).normalized();
    let dev = st.iter().map(|x| x.vel.normalized().dot(vm).clamp(-1.0, 1.0).acos().to_degrees()).fold(0.0, f64::max);
    json!({"turn_deg": turn, "gs_lo": gs.iter().copied().fold(f64::MAX, f64::min), "gs_hi": gs.iter().copied().fold(0.0, f64::max),
           "vz": vz, "dev_deg": dev})
}

#[test]
#[ignore]
fn pass_dump() {
    let path = std::env::var("MEASURE_EXPERIMENT").expect("MEASURE_EXPERIMENT");
    let out = std::env::var("MEASURE_OUT").expect("MEASURE_OUT");
    let sim_dir = std::path::PathBuf::from(std::env::var("MEASURE_SIM_DIR").unwrap_or_default());
    let cfg = Config::from_toml(&std::fs::read_to_string(&path).expect("experiment")).expect("toml");
    let p = &cfg.eval.discovery;
    let mut w = std::io::BufWriter::new(std::fs::File::create(&out).expect("out"));
    for src in case::list(&cfg.eval, &sim_dir).expect("cases") {
        let Ok(c) = case::pose(&src, &cfg.eval) else { continue };
        let a = decimate_for(&c.audio, p.discover.front.f_max);
        let events = discover_refined(&a, &p.discover.front.spectrogram(&a), &p.discover);
        let labelled: Vec<Labelled> = c
            .events
            .iter()
            .zip(&c.truth_flights)
            .map(|(e, f)| Labelled { t_cpa: e.t_cpa_audio, f0: 0.5 * (e.f0_range.0 + e.f0_range.1), flight: f.clone() })
            .collect();
        let found: Vec<Found> = events.iter().map(Found::from_event).collect();
        let ground = c.truth_h_ell.unwrap_or_else(|| geoid_undulation(c.truth_lat, c.truth_lon));
        let obs = geodetic_to_ecef(Geodetic { lat_deg: c.truth_lat, lon_deg: c.truth_lon, h_ell_m: ground + cfg.search.mic_height_m });
        let atm: Atmosphere = c.sim_atmosphere.unwrap_or(cfg.search.atmosphere);
        let passes = truth::passes(&c.tracks, obs, &atm, c.t_start_true, c.audio.duration_s(), p);
        let mut labels = truth::label(&found, &labelled, &passes, p);
        truth::gate_clock(&mut labels, cfg.eval.time_prior_s);
        for (i, (e, l)) in events.iter().zip(&labels).enumerate() {
            let pass = l.flight.as_deref().and_then(|f| {
                passes.iter().filter(|q| same_flight(&q.flight, f)).min_by(|x, y| {
                    (x.t_cpa - e.fit.best.t_cpa_arrive).abs().total_cmp(&(y.t_cpa - e.fit.best.t_cpa_arrive).abs())
                })
            });
            let diag = pass.and_then(|q| c.tracks.iter().find(|t| t.id == q.flight)).map(|tr| {
                let t_abs: Vec<f64> = e.ridge.t.iter().map(|t| t + c.t_start_true).collect();
                let lag = pass.map_or(0.0, |q| q.slant_m / e.fit.best.c);
                let mut d = diagnose(e, &physics::doppler_factors(tr, obs, &t_abs, &atm), e.fit.best.c);
                if let Some(o) = d.as_object_mut() {
                    o.insert("geom".into(), geometry(tr, obs, &t_abs, lag));
                    let t0 = pass.map_or(0.0, |q| q.t_cpa);
                    let tg: Vec<f64> = (-240..=240).map(|k| t0 + 0.25 * k as f64).collect();
                    let tga: Vec<f64> = tg.iter().map(|t| t + c.t_start_true).collect();
                    let dg = physics::doppler_factors(tr, obs, &tga, &atm);
                    let full = Ridge { t: tg, f_hz: dg.iter().map(|x| 100.0 * x).collect(), snr: vec![1.0; 481] };
                    let m: Vec<bool> = full.f_hz.iter().map(|f| f.is_finite()).collect();
                    o.insert("fit_truthfull".into(), fit_json(&keep(&full, &m), e.fit.best.c));
                }
                d
            });
            let row = json!({
                "case": c.id, "i": i, "n_labelled": labelled.len(),
                "event": e, "label": l,
                "truth_pass": pass.map(|q| json!({"t_cpa": q.t_cpa, "slant": q.slant_m, "speed": q.speed_mps})),
                "diag": diag,
            });
            writeln!(w, "{row}").expect("write");
        }
        eprintln!("{} events {}", c.id, events.len());
    }
}
