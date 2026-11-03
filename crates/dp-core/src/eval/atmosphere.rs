//! Per-case model atmosphere for eval experiments: `search.atmosphere` plus wind overrides given in the
//! case's local frame (ENU, or along/cross the truth flight's ground track), converted to ECEF.
//! Track-relative wind uses truth geometry, so it is a sensitivity probe, never a deployable setting.

use super::case::Posed;
use super::metrics::same_flight;
use crate::config::EvalParams;
use crate::geo::{Vec3, east_north, up_vector};
use crate::physics::Atmosphere;
use crate::search::Region;
use crate::weather::{self, Site};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherSource {
    /// `search.atmosphere` as configured.
    #[default]
    None,
    /// Open-Meteo archive at the search-region centre and true start (corpus cases; cache
    /// `<data_dir>/weather`). Sim cases keep `search.atmosphere`.
    Archive,
    /// The medium a sim case was rendered in (sim cases only).
    Truth,
}

/// The case's starting atmosphere and a note on where it came from (`None` = `search.atmosphere`).
pub fn sourced(base: Atmosphere, c: &Posed, e: &EvalParams, region: &Region) -> (Atmosphere, Option<String>) {
    match e.weather {
        WeatherSource::None => (base, None),
        WeatherSource::Truth => match c.sim_atmosphere {
            Some(a) => (a, Some("sim truth".into())),
            None => (base, None),
        },
        WeatherSource::Archive if c.sim_atmosphere.is_some() => (base, None),
        WeatherSource::Archive => archived(base, c, e, region),
    }
}

fn archived(base: Atmosphere, c: &Posed, e: &EvalParams, r: &Region) -> (Atmosphere, Option<String>) {
    let got = weather::archive(&e.data_dir.join("weather"), r.lat_deg, r.lon_deg, c.t_start_true, false);
    let profile = match got {
        Ok(Some(p)) => p,
        Ok(None) => return (base, Some("archive: no data, still air".into())),
        Err(err) => return (base, Some(format!("archive failed ({err:#}), still air"))),
    };
    let dem_ground = c.dem.as_ref().and_then(|d| d.height_ellipsoid_latlon(r.lat_deg, r.lon_deg));
    let ground_ell = dem_ground.or(c.truth_h_ell).unwrap_or_else(|| profile.ground_ell(r.lat_deg, r.lon_deg));
    let site = Site { lat: r.lat_deg, lon: r.lon_deg, ground_ell };
    match profile.atmosphere(&site) {
        Some(a) => (a, Some(format!("{}: {}", profile.source, weather::describe(&a, &site, ground_ell + weather::REF_DEPTH_M)))),
        None => (base, Some("archive: unusable profile, still air".into())),
    }
}

pub fn case_atmosphere(base: Atmosphere, c: &Posed, e: &EvalParams) -> Atmosphere {
    let (east, north) = east_north(c.truth_lat, c.truth_lon);
    let up = up_vector(c.truth_lat, c.truth_lon);
    let [we, wn, wu] = e.wind_enu_mps;
    let mut wind = base.wind + east * we + north * wn + up * wu;
    let (a, x) = (e.wind_along_track_mps, e.wind_cross_track_mps);
    if (a != 0.0 || x != 0.0)
        && let Some((de, dn)) = truth_heading_en(c, east, north)
    {
        // Cross-track is positive to the right of the direction of flight.
        wind += east * (a * de + x * dn) + north * (a * dn - x * de);
    }
    Atmosphere { wind, ..base }
}

/// Unit ground-track direction (east, north) of the first event's truth flight at its CPA.
fn truth_heading_en(c: &Posed, east: Vec3, north: Vec3) -> Option<(f64, f64)> {
    let (ev, flight) = c.events.iter().zip(&c.truth_flights).find_map(|(e, f)| Some((e, f.as_ref()?)))?;
    let t = c.t_start_true + ev.t_cpa_audio;
    let v = c.tracks.iter().filter(|tr| same_flight(&tr.id, flight)).find_map(|tr| tr.state_at(t))?.vel;
    let (de, dn) = (v.dot(east), v.dot(north));
    let h = de.hypot(dn);
    (h > 1.0).then(|| (de / h, dn / h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Audio;
    use crate::geo::{Geodetic, geodetic_to_ecef};
    use crate::search::EventSpec;
    use crate::track::{EngineClass, Track, TrackPoint};

    #[test]
    fn along_track_is_a_tailwind_and_cross_track_points_right() {
        let (lat, lon) = (40.0, -100.0);
        let (east, north) = east_north(lat, lon);
        let p0 = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: 500.0 });
        let pt = |t: f64| TrackPoint { t, pos: p0 + north * (60.0 * t), vel: north * 60.0 };
        let track = Track { id: "x".into(), icao24: 1, type_code: None, engine: EngineClass::Piston, points: vec![pt(-100.0), pt(100.0)] };
        let ev = EventSpec { window: (0.0, 1.0), t_cpa_audio: 0.0, d_range: (0.0, 1.0), v_range: (0.0, 1.0), f0_range: (1.0, 2.0) };
        let c = Posed {
            id: "t".into(),
            audio: Audio { sample_rate: 8000, samples: Vec::new() },
            truth_lat: lat,
            truth_lon: lon,
            truth_h_ell: None,
            t_start_true: 0.0,
            events: vec![ev],
            truth_flights: vec![Some("x".into())],
            tracks: vec![track],
            dem: None,
            sim_atmosphere: None,
        };
        let e = EvalParams { wind_along_track_mps: 5.0, wind_cross_track_mps: 2.0, ..Default::default() };
        let w = case_atmosphere(Atmosphere::default(), &c, &e).wind;
        assert!((w.dot(north) - 5.0).abs() < 1e-9 && (w.dot(east) - 2.0).abs() < 1e-9);
    }
}
