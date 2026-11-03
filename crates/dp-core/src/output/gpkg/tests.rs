use rusqlite::Connection;

use super::write;
use crate::output::sample::demo_results;

fn q<T: rusqlite::types::FromSql>(db: &Connection, sql: &str) -> T {
    db.query_row(sql, [], |r| r.get(0)).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

#[test]
fn demo_gpkg_is_well_formed() {
    let path = std::env::temp_dir().join(format!("dp_gpkg_test_{}.gpkg", std::process::id()));
    let r = demo_results();
    write(&path, &r).unwrap();
    let db = Connection::open(&path).unwrap();

    assert_eq!(q::<i64>(&db, "PRAGMA application_id"), 1196444487);
    assert_eq!(q::<i64>(&db, "PRAGMA user_version"), 10400);
    assert_eq!(q::<String>(&db, "PRAGMA integrity_check"), "ok");
    assert_eq!(q::<i64>(&db, "SELECT count(*) FROM gpkg_spatial_ref_sys WHERE srs_id IN (4326, 5070)"), 2);

    let contents: Vec<(String, String)> = db
        .prepare("SELECT table_name, data_type FROM gpkg_contents ORDER BY table_name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let names: Vec<&str> = contents.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(names, ["candidates", "cells", "events", "layer_styles", "regions", "tracks"]);
    assert!(contents.iter().all(|(t, d)| d == if matches!(t.as_str(), "events" | "layer_styles") { "attributes" } else { "features" }));

    assert_eq!(q::<i64>(&db, "SELECT srs_id FROM gpkg_geometry_columns WHERE table_name='cells'"), 5070);
    assert_eq!(q::<i64>(&db, "SELECT z FROM gpkg_geometry_columns WHERE table_name='tracks'"), 1);
    assert_eq!(q::<i64>(&db, "SELECT count(*) FROM gpkg_extensions WHERE extension_name='gpkg_rtree_index'"), 4);
    assert_eq!(q::<i64>(&db, "SELECT count(*) FROM rtree_cells_geom"), r.cells.len() as i64);
    assert_eq!(q::<i64>(&db, "SELECT count(*) FROM layer_styles WHERE useAsDefault=1"), 4);
    assert_eq!(q::<i64>(&db, "SELECT count(*) FROM candidates WHERE score_norm=1.0 AND rank=1"), 1);

    let (lon, lat) = (-73.524951, 40.876466);
    let hits: i64 = db
        .query_row(
            "SELECT count(*) FROM rtree_candidates_geom WHERE minx <= ?1 AND maxx >= ?1 AND miny <= ?2 AND maxy >= ?2",
            (lon, lat),
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hits, 1);
    std::fs::remove_file(&path).ok();
}
