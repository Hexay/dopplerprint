//! Arrow schemas and row → RecordBatch builders for the store tables. Column docs: docs/DATA.md.
//! Continuous values are integer-scaled so DELTA_BINARY_PACKED + zstd can exploit the (icao, t) sort.

use std::sync::Arc;

use arrow_array::types::{Float32Type, Int16Type, Int32Type, Int64Type, UInt8Type, UInt32Type, UInt64Type};
use arrow_array::{Array, ArrayRef, PrimitiveArray, RecordBatch, StringArray};
use arrow_schema::{DataType as D, Field, Schema, SchemaRef};
use parquet::basic::{Compression, Encoding, ZstdLevel};
use parquet::file::metadata::KeyValue;
use parquet::file::properties::{EnabledStatistics, WriterProperties};
use parquet::schema::types::ColumnPath;

use super::coverage::{CovKey, CovVal};
use super::model::CleanPoint;
use super::pieces::Piece;

pub const SCHEMA_VERSION: &str = "doppler_print.adsb.v2";

pub const DEG_SCALE: f64 = 1e6;
pub const ALT_SCALE: f64 = 10.0;
pub const VEL_SCALE: f64 = 100.0;
pub const T_SCALE: f64 = 1000.0;

pub fn to_ms(t: f64) -> i64 {
    (t * T_SCALE).round() as i64
}

fn q32(v: f64, scale: f64) -> i32 {
    (v * scale).round() as i32
}

fn schema(cols: &[(&str, D, bool)]) -> SchemaRef {
    Arc::new(Schema::new(cols.iter().map(|(n, d, null)| Field::new(*n, d.clone(), *null)).collect::<Vec<_>>()))
}

pub fn points_schema() -> SchemaRef {
    schema(&[
        ("icao", D::UInt32, false),
        ("track_no", D::UInt32, false),
        ("t_ms", D::Int64, false),
        ("lat_e6", D::Int32, false),
        ("lon_e6", D::Int32, false),
        ("alt_dm", D::Int32, false),
        ("alt_source", D::UInt8, false),
        ("ve_cms", D::Int32, false),
        ("vn_cms", D::Int32, false),
        ("vu_cms", D::Int32, false),
        ("src", D::UInt8, false),
        ("flags", D::UInt8, false),
    ])
}

pub fn segments_schema() -> SchemaRef {
    schema(&[
        ("icao", D::UInt32, false),
        ("track_no", D::UInt32, false),
        ("hilbert", D::UInt64, false),
        ("t0_ms", D::Int64, false),
        ("t1_ms", D::Int64, false),
        ("lat_min_e6", D::Int32, false),
        ("lat_max_e6", D::Int32, false),
        ("lon_min_e6", D::Int32, false),
        ("lon_max_e6", D::Int32, false),
        ("alt_min_m", D::Int32, false),
        ("alt_max_m", D::Int32, false),
        ("n_points", D::UInt32, false),
        ("engine", D::UInt8, false),
        ("engine_source", D::UInt8, false),
        ("type_code", D::Utf8, true),
        ("alt_source", D::UInt8, false),
        ("src", D::UInt8, false),
    ])
}

pub fn coverage_schema() -> SchemaRef {
    schema(&[
        ("hour", D::Int16, false),
        ("lat_cell", D::Int16, false),
        ("lon_cell", D::Int16, false),
        ("alt_band", D::UInt8, false),
        ("n_aircraft", D::UInt32, false),
        ("track_s", D::Float32, false),
        ("n_appear", D::UInt32, false),
        ("n_vanish", D::UInt32, false),
    ])
}

#[derive(Clone, Debug)]
pub struct PointRow {
    pub icao: u32,
    pub track_no: u32,
    pub p: CleanPoint,
}

#[derive(Clone, Debug)]
pub struct SegRow {
    pub piece: Piece,
    pub engine: u8,
    pub engine_source: u8,
    pub type_code: Option<Arc<str>>,
}

macro_rules! col {
    ($ty:ty, $rows:expr, $f:expr) => {
        Arc::new($rows.iter().map(|r| Some($f(r))).collect::<PrimitiveArray<$ty>>()) as ArrayRef
    };
}

pub fn points_batch(rows: &[PointRow]) -> RecordBatch {
    let cols = vec![
        col!(UInt32Type, rows, |r: &PointRow| r.icao),
        col!(UInt32Type, rows, |r: &PointRow| r.track_no),
        col!(Int64Type, rows, |r: &PointRow| to_ms(r.p.t)),
        col!(Int32Type, rows, |r: &PointRow| q32(r.p.lat, DEG_SCALE)),
        col!(Int32Type, rows, |r: &PointRow| q32(r.p.lon, DEG_SCALE)),
        col!(Int32Type, rows, |r: &PointRow| q32(r.p.alt_m, ALT_SCALE)),
        col!(UInt8Type, rows, |r: &PointRow| r.p.alt_source as u8),
        col!(Int32Type, rows, |r: &PointRow| q32(r.p.ve, VEL_SCALE)),
        col!(Int32Type, rows, |r: &PointRow| q32(r.p.vn, VEL_SCALE)),
        col!(Int32Type, rows, |r: &PointRow| q32(r.p.vu, VEL_SCALE)),
        col!(UInt8Type, rows, |r: &PointRow| r.p.src),
        col!(UInt8Type, rows, |r: &PointRow| r.p.flags),
    ];
    RecordBatch::try_new(points_schema(), cols).expect("points schema matches builder")
}

