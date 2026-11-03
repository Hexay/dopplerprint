//! Neighbour geom−baro offsets for aircraft that never report geometric altitude (mostly TIS-B / MLAT).
//! Geometric-reporting aircraft give samples per (hour, 1° cell, 2000 ft baro band); each aircraft counts
//! once per bin (its own mean), and a bin needs `MIN_AIRCRAFT` of them. Applied to `Baro` rows at finalize.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use arrow_array::types::{Int32Type, Int64Type, UInt8Type};
use arrow_array::{ArrayRef, Int32Array, RecordBatch, UInt8Array};
use rustc_hash::FxHashMap as HashMap;

use super::altitude::FT_TO_M;
use super::model::AltSource;
use super::pieces::hour_of;
use super::schema::{ALT_SCALE, DEG_SCALE, T_SCALE, column};
use super::trace::{Alt, FLAG_ALT_GEOM, RawTrace};

pub const MIN_AIRCRAFT: u32 = 2;
const BAND_M: f64 = 2000.0 * FT_TO_M;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OffKey {
    hour: i16,
    lat: i16,
    lon: i16,
    band: i16,
}

pub fn off_key(t: f64, lat: f64, lon: f64, baro_m: f64, day0: f64) -> OffKey {
    OffKey {
        hour: hour_of(t, day0) as i16,
        lat: lat.floor() as i16,
        lon: lon.floor() as i16,
        band: (baro_m / BAND_M).floor() as i16,
    }
}

/// This aircraft's mean offset (m) per bin, from points carrying both baro and geometric altitude.
pub fn trace_offsets(raw: &RawTrace, day0: f64) -> Vec<(OffKey, f32)> {
    let mut m: HashMap<OffKey, (f64, u32)> = HashMap::default();
    for p in &raw.trace {
        if let (Alt::Feet(b), Some(g)) = (p.alt, p.alt_geom_ft)
            && p.flags & FLAG_ALT_GEOM == 0
        {
            let e = m.entry(off_key(raw.timestamp + p.dt, p.lat, p.lon, b * FT_TO_M, day0)).or_default();
            e.0 += (g - b) * FT_TO_M;
            e.1 += 1;
        }
    }
    m.into_iter().map(|(k, (s, n))| (k, (s / f64::from(n)) as f32)).collect()
}

#[derive(Default)]
pub struct OffsetAgg {
    bins: HashMap<OffKey, (f64, u32)>,
}

impl OffsetAgg {
    pub fn add(&mut self, contrib: &[(OffKey, f32)]) {
        for (k, off) in contrib {
            let e = self.bins.entry(*k).or_default();
            e.0 += f64::from(*off);
            e.1 += 1;
        }
    }

    pub fn into_grid(self) -> OffsetGrid {
        let grid = self.bins.into_iter().filter(|(_, v)| v.1 >= MIN_AIRCRAFT).map(|(k, (s, n))| (k, s / f64::from(n))).collect();
        OffsetGrid { grid, applied: AtomicU64::new(0) }
    }
}

pub struct OffsetGrid {
    grid: HashMap<OffKey, f64>,
    pub applied: AtomicU64,
}

impl OffsetGrid {
    pub fn len(&self) -> usize {
        self.grid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.grid.is_empty()
    }

    pub fn offset(&self, t: f64, lat: f64, lon: f64, baro_m: f64, day0: f64) -> Option<f64> {
        self.grid.get(&off_key(t, lat, lon, baro_m, day0)).copied()
    }

    /// Shift `Baro` rows of a points batch onto the neighbour offset and relabel them.
    pub fn apply_points(&self, b: RecordBatch, day0: f64) -> anyhow::Result<RecordBatch> {
        let (src, alt) = (column::<UInt8Type>(&b, "alt_source"), column::<Int32Type>(&b, "alt_dm"));
        let t = column::<Int64Type>(&b, "t_ms");
        let (lat, lon) = (column::<Int32Type>(&b, "lat_e6"), column::<Int32Type>(&b, "lon_e6"));
        let mut new_src: Vec<u8> = src.values().to_vec();
        let mut new_alt: Vec<i32> = alt.values().to_vec();
        let mut n = 0;
        for i in 0..b.num_rows() {
            if new_src[i] != AltSource::Baro as u8 {
                continue;
            }
            let baro = f64::from(new_alt[i]) / ALT_SCALE;
            let (la, lo) = (f64::from(lat.value(i)) / DEG_SCALE, f64::from(lon.value(i)) / DEG_SCALE);
            if let Some(off) = self.offset(t.value(i) as f64 / T_SCALE, la, lo, baro, day0) {
                new_alt[i] = ((baro + off) * ALT_SCALE).round() as i32;
                new_src[i] = AltSource::BaroNeighbour as u8;
                n += 1;
            }
        }
        self.applied.fetch_add(n, Ordering::Relaxed);
        replace(
            b,
            [("alt_dm", Arc::new(Int32Array::from(new_alt)) as ArrayRef), ("alt_source", Arc::new(UInt8Array::from(new_src)))],
        )
    }

    /// Same for segment pieces whose worst basis is `Baro`, using the bbox centre and `alt_min_m`.
    pub fn apply_segments(&self, b: RecordBatch, day0: f64) -> anyhow::Result<RecordBatch> {
        let src = column::<UInt8Type>(&b, "alt_source");
        let (amin, amax) = (column::<Int32Type>(&b, "alt_min_m"), column::<Int32Type>(&b, "alt_max_m"));
        let t0 = column::<Int64Type>(&b, "t0_ms");
        let bbox = ["lat_min_e6", "lat_max_e6", "lon_min_e6", "lon_max_e6"].map(|n| column::<Int32Type>(&b, n));
        let (mut s2, mut lo2, mut hi2) = (src.values().to_vec(), amin.values().to_vec(), amax.values().to_vec());
        for i in 0..b.num_rows() {
            if s2[i] != AltSource::Baro as u8 {
                continue;
            }
            let c = |a: usize, z: usize| (f64::from(bbox[a].value(i)) + f64::from(bbox[z].value(i))) * 0.5 / DEG_SCALE;
            if let Some(off) = self.offset(t0.value(i) as f64 / T_SCALE, c(0, 1), c(2, 3), f64::from(lo2[i]), day0) {
                lo2[i] = (f64::from(lo2[i]) + off).floor() as i32;
                hi2[i] = (f64::from(hi2[i]) + off).ceil() as i32;
                s2[i] = AltSource::BaroNeighbour as u8;
            }
        }
        replace(
            b,
            [
                ("alt_min_m", Arc::new(Int32Array::from(lo2)) as ArrayRef),
                ("alt_max_m", Arc::new(Int32Array::from(hi2))),
                ("alt_source", Arc::new(UInt8Array::from(s2))),
            ],
        )
    }
}

fn replace<const N: usize>(b: RecordBatch, cols: [(&str, ArrayRef); N]) -> anyhow::Result<RecordBatch> {
    let schema = b.schema();
    let mut arrays = b.columns().to_vec();
    for (name, arr) in cols {
        arrays[schema.index_of(name)?] = arr;
    }
    Ok(RecordBatch::try_new(schema, arrays)?)
}
