//! truth.json: observer, per-event CPA (found by direct scan, not physics::cpa) and the tracks.

use serde::{Deserialize, Serialize};

use super::scenario::{Aircraft, Scenario};
use super::trajectory::Frame;
use crate::geo::Vec3;
use crate::track::{EngineClass, Track};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Truth {
    pub observer: TruthObserver,
    pub sample_rate: u32,
    pub duration_s: f64,
    pub start_unix: f64,
    pub seed: u64,
    pub c: f64,
    pub wind_ecef: Vec3,
    /// Linear factor from the physical mix (amplitude-1 tone at 1 km = 1.0) to output samples.
    pub output_scale: f64,
    pub events: Vec<TruthEvent>,
    /// Tracks in UNIX seconds, same schema as `adsb::load_tracks` output.
    pub tracks: Vec<Track>,
}

impl Truth {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TruthObserver {
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// Ground ellipsoidal height; the microphone is `mic_height_m` above it.
    pub h_ell_m: f64,
    pub mic_height_m: f64,
    pub mic_ecef: Vec3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TruthEvent {
    pub track_id: String,
    pub icao24: u32,
    pub engine: EngineClass,
    pub f0: f64,
    pub harmonics: Vec<f64>,
    pub cpa_emit_audio_s: f64,
    pub cpa_arrival_audio_s: f64,
    pub cpa_arrival_unix: f64,
    pub slant_m: f64,
    pub speed_mps: f64,
    /// Whether the CPA arrival lies inside the recording.
    pub in_recording: bool,
}

/// Minimum of |p(τ) − mic| by a coarse scan and four ×10 zoom passes (local time, 1e-5 s).
fn scan_cpa(track: &Track, mic: Vec3) -> (f64, f64) {
    let range = |t: f64| track.state_at(t).map_or(f64::INFINITY, |s| (s.pos - mic).norm());
    let (ts, te) = (track.t_start(), track.t_end());
    let mut step = 0.1;
    let n = ((te - ts) / step).ceil() as usize;
    let mut best = (0..=n).map(|i| (ts + i as f64 * step).min(te)).fold((ts, f64::INFINITY), |b, t| {
        let r = range(t);
        if r < b.1 { (t, r) } else { b }
    });
    for _ in 0..4 {
        let centre = best.0;
        step /= 10.0;
        for i in -10..=10 {
            let t = centre + i as f64 * step;
            let r = range(t);
            if r < best.1 {
                best = (t, r);
            }
        }
    }
    best
}

/// Acoustic delay s with |q − w·s| = c·s, by fixed point.
fn delay(q: Vec3, c: f64, wind: Vec3) -> f64 {
    let mut s = q.norm() / c;
    for _ in 0..100 {
        s = (q - wind * s).norm() / c;
    }
    s
}

pub fn event(sc: &Scenario, frame: &Frame, wind: Vec3, a: &Aircraft, local: &Track) -> TruthEvent {
    let (t_emit, slant) = scan_cpa(local, frame.mic);
    let st = local.state_at(t_emit).expect("scan stays inside the track");
    let arrival = t_emit + delay(frame.mic - st.pos, sc.atmosphere.c, wind);
    TruthEvent {
        track_id: a.id.clone(),
        icao24: a.icao24,
        engine: a.engine,
        f0: a.source.f0,
        harmonics: a.source.harmonics.clone(),
        cpa_emit_audio_s: t_emit,
        cpa_arrival_audio_s: arrival,
        cpa_arrival_unix: sc.start_unix + arrival,
        slant_m: slant,
        speed_mps: st.vel.norm(),
        in_recording: (0.0..=sc.duration_s).contains(&arrival),
    }
}

pub fn to_unix(local: &Track, start_unix: f64) -> Track {
    let mut t = local.clone();
    for p in &mut t.points {
        p.t += start_unix;
    }
    t
}

pub fn observer(sc: &Scenario, frame: &Frame) -> TruthObserver {
    let o = &sc.observer;
    TruthObserver {
        lat_deg: o.lat_deg,
        lon_deg: o.lon_deg,
        h_ell_m: o.h_ell_m,
        mic_height_m: o.mic_height_m,
        mic_ecef: frame.mic,
    }
}
