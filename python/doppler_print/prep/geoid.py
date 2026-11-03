"""Crop PROJ-data's EGM2008 2.5' grid (us_nga_egm08_25.tif) to CONUS into the compact asset read by
`dp_core::dem::geoid`. Binary layout: see docs/GEO.md "Geoid asset".

    python -I -m doppler_print.prep.geoid SRC.tif assets/geoid/egm2008_conus_2p5.bin
"""

import argparse
import struct
from pathlib import Path

import numpy as np

MAGIC = b"DPGEOID1"
LAT_S, LAT_N = 24.0, 50.0
LON_W, LON_E = -125.5, -66.0


def crop(src: Path) -> tuple[np.ndarray, float, float, float]:
    """Return (grid south-to-north, lat_south, lon_west, step_deg) at the source's node spacing."""
    import rasterio
    from rasterio.windows import Window

    with rasterio.open(src) as ds:
        if ds.tags().get("AREA_OR_POINT") != "Point":
            raise SystemExit("expected a pixel-is-point geoid grid")
        step = ds.res[0]
        # GDAL shifts PixelIsPoint rasters by half a pixel, so pixel centres are the grid nodes.
        lon0 = ds.transform.c + step / 2
        lat0 = ds.transform.f - step / 2
        c0, c1 = round((LON_W - lon0) / step), round((LON_E - lon0) / step)
        r0, r1 = round((lat0 - LAT_N) / step), round((lat0 - LAT_S) / step)
        data = ds.read(1, window=Window(c0, r0, c1 - c0 + 1, r1 - r0 + 1)).astype(np.float64)
        lat_s, lon_w = lat0 - r1 * step, lon0 + c0 * step
    return data[::-1].copy(), lat_s, lon_w, step


def encode(grid: np.ndarray) -> tuple[np.ndarray, float, float]:
    offset = float(np.floor(grid.min()))
    scale = 0.001 if grid.max() - offset < 65.5 else 0.002
    q = np.round((grid - offset) / scale)
    assert q.max() <= 65535, "undulation range too wide for u16"
    return q.astype("<u2"), scale, offset


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src", type=Path)
    ap.add_argument("dst", type=Path)
    a = ap.parse_args()
    grid, lat_s, lon_w, step = crop(a.src)
    q, scale, offset = encode(grid)
    nrows, ncols = q.shape
    a.dst.parent.mkdir(parents=True, exist_ok=True)
    with open(a.dst, "wb") as f:
        f.write(MAGIC + struct.pack("<dddIIff", lat_s, lon_w, step, nrows, ncols, scale, offset))
        f.write(q.tobytes())
    err = np.abs(q * scale + offset - grid).max()
    print(f"{a.dst}: {nrows}x{ncols} nodes, lat {lat_s:.4f}+, lon {lon_w:.4f}+, step {step * 60:.2f}', "
          f"N {grid.min():.2f}..{grid.max():.2f} m, scale {scale} m, max quantisation {err * 1000:.2f} mm, "
          f"{a.dst.stat().st_size / 1e6:.2f} MB")


if __name__ == "__main__":
    main()
