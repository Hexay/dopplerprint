"""AeroSonicDB YPAD-0523 (Zenodo 8371595) -> one case per propeller/rotor clip, truth from the labelled hex's
adsb.lol track (data/adsb/<day>/adelaide.parquet; run adsb-fetch/adsb-clip first).

WAVs come out of the 1.2 GB audio.zip by HTTP Range (remote_zip). Clip start = sample_meta date+time as
Australia/Adelaide wall clock; a clip is kept only if the labelled hex's CPA sound arrival falls inside it.
f0 comes from `dopplerprint discover` (target/fastrel); the hum verdict from `dopplerprint hum --json`.
Needs rasterio for site heights: add `--with rasterio` to the fetch_corpus uv command.
"""
import csv
import json
import subprocess
import urllib.request
from datetime import datetime, timezone
from zoneinfo import ZoneInfo

from . import sources as S
from .common import CASES, DOWNLOADS, PROJECT, download, log, place, utc_iso, write_json
from .cpa import closest_approach, load_track, site_height
from .dem import tile_path
from .remote_zip import central_directory, read_member

ZDIR = DOWNLOADS / f"zenodo-{S.ZENODO_AEROSONIC}"
RECORD = f"https://zenodo.org/records/{S.ZENODO_AEROSONIC}"
API = f"https://zenodo.org/api/records/{S.ZENODO_AEROSONIC}"
META = ["sample_meta.csv", "locations.json", "aircraft_meta.csv", "README.md", "LICENSE.txt"]
BIN = PROJECT / "target" / "fastrel" / "dopplerprint"
REGION = "adelaide"
TRACK_PAD_S = 300.0
F0_MATCH_S = 5.0
LOCATION_CONFIDENCE = "site coords are a nearby road per dataset README, <= ~200 m"


def fetch_meta():
    with urllib.request.urlopen(API, timeout=60) as r:
        rec = json.load(r)
    files = {f["key"]: f for f in rec["files"]}
    for name in META:
        f = files[name]
        download(f["links"]["self"], ZDIR / name, md5sum=f["checksum"].split(":", 1)[1])
    return files["audio.zip"]["links"]["self"]


def select(rows):
    return [r for r in rows if r["class"] == "1" and r["engtype"] in S.AEROSONIC_ENGINES
            and r["location"] in S.AEROSONIC_LOCATIONS and r["date"] in S.AEROSONIC_DATES]


def start_unix(row):
    local = datetime.strptime(f"{row['date']} {row['time']}", "%Y-%m-%d %H-%M-%S")
    aware = local.replace(tzinfo=ZoneInfo(S.AEROSONIC_TZ))
    return aware.timestamp(), aware.utcoffset().total_seconds() / 3600


def fetch_wav(zip_url, cd, name):
    dest = ZDIR / "audio" / name
    if not dest.exists():
        key = next(k for k in cd if k.endswith("/" + name) or k == name)
        data = read_member(zip_url, cd[key])
        if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
            raise ValueError(f"{name}: not a RIFF/WAVE member")
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.with_suffix(".part").write_bytes(data)
        dest.with_suffix(".part").rename(dest)
    return dest


def _wav_info(path):
    import wave
    with wave.open(str(path)) as w:
        return w.getnframes() / w.getframerate(), w.getframerate()


def _run_json(args, out=None):
    p = subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True, timeout=600)
    if p.returncode != 0:
        raise RuntimeError(f"dopplerprint {args[0]} failed: {p.stderr.strip()[-300:]}")
    return json.loads(out.read_text() if out else p.stdout)


def pick_f0(events, cpa_s):
    """Strongest discovered event within F0_MATCH_S of the ADS-B CPA: (f0_hz, detail) or (None, reason)."""
    near = [e for e in events if abs(e["fit"]["best"]["t_cpa_arrive"] - cpa_s) <= F0_MATCH_S]
    if not near:
        got = ", ".join(f"{e['fit']['best']['t_cpa_arrive']:.1f}" for e in events) or "none"
        return None, f"no discovered event within {F0_MATCH_S:.0f} s of ADS-B CPA {cpa_s:.1f} s (events at: {got})"
    e = max(near, key=lambda e: e["snr"])
    h = e.get("ridge_harmonic") or 1
    b = e["fit"]["best"]
    return b["f0"] / h, dict(event_id=e["id"], ridge_f0_hz=b["f0"], ridge_harmonic=h, snr=e["snr"],
                             t_cpa_arrive_s=b["t_cpa_arrive"], d_cpa_m=b["d_cpa"], v_mps=b["v"])


def _hum_verdict(h):
    keep = ("present", "nominal_hz", "detect_frac", "ambiguous", "harmonics")
    return {k: h.get(k) for k in keep} | {"rate_status": (h.get("rate") or {}).get("status"), "tool": "dopplerprint hum"}


def _locations():
    text = (ZDIR / "locations.json").read_text().strip()
    # v1.1.1 ships locations.json without its final closing brace.
    return json.loads(text if text.count("{") == text.count("}") else text + "}")


