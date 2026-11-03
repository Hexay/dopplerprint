//! Scenario TOML schema. Units: metres, m/s, seconds; `*_s` times are audio-relative unless named `unix`.
//! Field semantics and an annotated example: see docs/PHYSICS.md §4 and experiments/sim/*.toml.

use serde::{Deserialize, Serialize};

use crate::track::EngineClass;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default)]
    pub seed: u64,
    pub sample_rate: u32,
    pub duration_s: f64,
    pub start_unix: f64,
    /// Sampling interval of the emitted (ADS-B-like) tracks.
    #[serde(default = "one")]
    pub adsb_dt_s: f64,
    pub observer: Observer,
    #[serde(default)]
    pub atmosphere: SimAtmosphere,
    #[serde(default)]
    pub ground: Option<Ground>,
    #[serde(default)]
    pub noise: Noise,
    #[serde(default)]
    pub device: Device,
    pub aircraft: Vec<Aircraft>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observer {
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// Ellipsoidal height of the ground under the microphone.
    pub h_ell_m: f64,
    #[serde(default)]
    pub mic_height_m: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimAtmosphere {
    pub c: f64,
    /// Uniform wind (east, north, up) in m/s.
    #[serde(default)]
    pub wind_enu: [f64; 3],
}

impl Default for SimAtmosphere {
    fn default() -> Self {
        Self { c: 343.0, wind_enu: [0.0; 3] }
    }
}

/// Flat ground at the observer with a real reflection coefficient (image source).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ground {
    pub reflection: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Noise {
    /// SNR of an amplitude-1 fundamental at 1 km against the noise power in a 1 Hz band at 100 Hz.
    pub snr_db: f64,
    /// Low-frequency (one-pole low-passed) noise level above the white floor at DC.
    pub red_db: f64,
    pub red_corner_hz: f64,
    pub hum_hz: Option<f64>,
    /// Hum amplitude relative to an amplitude-1 tone.
    pub hum_db: f64,
}

impl Default for Noise {
    fn default() -> Self {
        Self { snr_db: 40.0, red_db: 10.0, red_corner_hz: 50.0, hum_hz: None, hum_db: -20.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Device {
    /// Mix is normalised to this peak, then `gain_db` is applied and the result hard-clipped.
    pub peak_dbfs: f64,
    pub gain_db: f64,
    pub highpass_hz: Option<f64>,
}

impl Default for Device {
    fn default() -> Self {
        Self { peak_dbfs: -1.0, gain_db: 0.0, highpass_hz: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aircraft {
    pub id: String,
    #[serde(default)]
    pub icao24: u32,
    #[serde(default)]
    pub type_code: Option<String>,
    #[serde(default = "turboprop")]
    pub engine: EngineClass,
    /// Fundamental amplitude at 1 km slant range.
    #[serde(default = "one")]
    pub amplitude: f64,
    pub source: Source,
    #[serde(default)]
    pub straight: Option<Straight>,
    #[serde(default)]
    pub waypoints: Option<Waypoints>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub f0: f64,
    /// Relative amplitude of harmonic k = 1, 2, ...
    pub harmonics: Vec<f64>,
    /// Linear f0 drift (1/s), referenced to the recording start.
    #[serde(default)]
    pub drift_per_s: f64,
    /// RMS of slow random rpm wander as a fraction of f0.
    #[serde(default)]
    pub jitter_frac: f64,
}

/// Straight line in the observer's ECEF tangent plane (exactly straight in ECEF).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Straight {
    /// Degrees clockwise from north.
    pub heading_deg: f64,
    pub speed_mps: f64,
    /// Height above the observer's ground point, along its up vector.
    pub alt_agl_m: f64,
    /// Horizontal CPA offset; positive puts the observer to the right of the track.
    pub cpa_offset_m: f64,
    /// Audio time at which the aircraft is at CPA (emission, not arrival).
    pub cpa_time_s: f64,
    #[serde(default = "default_half_span")]
    pub half_span_s: f64,
}

/// Knots `[t_audio_s, east_m, north_m, up_m]` relative to the observer ground point; a C¹ spline
/// (weighted three-point tangents) through them gives turns and climbs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Waypoints {
    pub points: Vec<[f64; 4]>,
}

fn one() -> f64 {
    1.0
}
fn turboprop() -> EngineClass {
    EngineClass::Turboprop
}
fn default_half_span() -> f64 {
    300.0
}

impl Scenario {
    pub fn from_toml_str(s: &str) -> anyhow::Result<Self> {
        let sc: Scenario = toml::from_str(s)?;
        sc.validate()?;
        Ok(sc)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.sample_rate > 0 && self.duration_s > 0.0, "sample_rate and duration_s must be > 0");
        anyhow::ensure!(self.adsb_dt_s > 0.0, "adsb_dt_s must be > 0");
        anyhow::ensure!(!self.aircraft.is_empty(), "scenario needs at least one [[aircraft]]");
        for a in &self.aircraft {
            anyhow::ensure!(
                a.straight.is_some() != a.waypoints.is_some(),
                "aircraft {}: give exactly one of [aircraft.straight] or [aircraft.waypoints]",
                a.id
            );
            if let Some(w) = &a.waypoints {
                anyhow::ensure!(w.points.len() >= 2, "aircraft {}: need >= 2 waypoints", a.id);
                anyhow::ensure!(
                    w.points.windows(2).all(|p| p[1][0] > p[0][0]),
                    "aircraft {}: waypoint times must increase",
                    a.id
                );
            }
        }
        Ok(())
    }
}
