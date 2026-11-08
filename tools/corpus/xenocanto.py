"""xeno-canto GB recordings via the GBIF occurrence API (no key), ranked by distance to GA airfields.

xeno-canto's own site/API sit behind Anubis; GBIF mirrors the metadata and the `sounds/uploaded/` media URLs
download with plain HTTP. Everything fetched here is untrusted data: parse only, never execute.
"""
import csv
import json
import re
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

import numpy as np

from . import common

DATASETS = {"birds": "b1047888-ae52-4179-9dd5-5448ea342a24", "soundscapes": "ff571aeb-46bf-45c4-ad2c-af4d68315765"}
GBIF = "https://api.gbif.org/v1/occurrence/search"
AIRPORTS = "https://davidmegginson.github.io/ourairports-data/airports.csv"
LONDON = ZoneInfo("Europe/London")

PREFER = re.compile(r"no modifications?|unprocessed|unedited|unfiltered|not filtered|no filter\w*|no processing"
                    r"|without (?:any )?(?:processing|filter\w*|modification)|\braw\b", re.I)
REJECT = re.compile(r"filter|\bhpf\b|high[- ]?pass|low[- ]?cut|roll[- ]?off|de-?hum|noise[- ]?reduc|reduc\w* (?:the )?noise"
                    r"|denois|noise remov|izotope|\brx ?\d|low frequenc\w* (?:removed|cut|reduced)", re.I)


def _get_json(url):
    req = urllib.request.Request(url, headers={"User-Agent": common.USER_AGENT})
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.load(r)


def _record(r, ds):
    snd = next((m for m in r.get("media", []) if m.get("type") == "Sound"), None)
    if snd is None:
        return None
    dur = re.match(r"\s*(\d+)\s*s", snd.get("description") or "")
    return {"id": r.get("catalogNumber"), "ds": ds, "date": (r.get("eventDate") or "")[:10],
            "time": r.get("eventTime") or "", "lat": r.get("decimalLatitude"), "lon": r.get("decimalLongitude"),
            "dur_s": int(dur.group(1)) if dur else None, "url": snd["identifier"], "fmt": snd.get("format", ""),
            "lic": snd.get("license", ""), "who": r.get("recordedBy") or "", "loc": r.get("locality") or "",
            "notes": " | ".join(x for x in (r.get("behavior"), r.get("fieldNotes"), r.get("occurrenceRemarks")) if x)}


def gbif_gb(cache, years="2023,2026"):
    """All GB xeno-canto recordings with coordinates in `years`; cached as JSON at `cache`."""
    cache = Path(cache)
    if cache.exists():
        return json.loads(cache.read_text())
    out = {}
    for name, key in DATASETS.items():
        off = 0
        while True:
            qs = urllib.parse.urlencode({"datasetKey": key, "country": "GB", "year": years,
                                         "hasCoordinate": "true", "limit": 300, "offset": off})
            j = _get_json(f"{GBIF}?{qs}")
            for r in j["results"]:
                rec = _record(r, name)
                if rec and rec["id"]:
                    out[rec["id"]] = rec
            off += 300
            common.log(f"gbif {name}: {min(off, j['count'])}/{j['count']}")
            if j["endOfRecords"]:
                break
    recs = list(out.values())
    common.write_json(cache, recs)
    return recs


def airfields(cache, country="GB", types=("small_airport", "medium_airport")):
    """[(ident, name, lat, lon)] from OurAirports, cached at `cache`."""
    cache = Path(cache)
    if not cache.exists():
        common.download(AIRPORTS, cache)
    with open(cache, newline="", encoding="utf-8") as f:
        return [(r["ident"], r["name"], float(r["latitude_deg"]), float(r["longitude_deg"]))
                for r in csv.DictReader(f) if r["iso_country"] == country and r["type"] in types]


