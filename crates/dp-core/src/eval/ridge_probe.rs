//! Along-track ridge probe (docs/research/clock_along_track_2026-11-04.md). For each case's first labelled event,
//! the observer slides along the truth flight's ground track by v·Δ while the start clock shifts by Δ, which
//! keeps that flight's Doppler curve fixed. At each Δ it scores the truth flight (should stay flat) and every
//! other flight passing within `REACH_M` (window = whole recording, wide f0), with the clock locked. A peak of
//! the other flights' sum at Δ = 0 means spectrogram evidence from unassociated flights can break the ridge.
//! Output: runs/<stamp>/ridge_probe.json.

use std::path::Path;

use anyhow::Result;

use super::{atmosphere, case, metrics::same_flight};
use crate::audio::spectrogram::log_spectrogram;
use crate::config::Config;
use crate::dem::geoid_undulation;
use crate::geo::{Geodetic, geodetic_to_ecef, up_vector};
use crate::physics::cpa;
use crate::search::score::{EventTarget, ScoreParams, ScoringPlane, score_joint};
use crate::track::Track;

const REACH_M: f64 = 8_000.0;
const STEP_S: f64 = 10.0;
const OTHER_F0_HZ: (f64, f64) = (50.0, 300.0);

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Probe {
    pub case: String,
    pub deltas_s: Vec<f64>,
    pub truth_z: Vec<f64>,
    pub others: Vec<(String, Vec<f64>)>,
    pub others_sum: Vec<f64>,
}

pub fn run(cfg: &Config, sim_dir: &Path, out_dir: &Path) -> Result<Vec<Probe>> {
    std::fs::create_dir_all(out_dir)?;
    let mut all = Vec::new();
    for src in case::list(&cfg.eval, sim_dir)? {
        let Ok(c) = case::pose(&src, &cfg.eval) else { continue };
        let Some(p) = probe_case(cfg, &c) else { continue };
        let argmax = |v: &[f64]| v.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(f64::NAN, |(i, _)| p.deltas_s[i]);
        let span = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min);
        eprintln!(
            "{:<30} truth argmax {:>5.0} s span {:>6.1} | {:>2} others argmax {:>5.0} s span {:>6.1}",
            c.id,
            argmax(&p.truth_z),
            span(&p.truth_z),
            p.others.len(),
            argmax(&p.others_sum),
            span(&p.others_sum)
        );
        all.push(p);
    }
    std::fs::write(out_dir.join("ridge_probe.json"), serde_json::to_vec_pretty(&all)?)?;
    Ok(all)
}

fn probe_case(cfg: &Config, c: &case::Posed) -> Option<Probe> {
    let (ev, truth) = c.events.iter().zip(&c.truth_flights).find_map(|(e, t)| t.as_ref().map(|t| (e, t)))?;
    let track = c.tracks.iter().find(|t| same_flight(&t.id, truth))?;
    let mut p = cfg.search.clone();
    p.atmosphere = atmosphere::case_atmosphere(p.atmosphere, c, &cfg.eval);
    let locked = ScoreParams { clock_sweep_s: 0.0, ..p.score.clone() };
    let flat = c.truth_h_ell.unwrap_or_else(|| geoid_undulation(c.truth_lat, c.truth_lon));
    let ground = c.dem.as_ref().and_then(|d| d.height_ellipsoid_latlon(c.truth_lat, c.truth_lon)).unwrap_or(flat);
    let obs0 = geodetic_to_ecef(Geodetic { lat_deg: c.truth_lat, lon_deg: c.truth_lon, h_ell_m: ground + p.mic_height_m });
    let vel = track.state_at(c.t_start_true + ev.t_cpa_audio)?.vel;
    let up = up_vector(c.truth_lat, c.truth_lon);
    let horiz = vel + up * -vel.dot(up);
    let (speed, dir) = (horiz.norm(), horiz.normalized());
    let plane = ScoringPlane::new(&log_spectrogram(&c.audio, &cfg.spec), &p.score);
    let dur = c.audio.duration_s();
    let span = cfg.eval.time_prior_s;
    let deltas: Vec<f64> = (0..=(2.0 * span / STEP_S) as usize).map(|i| -span + i as f64 * STEP_S).collect();
    let at = |d: f64| obs0 + dir * (speed * d);
    let z = |t: &Track, window: (f64, f64), f0: (f64, f64), d: f64| {
        let target = [EventTarget { track: t, window, f0_range: f0 }];
        score_joint(&plane, &target, at(d), &p.atmosphere, c.t_start_true + d, &locked).map_or(0.0, |s| s.total)
    };
    let is_truth = |t: &Track| c.truth_flights.iter().flatten().any(|f| same_flight(&t.id, f));
    let audible = |t: &Track| {
        cpa(t, obs0, &p.atmosphere).is_some_and(|k| {
            k.slant_m <= REACH_M && k.t_arrive >= c.t_start_true - span && k.t_arrive <= c.t_start_true + dur + span
        })
    };
    let others: Vec<(String, Vec<f64>)> = c
        .tracks
        .iter()
        .filter(|t| !is_truth(t) && audible(t))
        .map(|t| (t.id.clone(), deltas.iter().map(|&d| z(t, (0.0, dur), OTHER_F0_HZ, d)).collect()))
        .collect();
    let others_sum = (0..deltas.len()).map(|i| others.iter().map(|(_, v)| v[i]).sum()).collect();
    Some(Probe {
        case: c.id.clone(),
        truth_z: deltas.iter().map(|&d| z(track, ev.window, ev.f0_range, d)).collect(),
        deltas_s: deltas,
        others,
        others_sum,
    })
}
