"""Give dopplerprint the synthetic cases' world: readsb traces of the finite track population (for
`dopplerprint ingest`) and a constant ground DEM. Run in the dg_upstream venv (pandas, rasterio), python -I.

usage: prep_synth.py INPUTS.json DATA_DIR PREP_DIR
DOPPLERGEIST's synthetic world has ground at 120 m with a zero geoid; dopplerprint adds the real geoid N to its
MSL DEM, so the DEM holds 120 - N to put the microphone at the same ellipsoidal height.
"""
import hashlib
import json
import math
import sys
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import pandas as pd
import rasterio
from rasterio.warp import transform_bounds

GROUND_ELLIPSOID_M, STEP_M, DEM_RADIUS_KM = 120.0, 200.0, 30.0
SCALE_M, OFFSET_M = 0.1, -100.0


def load_tracks(case_dir: Path) -> pd.DataFrame:
    pq = case_dir / "adsb/tracks.parquet"
    return pd.read_parquet(pq) if pq.exists() else pd.read_csv(case_dir / "adsb/tracks.csv")


def trace_rows(g: pd.DataFrame, day0: float) -> list:
    g = g.sort_values("time_s")
    t, lat, lon, alt = (g[c].to_numpy(float) for c in ("time_s", "lat", "lon", "alt_m"))
    north = np.gradient(lat, t) * 111_320.0
    east = np.gradient(lon, t) * 111_320.0 * np.cos(np.radians(lat))
    up = np.gradient(alt, t)
    gs_kt = np.hypot(east, north) / 0.514444
    trk = np.degrees(np.arctan2(east, north)) % 360.0
    ft, fpm = alt / 0.3048, up * 196.850
    return [[round(ti - day0, 2), round(la, 7), round(lo, 7), round(f), round(g_, 1), round(k, 1), 0,
             round(v), None, "adsb_icao", round(f), round(v), None, None]
            for ti, la, lo, f, g_, k, v in zip(t, lat, lon, ft, gs_kt, trk, fpm)]


def write_traces(case: dict, case_dir: Path, out: Path):
    tracks = load_tracks(case_dir)
    day0 = math.floor(tracks.time_s.min() / 86400.0) * 86400.0
    for fid, g in tracks.groupby("flight_id"):
        icao = "f" + hashlib.sha1(str(fid).encode()).hexdigest()[:5]  # same as cases.synth_icao
        tc = str(g.typecode.iloc[0]) if "typecode" in g else ""
        doc = {"icao": icao, "r": None, "t": tc, "timestamp": day0, "trace": trace_rows(g, day0)}
        d = out / icao[-2:]
        d.mkdir(parents=True, exist_ok=True)
        (d / f"trace_full_{icao}.json").write_text(json.dumps(doc))
    return tracks.flight_id.nunique()


def write_dem(case: dict, data: Path, stem: Path):
    lat, lon = case["truth"]["lat"], case["truth"]["lon"]
    with rasterio.open(data / "dem/geoid/us_nga_egm96_15.tif") as g:
        n = float(next(g.sample([(lon, lat)]))[0])
    dlat, dlon = DEM_RADIUS_KM / 111.0, DEM_RADIUS_KM / (111.0 * math.cos(math.radians(lat)))
    box = (lon - dlon, lat - dlat, lon + dlon, lat + dlat)
    xmin, ymin, xmax, ymax = transform_bounds("EPSG:4326", "EPSG:5070", *box, densify_pts=64)
    x0, x1 = math.floor(xmin / STEP_M) * STEP_M, math.ceil(xmax / STEP_M) * STEP_M
    y1, y0 = math.floor(ymin / STEP_M) * STEP_M, math.ceil(ymax / STEP_M) * STEP_M
    ncols, nrows = round((x1 - x0) / STEP_M) + 1, round((y0 - y1) / STEP_M) + 1
    h = GROUND_ELLIPSOID_M - n
    np.full((nrows, ncols), round((h - OFFSET_M) / SCALE_M), dtype="<u2").tofile(stem.with_suffix(".u16"))
    meta = {"format": "dp-dem-u16-v1", "crs": "EPSG:5070", "x0": x0, "y0": y0, "step_m": STEP_M,
            "ncols": ncols, "nrows": nrows, "scale_m": SCALE_M, "offset_m": OFFSET_M, "nodata": 0,
            "sources": [f"constant {GROUND_ELLIPSOID_M} m ellipsoidal (DOPPLERGEIST synthetic) - geoid {n:.2f} m"],
            "resampling": "none", "vertical_datum": "MSL", "bbox_lonlat": list(box),
            "layout": "row-major little-endian u16, row 0 north; node = (x0 + col*step, y0 - row*step)",
            "created_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "stats": {"valid_fraction": 1.0, "min_m": h, "max_m": h, "mean_m": h}}
    stem.with_suffix(".json").write_text(json.dumps(meta, indent=1))
    return n


def main():
    inputs, data, prep = sys.argv[1:]
    data, prep = Path(data), Path(prep)
    (prep / "dem").mkdir(parents=True, exist_ok=True)
    for case in json.loads(Path(inputs).read_text()):
        if case["kind"] != "synthetic":
            continue
        out = prep / "synth_traces" / case["id"]
        nf = write_traces(case, data / "cases" / case["id"], out)
        n = write_dem(case, data, Path(case["ours"]["dem"]).with_suffix(""))
        print(f"{case['id']}: {nf} flights -> {out}; DEM geoid N {n:.2f} m")


if __name__ == "__main__":
    main()