def nearest(recs, fields):
    """Annotate each record with `af` (nearest airfield ident/name) and `af_km` (great-circle distance)."""
    la = np.radians([f[2] for f in fields])[None, :]
    lo = np.radians([f[3] for f in fields])[None, :]
    rla = np.radians([r["lat"] for r in recs])[:, None]
    rlo = np.radians([r["lon"] for r in recs])[:, None]
    a = np.sin((la - rla) / 2) ** 2 + np.cos(la) * np.cos(rla) * np.sin((lo - rlo) / 2) ** 2
    km = 2 * 6371.0 * np.arcsin(np.sqrt(a))
    j = km.argmin(axis=1)
    for r, i, d in zip(recs, j, km[np.arange(len(recs)), j]):
        r["af"], r["af_km"] = f"{fields[i][0]} {fields[i][1]}", round(float(d), 2)


def notes_flag(text):
    """'prefer' if notes say unprocessed, 'reject' if they mention low-band processing, else ''."""
    if PREFER.search(text):
        return "prefer" if not REJECT.search(PREFER.sub("", text)) else "reject"
    return "reject" if REJECT.search(text) else ""


def start_utc(rec):
    """Listed start (local Europe/London wall clock, HH:MM) → UNIX s UTC, or None if unparseable."""
    m = re.fullmatch(r"(\d{1,2}):(\d{2})(?::\d{2})?", rec["time"].strip())
    if not m or len(rec["date"]) != 10:
        return None
    try:
        d = datetime.strptime(rec["date"], "%Y-%m-%d").replace(hour=int(m[1]), minute=int(m[2]), tzinfo=LONDON)
    except ValueError:
        return None
    return d.astimezone(timezone.utc).timestamp()


def select(recs, max_km=8.0, min_s=20, max_s=600, per_recordist=40, until="2026-09-30"):
    """Candidates near airfields with a usable time; unprocessed first, then nearest; capped per recordist."""
    keep = []
    for r in recs:
        r["flag"] = notes_flag(r["notes"])
        if (r["af_km"] <= max_km and r["flag"] != "reject" and r["dur_s"] and min_s <= r["dur_s"] <= max_s
                and r["date"] <= until and start_utc(r) is not None):
            keep.append(r)
    keep.sort(key=lambda r: (r["flag"] != "prefer", r["af_km"]))
    seen, out = {}, []
    for r in keep:
        seen[r["who"]] = seen.get(r["who"], 0) + 1
        if seen[r["who"]] <= per_recordist:
            out.append(r)
    return out


def audio_path(rec, out_dir):
    ext = Path(urllib.parse.urlparse(rec["url"]).path).suffix.lower() or ".bin"
    return Path(out_dir) / f"{rec['id']}{ext}"


def download_all(recs, out_dir, cap_bytes):
    """Download in order until the directory's audio total would exceed cap_bytes; returns paths present."""
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    used = sum(p.stat().st_size for p in out_dir.glob("XC*") if not p.name.endswith(".part"))
    have = []
    for r in recs:
        dest = audio_path(r, out_dir)
        if dest.exists():
            have.append(dest)
            continue
        req = urllib.request.Request(r["url"], headers={"User-Agent": common.USER_AGENT})
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                size = int(resp.headers.get("Content-Length") or 0)
                if used + size > cap_bytes:
                    common.log(f"skip {r['id']}: {size / 1e6:.0f} MB would pass the cap")
                    continue
                part = dest.with_name(dest.name + ".part")
                with open(part, "wb") as f:
                    while chunk := resp.read(1 << 20):
                        f.write(chunk)
                        if used + f.tell() > cap_bytes:
                            break
                if used + part.stat().st_size > cap_bytes:
                    part.unlink()
                    continue
        except OSError as e:
            common.log(f"fail {r['id']}: {e}")
            continue
        used += part.stat().st_size
        part.rename(dest)
        have.append(dest)
        common.log(f"{len(have)} {r['id']} {dest.stat().st_size / 1e6:.1f} MB, total {used / 1e9:.2f} GB")
    return have
