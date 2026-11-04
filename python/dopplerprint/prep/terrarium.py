"""AWS Terrain Tiles (Terrarium PNG, Web Mercator, metres MSL) mosaicked into one GeoTIFF for `prep.dem`.

Coarse national source for the region-free search (docs/GEO.md "National DEM"). Zoom 7 is ~0.9 km/px at
40° N; the lower 48 (-125 24 -66 50) is 286 tiles. Tiles: https://registry.opendata.aws/terrain-tiles/.
"""

import math
from pathlib import Path

import numpy as np

URL = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png"
HALF_WORLD = 20037508.342789244
PX = 256


def tile_xy(lon: float, lat: float, z: int) -> tuple[int, int]:
    n = 2**z
    lat_r = math.radians(lat)
    x = int((lon + 180.0) / 360.0 * n)
    y = int((1.0 - math.asinh(math.tan(lat_r)) / math.pi) / 2.0 * n)
    return min(max(x, 0), n - 1), min(max(y, 0), n - 1)


def decode(png: Path) -> np.ndarray:
    """Terrarium RGB → metres: R·256 + G + B/256 − 32768, floored at 0 (sea floor → water surface)."""
    import rasterio

    with rasterio.open(png) as src:
        r, g, b = (src.read(i).astype(np.float32) for i in (1, 2, 3))
    # Tiles carry bathymetry; a recorder on water sits at ~0 MSL. Costs ≤ 86 m below-sea-level land.
    return np.maximum(r * 256.0 + g + b / 256.0 - 32768.0, 0.0)


def fetch(z: int, x: int, y: int, cache: Path) -> Path:
    import requests

    p = cache / str(z) / f"{x}_{y}.png"
    if not p.exists():
        p.parent.mkdir(parents=True, exist_ok=True)
        r = requests.get(URL.format(z=z, x=x, y=y), timeout=60)
        r.raise_for_status()
        p.with_suffix(".part").write_bytes(r.content)
        p.with_suffix(".part").rename(p)
    return p


def mosaic(bbox, z: int, cache: Path) -> Path:
    """One EPSG:3857 GeoTIFF covering the lon/lat bbox at zoom `z`; reused when present."""
    import rasterio
    from rasterio.transform import from_origin

    lon_w, lat_s, lon_e, lat_n = bbox
    (x0, y0), (x1, y1) = tile_xy(lon_w, lat_n, z), tile_xy(lon_e, lat_s, z)
    out = cache / f"z{z}_{x0}_{y0}_{x1}_{y1}.tif"
    if out.exists():
        return out
    h = np.full(((y1 - y0 + 1) * PX, (x1 - x0 + 1) * PX), np.nan, dtype=np.float32)
    for x in range(x0, x1 + 1):
        for y in range(y0, y1 + 1):
            h[(y - y0) * PX:(y - y0 + 1) * PX, (x - x0) * PX:(x - x0 + 1) * PX] = decode(fetch(z, x, y, cache))
    size = 2 * HALF_WORLD / 2**z
    tr = from_origin(-HALF_WORLD + x0 * size, HALF_WORLD - y0 * size, size / PX, size / PX)
    profile = {"driver": "GTiff", "height": h.shape[0], "width": h.shape[1], "count": 1, "dtype": "float32",
               "crs": "EPSG:3857", "transform": tr, "nodata": np.nan, "compress": "deflate"}
    with rasterio.open(out.with_suffix(".part.tif"), "w", **profile) as dst:
        dst.write(h, 1)
    out.with_suffix(".part.tif").rename(out)
    print(f"terrarium z{z}: {(x1 - x0 + 1) * (y1 - y0 + 1)} tiles -> {out}")
    return out
