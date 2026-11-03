//! Locate's weather hook: archived winds aloft and temperature at the area centre and recording time
//! become the search's model atmosphere (default on; `--no-weather` = still air; `--offline` = cache only).

use super::LocateRequest;
use super::cache::Cache;
use super::place::Area;
use super::ui::Ui;
use super::when::fmt_utc;
use crate::dem::Dem;
use crate::geo::{Geodetic, ecef_to_geodetic, geodetic_to_ecef};
use crate::physics::{self, Atmosphere};
use crate::search::pipeline::Candidate;
use crate::track::Track;
use crate::weather::{self, Profile, Site};

/// The atmosphere locate searches with and what to say about it.
pub struct Air {
    pub atmosphere: Atmosphere,
    /// Source and valid time, or why still air was used.
    pub source: String,
    pub site: Option<Site>,
}

/// `base` (the config's atmosphere) is used unchanged when weather is off or unavailable.
pub fn model_atmosphere(req: &LocateRequest, cache: &Cache, a: &Area, dem: Option<&Dem>, t: f64, base: Atmosphere, ui: &dyn Ui) -> Air {
    let still = |why: &str| Air { atmosphere: base, source: format!("still air, c {:.1} m/s ({why})", base.c), site: None };
    if req.no_weather {
        return still("--no-weather");
    }
    if !req.offline {
        ui.step("weather", 0.45, "Fetching archived winds aloft...");
    }
    let profile: Profile = match weather::archive(&cache.weather_dir(), a.lat, a.lon, t, req.offline) {
        Ok(Some(p)) => p,
        Ok(None) if req.offline => return still("offline and no cached weather for this hour"),
        Ok(None) => return still("no archived weather for this time and place"),
        Err(e) => return still(&format!("weather download failed: {e:#}")),
    };
    let ground_ell = dem.and_then(|d| d.height_ellipsoid_latlon(a.lat, a.lon)).unwrap_or_else(|| profile.ground_ell(a.lat, a.lon));
    let site = Site { lat: a.lat, lon: a.lon, ground_ell };
    match profile.atmosphere(&site) {
        Some(atm) => Air { atmosphere: atm, source: format!("{}, {}", profile.source, fmt_utc(profile.valid_unix)), site: Some(site) },
        None => still("archived weather profile was unusable"),
    }
}

/// "wind 6 m/s from 250° averaged 0–900 m; c 341.2 m/s (Open-Meteo …)": the layer from the ground at the
/// best spot up to each matched aircraft's height at closest approach (one entry per distinct layer).
pub fn describe_used(air: &Air, best: &Candidate, tracks: &[Track], dem: Option<&Dem>) -> String {
    let Some(site0) = air.site else { return air.source.clone() };
    let ground = dem.and_then(|d| d.height_ellipsoid_latlon(best.lat_deg, best.lon_deg)).unwrap_or(site0.ground_ell);
    let site = Site { lat: best.lat_deg, lon: best.lon_deg, ground_ell: ground };
    let obs = geodetic_to_ecef(Geodetic { lat_deg: site.lat, lon_deg: site.lon, h_ell_m: ground });
    let mut tops: Vec<f64> = best
        .track_ids
        .iter()
        .filter_map(|id| tracks.iter().find(|t| &t.id == id))
        .filter_map(|t| t.state_at(physics::cpa(t, obs, &air.atmosphere)?.t_emit))
        .map(|s| ecef_to_geodetic(s.pos).h_ell_m)
        .collect();
    tops.sort_by(f64::total_cmp);
    tops.dedup_by(|a, b| (*a - *b).abs() < 100.0);
    if tops.is_empty() {
        tops.push(ground + weather::REF_DEPTH_M);
    }
    let layers: Vec<String> = tops.iter().map(|&top| weather::describe(&air.atmosphere, &site, top)).collect();
    format!("{} ({})", layers.join("; "), air.source)
}
