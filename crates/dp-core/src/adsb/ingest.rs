//! Streaming day ingest: tar reader thread → rayon per-trace parse/clean → single writer (bucketed spill)
//! → `finalize`. Layout and schema: docs/DATA.md.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::sync_channel;
use std::time::Instant;

use anyhow::{Context, bail};
use rayon::prelude::*;

use super::clean::{CleanParams, CleanStats, clean_trace};
use super::coverage::{CovKey, CovVal, CoverageAgg, aircraft_coverage};
use super::finalize::{Spilled, finalize};
use super::manifest::{LICENCE, Manifest, civil_date, day_dir, points_bucket};
use super::model::{CleanTrack, engine_code, parse_icao};
use super::neighbour::{OffKey, OffsetAgg, trace_offsets};
use super::pieces::{Piece, cut_pieces};
use super::schema::{self, PointRow, SegRow, points_schema, segments_schema};
use super::source::{Archive, TraceFile};
use super::spill::BucketSpill;
use super::trace::parse_trace;
use super::typedb::{Classification, TypeDb};

#[derive(Clone, Debug)]
pub struct IngestOptions {
    pub typedb_dir: Option<PathBuf>,
    pub clean: CleanParams,
    /// Trace files handed to rayon per batch; bounds in-flight memory.
    pub batch_files: usize,
    pub zstd_level: i32,
    /// Buckets sorted and written concurrently at the end; each holds one bucket in memory.
    pub finalize_threads: usize,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            typedb_dir: default_typedb_dir(),
            clean: CleanParams::default(),
            batch_files: 512,
            zstd_level: 6,
            finalize_threads: 4,
        }
    }
}

/// `$DP_TYPEDB`, else `./data/typedb` when it exists.
pub fn default_typedb_dir() -> Option<PathBuf> {
    std::env::var_os("DP_TYPEDB").map(PathBuf::from).or_else(|| {
        let p = PathBuf::from("data/typedb");
        p.is_dir().then_some(p)
    })
}

struct Processed {
    day0: f64,
    cls: Classification,
    tracks: Vec<CleanTrack>,
    pieces: Vec<Piece>,
    cov: Vec<(CovKey, CovVal)>,
    offsets: Vec<(OffKey, f32)>,
    stats: CleanStats,
}

fn process(f: &TraceFile, db: &TypeDb, p: &CleanParams) -> anyhow::Result<Processed> {
    let raw = parse_trace(&f.bytes).with_context(|| f.name.clone())?;
    let icao = parse_icao(&raw.icao).with_context(|| format!("{}: bad icao {:?}", f.name, raw.icao))?;
    let cls = db.classify(icao, raw.t.as_deref(), raw.category());
    let day0 = (raw.timestamp / 86_400.0).floor() * 86_400.0;
    if cls.non_aircraft {
        let stats = CleanStats { points_in: raw.trace.len() as u64, ..CleanStats::default() };
        let (tracks, pieces, cov, offsets) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        return Ok(Processed { day0, cls, tracks, pieces, cov, offsets, stats });
    }
    let (tracks, stats) = clean_trace(&raw, icao, cls.type_code.clone(), cls.engine, p);
    let pieces = tracks.iter().flat_map(|t| cut_pieces(t, day0)).collect();
    let cov = aircraft_coverage(&tracks, day0);
    let offsets = trace_offsets(&raw, day0);
    Ok(Processed { day0, cls, tracks, pieces, cov, offsets, stats })
}

struct Writer {
    points: BucketSpill<PointRow>,
    segments: BucketSpill<SegRow>,
    coverage: CoverageAgg,
    offsets: OffsetAgg,
    clean: CleanStats,
    mf: Manifest,
    day0: Option<f64>,
    alt_counts: [u64; 4],
}

