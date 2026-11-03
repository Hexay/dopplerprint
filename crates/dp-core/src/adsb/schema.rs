//! Arrow schemas and row → RecordBatch builders for the store tables. Column docs: docs/DATA.md.

use std::sync::Arc;

use arrow_array::types::{Float32Type, Float64Type, Int16Type, UInt8Type, UInt32Type, UInt64Type};
use arrow_array::{Array, ArrayRef, PrimitiveArray, RecordBatch, StringArray};
use arrow_schema::{DataType as D, Field, Schema, SchemaRef};
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::metadata::KeyValue;
use parquet::file::properties::{EnabledStatistics, WriterProperties};

use super::coverage::{CovKey, CovVal};
use super::model::CleanPoint;
use super::pieces::Piece;

pub const SCHEMA_VERSION: &str = "doppler_print.adsb.v1";

fn schema(cols: &[(&str, D, bool)]) -> SchemaRef {
    Arc::new(Schema::new(cols.iter().map(|(n, d, null)| Field::new(*n, d.clone(), *null)).collect::<Vec<_>>()))
}

pub fn points_schema() -> SchemaRef {
    schema(&[
        ("icao", D::UInt32, false),
        ("track_no", D::UInt32, false),
        ("t", D::Float64, false),
        ("lat", D::Float64, false),
        ("lon", D::Float64, false),
        ("alt_m", D::Float32, false),
        ("alt_source", D::UInt8, false),
        ("ve", D::Float32, false),
        ("vn", D::Float32, false),
        ("vu", D::Float32, false),
        ("src", D::UInt8, false),
        ("flags", D::UInt8, false),
    ])
}

pub fn segments_schema() -> SchemaRef {
    schema(&[
        ("seg_id", D::UInt64, false),
        ("icao", D::UInt32, false),
        ("track_no", D::UInt32, false),
        ("hilbert", D::UInt64, false),
        ("t0", D::Float64, false),
        ("t1", D::Float64, false),
        ("lat_min", D::Float64, false),
        ("lat_max", D::Float64, false),
        ("lon_min", D::Float64, false),
        ("lon_max", D::Float64, false),
        ("alt_min_m", D::Float32, false),
        ("alt_max_m", D::Float32, false),
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
        Arc::new($rows.iter().map($f).collect::<PrimitiveArray<$ty>>()) as ArrayRef
    };
}

pub fn points_batch(rows: &[PointRow]) -> RecordBatch {
    let cols = vec![
        col!(UInt32Type, rows, |r| Some(r.icao)),
        col!(UInt32Type, rows, |r| Some(r.track_no)),
        col!(Float64Type, rows, |r| Some(r.p.t)),
        col!(Float64Type, rows, |r| Some(r.p.lat)),
        col!(Float64Type, rows, |r| Some(r.p.lon)),
        col!(Float32Type, rows, |r| Some(r.p.alt_m as f32)),
        col!(UInt8Type, rows, |r| Some(r.p.alt_source as u8)),
        col!(Float32Type, rows, |r| Some(r.p.ve as f32)),
        col!(Float32Type, rows, |r| Some(r.p.vn as f32)),
        col!(Float32Type, rows, |r| Some(r.p.vu as f32)),
        col!(UInt8Type, rows, |r| Some(r.p.src)),
        col!(UInt8Type, rows, |r| Some(r.p.flags)),
    ];
    RecordBatch::try_new(points_schema(), cols).expect("points schema matches builder")
}

pub fn segments_batch(rows: &[SegRow]) -> RecordBatch {
    let cols = vec![
        col!(UInt64Type, rows, |r| Some(r.piece.seg_id)),
        col!(UInt32Type, rows, |r| Some(r.piece.icao)),
        col!(UInt32Type, rows, |r| Some(r.piece.track_no)),
        col!(UInt64Type, rows, |r| Some(r.piece.hilbert)),
        col!(Float64Type, rows, |r| Some(r.piece.t0)),
        col!(Float64Type, rows, |r| Some(r.piece.t1)),
        col!(Float64Type, rows, |r| Some(r.piece.lat_min)),
        col!(Float64Type, rows, |r| Some(r.piece.lat_max)),
        col!(Float64Type, rows, |r| Some(r.piece.lon_min)),
        col!(Float64Type, rows, |r| Some(r.piece.lon_max)),
        col!(Float32Type, rows, |r| Some(r.piece.alt_min_m)),
        col!(Float32Type, rows, |r| Some(r.piece.alt_max_m)),
        col!(UInt32Type, rows, |r| Some(r.piece.n_points)),
        col!(UInt8Type, rows, |r| Some(r.engine)),
        col!(UInt8Type, rows, |r| Some(r.engine_source)),
        Arc::new(rows.iter().map(|r| r.type_code.as_deref()).collect::<StringArray>()),
        col!(UInt8Type, rows, |r| Some(r.piece.alt_source as u8)),
        col!(UInt8Type, rows, |r| Some(r.piece.src)),
    ];
    RecordBatch::try_new(segments_schema(), cols).expect("segments schema matches builder")
}

pub fn coverage_batch(rows: &[(CovKey, CovVal)]) -> RecordBatch {
    let cols = vec![
        col!(Int16Type, rows, |r| Some(r.0.hour)),
        col!(Int16Type, rows, |r| Some(r.0.lat_cell)),
        col!(Int16Type, rows, |r| Some(r.0.lon_cell)),
        col!(UInt8Type, rows, |r| Some(r.0.band)),
        col!(UInt32Type, rows, |r| Some(r.1.n_aircraft)),
        col!(Float32Type, rows, |r| Some(r.1.track_s as f32)),
    ];
    RecordBatch::try_new(coverage_schema(), cols).expect("coverage schema matches builder")
}

/// zstd Parquet with page-level statistics (for row-group/page pruning) and provenance KV metadata.
pub fn writer_props(zstd_level: i32, row_group_rows: usize, kv: Vec<(String, String)>) -> WriterProperties {
    let kv = std::iter::once(("doppler_print.schema".to_string(), SCHEMA_VERSION.to_string()))
        .chain(kv)
        .map(|(k, v)| KeyValue::new(k, v))
        .collect();
    WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(zstd_level).expect("valid zstd level")))
        .set_max_row_group_row_count(Some(row_group_rows))
        .set_statistics_enabled(EnabledStatistics::Page)
        .set_key_value_metadata(Some(kv))
        .build()
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
