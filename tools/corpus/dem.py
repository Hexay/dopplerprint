"""Copernicus GLO-30 tiles from the public AWS bucket into data/dem/glo30, linked into each case group."""
import urllib.error

from . import sources as S
from .common import SHARED_DEM, download, log, write_json


def tile_name(lat, lon):
    ns, ew = ("N" if lat >= 0 else "S"), ("E" if lon >= 0 else "W")
    return f"Copernicus_DSM_COG_10_{ns}{abs(lat):02d}_00_{ew}{abs(lon):03d}_00_DEM"


def tile_path(lat, lon):
    return SHARED_DEM / "glo30" / f"{tile_name(lat, lon)}.tif"


def fetch():
    missing = []
    for group, tiles in S.DEM_TILES.items():
        for lat, lon in tiles:
            name = tile_name(lat, lon)
            try:
                download(S.COPERNICUS_URL.format(name=name), tile_path(lat, lon))
            except urllib.error.HTTPError as e:
                # Tiles that are entirely ocean are absent from the bucket.
                log("DEM missing", name, e.code)
                missing.append(name)
    write_json(SHARED_DEM / "glo30" / "SOURCE.json", dict(
        source="s3://copernicus-dem-30m (https://copernicus-dem-30m.s3.amazonaws.com)",
        license=S.COPERNICUS_LICENSE, heights="EGM2008 orthometric (m); see geoid/ for EGM96 used by DOPPLERGEIST",
        groups={g: [tile_name(*t) for t in ts] for g, ts in S.DEM_TILES.items()}, absent_tiles=missing))
