"""NESO (GB) 1 s system frequency: monthly CSVs via CKAN, cached under data/enf/gb, loaded as numpy arrays.

CSV columns are `dtm,f` with dtm in UTC (verified: XC1146616 matches at 23:20 UTC = 00:20 BST).
"""
import functools
import json
import threading
import urllib.request
from datetime import datetime, timezone

import numpy as np

from . import common

PACKAGE = "cb1cc925-ecd8-4406-b021-3a3f368196e1"
CKAN = f"https://api.neso.energy/api/3/action/package_show?id={PACKAGE}"
ENF_GB = common.DATA / "enf" / "gb"


def resource_urls():
    """{(year, month): csv_url} from CKAN, cached as index.json (delete it to pick up new months)."""
    idx = ENF_GB / "index.json"
    if idx.exists():
        return {tuple(map(int, k.split("-"))): v for k, v in json.loads(idx.read_text()).items()}
    req = urllib.request.Request(CKAN, headers={"User-Agent": common.USER_AGENT})
    with urllib.request.urlopen(req, timeout=120) as r:
        res = json.load(r)["result"]["resources"]
    out = {}
    for r in res:
        name = r["url"].rsplit("/", 1)[-1]
        if name.startswith("fnew-") and name.endswith(".csv"):
            y, m = name[5:-4].split("-")
            out[(int(y), int(m))] = r["url"]
    ENF_GB.mkdir(parents=True, exist_ok=True)
    common.write_json(idx, {f"{y}-{m}": u for (y, m), u in sorted(out.items())})
    return out


def month_csv(year, month):
    """Download fnew-YYYY-M.csv if missing (urllib follows the 302 to object storage)."""
    dest = ENF_GB / f"fnew-{year}-{month}.csv"
    if dest.exists() and dest.stat().st_size > 0:
        return dest
    url = resource_urls().get((year, month))
    if url is None:
        raise KeyError(f"NESO has no frequency CSV for {year}-{month:02d}")
    return common.download(url, dest)


def _parse_csv(path):
    t, f = [], []
    with open(path) as fh:
        for line in fh:
            a, _, b = line.strip().partition(",")
            if not b or a == "dtm":
                continue
            ts = datetime.fromisoformat(a[:19]).replace(tzinfo=timezone.utc).timestamp()
            t.append(ts)
            f.append(float(b))
    return np.asarray(t, np.int64), np.asarray(f, np.float32)


_LOCK = threading.Lock()


@functools.lru_cache(maxsize=4)
def _month_cached(year, month_):
    npz = ENF_GB / f"fnew-{year}-{month_}.npz"
    if npz.exists():
        z = np.load(npz)
        return z["t"], z["f"]
    t, f = _parse_csv(month_csv(year, month_))
    order = np.argsort(t, kind="stable")
    t, f = t[order], f[order]
    part = npz.with_name(npz.name + ".part")
    with open(part, "wb") as fh:
        np.savez(fh, t=t, f=f)
    part.rename(npz)
    return t, f


def month(year, month_):
    """(t_unix int64, f_hz float32) for one month; parsed once into a sibling .npz. Thread-safe."""
    with _LOCK:
        return _month_cached(year, month_)


def _months(t0, t1):
    d = datetime.fromtimestamp(t0, timezone.utc).replace(day=1, hour=0, minute=0, second=0, microsecond=0)
    end = datetime.fromtimestamp(t1, timezone.utc)
    while d <= end:
        yield d.year, d.month
        d = d.replace(year=d.year + d.month // 12, month=d.month % 12 + 1)


def load(t0, t1):
    """1 s GB grid frequency over [t0, t1] UNIX s UTC → (t_unix float64, f_hz float64)."""
    ts, fs = [], []
    for y, m in _months(t0, t1):
        t, f = month(y, m)
        k = (t >= t0) & (t <= t1)
        ts.append(t[k])
        fs.append(f[k])
    return np.concatenate(ts).astype(np.float64), np.concatenate(fs).astype(np.float64)


def match(trace_t, trace_f, frame_s, t_guess, span_s=3600, sep_s=120, near_s=300):
    """ENF-match a hum trace (audio-relative frame centres, Hz) against the grid over start lags ±span_s.

    Grid is box-averaged over frame_s to match the detector's frame. Returns None if the grid has gaps;
    else best lag (s, true start = t_guess + lag), rms at best, the best within ±near_s of the listed
    time, the best rms ≥ sep_s from the overall best (ambiguity), and the median rms over all lags (chance).
    """
    trace_t, trace_f = np.asarray(trace_t, float), np.asarray(trace_f, float)
    lo = int(t_guess - span_s - frame_s - 2)
    hi = int(t_guess + span_s + trace_t.max() + frame_s + 2)
    gt, gf = load(lo, hi)
    if len(gt) < 0.99 * (hi - lo):
        return None
    grid = np.interp(np.arange(lo, hi + 1), gt, gf)
    w = max(1, int(round(frame_s)))
    smooth = np.convolve(grid, np.ones(w) / w, mode="same")
    lags = np.arange(-span_s, span_s + 1)
    idx = np.rint(t_guess - lo + trace_t)[None, :].astype(int) + lags[:, None]
    rms = np.sqrt(np.mean((trace_f[None, :] - smooth[idx]) ** 2, axis=1))
    b = int(rms.argmin())
    far = np.abs(lags - lags[b]) >= sep_s
    near = np.abs(lags) <= near_s
    n = int(np.flatnonzero(near)[rms[near].argmin()])
    return {"lag_s": int(lags[b]), "rms_mhz": 1e3 * float(rms[b]), "median_mhz": 1e3 * float(np.median(rms)),
            "near_lag_s": int(lags[n]), "near_rms_mhz": 1e3 * float(rms[n]),
            "alt_lag_s": int(lags[far][rms[far].argmin()]), "alt_rms_mhz": 1e3 * float(rms[far].min()),
            "grid_mean_hz": float(grid[idx[b]].mean())}
