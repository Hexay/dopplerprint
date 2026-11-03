//! Turn a corpus case (data/cases/*/truth.json) or a sim scenario into a posed search problem.
//! Adapters: `real` cases with oracle event windows (Oyster Bay style) and rendered sim scenarios.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::split;
use crate::adsb::{self, TrackQuery};
use crate::audio::{self, Audio};
use crate::config::EvalParams;
use crate::dem::Dem;
use crate::physics::Atmosphere;
use crate::search::EventSpec;
use crate::sim;
use crate::track::Track;

pub struct Posed {
    pub id: String,
    pub audio: Audio,
    pub truth_lat: f64,
    pub truth_lon: f64,
    pub truth_h_ell: Option<f64>,
    pub t_start_true: f64,
    pub events: Vec<EventSpec>,
    /// Lowercase icao hex (real) or track id (sim) per event, when known.
    pub truth_flights: Vec<Option<String>>,
    pub tracks: Vec<Track>,
    pub dem: Option<Dem>,
    /// The medium a sim case was rendered in.
    pub sim_atmosphere: Option<Atmosphere>,
}

pub enum CaseSource {
    Corpus(PathBuf),
    Sim(PathBuf),
}

impl CaseSource {
    pub fn id(&self) -> String {
        let p = match self {
            CaseSource::Corpus(p) | CaseSource::Sim(p) => p,
        };
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match self {
            CaseSource::Corpus(_) => stem,
            CaseSource::Sim(_) => format!("sim_{stem}"),
        }
    }
}

/// Cases selected by `p.cases` and `p.split`; frozen test cases are fingerprint-checked.
pub fn list(p: &EvalParams, sim_dir: &Path) -> Result<Vec<CaseSource>> {
    let test = split::load(&p.split_file)?;
    let mut out = list_all(p, sim_dir);
    out.retain(|c| split::wanted(p.split, &test, &c.id(), matches!(c, CaseSource::Sim(_))));
    for c in &out {
        if let (CaseSource::Corpus(dir), Some(fp)) = (c, test.get(&c.id())) {
            split::verify(dir, fp)?;
        }
    }
    Ok(out)
}

fn list_all(p: &EvalParams, sim_dir: &Path) -> Vec<CaseSource> {
    let mut out: Vec<CaseSource> = Vec::new();
    for (dir, sim) in [(p.data_dir.join("cases"), false), (sim_dir.to_path_buf(), true)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for path in paths {
            if sim && path.extension().is_some_and(|e| e == "toml") {
                out.push(CaseSource::Sim(path));
            } else if !sim && path.join("truth.json").exists() {
                out.push(CaseSource::Corpus(path));
            }
        }
    }
    out.retain(|c| p.cases.is_empty() || p.cases.iter().any(|f| c.id().contains(f.as_str())));
    out
}

pub fn pose(src: &CaseSource, p: &EvalParams) -> Result<Posed> {
    match src {
        CaseSource::Corpus(dir) => pose_real(dir, p),
        CaseSource::Sim(path) => pose_sim(path, src.id(), p),
    }
}

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

pub(super) fn oracle(window: (f64, f64), t_cpa: f64, f0: f64, p: &EvalParams) -> EventSpec {
    EventSpec {
        window,
        t_cpa_audio: t_cpa,
        d_range: p.oracle_d_range,
        v_range: p.oracle_v_range,
        f0_range: (f0 * (1.0 - p.oracle_f0_frac), f0 * (1.0 + p.oracle_f0_frac)),
    }
}

