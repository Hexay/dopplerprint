//! Open-Meteo Historical Forecast API (archived hourly model runs, HRRR over the US via `best_match`):
//! one request per site and UTC day, cached verbatim. Terms, licence and limits: docs/USAGE.md#weather.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::Value;

use super::thermo::{specific_humidity, wind_en};
use super::{Level, Profile};
use crate::app::http::{self, Fetched};

pub const URL: &str = "https://historical-forecast-api.open-meteo.com/v1/forecast";
pub const MODEL: &str = "best_match";
/// Pressure levels requested; 300 hPa (~9 km) tops the 9 km ADS-B ceiling the search loads.
const LEVELS_HPA: [u32; 12] = [1000, 975, 950, 925, 900, 850, 800, 700, 600, 500, 400, 300];
/// Height-above-model-ground levels with wind and temperature.
const HEIGHTS_M: [u32; 3] = [80, 120, 180];
/// Days must be this old before a response is cached: the archive fills in over the first day or two.
const SETTLED_DAYS: f64 = 2.0;

/// Sent coordinates are rounded to this many decimals (~1 km): finer than the model grid, coarser than a
/// recording spot.
pub fn rounded(x: f64) -> String {
    format!("{x:.2}")
}

fn hourly_vars() -> String {
    let mut v: Vec<String> =
        ["temperature_2m", "relative_humidity_2m", "surface_pressure", "wind_speed_10m", "wind_direction_10m"].map(String::from).into();
    for h in HEIGHTS_M {
        v.extend([format!("temperature_{h}m"), format!("wind_speed_{h}m"), format!("wind_direction_{h}m")]);
    }
    for p in LEVELS_HPA {
        for k in ["temperature", "relative_humidity", "wind_speed", "wind_direction", "geopotential_height"] {
            v.push(format!("{k}_{p}hPa"));
        }
    }
    v.join(",")
}

pub fn cache_file(dir: &Path, lat: f64, lon: f64, date: &str) -> PathBuf {
    dir.join(format!("openmeteo-{MODEL}-{}_{}-{date}.json", rounded(lat), rounded(lon)))
}

/// The day's response: from the cache, else (unless `offline`) fetched. `None` = not available.
pub fn day(dir: &Path, lat: f64, lon: f64, date: &str, offline: bool, now_unix: f64) -> Result<Option<Value>> {
    let file = cache_file(dir, lat, lon, date);
    if let Ok(bytes) = std::fs::read(&file) {
        return Ok(Some(serde_json::from_slice(&bytes)?));
    }
    if offline {
        return Ok(None);
    }
    let (la, lo, vars) = (rounded(lat), rounded(lon), hourly_vars());
    let q = [
        ("latitude", la.as_str()),
        ("longitude", lo.as_str()),
        ("start_date", date),
        ("end_date", date),
        ("hourly", vars.as_str()),
        ("models", MODEL),
        ("wind_speed_unit", "ms"),
        ("timeformat", "unixtime"),
    ];
    let v = match http::get_json(URL, &q)? {
        Fetched::NotFound => return Ok(None),
        Fetched::Json(v) => v,
    };
    if v.get("error").and_then(Value::as_bool) == Some(true) {
        bail!("Open-Meteo: {}", v.get("reason").and_then(Value::as_str).unwrap_or("error"));
    }
    let day_end = crate::adsb::manifest::parse_utc(&format!("{date}T00:00:00Z")).map_or(f64::INFINITY, |t| t + 86_400.0);
    if now_unix - day_end > SETTLED_DAYS * 86_400.0 {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&file, serde_json::to_vec(&v)?)?;
    }
    Ok(Some(v))
}

/// Profile at `t` (UNIX s), linear in time between the bracketing hours. `None` if `t` is outside the
/// response or the needed hours are missing.
pub fn profile_at(v: &Value, t: f64) -> Option<Profile> {
    let h = v.get("hourly")?;
    let times: Vec<f64> = h.get("time")?.as_array()?.iter().filter_map(Value::as_f64).collect();
    let i = times.partition_point(|&x| x <= t).checked_sub(1)?;
    let ground = v.get("elevation")?.as_f64()?;
    let a = hour(h, i, ground)?;
    // The day's last hour (23:00) has no successor in a one-day response; hold it to midnight.
    let (b, w) = match times.get(i + 1) {
        Some(&t1) => (hour(h, i + 1, ground)?, (t - times[i]) / (t1 - times[i])),
        None if t - times[i] <= 3600.0 => (a.clone(), 0.0),
        None => return None,
    };
    let levels = a
        .iter()
        .filter_map(|(k, la)| b.iter().find(|(kb, _)| kb == k).map(|(_, lb)| lerp(la, lb, w)))
        .collect();
    Some(Profile { levels, ground_msl: ground, valid_unix: t, source: format!("weather data by Open-Meteo.com, {MODEL}") })
}

fn lerp(a: &Level, b: &Level, w: f64) -> Level {
    let l = |x: f64, y: f64| x + (y - x) * w;
    Level {
        z_msl: l(a.z_msl, b.z_msl),
        t_k: l(a.t_k, b.t_k),
        q: a.q.zip(b.q).map(|(x, y)| l(x, y)),
        wind_en: [l(a.wind_en[0], b.wind_en[0]), l(a.wind_en[1], b.wind_en[1])],
    }
}

/// Levels at hour index `i`, keyed by name; a level with any missing field is left out, and pressure
/// levels at or below the 10 m surface node (underground extrapolations) are dropped.
fn hour(h: &Value, i: usize, ground: f64) -> Option<Vec<(String, Level)>> {
    let get = |k: &str| h.get(k)?.as_array()?.get(i)?.as_f64();
    let wind = |s: &str, d: &str| Some(wind_en(get(s)?, get(d)?));
    let mut out = Vec::new();
    let sfc = (|| {
        let t_k = get("temperature_2m")? + 273.15;
        let q = specific_humidity(get("relative_humidity_2m")?, t_k, get("surface_pressure")?);
        Some(Level { z_msl: ground + 10.0, t_k, q: Some(q), wind_en: wind("wind_speed_10m", "wind_direction_10m")? })
    })();
    out.extend(sfc.map(|l| ("sfc".to_string(), l)));
    for hm in HEIGHTS_M {
        let lvl = (|| {
            let t_k = get(&format!("temperature_{hm}m"))? + 273.15;
            let w = wind(&format!("wind_speed_{hm}m"), &format!("wind_direction_{hm}m"))?;
            Some(Level { z_msl: ground + hm as f64, t_k, q: None, wind_en: w })
        })();
        out.extend(lvl.map(|l| (format!("{hm}m"), l)));
    }
    for p in LEVELS_HPA {
        let lvl = (|| {
            let z = get(&format!("geopotential_height_{p}hPa"))?;
            let t_k = get(&format!("temperature_{p}hPa"))? + 273.15;
            let q = specific_humidity(get(&format!("relative_humidity_{p}hPa"))?, t_k, p as f64);
            let w = wind(&format!("wind_speed_{p}hPa"), &format!("wind_direction_{p}hPa"))?;
            Some(Level { z_msl: z, t_k, q: Some(q), wind_en: w })
        })();
        out.extend(lvl.filter(|l| l.z_msl > ground + 10.0).map(|l| (format!("{p}hPa"), l)));
    }
    (!out.is_empty()).then_some(out)
}
