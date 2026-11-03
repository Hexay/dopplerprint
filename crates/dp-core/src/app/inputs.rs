//! Resolve the request's loose inputs (start time, place, terrain) into what the search needs.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use anyhow::Result;

use super::cache::Cache;
use super::filetime::{FileTime, recording_time};
use super::geocode::geocode;
use super::place::{Area, Near, area, missing_near, parse_near};
use super::report::{Report, Status};
use super::ui::Ui;
use super::when::{DEFAULT_TOLERANCE_S, ParsedStart, StartPrior, missing_start, parse_start, parse_tolerance};
use super::{LocateRequest, NeedsInput};
use crate::dem::Dem;

pub enum Given {
    User(ParsedStart),
    File(f64, String),
}

/// `--start`, else the file's own timestamp, else ask.
pub fn given_start(req: &LocateRequest) -> Result<Given, NeedsInput> {
    if let Some(s) = &req.start {
        return parse_start(s).map(Given::User);
    }
    match recording_time(&req.audio) {
        FileTime::Found(t, what) => Ok(Given::File(t, what)),
        FileTime::NoZone(raw) => Err(NeedsInput(format!(
            "The file says it was recorded at \"{raw}\" but not in which time zone; add --start \"{raw} -07:00\" with your UTC offset."
        ))),
        FileTime::Unknown => Err(missing_start()),
    }
}

/// Which of "start" and "near" a `NeedsInput` report is waiting on (both when both are missing); empty when
/// it is waiting on something else, such as consent to download.
pub fn missing_fields(req: &LocateRequest, rep: &Report) -> Vec<&'static str> {
    if rep.status != Status::NeedsInput {
        return Vec::new();
    }
    let start_bad = given_start(req).is_err();
    // Steps run start → area → tolerance, so a failure after a good start and before the area is the place.
    let near_bad = req.near.as_deref().is_none_or(|s| parse_near(s).is_err()) || (!start_bad && rep.area.is_none());
    let tolerance_bad = rep.area.is_some() && rep.start.is_none();
    [("start", start_bad || tolerance_bad), ("near", near_bad)].into_iter().filter(|f| f.1).map(|f| f.0).collect()
}

pub fn start_prior(given: &Given, tolerance: Option<&str>, duration_s: f64) -> Result<StartPrior, NeedsInput> {
    let tol = |own: Option<f64>| -> Result<f64, NeedsInput> {
        match tolerance {
            Some(s) => parse_tolerance(s),
            None => Ok(own.unwrap_or(DEFAULT_TOLERANCE_S)),
        }
    };
    Ok(match given {
        Given::User(p) => {
            let t = tol(p.tolerance_s)?;
            StartPrior { lo: p.t - t, hi: p.t + t, nominal: p.t, source: "--start".into() }
        }
        // Devices stamp either the start or the end of a recording, so allow both.
        Given::File(ts, what) => {
            let t = tol(None)?;
            StartPrior { lo: ts - duration_s - t, hi: ts + t, nominal: *ts, source: format!("{what} in the file") }
        }
    })
}

pub fn resolve_area(req: &LocateRequest, cache: &Cache, ui: &dyn Ui) -> Result<Area> {
    let s = req.near.as_deref().ok_or_else(missing_near)?;
    let (near, label) = match parse_near(s)? {
        Near::Place(q) => {
            let p = geocode(&q, &cache.geocode_file(), req.offline)?;
            ui.info(&format!("{q}: {} ({:.4}, {:.4})", p.name, p.lat, p.lon));
            (Near::Point { lat: p.lat, lon: p.lon, radius_km: None }, format!("{q} ({:.4}, {:.4})", p.lat, p.lon))
        }
        Near::BBox(b) => (Near::BBox(b), format!("the box {}", s.trim())),
        p => (p, String::new()),
    };
    let label = if label.is_empty() { None } else { Some(label) };
    Ok(area(&near, label.as_deref(), req.radius_km)?)
}

/// `--dem`, else the first DEM in `<cache>/dem/` covering the area centre, else flat ground at the geoid.
pub fn terrain(req: &LocateRequest, cache: &Cache, a: &Area) -> Result<(Option<Dem>, String)> {
    if let Some(p) = &req.dem {
        return Ok((Some(Dem::open(p)?), format!("DEM {}", p.display())));
    }
    let mut found: Vec<_> = std::fs::read_dir(cache.dem_dir())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "u16"))
        .collect();
    found.sort();
    for p in found {
        if let Ok(d) = Dem::open(&p)
            && d.height_ellipsoid_latlon(a.lat, a.lon).is_some()
        {
            return Ok((Some(d), format!("DEM {}", p.display())));
        }
    }
    Ok((None, "flat ground at the local geoid height (no DEM in the cache)".into()))
}

/// Registrations for `icaos` from the cached tar1090-db registry (`ICAO;reg;type;...`), if present.
pub fn registrations(typedb: &Path, icaos: &[u32]) -> HashMap<u32, String> {
    let Some(raw) = ["aircraft.csv.gz", "aircraft.csv"].iter().find_map(|f| std::fs::read(typedb.join(f)).ok()) else {
        return HashMap::new();
    };
    let reader: Box<dyn Read> =
        if raw.starts_with(&[0x1f, 0x8b]) { Box::new(flate2::read::MultiGzDecoder::new(&raw[..])) } else { Box::new(&raw[..]) };
    let mut out = HashMap::new();
    for line in BufReader::new(reader).lines().map_while(std::io::Result::ok) {
        let mut f = line.split(';');
        let (Some(hex), Some(reg)) = (f.next(), f.next()) else { continue };
        if let Ok(icao) = u32::from_str_radix(hex, 16)
            && icaos.contains(&icao)
            && !reg.is_empty()
        {
            out.insert(icao, reg.to_string());
        }
    }
    out
}
