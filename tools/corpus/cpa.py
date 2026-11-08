"""Closest approach of one ADS-B track to a fixed site, from the clipped region Parquet, and site heights.

Heights: track uses geometric (ellipsoidal) altitude when the track mostly carries it, else baro + undulation
(recorded as altitude_basis). Site height is Copernicus GLO-30 (EGM2008 MSL) + EGM2008 undulation.
"""
from datetime import datetime, timezone

from .common import SHARED_ADSB, SHARED_DEM
from .dem import tile_path

FT = 0.3048
MAX_GAP_S = 30.0
EGM2008 = SHARED_DEM / "egm2008" / "us_nga_egm08_25.tif"  # global 2.5' grid despite the PROJ-data name


def _sample(path, lat, lon):
    import rasterio
    with rasterio.open(path) as ds:
        return float(next(ds.sample([(lon, lat)]))[0])


def site_height(lat, lon):
    import math
    ground = _sample(tile_path(math.floor(lat), math.floor(lon)), lat, lon)
    n = _sample(EGM2008, lat, lon)
    return dict(ground_msl_m=round(ground, 2), geoid_undulation_m=round(n, 2), h_ell_m=round(ground + n, 2))


def _days(t_lo, t_hi):
    d0 = datetime.fromtimestamp(t_lo, tz=timezone.utc).date()
    d1 = datetime.fromtimestamp(t_hi, tz=timezone.utc).date()
    return sorted({d0.isoformat(), d1.isoformat()})


def load_track(icao, region, t_lo, t_hi):
    """Points of one aircraft in [t_lo, t_hi] as a dict of numpy arrays (sorted by time), or None."""
    import numpy as np
    import pyarrow.compute as pc
    import pyarrow.parquet as pq
    tables = []
    for day in _days(t_lo, t_hi):
        path = SHARED_ADSB / day / f"{region}.parquet"
        if not path.exists():
            raise FileNotFoundError(f"{path}: run adsb-fetch/adsb-clip for {day}")
        t = pq.read_table(path, columns=["icao", "t_unix", "lat", "lon", "alt_baro_ft", "alt_geom_ft"],
                          filters=[("icao", "=", icao.lower())])
        tables.append(t.filter(pc.and_(pc.greater_equal(t["t_unix"], t_lo), pc.less_equal(t["t_unix"], t_hi))))
    cols = {c: np.concatenate([t[c].to_numpy(zero_copy_only=False) for t in tables]).astype(float)
            for c in ("t_unix", "lat", "lon", "alt_baro_ft", "alt_geom_ft")}
    if len(cols["t_unix"]) < 2:
        return None
    order = np.argsort(cols["t_unix"], kind="stable")
    return {k: v[order] for k, v in cols.items()}


def _ecef(lat, lon, h):
    import numpy as np
    a, e2 = 6378137.0, 6.69437999014e-3
    la, lo = np.radians(lat), np.radians(lon)
    n = a / np.sqrt(1 - e2 * np.sin(la) ** 2)
    return np.stack([(n + h) * np.cos(la) * np.cos(lo), (n + h) * np.cos(la) * np.sin(lo),
                     (n * (1 - e2) + h) * np.sin(la)], axis=-1)


def closest_approach(track, site_lat, site_lon, site_h_ell, undulation, c=343.0, step_s=0.1):
    """CPA by linear interpolation between points no more than MAX_GAP_S apart. Returns (dict, None) or
    (None, reason) when the minimum is at a track end or inside a gap (no bracketed CPA)."""
    import numpy as np
    geom_ok = np.isfinite(track["alt_geom_ft"])
    basis = "geometric" if geom_ok.mean() >= 0.5 else "baro+undulation"
    alt = track["alt_geom_ft"] * FT if basis == "geometric" else track["alt_baro_ft"] * FT + undulation
    ok = np.isfinite(alt)
    t, lat, lon, alt = track["t_unix"][ok], track["lat"][ok], track["lon"][ok], alt[ok]
    if len(t) < 2:
        return None, "fewer than 2 points with altitude"
    grid = np.arange(t[0], t[-1], step_s)
    j = np.clip(np.searchsorted(t, grid, side="right") - 1, 0, len(t) - 2)
    covered = (t[j + 1] - t[j]) <= MAX_GAP_S
    p = _ecef(np.interp(grid, t, lat), np.interp(grid, t, lon), np.interp(grid, t, alt))
    s = _ecef(np.array(site_lat), np.array(site_lon), np.array(site_h_ell))
    r = np.where(covered, np.linalg.norm(p - s, axis=1), np.inf)
    k = int(np.argmin(r))
    if not np.isfinite(r[k]):
        return None, "no gap-free track span"
    if k in (0, len(grid) - 1) or not (covered[max(k - 1, 0)] and covered[min(k + 1, len(grid) - 1)]):
        return None, "range minimum at a track end or gap (CPA not bracketed)"
    up = s / np.linalg.norm(s)
    h_above = float(np.dot(p[k] - s, up))
    return dict(t_emit_unix=float(grid[k]), slant_m=float(r[k]), height_above_site_m=h_above,
                horiz_m=float(np.sqrt(max(r[k] ** 2 - h_above ** 2, 0.0))),
                t_receive_unix=float(grid[k] + r[k] / c), altitude_basis=basis, points=int(len(t))), None
