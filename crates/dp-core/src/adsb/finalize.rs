//! End of ingest: coverage first (frees its map), then each spill bucket is loaded, neighbour-offset
//! corrected, sorted and written on `finalize_threads` workers. Peak memory ≈ threads × largest bucket.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Instant;

use arrow_array::types::{Int64Type, UInt32Type, UInt64Type};
use parquet::arrow::ArrowWriter;
use rayon::prelude::*;

use super::coverage::CoverageAgg;
use super::ingest::IngestOptions;
use super::manifest::{LICENCE, Manifest, hour_dir, points_file_name};
use super::model::AltSource;
use super::neighbour::OffsetAgg;
use super::schema::{self, PointRow, SegRow, column, points_schema, segments_schema, writer_props};
use super::spill::{BucketSpill, permutation, sort_spill};

pub struct Spilled {
    pub points: BucketSpill<PointRow>,
    pub segments: BucketSpill<SegRow>,
    pub coverage: CoverageAgg,
    pub offsets: OffsetAgg,
    /// Points per `AltSource` code as streamed (before neighbour correction).
    pub alt_counts: [u64; 4],
}

pub fn finalize(s: Spilled, day0: f64, stage: &Path, opts: &IngestOptions, mf: &mut Manifest) -> anyhow::Result<()> {
    let t = Instant::now();
    let kv = vec![("doppler_print.date".into(), mf.date.clone()), ("doppler_print.licence".into(), LICENCE.into())];
    std::fs::create_dir_all(stage)?;
    let cov = s.coverage.into_sorted();
    mf.coverage_rows = cov.len() as u64;
    let cs = schema::coverage_schema();
    let f = std::fs::File::create(stage.join("coverage.parquet"))?;
    let mut cw = ArrowWriter::try_new(f, cs.clone(), Some(writer_props(&cs, opts.zstd_level, 1 << 20, kv.clone())))?;
    cw.write(&schema::coverage_batch(&cov))?;
    cw.close()?;
    drop(cov);
    let grid = s.offsets.into_grid();
    mf.neighbour_offset_bins = grid.len() as u64;
    mf.phase_s.insert("coverage".into(), t.elapsed().as_secs_f64());

    let pool = rayon::ThreadPoolBuilder::new().num_threads(opts.finalize_threads.max(1)).build()?;
    let t = Instant::now();
    let (ps, ss) = (points_schema(), segments_schema());
    let spills = s.points.finish()?;
    mf.points_rows = pool.install(|| {
        spills
            .par_iter()
            .map(|(key, spill)| {
                let out_file = stage.join("points").join(points_file_name((*key as u32) << 16));
                let props = writer_props(&ps, opts.zstd_level, 131_072, kv.clone());
                let n = sort_spill(spill, &out_file, props, |b| grid.apply_points(b, day0), |b| {
                    let (icao, t) = (column::<UInt32Type>(b, "icao"), column::<Int64Type>(b, "t_ms"));
                    permutation(b.num_rows(), |i| (icao.value(i), t.value(i)))
                })?;
                Ok(n as u64)
            })
            .sum::<anyhow::Result<u64>>()
    })?;
    mf.phase_s.insert("points".into(), t.elapsed().as_secs_f64());

    let t = Instant::now();
    let spills = s.segments.finish()?;
    mf.segment_rows = pool.install(|| {
        spills
            .par_iter()
            .map(|(hour, spill)| {
                let out_file = hour_dir(stage, *hour).join("part-0.parquet");
                let props = writer_props(&ss, opts.zstd_level, 65_536, kv.clone());
                let n = sort_spill(spill, &out_file, props, |b| grid.apply_segments(b, day0), |b| {
                    let (h, t0) = (column::<UInt64Type>(b, "hilbert"), column::<Int64Type>(b, "t0_ms"));
                    permutation(b.num_rows(), |i| (h.value(i), t0.value(i)))
                })?;
                Ok(n as u64)
            })
            .sum::<anyhow::Result<u64>>()
    })?;
    mf.phase_s.insert("segments".into(), t.elapsed().as_secs_f64());

    let moved = grid.applied.load(Ordering::Relaxed);
    let mut counts = s.alt_counts;
    counts[AltSource::Baro as usize] -= moved;
    counts[AltSource::BaroNeighbour as usize] += moved;
    for (code, n) in counts.iter().enumerate() {
        mf.alt_source_points.insert(format!("{:?}", AltSource::from_code(code as u8)), *n);
    }
    Ok(())
}