fn pose_real(dir: &Path, p: &EvalParams) -> Result<Posed> {
    let id = dir.file_name().unwrap().to_string_lossy().into_owned();
    let truth: Value = serde_json::from_slice(&std::fs::read(dir.join("truth.json"))?)?;
    if truth.get("kind").and_then(Value::as_str) != Some("real") {
        bail!("adapter: only real corpus cases are posed (kind != real)");
    }
    if super::livermore::is_livermore(&truth) {
        return super::livermore::pose(dir, &truth, p);
    }
    let t0 = f(&truth, "recording_start_unix").context("needs recording_start_unix")?;
    let date = truth.get("date_utc").and_then(Value::as_str).context("needs date_utc")?.to_string();
    let mut events = Vec::new();
    let mut flights = Vec::new();
    for e in truth.get("events").and_then(Value::as_array).context("needs events")? {
        let (Some(w), Some(t), Some(f0)) = (e.get("window_s"), f(e, "cpa_receive_s"), f(e, "f0_hz")) else { continue };
        let w = (w[0].as_f64().unwrap_or(t - 60.0), w[1].as_f64().unwrap_or(t + 60.0));
        events.push(oracle(w, t, f0, p));
        flights.push(e.get("flight_icao").and_then(Value::as_str).map(str::to_lowercase));
    }
    if events.is_empty() {
        bail!("adapter: no events with window_s + cpa_receive_s + f0_hz (needs automatic discovery)");
    }
    let wav = first_file(&dir.join("audio"))?;
    let (lat, lon) = (f(&truth, "lat").context("lat")?, f(&truth, "lon").context("lon")?);
    let (audio, tracks) = decode_with_tracks(&wav, |dur| load_tracks(p, &date, lat, lon, t0, dur))?;
    let prefix = id.split('_').next().unwrap_or_default();
    let dem_path = p.data_dir.join("dem/5070").join(format!("{prefix}_30m.u16"));
    let dem = dem_path.exists().then(|| Dem::open(&dem_path)).transpose()?;
    Ok(Posed {
        id,
        audio,
        truth_lat: lat,
        truth_lon: lon,
        truth_h_ell: None,
        t_start_true: t0,
        events,
        truth_flights: flights,
        tracks,
        dem,
        sim_atmosphere: None,
    })
}

/// Decode `wav` while `tracks(duration_s)` runs on the header's duration; reloads if the decoded length differs.
fn decode_with_tracks(wav: &Path, tracks: impl Fn(f64) -> Result<Vec<Track>> + Sync) -> Result<(Audio, Vec<Track>)> {
    let info = audio::probe(wav)?;
    let declared = info.n_frames.map(|n| n as f64 / info.sample_rate as f64);
    let (audio, early) = rayon::join(|| audio::decode(wav), || declared.map(&tracks));
    let audio = audio?;
    let tracks = match early {
        Some(Ok(t)) if declared == Some(audio.duration_s()) => t,
        _ => tracks(audio.duration_s())?,
    };
    Ok((audio, tracks))
}

pub(super) fn first_file(dir: &Path) -> Result<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    v.sort();
    v.into_iter().find(|p| p.is_file()).with_context(|| format!("no audio in {}", dir.display()))
}

pub(super) fn load_tracks(p: &EvalParams, date: &str, lat: f64, lon: f64, t0: f64, dur: f64) -> Result<Vec<Track>> {
    let store = p.store_dir.clone();
    if !store.join(format!("date={date}")).exists() {
        bail!("no ADS-B store for {date} under {} (run dopplerprint ingest)", store.display());
    }
    let reach = p.region_radius_m + p.region_offset_m + 15_000.0;
    let (dlat, dlon) = (reach / 111_000.0, reach / (111_000.0 * lat.to_radians().cos()));
    let q = TrackQuery {
        t_start: t0 - p.time_prior_s - 900.0,
        t_end: t0 + dur + p.time_prior_s + 900.0,
        bbox: Some((lat - dlat, lon - dlon, lat + dlat, lon + dlon)),
        max_alt_m: Some(9_000.0),
        exclude_engines: Vec::new(),
    };
    adsb::load_tracks(&store, &q)
}

fn pose_sim(path: &Path, id: String, _p: &EvalParams) -> Result<Posed> {
    let r = sim::render_toml(&std::fs::read_to_string(path)?)?;
    let t = r.truth;
    // All-zero harmonics = a silent ADS-B-only distractor: a track, never an event.
    let audible = |e: &&sim::truth::TruthEvent| e.in_recording && e.harmonics.iter().any(|&h| h > 0.0);
    let events: Vec<EventSpec> = t
        .events
        .iter()
        .filter(audible)
        .map(|e| EventSpec {
            window: (e.cpa_arrival_audio_s - 60.0, e.cpa_arrival_audio_s + 60.0),
            t_cpa_audio: e.cpa_arrival_audio_s,
            d_range: (e.slant_m * 0.6, e.slant_m * 1.6),
            v_range: (e.speed_mps * 0.75, e.speed_mps * 1.3),
            f0_range: (e.f0 * 0.94, e.f0 * 1.06),
        })
        .collect();
    let flights = t.events.iter().filter(audible).map(|e| Some(e.track_id.clone())).collect();
    Ok(Posed {
        id,
        audio: Audio { sample_rate: r.sample_rate, samples: r.samples },
        truth_lat: t.observer.lat_deg,
        truth_lon: t.observer.lon_deg,
        truth_h_ell: Some(t.observer.h_ell_m),
        t_start_true: t.start_unix,
        events,
        truth_flights: flights,
        tracks: t.tracks,
        dem: None,
        sim_atmosphere: Some(Atmosphere::uniform(t.c, t.wind_ecef)),
    })
}