impl Writer {
    fn absorb(&mut self, pr: Processed) -> anyhow::Result<()> {
        if *self.day0.get_or_insert(pr.day0) != pr.day0 {
            self.mf.other_day_traces += 1;
            return Ok(());
        }
        self.clean.add(&pr.stats);
        if pr.cls.non_aircraft {
            self.mf.non_aircraft_traces += 1;
            return Ok(());
        }
        self.offsets.add(&pr.offsets);
        if pr.tracks.is_empty() {
            return Ok(());
        }
        self.mf.aircraft += 1;
        *self.mf.aircraft_by_engine.entry(format!("{:?}", pr.cls.engine)).or_default() += 1;
        for tr in pr.tracks {
            for p in tr.points {
                self.alt_counts[p.alt_source as usize] += 1;
                self.points.push(points_bucket(tr.icao), PointRow { icao: tr.icao, track_no: tr.track_no, p })?;
            }
        }
        let type_code: Option<Arc<str>> = pr.cls.type_code.as_deref().map(Arc::from);
        let (engine, engine_source) = (engine_code(pr.cls.engine), pr.cls.source as u8);
        for piece in pr.pieces {
            let hour = i64::from(piece.hour);
            self.segments.push(hour, SegRow { piece, engine, engine_source, type_code: type_code.clone() })?;
        }
        self.coverage.add_aircraft(&pr.cov);
        Ok(())
    }
}

/// Ingest one adsb.lol day (split tar parts, single tar, or extracted trace dir) into `out/date=YYYY-MM-DD/`.
pub fn ingest_day(archive: &Path, out: &Path, opts: &IngestOptions) -> anyhow::Result<Manifest> {
    let started = Instant::now();
    let db = match &opts.typedb_dir {
        Some(d) => TypeDb::load_dir(d)?,
        None => TypeDb::default(),
    };
    let arc = Archive::open(archive)?;
    let tmp = out.join(format!(".ingest-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let mut w = Writer {
        points: BucketSpill::new(&tmp, "points", points_schema(), schema::points_batch),
        segments: BucketSpill::new(&tmp, "segments", segments_schema(), schema::segments_batch),
        coverage: CoverageAgg::default(),
        offsets: OffsetAgg::default(),
        clean: CleanStats::default(),
        mf: Manifest::default(),
        day0: None,
        alt_counts: [0; 4],
    };
    let (tx, rx) = sync_channel::<Vec<TraceFile>>(2);
    let (arc_ref, batch_files) = (&arc, opts.batch_files.max(1));
    let src_stats = std::thread::scope(|s| -> anyhow::Result<_> {
        let reader = s.spawn(move || {
            let mut batch = Vec::with_capacity(batch_files);
            let st = arc_ref.for_each(|f| {
                batch.push(f);
                if batch.len() >= batch_files {
                    return Ok(tx.send(std::mem::take(&mut batch)).is_ok());
                }
                Ok(true)
            });
            if !batch.is_empty() {
                let _ = tx.send(batch);
            }
            drop(tx);
            st
        });
        for batch in rx {
            let results: Vec<_> = batch.par_iter().map(|f| process(f, &db, &opts.clean)).collect();
            drop(batch);
            for r in results {
                match r {
                    Ok(pr) => w.absorb(pr)?,
                    Err(_) => w.mf.parse_errors += 1,
                }
            }
        }
        reader.join().map_err(|_| anyhow::anyhow!("archive reader panicked"))?
    })?;
    let Some(day0) = w.day0 else { bail!("no readable traces in {}", archive.display()) };
    let mut mf = std::mem::take(&mut w.mf);
    mf.record_stats(&src_stats, &w.clean, &opts.clean);
    mf.schema = schema::SCHEMA_VERSION.into();
    mf.tool_version = env!("CARGO_PKG_VERSION").into();
    mf.licence = LICENCE.into();
    mf.archive = match &arc {
        Archive::Tar(p) | Archive::TraceDir(p) if p.len() <= 8 => p.iter().map(|x| x.display().to_string()).collect(),
        _ => vec![archive.display().to_string()],
    };
    mf.date = civil_date(day0);
    mf.day0 = day0;
    mf.typedb_files = db.files.iter().map(|p| p.display().to_string()).collect();
    (mf.typedb_types, mf.typedb_registry) = (db.types.len(), db.registry.len());
    drop(db);

    mf.phase_s.insert("stream".into(), started.elapsed().as_secs_f64());
    let stage = tmp.join("stage");
    let spilled = Spilled {
        points: w.points,
        segments: w.segments,
        coverage: w.coverage,
        offsets: w.offsets,
        alt_counts: w.alt_counts,
    };
    finalize(spilled, day0, &stage, opts, &mut mf)?;
    mf.elapsed_s = started.elapsed().as_secs_f64();
    std::fs::write(stage.join("manifest.json"), serde_json::to_vec_pretty(&mf)?)?;

    let final_dir = day_dir(out, &mf.date);
    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir)?;
    }
    std::fs::rename(&stage, &final_dir)?;
    std::fs::remove_dir_all(&tmp)?;
    Ok(mf)
}
