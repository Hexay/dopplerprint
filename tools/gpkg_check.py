"""Check a doppler_print GeoPackage: core tables via sqlite3, then (if pyogrio is installed) open it with GDAL.

    .venv/bin/python -I tools/gpkg_check.py target/demo_outputs/results.gpkg
"""

import sqlite3
import sys


def sqlite_checks(path: str) -> None:
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    q = lambda sql: db.execute(sql).fetchall()  # noqa: E731
    assert q("PRAGMA application_id")[0][0] == 0x47504B47, "application_id is not GPKG"
    print("integrity:", q("PRAGMA integrity_check")[0][0])
    print("srs:", [r[0] for r in q("SELECT srs_id FROM gpkg_spatial_ref_sys ORDER BY srs_id")])
    for t, dt, srs in q("SELECT table_name, data_type, srs_id FROM gpkg_contents ORDER BY table_name"):
        n = q(f'SELECT count(*) FROM "{t}"')[0][0]
        print(f"  {t:12s} {dt:10s} srs={srs} rows={n}")
    for row in q("SELECT table_name, geometry_type_name, srs_id, z, m FROM gpkg_geometry_columns ORDER BY 1"):
        t = row[0]
        rt = q(f'SELECT count(*) FROM "rtree_{t}_geom"')[0][0]
        print(f"  geom {row} rtree={rt}")
    print("styles:", q("SELECT f_table_name, useAsDefault, length(styleQML) FROM layer_styles ORDER BY 1"))


def gdal_checks(path: str) -> None:
    try:
        import pyogrio
    except ImportError:
        print("pyogrio not installed; skipped GDAL open")
        return
    print("GDAL", pyogrio.__gdal_version_string__)
    for name, gtype in pyogrio.list_layers(path):
        info = pyogrio.read_info(path, layer=name)
        crs = info["crs"]
        print(f"  {name:12s} {str(gtype):18s} n={info['features']:4d} crs={crs} "
              f"index={info['capabilities'].get('fast_spatial_filter')} fields={list(info['fields'])[:6]}")
    rows = pyogrio.read_dataframe(path, layer="candidates", read_geometry=False) if _has_pandas() else None
    if rows is not None:
        print(rows.head(3).to_string())


def _has_pandas() -> bool:
    try:
        import pandas  # noqa: F401
        return True
    except ImportError:
        return False


if __name__ == "__main__":
    sqlite_checks(sys.argv[1])
    gdal_checks(sys.argv[1])
