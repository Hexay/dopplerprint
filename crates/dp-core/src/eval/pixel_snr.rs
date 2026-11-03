//! Per-pixel SNR along the truth curve of each event (settling measurement for the power-LLR switch rule).
//! At the truth observer and truth flight, the clock and f0 come from a z-sum sweep; then each harmonic's
//! curve is read on the power plane (ŝ = mean x − 1 per 5 s segment, x = P/N̂) and on the raw whitened plane
//! (per-frame z of the weighted harmonic sum, as z-sum scores it). Output: runs/<stamp>/pixel_snr.json.

use std::path::Path;

use anyhow::Result;

use super::{atmosphere, case, metrics::same_flight};
use crate::audio::LogSpectrogram;
use crate::audio::spectrogram::log_spectrogram;
use crate::config::Config;
use crate::dem::geoid_undulation;
use crate::geo::{Geodetic, geodetic_to_ecef};
use crate::search::power_llr;
use crate::search::job::prepare_job;
use crate::search::score::{EventTarget, ScoreParams, ScoringPlane, score_joint};

const SEGMENT_FRAMES: usize = 20;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct EventSnr {
    pub case: String,
    pub event: usize,
    pub f0: f64,
    /// Per harmonic (1-based index = position + 1): ŝ per 5 s segment.
    pub seg_snr: Vec<Vec<f64>>,
    /// Per harmonic: mean ŝ over the whole event window.
    pub mean_snr: Vec<f64>,
    /// Median over frames of the per-frame weighted-harmonic z (z-sum weights, ÷ √Σw²).
    pub median_frame_z: f64,
}

pub fn run(cfg: &Config, sim_dir: &Path, out_dir: &Path) -> Result<Vec<EventSnr>> {
    std::fs::create_dir_all(out_dir)?;
    let mut all = Vec::new();
    for src in case::list(&cfg.eval, sim_dir)? {
        let Ok(c) = case::pose(&src, &cfg.eval) else { continue };
        let found = measure_case(cfg, &c);
        eprintln!("{:<34} {} events measured", c.id, found.len());
        all.extend(found);
    }
    std::fs::write(out_dir.join("pixel_snr.json"), serde_json::to_vec_pretty(&all)?)?;
    Ok(all)
}

fn measure_case(cfg: &Config, c: &case::Posed) -> Vec<EventSnr> {
    let mut p = cfg.search.clone();
    // Model atmosphere (no weather fetch): the SNR read along the curve barely depends on it.
    p.atmosphere = atmosphere::case_atmosphere(p.atmosphere, c, &cfg.eval);
    p.score.clock_sweep_s = cfg.eval.time_prior_s.max(p.score.clock_sweep_s);
    let flat = c.truth_h_ell.unwrap_or_else(|| geoid_undulation(c.truth_lat, c.truth_lon));
    let ground = c.dem.as_ref().and_then(|d| d.height_ellipsoid_latlon(c.truth_lat, c.truth_lon)).unwrap_or(flat);
    let obs = geodetic_to_ecef(Geodetic { lat_deg: c.truth_lat, lon_deg: c.truth_lon, h_ell_m: ground + p.mic_height_m });
    let z_raw = log_spectrogram(&c.audio, &cfg.spec);
    let z_plane = ScoringPlane::new(&z_raw, &p.score);
    let x_plane = power_llr::plane(&c.audio, &cfg.spec, f32::INFINITY);
    let mut out = Vec::new();
    for (i, (ev, truth)) in c.events.iter().zip(&c.truth_flights).enumerate() {
        let Some(truth) = truth else { continue };
        let best = c
            .tracks
            .iter()
            .filter(|t| same_flight(&t.id, truth))
            .filter_map(|track| {
                let t = [EventTarget { track, window: ev.window, f0_range: ev.f0_range }];
                score_joint(&z_plane, &t, obs, &p.atmosphere, c.t_start_true, &p.score).map(|js| (track, js))
            })
            .max_by(|a, b| a.1.total.total_cmp(&b.1.total));
        let Some((track, js)) = best else { continue };
        let t = EventTarget { track, window: ev.window, f0_range: ev.f0_range };
        let at_clock = ScoreParams { clock_sweep_s: 0.0, ..p.score.clone() };
        let job = prepare_job(&x_plane, &[t], obs, &p.atmosphere, js.clock_offset, &at_clock);
        let frames = job.events[0].frames.clone();
        let mut offs = Vec::new();
        power_llr::offsets_at(&job.events[0], 0.0, &mut offs);
        let b0 = x_plane.bin_of(js.events[0].f0);
        let bins = |h: u32| -> Vec<Option<(usize, f64)>> {
            let hs = x_plane.bins_per_octave as f64 * (h as f64).log2();
            frames.clone().zip(&offs).map(|(f, o)| o.is_finite().then_some((f, b0 + o + hs))).collect()
        };
        let mut e = EventSnr { case: c.id.clone(), event: i, f0: js.events[0].f0, ..Default::default() };
        let mut frame_z = vec![0.0; frames.len()];
        let norm = p.score.harmonics.iter().map(|(_, w)| (*w as f64).powi(2)).sum::<f64>().sqrt();
        for &(h, w) in &p.score.harmonics {
            let pts = bins(h);
            let xs: Vec<Option<f64>> = pts.iter().map(|q| q.and_then(|(f, b)| sample(&x_plane, f, b))).collect();
            let segs: Vec<f64> = xs.chunks(SEGMENT_FRAMES).filter_map(|s| mean(s.iter().flatten())).map(|m| m - 1.0).collect();
            e.mean_snr.push(mean(xs.iter().flatten()).map_or(f64::NAN, |m| m - 1.0));
            e.seg_snr.push(segs);
            for (z, q) in frame_z.iter_mut().zip(&pts) {
                *z += q.and_then(|(f, b)| sample(&z_raw, f, b)).unwrap_or(0.0) * w as f64 / norm;
            }
        }
        let covered: Vec<f64> = frame_z.iter().zip(&offs).filter(|(_, o)| o.is_finite()).map(|(z, _)| *z).collect();
        e.median_frame_z = median(covered);
        out.push(e);
    }
    out
}

fn sample(s: &LogSpectrogram, frame: usize, bin: f64) -> Option<f64> {
    if !(0.0..(s.n_f - 1) as f64).contains(&bin) {
        return None;
    }
    let (i, fr) = (bin as usize, bin.fract());
    let row = s.row(frame);
    Some(row[i] as f64 * (1.0 - fr) + row[i + 1] as f64 * fr)
}

fn mean<'a>(v: impl Iterator<Item = &'a f64>) -> Option<f64> {
    let (s, n) = v.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| s / n as f64)
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}
