//! Ground truth for discovered events. Each event is matched to a labelled truth pass (CPA time and a
//! harmonic within tolerance, the `corpus_discovery` rule), else attributed to an ADS-B track whose CPA
//! arrival at the truth observer coincides and whose slant range the fit admits; the rest are spurious.

use super::DiscoveryParams;
use crate::eval::metrics::same_flight;
use crate::events::Event;
use crate::geo::Vec3;
use crate::physics::{self, Atmosphere};
use crate::track::Track;

/// What the search sees of a discovered event, plus what truth matching needs.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Found {
    pub t_cpa: f64,
    pub fund_hz: f64,
    /// Harmonic numbers relative to `fund_hz`.
    pub harmonics: Vec<u32>,
    pub d_m: f64,
    pub v_mps: f64,
    pub d_range: (f64, f64),
    pub v_range: (f64, f64),
    pub snr: f64,
}

impl Found {
    pub fn from_event(e: &Event) -> Self {
        let h = e.ridge_harmonic.max(1);
        let mut harmonics = e.harmonics.clone();
        if !harmonics.contains(&h) {
            harmonics.push(h);
        }
        Found {
            t_cpa: e.fit.best.t_cpa_arrive,
            fund_hz: e.fit.best.f0 / h as f64,
            harmonics,
            d_m: e.fit.best.d_cpa,
            v_mps: e.fit.best.v,
            d_range: e.fit.d_range,
            v_range: e.fit.v_range,
            snr: e.snr,
        }
    }

    /// Signed relative error of the detected harmonic closest to `f0`.
    fn harmonic_err(&self, f0: f64) -> Option<f64> {
        self.harmonics.iter().map(|&k| k.max(1) as f64 * self.fund_hz / f0 - 1.0).min_by(|a, b| a.abs().total_cmp(&b.abs()))
    }
}

/// A truth pass from the case labels (oracle events): audio-relative CPA arrival and the labelled tone.
#[derive(Clone, Debug)]
pub struct Labelled {
    pub t_cpa: f64,
    pub f0: f64,
    pub flight: Option<String>,
}

/// A track's closest approach to the truth observer, audio-relative.
#[derive(Clone, Debug)]
pub struct Pass {
    pub flight: String,
    pub t_cpa: f64,
    pub slant_m: f64,
    pub speed_mps: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Source {
    Labelled,
    Adsb,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Truthed {
    pub flight: Option<String>,
    pub source: Option<Source>,
    pub truth_index: Option<usize>,
    /// Fitted minus true CPA arrival (s).
    pub dt_cpa_s: Option<f64>,
    /// Relative errors; d and v against the ADS-B closest approach (v is ground speed).
    pub f0_err: Option<f64>,
    pub d_err: Option<f64>,
    pub v_err: Option<f64>,
    /// Would the true flight pass `search::hypothesis::associate`'s gates at the truth position?
    pub d_gate: Option<bool>,
    pub v_gate: Option<bool>,
    pub clock_gate: Option<bool>,
}

impl Truthed {
    pub fn is_real(&self) -> bool {
        self.source.is_some()
    }
}

/// At the truth position the implied clock error is −dt, so the true flight passes iff |dt| ≤ the prior.
pub fn gate_clock(labels: &mut [Truthed], time_prior_s: f64) {
    for l in labels {
        l.clock_gate = l.dt_cpa_s.map(|dt| dt.abs() <= time_prior_s);
    }
}

pub fn passes(tracks: &[Track], obs: Vec3, atm: &Atmosphere, t_start: f64, duration_s: f64, p: &DiscoveryParams) -> Vec<Pass> {
    tracks
        .iter()
        .filter_map(|t| {
            let c = physics::cpa(t, obs, atm)?;
            let t_cpa = c.t_arrive - t_start;
            let inside = t_cpa > -p.match_dt_s && t_cpa < duration_s + p.match_dt_s && c.slant_m <= p.max_slant_m;
            inside.then(|| Pass { flight: t.id.clone(), t_cpa, slant_m: c.slant_m, speed_mps: c.ground_speed })
        })
        .collect()
}

/// One-to-one, closest CPA time first.
pub fn match_labelled(found: &[Found], truth: &[Labelled], p: &DiscoveryParams) -> Vec<Option<(usize, f64, f64)>> {
    let mut pairs = Vec::new();
    for (i, e) in found.iter().enumerate() {
        for (j, t) in truth.iter().enumerate() {
            let dt = e.t_cpa - t.t_cpa;
            let df = e.harmonic_err(t.f0);
            if let Some(df) = df.filter(|df| dt.abs() <= p.match_dt_s && df.abs() <= p.match_f0_frac) {
                pairs.push((i, j, dt, df));
            }
        }
    }
    pairs.sort_by(|a, b| a.2.abs().total_cmp(&b.2.abs()));
    let mut out = vec![None; found.len()];
    let mut taken = vec![false; truth.len()];
    for (i, j, dt, df) in pairs {
        if out[i].is_none() && !taken[j] {
            out[i] = Some((j, dt, df));
            taken[j] = true;
        }
    }
    out
}

pub fn label(found: &[Found], truth: &[Labelled], passes: &[Pass], p: &DiscoveryParams) -> Vec<Truthed> {
    let matched = match_labelled(found, truth, p);
    found
        .iter()
        .zip(matched)
        .map(|(e, m)| match m {
            Some((j, dt, df)) => {
                let flight = truth[j].flight.clone();
                let pass = flight.as_deref().and_then(|f| nearest(passes.iter().filter(|q| same_flight(&q.flight, f)), truth[j].t_cpa));
                Truthed {
                    source: Some(Source::Labelled),
                    truth_index: Some(j),
                    dt_cpa_s: Some(dt),
                    f0_err: Some(df),
                    ..kinematics(e, pass, flight)
                }
            }
            None => {
                let admits = |q: &&Pass| (q.t_cpa - e.t_cpa).abs() <= p.match_dt_s && q.slant_m >= e.d_range.0 && q.slant_m <= e.d_range.1;
                match nearest(passes.iter().filter(admits), e.t_cpa) {
                    Some(q) => Truthed {
                        source: Some(Source::Adsb),
                        dt_cpa_s: Some(e.t_cpa - q.t_cpa),
                        ..kinematics(e, Some(q), Some(q.flight.clone()))
                    },
                    None => Truthed::default(),
                }
            }
        })
        .collect()
}

fn nearest<'a>(it: impl Iterator<Item = &'a Pass>, t: f64) -> Option<&'a Pass> {
    it.min_by(|a, b| (a.t_cpa - t).abs().total_cmp(&(b.t_cpa - t).abs()))
}

fn kinematics(e: &Found, pass: Option<&Pass>, flight: Option<String>) -> Truthed {
    Truthed {
        flight,
        d_err: pass.map(|q| e.d_m / q.slant_m - 1.0),
        v_err: pass.filter(|q| q.speed_mps > 1.0).map(|q| e.v_mps / q.speed_mps - 1.0),
        d_gate: pass.map(|q| (e.d_range.0..=e.d_range.1).contains(&q.slant_m)),
        v_gate: pass.map(|q| (e.v_range.0..=e.v_range.1).contains(&q.speed_mps)),
        ..Default::default()
    }
}
