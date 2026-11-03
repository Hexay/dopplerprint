//! Grid hypothesis search on the long-term spectrum: nominal 50/60 Hz × sample-rate ratio × fine offset.

use super::HumParams;
use super::spectrum::{FLOOR_HALF_HZ, Spectrum, combine};

/// Rates a recorder or transcoder plausibly uses; a mislabel is a ratio between two of them.
pub(super) const COMMON_RATES: [u32; 11] = [8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 88200, 96000];
const NOMINALS: [f64; 2] = [50.0, 60.0];
/// Fine search half-width (relative) around nominal·ratio: grid deviation plus device clock error.
/// Keeps every (nominal, ratio) window disjoint from the others.
pub(super) const FINE_REL: f64 = 0.004;
const STEP_HZ: f64 = 0.005;

pub(super) fn rate_ratios() -> Vec<f64> {
    let mut r: Vec<f64> = COMMON_RATES
        .iter()
        .flat_map(|&a| COMMON_RATES.iter().map(move |&b| a as f64 / b as f64))
        .filter(|r| (0.7..=1.45).contains(r))
        .collect();
    r.sort_by(f64::total_cmp);
    r.dedup_by(|a, b| (*a / *b - 1.0).abs() < 1e-9);
    r
}

#[derive(Clone, Debug)]
pub(super) struct Line {
    pub k: u32,
    pub f_hz: f64,
    pub snr_db: f64,
}

#[derive(Clone, Debug)]
pub(super) struct Hypothesis {
    pub nominal: f64,
    pub ratio: f64,
    pub f_hz: f64,
    pub lines: Vec<Line>,
    pub score: f64,
}

pub(super) struct Search {
    pub best: Option<Hypothesis>,
    /// A different (nominal, ratio) explains the lines nearly as well (e.g. only a shared harmonic present).
    pub ambiguous: bool,
}

pub(super) fn search(lt: &Spectrum, p: &HumParams, f_hi: f64) -> Search {
    let mut hyps: Vec<Hypothesis> = rate_ratios()
        .into_iter()
        .flat_map(|r| NOMINALS.iter().map(move |&n| (n, r)))
        .filter_map(|(n, r)| fit(lt, p, f_hi, n, r))
        .collect();
    hyps.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then((a.ratio - 1.0).abs().total_cmp(&(b.ratio - 1.0).abs()))
    });
    let mut it = hyps.into_iter();
    let best = it.next();
    let ambiguous = best.as_ref().is_some_and(|b| {
        it.any(|h| (h.f_hz - b.f_hz).abs() > 0.5 && h.score >= 0.8 * b.score)
    });
    Search { best, ambiguous }
}

pub(super) fn orders(f: f64, max_k: usize, f_hi: f64) -> impl Iterator<Item = u32> {
    (1..=max_k as u32).take_while(move |&k| k as f64 * f + FLOOR_HALF_HZ <= f_hi)
}

fn window(k: u32, df: f64) -> f64 {
    (k as f64 * 0.02).max(2.0 * df)
}

fn line_db(lt: &Spectrum, k: u32, f: f64, floor: f64) -> Option<(f64, f64, bool)> {
    let w = window(k, lt.df);
    let pk = lt.peak(k as f64 * f - w, k as f64 * f + w, floor)?;
    Some((pk.f_hz, 10.0 * pk.snr.log10(), pk.interior))
}

fn fit(lt: &Spectrum, p: &HumParams, f_hi: f64, nominal: f64, ratio: f64) -> Option<Hypothesis> {
    let centre = nominal * ratio;
    let ks: Vec<u32> = orders(centre * (1.0 + FINE_REL), p.max_harmonics, f_hi).collect();
    let floors: Vec<f64> = ks.iter().map(|&k| lt.floor(k as f64 * centre, FLOOR_HALF_HZ)).collect();
    let score_at = |f: f64| -> f64 {
        ks.iter()
            .zip(&floors)
            .filter_map(|(&k, &fl)| line_db(lt, k, f, fl))
            .map(|(_, db, _)| (db - p.min_line_snr_db).max(0.0))
            .sum()
    };
    let n = (centre * FINE_REL / STEP_HZ).ceil() as i64;
    let (score, f) = (-n..=n)
        .map(|i| centre + i as f64 * STEP_HZ)
        .map(|f| (score_at(f), f))
        .max_by(|a, b| a.0.total_cmp(&b.0).then((b.1 - centre).abs().total_cmp(&(a.1 - centre).abs())))?;
    if score <= 0.0 {
        return None;
    }
    let lines: Vec<Line> = ks
        .iter()
        .zip(&floors)
        .filter_map(|(&k, &fl)| {
            let (f_hz, snr_db, interior) = line_db(lt, k, f, fl)?;
            (interior && snr_db >= p.min_line_snr_db).then_some(Line { k, f_hz, snr_db })
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let hits: Vec<(u32, f64, f64)> = lines.iter().map(|l| (l.k, l.f_hz, 10f64.powf(l.snr_db / 10.0))).collect();
    Some(Hypothesis {
        nominal,
        ratio,
        f_hz: combine(&hits).0,
        lines,
        score,
    })
}
