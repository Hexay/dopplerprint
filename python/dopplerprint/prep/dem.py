"""Build an EPSG:5070 ground raster (`<name>.u16` + `<name>.json`) read by `dp_core::dem::Dem`.

Format, alignment and sources: docs/GEO.md. Examples:

    python -m dopplerprint.prep.dem --case data/cases/livermore_N86LP_R01 --radius-km 25
    python -m dopplerprint.prep.dem --bbox -125 24 -66 50 --step 100 --name conus_100m
    python -m dopplerprint.prep.dem --bbox -125 24 -66 50 --step 1000 --name conus_1000m --sources terrarium
"""

import argparse
import json
import math
import os
import time
from pathlib import Path

import numpy as np

from dopplerprint.prep import dem_sources, terrarium

TERRARIUM_ZOOM = 7

FORMAT = "dp-dem-u16-v1"
SCALE_M, OFFSET_M, NODATA = 0.1, -100.0, 0
STRIP_ROWS = 1024
DATA = Path(os.environ.get("DP_DATA", "data")) / "dem"


def case_bbox(case_dir: Path, radius_km: float) -> tuple[float, float, float, float]:
    truth = json.loads((case_dir / "truth.json").read_text())
    lat, lon = float(truth["lat"]), float(truth["lon"])
    dlat = radius_km / 111.0
    dlon = radius_km / (111.0 * math.cos(math.radians(lat)))
    return lon - dlon, lat - dlat, lon + dlon, lat + dlat


def grid_for_bbox(bbox, step: float) -> dict:
    """Node grid snapped to multiples of `step` covering the lon/lat bbox."""
    from rasterio.warp import transform_bounds

    xmin, ymin, xmax, ymax = transform_bounds("EPSG:4326", "EPSG:5070", *bbox, densify_pts=64)
    x0, x1 = math.floor(xmin / step) * step, math.ceil(xmax / step) * step
    y1, y0 = math.floor(ymin / step) * step, math.ceil(ymax / step) * step
    return {"x0": x0, "y0": y0, "step_m": step,
            "ncols": round((x1 - x0) / step) + 1, "nrows": round((y0 - y1) / step) + 1}


def encode(h: np.ndarray) -> np.ndarray:
    q = np.round((h - OFFSET_M) / SCALE_M)
    return np.where(np.isfinite(q), np.clip(q, 1, 65535), NODATA).astype("<u2")


def warp_strip(g: dict, r0: int, nrows: int, cache: Path, sources, resampling, used: set,
               mosaic: Path | None = None) -> np.ndarray:
    import rasterio
    from rasterio.transform import from_origin
    from rasterio.warp import reproject, transform_bounds

    s = g["step_m"]
    # Pixel-is-area transform whose pixel centres are the nodes.
    tr = from_origin(g["x0"] - s / 2, g["y0"] - r0 * s + s / 2, s, s)
    out = np.full((nrows, g["ncols"]), np.nan, dtype=np.float32)
    bounds = (g["x0"] - s, g["y0"] - (r0 + nrows) * s, g["x0"] + g["ncols"] * s, g["y0"] - r0 * s + s)
    if mosaic is not None:
        paths = [mosaic]
        used.add(f"terrarium:{mosaic.name}")
    else:
        paths = []
        for tile in dem_sources.tiles_for_bbox(*transform_bounds("EPSG:5070", "EPSG:4326", *bounds, densify_pts=64)):
            st = dem_sources.fetch(tile, cache, sources)
            if st is not None:
                used.add(f"{st.source}:{st.path.name}")
                paths.append(st.path)
    for path in paths:
        with rasterio.open(path) as src:
            reproject(rasterio.band(src, 1), out, src_nodata=src.nodata, dst_transform=tr, dst_crs="EPSG:5070",
                      dst_nodata=np.nan, resampling=resampling, init_dest_nodata=False)
    return out


def build(bbox, step: float, name: str, out_dir: Path, cache: Path, sources, resampling_name: str) -> Path:
    from rasterio.enums import Resampling

    g = grid_for_bbox(bbox, step)
    resampling = Resampling[resampling_name]
    out_dir.mkdir(parents=True, exist_ok=True)
    raw = out_dir / f"{name}.u16"
    print(f"{name}: {g['ncols']}x{g['nrows']} nodes at {step:g} m ({g['ncols'] * g['nrows'] * 2 / 1e6:.1f} MB)")
    mm = np.memmap(raw, dtype="<u2", mode="w+", shape=(g["nrows"], g["ncols"]))
    used: set[str] = set()
    mosaic = terrarium.mosaic(bbox, TERRARIUM_ZOOM, cache / "terrarium") if "terrarium" in sources else None
    for r0 in range(0, g["nrows"], STRIP_ROWS):
        n = min(STRIP_ROWS, g["nrows"] - r0)
        mm[r0:r0 + n] = encode(warp_strip(g, r0, n, cache, sources, resampling, used, mosaic))
    mm.flush()
    valid = mm[mm != NODATA]
    stats = {"valid_fraction": float(valid.size / mm.size)}
    if valid.size:
        h = valid.astype(np.float64) * SCALE_M + OFFSET_M
        stats |= {"min_m": float(h.min()), "max_m": float(h.max()), "mean_m": float(h.mean())}
    del mm
    header = {"format": FORMAT, "crs": "EPSG:5070", **g, "scale_m": SCALE_M, "offset_m": OFFSET_M,
              "nodata": NODATA, "sources": sorted(used), "resampling": resampling_name,
              "vertical_datum": "MSL: NAVD88 (3DEP) / EGM2008 (GLO-30) / mixed MSL (terrarium)",
              "bbox_lonlat": list(bbox),
              "layout": "row-major little-endian u16, row 0 north; node = (x0 + col*step, y0 - row*step)",
              "created_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "stats": stats}
    raw.with_suffix(".json").write_text(json.dumps(header, indent=1) + "\n")
    print(f"wrote {raw} ({', '.join(f'{k}={v:.3f}' for k, v in stats.items())})")
    return raw


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    where = ap.add_mutually_exclusive_group(required=True)
    where.add_argument("--bbox", nargs=4, type=float, metavar=("LON_W", "LAT_S", "LON_E", "LAT_N"))
    where.add_argument("--case", type=Path, help="case directory holding truth.json")
    ap.add_argument("--radius-km", type=float, default=25.0)
    ap.add_argument("--step", type=float, default=30.0, help="node spacing in metres (100 for CONUS)")
    ap.add_argument("--name", help="output stem (default: <case>_<step>m)")
    ap.add_argument("--out-dir", type=Path, default=DATA / "5070")
    ap.add_argument("--cache", type=Path, default=DATA, help="tiles go to <cache>/3dep1 and <cache>/glo30")
    ap.add_argument("--sources", default="3dep,glo30", help="preference order")
    ap.add_argument("--resampling", help="rasterio Resampling name (default bilinear <= 45 m, else average)")
    ap.add_argument("--ipv4", action="store_true", help="force IPv4 for downloads")
    a = ap.parse_args()
    if a.ipv4:
        dem_sources.force_ipv4()
    bbox = tuple(a.bbox) if a.bbox else case_bbox(a.case, a.radius_km)
    name = a.name or (f"{a.case.name}_{a.step:g}m" if a.case else "dem")
    resampling = a.resampling or ("bilinear" if a.step <= 45 else "average")
    build(bbox, a.step, name, a.out_dir, a.cache, tuple(a.sources.split(",")), resampling)


if __name__ == "__main__":
    main()
