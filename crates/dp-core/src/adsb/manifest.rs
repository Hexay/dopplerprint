//! `manifest.json` per day directory, and store-layout helpers shared by ingest and query.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::clean::{CleanParams, CleanStats};
use super::source::SourceStats;

pub const LICENCE: &str = "Derived from adsb.lol globe_history (data © adsb.lol contributors), ODbL 1.0";

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub schema: String,
    pub tool_version: String,
    pub licence: String,
    pub archive: Vec<String>,
    pub date: String,
    /// UNIX seconds of 00:00 UTC on `date`; hour partitions are relative to it.
    pub day0: f64,
    pub elapsed_s: f64,
    /// Wall seconds per ingest phase (stream, coverage, points, segments).
    pub phase_s: BTreeMap<String, f64>,
    pub trace_files: u64,
    pub truncated_archive: bool,
    pub parse_errors: u64,
    pub other_day_traces: u64,
    /// Towers, ground vehicles and ships, dropped whole (see `typedb`).
    pub non_aircraft_traces: u64,
    /// (hour, 1° cell, 2000 ft band) bins with a neighbour geom−baro offset (see `neighbour`).
    pub neighbour_offset_bins: u64,
    pub aircraft: u64,
    pub tracks: u64,
    pub points_in: u64,
    pub points_rows: u64,
    pub segment_rows: u64,
    pub coverage_rows: u64,
    pub dropped: BTreeMap<String, u64>,
    pub aircraft_by_engine: BTreeMap<String, u64>,
    pub alt_source_points: BTreeMap<String, u64>,
    pub params: serde_json::Value,
    pub typedb_files: Vec<String>,
    pub typedb_types: usize,
    pub typedb_registry: usize,
}

impl Manifest {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("manifest is plain data")
    }

    pub fn record_stats(&mut self, src: &SourceStats, clean: &CleanStats, params: &CleanParams) {
        self.trace_files = src.trace_files;
        self.truncated_archive = src.truncated;
        self.points_in = clean.points_in;
        self.tracks = clean.tracks;
        for (k, v) in [
            ("ground", clean.ground),
            ("no_altitude", clean.no_alt),
            ("non_monotonic_time", clean.non_monotonic),
            ("position_or_altitude_jump", clean.jumps),
            ("isolated_single_point", clean.short_tracks),
        ] {
            self.dropped.insert(k.into(), v);
        }
        self.params = serde_json::json!({
            "clean": params,
            "max_piece_s": super::pieces::MAX_PIECE_S,
            "max_piece_m": super::pieces::MAX_PIECE_M,
            "coverage_cell_deg": super::coverage::CELL_DEG,
            "coverage_band_ft": super::coverage::BAND_FT,
            "coverage_max_band": super::coverage::MAX_BAND,
        });
    }
}

/// `YYYY-MM-DD` of a UNIX time (UTC), proleptic Gregorian.
pub fn civil_date(t: f64) -> String {
    let z = (t / 86_400.0).floor() as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// UNIX seconds from either a number or `YYYY-MM-DDTHH:MM[:SS[.fff]][Z]` (always read as UTC).
pub fn parse_utc(s: &str) -> Option<f64> {
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once(['T', ' ']).unwrap_or((s, "00:00"));
    let dp: Vec<i64> = date.split('-').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let tp: Vec<f64> = time.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let ([y, m, d], [hh, mm, rest @ ..]) = (dp.as_slice(), tp.as_slice()) else { return None };
    let sec = rest.first().copied().unwrap_or(0.0);
    Some(days_from_civil(*y, *m, *d) as f64 * 86_400.0 + hh * 3600.0 + mm * 60.0 + sec)
}

pub fn day_dir(store: &Path, date: &str) -> PathBuf {
    store.join(format!("date={date}"))
}

/// Points are bucketed by `icao >> 16` so ascending file names are ascending icao.
pub fn points_bucket(icao: u32) -> i64 {
    i64::from(icao >> 16)
}

pub fn points_file_name(icao: u32) -> String {
    format!("part-{:03}.parquet", points_bucket(icao))
}

pub fn hour_dir(day: &Path, hour: i64) -> PathBuf {
    day.join("segments").join(format!("hour={hour:02}"))
}

/// Every `date=*` directory with a readable manifest, sorted by date.
pub fn list_days(store: &Path) -> anyhow::Result<Vec<(PathBuf, Manifest)>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(store)? {
        let p = e?.path();
        let is_day = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("date="));
        let mf = p.join("manifest.json");
        if is_day && mf.is_file() {
            out.push((p, serde_json::from_slice(&std::fs::read(&mf)?)?));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil_dates() {
        assert_eq!(super::civil_date(0.0), "1970-01-01");
        assert_eq!(super::civil_date(1_788_825_600.0), "2026-09-08");
        assert_eq!(super::civil_date(951_782_400.0), "2000-02-29");
        assert_eq!(super::parse_utc("2026-09-08T00:00:00Z"), Some(1_788_825_600.0));
        assert_eq!(super::parse_utc("2026-09-08T20:53:25.5"), Some(1_788_825_600.0 + 75_205.5));
        assert_eq!(super::parse_utc("1788825600"), Some(1_788_825_600.0));
        assert_eq!(super::parse_utc("garbage"), None);
    }
}
