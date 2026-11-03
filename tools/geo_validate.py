"""Validate the Rust Albers/geoid against pyproj. Needs the built extension (tools/dev.sh py) and pyproj.

    .venv/bin/python -I tools/geo_validate.py [--egm08-dir data/dem/egm2008] [--n 1000] [--refs]

`--refs` prints a few reference rows for the hard-coded Rust tests instead of validating.
"""

import argparse

import numpy as np
import pyproj

CONUS = (24.5, 49.4, -124.8, -66.9)


def egm08(lat, lon):
    t = pyproj.Transformer.from_crs("EPSG:4979", "EPSG:9518", always_xy=True)
    _, _, h = t.transform(lon, lat, np.zeros_like(lat))
    return -np.asarray(h)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--egm08-dir", default="/srv/agents/work/doppler_print/data/dem/egm2008")
    ap.add_argument("--n", type=int, default=1000)
    ap.add_argument("--refs", action="store_true")
    a = ap.parse_args()
    pyproj.datadir.append_data_dir(a.egm08_dir)
    alb = pyproj.Transformer.from_crs("EPSG:4269", "EPSG:5070", always_xy=True)

    if a.refs:
        lat = np.array([24.5, 40.876466, 49.4, 37.66327, 23.0, 45.5])
        lon = np.array([-124.8, -73.524951, -66.9, -121.7819, -96.0, -110.0])
        x, y = alb.transform(lon, lat)
        n = egm08(lat, lon)
        for row in zip(lat, lon, x, y, n):
            print("({:.6f}, {:.6f}, {:.4f}, {:.4f}, {:.4f}),".format(*row))
        return

    from doppler_print import _core

    rng = np.random.default_rng(5070)
    lat = rng.uniform(CONUS[0], CONUS[1], a.n)
    lon = rng.uniform(CONUS[2], CONUS[3], a.n)
    xr, yr = alb.transform(lon, lat)
    x, y = _core.albers_forward(lat, lon)
    fwd = np.hypot(x - xr, y - yr)
    la, lo = _core.albers_inverse(x, y)
    rt = np.maximum(np.abs(la - lat), np.abs(lo - lon))
    plo, pla = alb.transform(xr, yr, direction="INVERSE")
    ila, ilo = _core.albers_inverse(np.asarray(xr), np.asarray(yr))
    inv = np.maximum(np.abs(ila - pla), np.abs(ilo - plo))
    n_err = np.abs(_core.geoid_undulation(lat, lon) - egm08(lat, lon))
    print(f"albers forward vs pyproj: max {fwd.max():.2e} m, mean {fwd.mean():.2e} m (n={a.n})")
    print(f"albers inverse vs pyproj: max {inv.max():.2e} deg")
    print(f"albers round trip:        max {rt.max():.2e} deg")
    print(f"geoid vs pyproj EGM2008:  max {n_err.max() * 1e3:.2f} mm, mean {n_err.mean() * 1e3:.2f} mm")
    ok = fwd.max() < 1e-3 and rt.max() < 1e-9 and inv.max() < 1e-9 and n_err.max() < 0.01
    print("PASS" if ok else "FAIL")
    raise SystemExit(0 if ok else 1)


if __name__ == "__main__":
    main()
