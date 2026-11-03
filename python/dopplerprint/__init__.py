"""dopplerprint: locate a recording from aircraft Doppler curves matched to historical ADS-B."""

import json
import os

from dopplerprint import _core
from dopplerprint._core import __version__

__all__ = ["_core", "__version__", "locate"]


def locate(path, start=None, near=None, *, quiet=False, **opts):
    """Locate a recording; same pipeline and defaults as `dopplerprint locate` (see docs/USAGE.md).

    `start` is e.g. "2026-05-14 10:02 -07:00" (optionally "±10m"), `near` a place name, "lat,lon",
    "lat,lon,radius_km" or "west,south,east,north". Other options: radius_km, start_tolerance, out, cache,
    dem, mains_hz, offline, no_weather, yes, keep_raw, max_cache_gb, max_events. Downloads need yes=True.

    Returns the report dict: "status" is "found", "needs_input" (see "message") or "inconclusive".
    """
    req = {"audio": os.fspath(path), "start": start, "near": near, **opts}
    for k in ("out", "cache", "dem"):
        if req.get(k) is not None:
            req[k] = os.fspath(req[k])
    return json.loads(_core.locate_json(json.dumps(req), quiet))