def build():
    zip_url = fetch_meta()
    rows = select(list(csv.DictReader(open(ZDIR / "sample_meta.csv", newline=""))))
    sites = {k: (v["lattitude"], v["longitude"]) for k, v in _locations().items()}
    heights = {k: site_height(*v) for k, v in sites.items() if k in {r["location"] for r in rows}}
    cd = central_directory(zip_url)
    log(f"aerosonic: {len(rows)} prop/rotor clips selected")
    built, skipped = [], []
    for row in rows:
        reason = _case(row, sites[row["location"]], heights[row["location"]], zip_url, cd)
        (skipped if reason else built).append((row["filename"], reason))
        if reason:
            log("skip", row["filename"], reason)
    write_json(ZDIR / "build_log.json", dict(built=[b for b, _ in built], skipped=dict(skipped)))
    log(f"aerosonic: built {len(built)}, skipped {len(skipped)}")


def _case(row, site, height, zip_url, cd):
    t0, offset_h = start_unix(row)
    wav = fetch_wav(zip_url, cd, row["filename"])
    dur, rate = _wav_info(wav)
    track = load_track(row["hex_id"], REGION, t0 - TRACK_PAD_S, t0 + dur + TRACK_PAD_S)
    if track is None:
        return "no ADS-B track for the labelled hex near the clip"
    cpa, why = closest_approach(track, *site, height["h_ell_m"], height["geoid_undulation_m"])
    if cpa is None:
        return why
    cpa_s = cpa["t_receive_unix"] - t0
    if not 0.0 <= cpa_s <= dur:
        return f"CPA sound arrival at {cpa_s:.1f} s lies outside the {dur:.1f} s clip"
    stamp = datetime.fromtimestamp(t0, tz=timezone.utc)
    local = row["date"].replace("-", "") + "_" + row["time"].replace("-", "")
    case = CASES / f"aerosonic_L{row['location']}_{row['hex_id'].upper()}_{local}"
    audio = place(wav, case / "audio" / row["filename"])
    (case / "source").mkdir(parents=True, exist_ok=True)
    events = _run_json(["discover", audio, "--out", case / "source/discover.json"], case / "source/discover.json")
    hum = _run_json(["hum", audio, "--json"])
    write_json(case / "source/hum.json", hum)
    f0, f0_detail = pick_f0(events, cpa_s)
    for lat, lon in S.DEM_TILES["aerosonic"]:
        if tile_path(lat, lon).exists():
            place(tile_path(lat, lon), case / "dem" / tile_path(lat, lon).name)
    event = dict(name="passage", cpa_receive_s=round(cpa_s, 3),
                 window_s=[round(max(0.0, cpa_s - 30), 3), round(min(dur, cpa_s + 30), 3)],
                 **({"f0_hz": round(f0, 3), "f0_source": dict(method="dopplerprint discover", **f0_detail)}
                    if f0 else {"f0_missing": f0_detail}),
                 flight_icao=row["hex_id"].lower(), aircraft_type=row["typedesig"], engtype=row["engtype"],
                 altitude_ft=float(row["altitude"]), audible_s=[float(row["offset"]),
                                                                 float(row["offset"]) + float(row["duration"])],
                 cpa_receive_utc=utc_iso(cpa["t_receive_unix"]), cpa_emit_utc=utc_iso(cpa["t_emit_unix"]),
                 cpa_slant_m=round(cpa["slant_m"], 1), cpa_horiz_m=round(cpa["horiz_m"], 1),
                 cpa_height_above_site_m=round(cpa["height_above_site_m"], 1),
                 altitude_basis=cpa["altitude_basis"], track_points=cpa["points"], c_mps=343.0)
    write_json(case / "truth.json", dict(
        case_id=case.name, kind="real", lat=site[0], lon=site[1],
        location_source=f"AeroSonicDB locations.json, location {row['location']} (sample_meta location column)",
        location_confidence=LOCATION_CONFIDENCE, site_height=height,
        date_utc=stamp.date().isoformat(), recording_start_utc=utc_iso(t0), recording_start_unix=t0,
        recording_start_source=(f"sample_meta date+time as {S.AEROSONIC_TZ} local (UTC{offset_h:+.1f} h); "
                                "verified: labelled hex CPA arrival inside the clip"),
        utc_offset_h=offset_h, audio=dict(file=f"audio/{row['filename']}", duration_s=round(dur, 3),
                                          sample_rate=rate),
        events=[event], hum=_hum_verdict(hum),
        aerosonic={k: row[k] for k in ("filename", "train-test", "fold", "location", "mic", "session", "subclass",
                                       "reg", "typedesig", "engtype", "engnum", "model", "file_length")},
        background_adsb=f"data/adsb/{stamp.date().isoformat()}/{REGION}.parquet",
        source_url=RECORD, license=S.AEROSONIC_LICENSE))
    return None
