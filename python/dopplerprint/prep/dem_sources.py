"""1-degree DEM source tiles: USGS 3DEP 1" (bare earth, NAVD88) with Copernicus GLO-30 (DSM, EGM2008) as the
fallback, downloaded once into a per-source cache directory."""

import math
import socket
from dataclasses import dataclass
from pathlib import Path

TDEP_URL = "https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1/TIFF/current/{n}/USGS_1_{n}.tif"
GLO30_URL = "https://copernicus-dem-30m.s3.amazonaws.com/{n}/{n}.tif"


@dataclass(frozen=True)
class Tile:
    """1x1 degree tile with south-west corner (lat, lon)."""

    lat: int
    lon: int

    def tdep_name(self) -> str:
        # 3DEP names tiles by their north-west corner.
        north = self.lat + 1
        ns = f"n{north:02d}" if north >= 0 else f"s{-north:02d}"
        ew = f"w{-self.lon:03d}" if self.lon < 0 else f"e{self.lon:03d}"
        return ns + ew

    def glo30_name(self) -> str:
        ns = f"N{self.lat:02d}" if self.lat >= 0 else f"S{-self.lat:02d}"
        ew = f"E{self.lon:03d}" if self.lon >= 0 else f"W{-self.lon:03d}"
        return f"Copernicus_DSM_COG_10_{ns}_00_{ew}_00_DEM"


@dataclass(frozen=True)
class SourceTile:
    path: Path
    source: str


def tiles_for_bbox(lon_w: float, lat_s: float, lon_e: float, lat_n: float) -> list[Tile]:
    return [
        Tile(la, lo)
        for la in range(math.floor(lat_s), math.ceil(lat_n))
        for lo in range(math.floor(lon_w), math.ceil(lon_e))
    ]


def force_ipv4() -> None:
    """This host's IPv6 route blackholes, which makes requests hang instead of falling back."""
    import urllib3.util.connection as uc

    uc.allowed_gai_family = lambda: socket.AF_INET


def _download(url: str, dst: Path) -> bool:
    """Download `url` to `dst`; returns False (and leaves a `.missing` marker) on HTTP 403/404."""
    import requests

    marker = dst.with_suffix(".missing")
    if dst.exists():
        return True
    if marker.exists():
        return False
    dst.parent.mkdir(parents=True, exist_ok=True)
    with requests.get(url, stream=True, timeout=60) as r:
        if r.status_code in (403, 404):
            marker.touch()
            return False
        r.raise_for_status()
        tmp = dst.with_suffix(".part")
        with open(tmp, "wb") as f:
            for chunk in r.iter_content(1 << 20):
                f.write(chunk)
        tmp.rename(dst)
    print(f"  downloaded {dst.name} ({dst.stat().st_size / 1e6:.0f} MB)")
    return True


def fetch(tile: Tile, cache: Path, sources: tuple[str, ...] = ("3dep", "glo30")) -> SourceTile | None:
    """First available source for `tile`, downloading into `cache/<source>/`; None for open ocean."""
    for src in sources:
        if src == "3dep":
            name = tile.tdep_name()
            path, url = cache / "3dep1" / f"USGS_1_{name}.tif", TDEP_URL.format(n=name)
        elif src == "glo30":
            name = tile.glo30_name()
            path, url = cache / "glo30" / f"{name}.tif", GLO30_URL.format(n=name)
        else:
            raise ValueError(f"unknown DEM source {src}")
        if _download(url, path):
            return SourceTile(path, src)
    return None
