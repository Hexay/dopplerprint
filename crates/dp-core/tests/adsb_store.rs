use std::path::{Path, PathBuf};

use dp_core::adsb::{IngestOptions, SegmentIndex, TrackQuery, ingest_day, load_tracks, manifest::list_days};
use dp_core::geo::ecef_to_geodetic;
use dp_core::track::EngineClass;

const DAY0: f64 = 1_788_825_600.0;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/adsb")
}

fn opts() -> IngestOptions {
    IngestOptions { typedb_dir: Some(fixtures().join("typedb")), batch_files: 3, ..IngestOptions::default() }
}

fn whole_day() -> TrackQuery {
    TrackQuery { t_start: DAY0 - 3600.0, t_end: DAY0 + 30.0 * 3600.0, bbox: None, max_alt_m: None, exclude_engines: vec![] }
}

fn trace_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { trace_files(&p, out) } else { out.push(p) }
    }
}

#[test]
fn ingest_then_load_roundtrip() {
    let store = tempfile::tempdir().unwrap();
    let mf = ingest_day(&fixtures().join("traces"), store.path(), &opts()).unwrap();
    assert_eq!((mf.date.as_str(), mf.trace_files, mf.aircraft, mf.parse_errors), ("2026-09-08", 8, 8, 0));
    assert!(mf.segment_rows > 0 && mf.coverage_rows > 0);
    assert_eq!(mf.typedb_types, 7);
    let day = store.path().join("date=2026-09-08");
    for f in ["manifest.json", "coverage.parquet", "points/part-080.parquet"] {
        assert!(day.join(f).is_file(), "{f}");
    }
    assert_eq!(list_days(store.path()).unwrap().len(), 1);

    let tracks = load_tracks(store.path(), &whole_day()).unwrap();
    assert_eq!(tracks.len() as u64, mf.tracks);
    assert_eq!(tracks.iter().map(|t| t.points.len() as u64).sum::<u64>(), mf.points_rows);
    for t in &tracks {
        assert!(t.points.windows(2).all(|w| w[1].t > w[0].t && w[1].t - w[0].t <= 60.0), "{}", t.id);
    }
    let engine_of = |hex: &str| tracks.iter().find(|t| t.id.contains(hex)).map(|t| t.engine);
    assert_eq!(engine_of("/a03a42-"), Some(EngineClass::Piston));
    assert_eq!(engine_of("/a62042-"), Some(EngineClass::Helicopter));
    assert_eq!(engine_of("/a27a35-"), Some(EngineClass::Turboprop));
    assert_eq!(engine_of("/a8ac42-"), Some(EngineClass::Jet));
    assert_eq!(engine_of("/~283635-"), Some(EngineClass::Unknown));

    let no_jets = TrackQuery { exclude_engines: vec![EngineClass::Jet], ..whole_day() };
    let props = load_tracks(store.path(), &no_jets).unwrap();
    assert!(props.iter().all(|t| t.engine != EngineClass::Jet) && props.len() < tracks.len());

    let c172 = tracks.iter().find(|t| t.id.contains("/a03a42-")).unwrap();
    let mid = c172.points[c172.points.len() / 2];
    let g = ecef_to_geodetic(mid.pos);
    let window = TrackQuery {
        t_start: mid.t - 30.0,
        t_end: mid.t + 30.0,
        bbox: Some((g.lat_deg - 0.02, g.lon_deg - 0.02, g.lat_deg + 0.02, g.lon_deg + 0.02)),
        ..whole_day()
    };
    let local = load_tracks(store.path(), &window).unwrap();
    let hit = local.iter().find(|t| t.id == c172.id).expect("C172 in its own bbox/time window");
    assert!(hit.t_start() <= window.t_start && hit.t_end() >= window.t_end);
    assert!(hit.points.len() < c172.points.len());
    let far = TrackQuery { bbox: Some((-10.0, 10.0, -9.0, 11.0)), ..window.clone() };
    assert!(load_tracks(store.path(), &far).unwrap().is_empty());
    let ceiling = TrackQuery { max_alt_m: Some(-1000.0), ..window };
    assert!(load_tracks(store.path(), &ceiling).unwrap().is_empty());
}

#[test]
fn split_tar_parts_match_directory() {
    let work = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    trace_files(&fixtures().join("traces"), &mut files);
    files.sort();
    let mut b = tar::Builder::new(Vec::new());
    for f in &files {
        let rel = f.strip_prefix(fixtures()).unwrap();
        b.append_path_with_name(f, Path::new("./").join(rel)).unwrap();
    }
    let bytes = b.into_inner().unwrap();
    let (aa, ab) = bytes.split_at(bytes.len() / 3);
    std::fs::write(work.path().join("day.tar.aa"), aa).unwrap();
    std::fs::write(work.path().join("day.tar.ab"), ab).unwrap();

    let from_tar = ingest_day(&work.path().join("day.tar.aa"), &work.path().join("s1"), &opts()).unwrap();
    let from_dir = ingest_day(&fixtures().join("traces"), &work.path().join("s2"), &opts()).unwrap();
    assert!(!from_tar.truncated_archive);
    assert_eq!(
        (from_tar.trace_files, from_tar.points_rows, from_tar.segment_rows, from_tar.coverage_rows),
        (from_dir.trace_files, from_dir.points_rows, from_dir.segment_rows, from_dir.coverage_rows)
    );

    std::fs::write(work.path().join("cut.tar"), &bytes[..bytes.len() * 2 / 3]).unwrap();
    let cut = ingest_day(&work.path().join("cut.tar"), &work.path().join("s3"), &opts()).unwrap();
    assert!(cut.truncated_archive && cut.trace_files < from_dir.trace_files && cut.trace_files > 0);
}

#[test]
fn segment_index_finds_nearby_legs() {
    let store = tempfile::tempdir().unwrap();
    ingest_day(&fixtures().join("traces"), store.path(), &opts()).unwrap();
    let tracks = load_tracks(store.path(), &whole_day()).unwrap();
    let idx = SegmentIndex::from_tracks(&tracks);
    assert_eq!(idx.len(), tracks.iter().map(|t| t.points.len() - 1).sum::<usize>());

    let (ti, tr) = tracks.iter().enumerate().find(|(_, t)| t.id.contains("/a03a42-")).unwrap();
    let a = tr.points[10];
    let b = tr.points[11];
    let p = (a.pos + b.pos) * 0.5;
    let hits = idx.within(p, 200.0, a.t, b.t);
    let first = hits.first().expect("leg under its own midpoint");
    assert_eq!((first.track, first.seg), (ti, 10));
    assert!(first.dist_m < 1.0 && (first.t_closest - (a.t + b.t) * 0.5).abs() < 0.5);
    assert!(hits.iter().all(|h| h.dist_m <= 200.0));
    assert!(idx.within(p, 200.0, a.t - 7200.0, a.t - 3600.0).iter().all(|h| h.track != ti));
    let g = ecef_to_geodetic(p);
    assert_eq!(idx.within_geodetic(g, 50.0, a.t, b.t).first().map(|h| h.track), Some(ti));
}
