//! `load_tracks`: hour-partition + row-group pruning on `segments`, then the matching tracks' points.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::path::Path;

use arrow_array::RecordBatch;
use arrow_array::types::{Int32Type, Int64Type, UInt8Type, UInt32Type};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::metadata::RowGroupMetaData;
use parquet::file::statistics::Statistics;
use rayon::prelude::*;

use super::TrackQuery;
use super::dedup::{DedupParams, drop_shadow_tracks};
use super::manifest::{hour_dir, list_days, points_file_name};
use super::model::{engine_code, engine_from_code, enu_to_track_point, track_id};
use super::pieces::MAX_PIECE_S;
use super::schema::{ALT_SCALE, DEG_SCALE, T_SCALE, VEL_SCALE, column, opt_str, string_column, to_ms};
use crate::track::{EngineClass, Track};

/// Track key: (icao, track_no).
type Key = (u32, u32);

struct Want {
    engine: EngineClass,
    type_code: Option<String>,
}

/// Row-group (min, max) of a column divided by its storage `scale`. Unsigned columns here stay below
/// 2^31, so their INT32 statistics read correctly as signed.
fn stat_range(rg: &RowGroupMetaData, name: &str, scale: f64) -> Option<(f64, f64)> {
    let col = rg.columns().iter().find(|c| c.column_path().string() == name)?;
    let (a, b) = match col.statistics()? {
        Statistics::Double(s) => (*s.min_opt()?, *s.max_opt()?),
        Statistics::Float(s) => (f64::from(*s.min_opt()?), f64::from(*s.max_opt()?)),
        Statistics::Int32(s) => (f64::from(*s.min_opt()?), f64::from(*s.max_opt()?)),
        Statistics::Int64(s) => (*s.min_opt()? as f64, *s.max_opt()? as f64),
        _ => return None,
    };
    Some((a / scale, b / scale))
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
        let deg = |n| stat_range(rg, n, DEG_SCALE);
        let space = q.bbox.is_none_or(|(la0, lo0, la1, lo1)| {
            overlaps(deg("lat_max_e6"), la0, f64::INFINITY)
                && overlaps(deg("lat_min_e6"), f64::NEG_INFINITY, la1)
                && overlaps(deg("lon_max_e6"), lo0, f64::INFINITY)
                && overlaps(deg("lon_min_e6"), f64::NEG_INFINITY, lo1)
        });
        let alt = q.max_alt_m.is_none_or(|m| overlaps(stat_range(rg, "alt_min_m", 1.0), f64::NEG_INFINITY, m));
        space && alt && overlaps(stat_range(rg, "t1_ms", T_SCALE), q.t_start, f64::INFINITY)
    })?;
    let (ts, te) = (to_ms(q.t_start), to_ms(q.t_end));
    let bbox_e6 = q.bbox.map(|(a0, o0, a1, o1)| [a0, o0, a1, o1].map(|v| (v * DEG_SCALE).round() as i32));
    for b in &batches {
        let (icao, trk) = (column::<UInt32Type>(b, "icao"), column::<UInt32Type>(b, "track_no"));
        let (t0, t1) = (column::<Int64Type>(b, "t0_ms"), column::<Int64Type>(b, "t1_ms"));
        let (la0, la1) = (column::<Int32Type>(b, "lat_min_e6"), column::<Int32Type>(b, "lat_max_e6"));
        let (lo0, lo1) = (column::<Int32Type>(b, "lon_min_e6"), column::<Int32Type>(b, "lon_max_e6"));
        let alt_min = column::<Int32Type>(b, "alt_min_m");
        let engine = column::<UInt8Type>(b, "engine");
        let types = string_column(b, "type_code");
        for i in 0..b.num_rows() {
            let in_time = t1.value(i) >= ts && t0.value(i) <= te;
            let in_box = bbox_e6.is_none_or(|[a0, o0, a1, o1]| {
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
    let per_file: Vec<BTreeMap<Key, Vec<Pt>>> = files
        .par_iter()
        .map(|f| {
            let batches = read_pruned(f, |rg| {
                let ic = stat_range(rg, "icao", 1.0);
                overlaps(ic, f64::from(lo), f64::from(hi))
                    && overlaps(stat_range(rg, "t_ms", T_SCALE), q.t_start - pad, q.t_end + pad)
                    && ic.is_none_or(|(a, b)| icaos.iter().any(|&i| (a..=b).contains(&f64::from(i))))
            })?;
            Ok(file_points(&batches, q, wants, pad))
        })
        .collect::<anyhow::Result<_>>()?;
    // Merged in file order, so each track's points keep the serial read order.
    for m in per_file {
        for (key, pts) in m {
            out.entry(key).or_default().extend(pts);
        }
    }
    Ok(out)
}

fn file_points(batches: &[RecordBatch], q: &TrackQuery, wants: &HashMap<Key, Want>, pad: f64) -> BTreeMap<Key, Vec<Pt>> {
    let mut out: BTreeMap<Key, Vec<Pt>> = BTreeMap::new();
    let (ts, te) = (to_ms(q.t_start - pad), to_ms(q.t_end + pad));
    for b in batches {
        let (icao, trk) = (column::<UInt32Type>(b, "icao"), column::<UInt32Type>(b, "track_no"));
        let t = column::<Int64Type>(b, "t_ms");
        let ints = ["lat_e6", "lon_e6", "alt_dm", "ve_cms", "vn_cms", "vu_cms"].map(|n| column::<Int32Type>(b, n));
        let scales = [DEG_SCALE, DEG_SCALE, ALT_SCALE, VEL_SCALE, VEL_SCALE, VEL_SCALE];
        for i in 0..b.num_rows() {
            let key = (icao.value(i), trk.value(i));
            let ti = t.value(i);
            if ti < ts || ti > te || !wants.contains_key(&key) {
                continue;
            }
            let mut p = [ti as f64 / T_SCALE, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            for (k, (c, s)) in ints.iter().zip(scales).enumerate() {
                p[k + 1] = f64::from(c.value(i)) / s;
            }
            out.entry(key).or_default().push(p);
        }
    }
    out
}

/// Keep `[last point ≤ t_start, first point ≥ t_end]` so interpolation covers the whole window.
fn trim(pts: &mut Vec<Pt>, t_start: f64, t_end: f64) {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let first = pts.partition_point(|p| p[0] <= t_start).saturating_sub(1);
    let last = pts.partition_point(|p| p[0] < t_end).min(pts.len().saturating_sub(1));
    pts.truncate(last + 1);
    pts.drain(..first.min(pts.len()));
}

#[derive(Clone, Copy, Debug)]
pub struct LoadOptions {
    /// Drop `~` tracks that shadow an ICAO track (see `dedup`).
    pub dedup: Option<DedupParams>,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self { dedup: Some(DedupParams::default()) }
    }
}

/// `load_tracks_with` and default options (TIS-B shadow dedup on).
pub fn load_tracks(store: &Path, q: &TrackQuery) -> anyhow::Result<Vec<Track>> {
    load_tracks_with(store, q, &LoadOptions::default())
}

/// Tracks with any segment piece inside the query window, bbox, altitude ceiling and engine filter.
/// Points are not clipped to the bbox; they are clipped to the time window plus one point either side.
pub fn load_tracks_with(store: &Path, q: &TrackQuery, opts: &LoadOptions) -> anyhow::Result<Vec<Track>> {
    let mut tracks = Vec::new();
    for (day, mf) in list_days(store)? {
        let h_lo = ((q.t_start - MAX_PIECE_S - mf.day0) / 3600.0).floor() as i64;
        let h_hi = ((q.t_end - mf.day0) / 3600.0).floor() as i64;
        let hours: Vec<_> = (h_lo.max(-1)..=h_hi.min(25))
            .map(|h| hour_dir(&day, h).join("part-0.parquet"))
            .filter(|f| f.is_file())
            .collect();
        let per_hour: Vec<HashMap<Key, Want>> = hours
            .par_iter()
            .map(|f| {
                let mut w = HashMap::new();
                select_segments(f, q, &mut w).map(|()| w)
            })
            .collect::<anyhow::Result<_>>()?;
        // First hour wins, as in the serial scan.
        let mut wants = HashMap::new();
        for m in per_hour {
            for (k, w) in m {
                wants.entry(k).or_insert(w);
            }
        }
        let gap = mf.params["clean"]["max_gap_s"].as_f64().unwrap_or(60.0);
        let points: Vec<(Key, Vec<Pt>)> = collect_points(&day, q, &wants, gap)?.into_iter().collect();
        tracks.par_extend(points.into_par_iter().filter_map(|(key, mut pts)| {
            trim(&mut pts, q.t_start, q.t_end);
            if pts.len() < 2 {
                return None;
            }
            let w = &wants[&key];
            Some(Track {
                id: format!("{}/{}", mf.date, track_id(key.0, key.1)),
                icao24: key.0,
                type_code: w.type_code.clone(),
                engine: w.engine,
                points: pts.iter().map(|p| enu_to_track_point(p[0], p[1], p[2], p[3], [p[4], p[5], p[6]])).collect(),
            })
        }));
    }
    tracks.sort_by(|a, b| a.t_start().total_cmp(&b.t_start()).then_with(|| a.id.cmp(&b.id)));
    Ok(match &opts.dedup {
        Some(p) => drop_shadow_tracks(tracks, p).0,
        None => tracks,
    })
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