pub fn segments_batch(rows: &[SegRow]) -> RecordBatch {
    let cols = vec![
        col!(UInt32Type, rows, |r: &SegRow| r.piece.icao),
        col!(UInt32Type, rows, |r: &SegRow| r.piece.track_no),
        col!(UInt64Type, rows, |r: &SegRow| r.piece.hilbert),
        col!(Int64Type, rows, |r: &SegRow| to_ms(r.piece.t0)),
        col!(Int64Type, rows, |r: &SegRow| to_ms(r.piece.t1)),
        col!(Int32Type, rows, |r: &SegRow| (r.piece.lat_min * DEG_SCALE).floor() as i32),
        col!(Int32Type, rows, |r: &SegRow| (r.piece.lat_max * DEG_SCALE).ceil() as i32),
        col!(Int32Type, rows, |r: &SegRow| (r.piece.lon_min * DEG_SCALE).floor() as i32),
        col!(Int32Type, rows, |r: &SegRow| (r.piece.lon_max * DEG_SCALE).ceil() as i32),
        col!(Int32Type, rows, |r: &SegRow| r.piece.alt_min_m.floor() as i32),
        col!(Int32Type, rows, |r: &SegRow| r.piece.alt_max_m.ceil() as i32),
        col!(UInt32Type, rows, |r: &SegRow| r.piece.n_points),
        col!(UInt8Type, rows, |r: &SegRow| r.engine),
        col!(UInt8Type, rows, |r: &SegRow| r.engine_source),
        Arc::new(rows.iter().map(|r| r.type_code.as_deref()).collect::<StringArray>()),
        col!(UInt8Type, rows, |r: &SegRow| r.piece.alt_source as u8),
        col!(UInt8Type, rows, |r: &SegRow| r.piece.src),
    ];
    RecordBatch::try_new(segments_schema(), cols).expect("segments schema matches builder")
}

pub fn coverage_batch(rows: &[(CovKey, CovVal)]) -> RecordBatch {
    let cols = vec![
        col!(Int16Type, rows, |r: &(CovKey, CovVal)| r.0.hour),
        col!(Int16Type, rows, |r: &(CovKey, CovVal)| r.0.lat_cell),
        col!(Int16Type, rows, |r: &(CovKey, CovVal)| r.0.lon_cell),
        col!(UInt8Type, rows, |r: &(CovKey, CovVal)| r.0.band),
        col!(UInt32Type, rows, |r: &(CovKey, CovVal)| r.1.n_aircraft),
        col!(Float32Type, rows, |r: &(CovKey, CovVal)| r.1.track_s),
        col!(UInt32Type, rows, |r: &(CovKey, CovVal)| r.1.n_appear),
        col!(UInt32Type, rows, |r: &(CovKey, CovVal)| r.1.n_vanish),
    ];
    RecordBatch::try_new(coverage_schema(), cols).expect("coverage schema matches builder")
}

/// zstd Parquet with page statistics and provenance KV metadata. Integer columns are delta-encoded;
/// dictionaries only for low-cardinality u8 / string columns (float/int dictionaries cost CPU for nothing).
pub fn writer_props(schema: &Schema, zstd_level: i32, row_group_rows: usize, kv: Vec<(String, String)>) -> WriterProperties {
    let kv = std::iter::once(("doppler_print.schema".to_string(), SCHEMA_VERSION.to_string()))
        .chain(kv)
        .map(|(k, v)| KeyValue::new(k, v))
        .collect();
    let mut b = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(zstd_level).expect("valid zstd level")))
        .set_max_row_group_row_count(Some(row_group_rows))
        .set_statistics_enabled(EnabledStatistics::Page)
        .set_dictionary_enabled(false)
        .set_key_value_metadata(Some(kv));
    for f in schema.fields() {
        let path = ColumnPath::from(f.name().as_str());
        match f.data_type() {
            D::UInt8 | D::Utf8 => b = b.set_column_dictionary_enabled(path, true),
            D::Int32 | D::Int64 | D::UInt32 | D::UInt64 => b = b.set_column_encoding(path, Encoding::DELTA_BINARY_PACKED),
            _ => {}
        }
    }
    b.build()
}

/// Typed column by name; panics on schema mismatch, which is a store-version bug, not bad input.
pub fn column<'a, T: arrow_array::ArrowPrimitiveType>(b: &'a RecordBatch, name: &str) -> &'a PrimitiveArray<T> {
    b.column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<PrimitiveArray<T>>())
        .unwrap_or_else(|| panic!("column {name} missing or mistyped"))
}

pub fn string_column<'a>(b: &'a RecordBatch, name: &str) -> &'a StringArray {
    b.column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .unwrap_or_else(|| panic!("column {name} missing or mistyped"))
}

pub fn opt_str(a: &StringArray, i: usize) -> Option<&str> {
    (!a.is_null(i)).then(|| a.value(i))
}
