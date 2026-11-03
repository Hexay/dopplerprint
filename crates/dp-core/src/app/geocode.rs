//! Place name → point via OSM Nominatim, cached in the user cache.
//! Usage policy (https://operations.osmfoundation.org/policies/nominatim/): an identifying User-Agent with no
//! personal data, at most one request per second, results cached so a name is only ever looked up once.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

use super::NeedsInput;
use super::http::{self, Fetched};

const URL: &str = "https://nominatim.openstreetmap.org/search";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub lat: f64,
    pub lon: f64,
    pub name: String,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Store {
    last_request_unix: f64,
    places: BTreeMap<String, Place>,
}

fn key(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn load(path: &Path) -> Store {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(path: &Path, s: &Store) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(s)?).with_context(|| format!("writing {}", path.display()))
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Cached lookup; `offline` turns a cache miss into a needs-input answer instead of a request.
pub fn geocode(query: &str, cache_file: &Path, offline: bool) -> Result<Place> {
    let mut store = load(cache_file);
    if let Some(p) = store.places.get(&key(query)) {
        return Ok(p.clone());
    }
    if offline {
        return Err(NeedsInput(format!(
            "\"{query}\" isn't in the place cache and --offline is set; give --near as lat,lon instead."
        ))
        .into());
    }
    let wait = store.last_request_unix + 1.0 - now();
    if wait > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(wait.min(1.0)));
    }
    let fetched = http::get_json(URL, &[("q", query), ("format", "jsonv2"), ("limit", "1")]);
    store.last_request_unix = now();
    save(cache_file, &store)?;
    let place = match fetched.context("looking up the place name on OpenStreetMap Nominatim")? {
        Fetched::Json(v) => first_place(&v),
        Fetched::NotFound => None,
    }
    .ok_or_else(|| {
        NeedsInput(format!("Couldn't find \"{query}\" on OpenStreetMap; try \"Town, ST\" or give --near as lat,lon."))
    })?;
    store.places.insert(key(query), place.clone());
    save(cache_file, &store)?;
    Ok(place)
}

fn first_place(v: &serde_json::Value) -> Option<Place> {
    let r = v.as_array()?.first()?;
    let num = |k: &str| r.get(k)?.as_str()?.parse::<f64>().ok();
    let name = r.get("display_name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
    Some(Place { lat: num("lat")?, lon: num("lon")?, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nominatim_and_serves_from_cache() {
        let v = serde_json::json!([{"lat": "37.6820583", "lon": "-121.7680088", "display_name": "Livermore, Alameda County"}]);
        let p = first_place(&v).unwrap();
        assert_eq!((p.lat, p.lon), (37.6820583, -121.7680088));
        assert!(first_place(&serde_json::json!([])).is_none());

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("geocode.json");
        let mut s = Store::default();
        s.places.insert(key("Livermore,  CA"), p.clone());
        save(&file, &s).unwrap();
        assert_eq!(geocode("livermore, ca", &file, true).unwrap(), p);
        let miss = geocode("Nowhere", &file, true).unwrap_err();
        assert!(miss.downcast_ref::<NeedsInput>().is_some_and(|n| n.0.contains("lat,lon")));
    }
}
