//! `load_tracks`: hour-partition + row-group pruning on `segments`, then the matching tracks' points.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::path::Path;

use arrow_array::RecordBatch;
use arrow_array::types::{Float32Type, Float64Type, UInt8Type, UInt32Type};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::metadata::RowGroupMetaData;
use parquet::file::statistics::Statistics;

use super::TrackQuery;
use super::manifest::{hour_dir, list_days, points_file_name};
use super::model::{engine_code, engine_from_code, enu_to_track_point, track_id};
use super::pieces::MAX_PIECE_S;
use super::schema::{column, opt_str, string_column};
use crate::track::{EngineClass, Track};

/// Track key: (icao, track_no).
type Key = (u32, u32);

struct Want {
    engine: EngineClass,
    type_code: Option<String>,
}

fn stat_range(rg: &RowGroupMetaData, name: &str) -> Option<(f64, f64)> {
    let col = rg.columns().iter().find(|c| c.column_path().string() == name)?;
    match col.statistics()? {
        Statistics::Double(s) => Some((*s.min_opt()?, *s.max_opt()?)),
        Statistics::Float(s) => Some((f64::from(*s.min_opt()?), f64::from(*s.max_opt()?))),
        Statistics::Int32(s) => Some((f64::from(*s.min_opt()? as u32), f64::from(*s.max_opt()? as u32))),
        _ => None,
    }
}

/// Read the row groups for which `keep(rg)` holds (missing statistics never prune).
fn read_pruned(path: &Path, keep: impl Fn(&RowGroupMetaData) -> bool) -> anyhow::Result<Vec<RecordBatch>> {
    let b = ParquetRecordBatchReaderBuilder::try_new(File::open(path)?)?;
    let rgs: Vec<usize> = (0..b.metadata().num_row_groups()).filter(|&i| keep(b.metadata().row_group(i))).collect();
    if rgs.is_empty() {
        return Ok(Vec::new());
    }
    Ok(b.with_row_groups(rgs).with_batch_size(65_536).build()?.collect::<Result<_, _>>()?)
}

fn overlaps(range: Option<(f64, f64)>, lo: f64, hi: f64) -> bool {
    range.is_none_or(|(a, b)| b >= lo && a <= hi)
}

fn select_segments(path: &Path, q: &TrackQuery, wants: &mut HashMap<Key, Want>) -> anyhow::Result<()> {
    let excluded: HashSet<u8> = q.exclude_engines.iter().map(|e| engine_code(*e)).collect();
    let batches = read_pruned(path, |rg| {
        let space = q.bbox.is_none_or(|(la0, lo0, la1, lo1)| {
            overlaps(stat_range(rg, "lat_max"), la0, f64::INFINITY)
                && overlaps(stat_range(rg, "lat_min"), f64::NEG_INFINITY, la1)
                && overlaps(stat_range(rg, "lon_max"), lo0, f64::INFINITY)
                && overlaps(stat_range(rg, "lon_min"), f64::NEG_INFINITY, lo1)
        });
        let alt = q.max_alt_m.is_none_or(|m| overlaps(stat_range(rg, "alt_min_m"), f64::NEG_INFINITY, m));
        space && alt && overlaps(stat_range(rg, "t1"), q.t_start, f64::INFINITY)
    })?;
    for b in &batches {
        let (icao, trk) = (column::<UInt32Type>(b, "icao"), column::<UInt32Type>(b, "track_no"));
        let (t0, t1) = (column::<Float64Type>(b, "t0"), column::<Float64Type>(b, "t1"));
        let (la0, la1) = (column::<Float64Type>(b, "lat_min"), column::<Float64Type>(b, "lat_max"));
        let (lo0, lo1) = (column::<Float64Type>(b, "lon_min"), column::<Float64Type>(b, "lon_max"));
        let alt_min = column::<Float32Type>(b, "alt_min_m");
        let engine = column::<UInt8Type>(b, "engine");
        let types = string_column(b, "type_code");
        for i in 0..b.num_rows() {
            let in_time = t1.value(i) >= q.t_start && t0.value(i) <= q.t_end;
            let in_box = q.bbox.is_none_or(|(a0, o0, a1, o1)| {
                la1.value(i) >= a0 && la0.value(i) <= a1 && lo1.value(i) >= o0 && lo0.value(i) <= o1
            });
            let low = q.max_alt_m.is_none_or(|m| f64::from(alt_min.value(i)) <= m);
            if in_time && in_box && low && !excluded.contains(&engine.value(i)) {
                wants.entry((icao.value(i), trk.value(i))).or_insert_with(|| Want {
                    engine: engine_from_code(engine.value(i)),
                    type_code: opt_str(types, i).map(str::to_string),
                });
            }
        }
    }
    Ok(())
}

