"""Write the benchmark inputs (same audio, start window, search box and ADS-B day for both tools).

usage: python3 -I cases.py DATA_DIR PREP_DIR OUT.json [case_id ...]
Protocol (docs/research/bench_vs_dopplergeist_2026-11-03.md): start typed to the nearest minute, ±2 min;
a 10 km-radius box around a public place centre (synthetic: a point 5 km NW of truth).
"""
import gzip
import hashlib
import json
import math
import sys
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_CASES = [
    "dg_synthetic_known_minute", "dg_synthetic_two_events", "dg_multievent_3", "dg_multievent_4", "dg_multievent_6",
    "livermore_N172EL_R35", "livermore_N248LF_R14", "livermore_N4040_R12", "livermore_N413AK_R16",
    "livermore_N456CS_R38", "livermore_N788BM_R03", "livermore_N86LP_R01",
    "livermore_N500MG_R17", "livermore_N60RS_R11", "livermore_N748SP_R36",  # all 2026-05-17: one DG day index
    "oysterbay_20260908_colsto",
]
PLACES = {"livermore": ("Livermore, CA", 37.6819, -121.7680), "oysterbay": ("Oyster Bay, NY", 40.8715, -73.5318)}
RADIUS_KM, TOL_S = 10.0, 120.0
RAW = {"2026-05-16": "v2026.05.16-planes-readsb-prod-0", "2026-05-17": "v2026.05.17-planes-readsb-prod-0",
       "2026-09-08": "v2026.09.08-planes-readsb-staging-0"}
DEM_OURS = {"livermore": "dem/5070/livermore_30m.u16", "oysterbay": "dem/5070/oysterbay_glo30_30m.u16"}


def unix(s: str) -> float:
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def iso(t: float) -> str:
    return datetime.fromtimestamp(t, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def offset(lat, lon, north_km, east_km):
    return lat + north_km / 111.0, lon + east_km / (111.0 * math.cos(math.radians(lat)))


def bbox(lat, lon, r_km):
    s, w = offset(lat, lon, -r_km, -r_km)
    n, e = offset(lat, lon, r_km, r_km)
    return [round(w, 5), round(s, 5), round(e, 5), round(n, 5)]


def synth_icao(flight_id: str) -> str:
    return "f" + hashlib.sha1(flight_id.encode()).hexdigest()[:5]


def registrations(data: Path) -> dict:
    with gzip.open(data / "typedb/aircraft.csv.gz", "rt") as f:
        return {r[1]: r[0].lower() for r in (line.split(";") for line in f) if len(r) > 1 and r[1]}


def truth_of(t: dict, kind: str, regs: dict) -> dict:
    if kind == "livermore":
        w = [unix(x) for x in t["recording_start_window_utc"]]
        icaos = [regs.get(f["registration"]) for f in t["flight_ids"]]
    else:
        s = t.get("recording_start_unix") or unix(t["recording_start_utc"])
        w = [s, s]
        icaos = ([e["flight_icao"] for e in t["events"] if e.get("flight_icao")] if kind == "oysterbay"
                 else [synth_icao(f) for f in t["flight_ids"]])
    return {"lat": t["lat"], "lon": t["lon"], "start_window_unix": w, "icaos": icaos,
            "n_events": len(t["events"]), "flight_ids": t.get("flight_ids")}


def case_inputs(data: Path, prep: Path, cid: str, regs: dict) -> dict:
    cdir = data / "cases" / cid
    t = json.loads((cdir / "truth.json").read_text())
    kind = "synthetic" if cid.startswith("dg_") else cid.split("_")[0]
    truth = truth_of(t, kind, regs)
    audio = sorted((cdir / "audio").iterdir(), key=lambda p: p.suffix != ".mp3")[0]  # the MP3 if published
    w = truth["start_window_unix"]
    minute = round(0.5 * (w[0] + w[1]) / 60.0) * 60.0
    if kind == "synthetic":
        place, (clat, clon) = "5 km NW of truth", offset(t["lat"], t["lon"], 3.54, -3.54)
    else:
        place, clat, clon = PLACES[kind]
    day = iso(minute)[:10]
    dg_terrain = ({"rasters": [str(cdir.parent / "dg_multievent_3/dem/terrain.tif")],
                   "geoid_raster": str(cdir.parent / "dg_multievent_3/dem/geoid.tif")} if kind == "synthetic" else
                  {"rasters": sorted(str(p) for p in (cdir / "dem").glob("Copernicus_*.tif")),
                   "geoid_raster": str(data / "dem/geoid/us_nga_egm96_15.tif")})
    synth = kind == "synthetic"
    return {
        "id": cid, "kind": kind, "audio": str(audio), "place": place, "center": [round(clat, 5), round(clon, 5)],
        # ours searches circles (a box becomes its circumscribed circle); DG searches boxes: DG gets the circle's
        # bounding box, a superset (400 vs 314 km2 at 10 km)
        "radius_km": RADIUS_KM, "bbox_wsen": bbox(clat, clon, RADIUS_KM),
        "near_ours": f"{clat:.5f},{clon:.5f},{RADIUS_KM:g}",
        "start_minute_utc": iso(minute), "tolerance_s": TOL_S,
        "window_utc": [iso(minute - TOL_S), iso(minute + TOL_S)], "truth": truth,
        "flight_map": {f: synth_icao(f) for f in t["flight_ids"]} if synth else None,
        "ours": {"cache": str(prep / ("ours_cache_" + cid if synth else "ours_cache_real")),
                 "dem": str(prep / f"dem/{cid}.u16") if synth else str(data / DEM_OURS[kind]),
                 "no_weather": synth},
        "dg": {"flight_indexes": [str(prep / "dg_index" / (f"{cid}.sqlite" if synth else f"{day}.sqlite"))],
               "raw_day": None if synth else str(data / "_downloads/adsblol" / RAW[day]), "day": day,
               "terrain": dg_terrain},
    }


def main():
    data, prep, out, *ids = sys.argv[1:]
    data, prep = Path(data).resolve(), Path(prep).resolve()
    regs = registrations(data)
    cases = [case_inputs(data, prep, c, regs) for c in (ids or DEFAULT_CASES)]
    Path(out).write_text(json.dumps(cases, indent=1))
    for c in cases:
        print(c["id"], c["start_minute_utc"], c["bbox_wsen"], Path(c["audio"]).name, c["truth"]["icaos"])


if __name__ == "__main__":
    main()