/// Point columns kept per track before trimming: t, lat, lon, alt, ve, vn, vu.
type Pt = [f64; 7];

fn collect_points(day: &Path, q: &TrackQuery, wants: &HashMap<Key, Want>, pad: f64) -> anyhow::Result<BTreeMap<Key, Vec<Pt>>> {
    let icaos: HashSet<u32> = wants.keys().map(|k| k.0).collect();
    let (lo, hi) = (icaos.iter().min().copied(), icaos.iter().max().copied());
    let mut out: BTreeMap<Key, Vec<Pt>> = BTreeMap::new();
    let (Some(lo), Some(hi)) = (lo, hi) else { return Ok(out) };
    let buckets: HashSet<String> = icaos.iter().map(|&i| points_file_name(i)).collect();
    let mut files: Vec<_> = std::fs::read_dir(day.join("points"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| buckets.contains(n)))
        .collect();
    files.sort();
    for f in files {
        let batches = read_pruned(&f, |rg| {
            overlaps(stat_range(rg, "icao"), f64::from(lo), f64::from(hi))
                && overlaps(stat_range(rg, "t"), q.t_start - pad, q.t_end + pad)
                && stat_range(rg, "icao").is_none_or(|(a, b)| icaos.iter().any(|&i| (a..=b).contains(&f64::from(i))))
        })?;
        for b in &batches {
            let (icao, trk) = (column::<UInt32Type>(b, "icao"), column::<UInt32Type>(b, "track_no"));
            let t = column::<Float64Type>(b, "t");
            let (lat, lon) = (column::<Float64Type>(b, "lat"), column::<Float64Type>(b, "lon"));
            let f32s = ["alt_m", "ve", "vn", "vu"].map(|n| column::<Float32Type>(b, n));
            for i in 0..b.num_rows() {
                let key = (icao.value(i), trk.value(i));
                let ti = t.value(i);
                if ti < q.t_start - pad || ti > q.t_end + pad || !wants.contains_key(&key) {
                    continue;
                }
                let v = f32s.map(|c| f64::from(c.value(i)));
                out.entry(key).or_default().push([ti, lat.value(i), lon.value(i), v[0], v[1], v[2], v[3]]);
            }
        }
    }
    Ok(out)
}

/// Keep `[last point ≤ t_start, first point ≥ t_end]` so interpolation covers the whole window.
fn trim(pts: &mut Vec<Pt>, t_start: f64, t_end: f64) {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let first = pts.partition_point(|p| p[0] <= t_start).saturating_sub(1);
    let last = pts.partition_point(|p| p[0] < t_end).min(pts.len().saturating_sub(1));
    pts.truncate(last + 1);
    pts.drain(..first.min(pts.len()));
}

/// Tracks with any segment piece inside the query window, bbox, altitude ceiling and engine filter.
/// Points are not clipped to the bbox; they are clipped to the time window plus one point either side.
pub fn load_tracks(store: &Path, q: &TrackQuery) -> anyhow::Result<Vec<Track>> {
    let mut tracks = Vec::new();
    for (day, mf) in list_days(store)? {
        let h_lo = ((q.t_start - MAX_PIECE_S - mf.day0) / 3600.0).floor() as i64;
        let h_hi = ((q.t_end - mf.day0) / 3600.0).floor() as i64;
        let mut wants = HashMap::new();
        for h in h_lo.max(-1)..=h_hi.min(25) {
            let f = hour_dir(&day, h).join("part-0.parquet");
            if f.is_file() {
                select_segments(&f, q, &mut wants)?;
            }
        }
        let gap = mf.params["clean"]["max_gap_s"].as_f64().unwrap_or(60.0);
        for (key, mut pts) in collect_points(&day, q, &wants, gap)? {
            trim(&mut pts, q.t_start, q.t_end);
            if pts.len() < 2 {
                continue;
            }
            let w = &wants[&key];
            tracks.push(Track {
                id: format!("{}/{}", mf.date, track_id(key.0, key.1)),
                icao24: key.0,
                type_code: w.type_code.clone(),
                engine: w.engine,
                points: pts.iter().map(|p| enu_to_track_point(p[0], p[1], p[2], p[3], [p[4], p[5], p[6]])).collect(),
            });
        }
    }
    tracks.sort_by(|a, b| a.t_start().total_cmp(&b.t_start()).then_with(|| a.id.cmp(&b.id)));
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    #[test]
    fn trim_keeps_bracketing_points() {
        let mut v: Vec<super::Pt> = (0..10).map(|i| [i as f64 * 10.0, 0., 0., 0., 0., 0., 0.]).collect();
        super::trim(&mut v, 25.0, 55.0);
        assert_eq!(v.iter().map(|p| p[0]).collect::<Vec<_>>(), vec![20.0, 30.0, 40.0, 50.0, 60.0]);
    }
}
