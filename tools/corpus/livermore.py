"""Pathak Livermore 40-passage set (Zenodo 23424278) -> one case per passage, with a trilaterated recorder.

The dataset publishes no recorder coordinate. Each result page gives L, the horizontal offset between the recorder
and the ADS-B ground track at closest approach (final_results_40_cases.csv: L_ADSB_m). The recorder is fitted as the
single point whose horizontal distance to every track best matches L (robust least squares). Per-passage residuals
are kept in truth.json so a passage recorded elsewhere stands out.
"""
import csv
import json
import re
import urllib.request
import zipfile
from datetime import datetime
from pathlib import Path

from . import sources as S
from .common import CASES, DOWNLOADS, log, download, place, utc_iso, write_json
from .dem import tile_path

ZDIR = DOWNLOADS / f"zenodo-{S.ZENODO_PATHAK}"
API = f"https://zenodo.org/api/records/{S.ZENODO_PATHAK}"
REF_LAT, REF_LON = 37.68, -121.77


def fetch():
    with urllib.request.urlopen(API, timeout=60) as r:
        rec = json.load(r)
    (ZDIR / "record.json").parent.mkdir(parents=True, exist_ok=True)
    (ZDIR / "record.json").write_text(json.dumps(rec, indent=1))
    for f in rec["files"]:
        dest = download(f["links"]["self"], ZDIR / f["key"], md5sum=f["checksum"].split(":", 1)[1])
        if dest.suffix == ".zip":
            _unzip(dest, ZDIR / "extracted")


def _unzip(path, dest):
    marker = dest / f".done-{path.name}"
    if marker.exists():
        return
    with zipfile.ZipFile(path) as z:
        for i in z.infolist():
            name = Path(i.filename)
            if i.is_dir() or "__MACOSX" in name.parts or name.is_absolute() or ".." in name.parts:
                continue
            out = dest / name
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(z.read(i))
    marker.write_text("ok\n")


def _iso(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def read_track(kml):
    text = Path(kml).read_text()
    when = [_iso(w) for w in re.findall(r"<when\s*>([^<]+)</when>", text)]
    coords = [tuple(map(float, c.split())) for c in re.findall(r"<gx:coord\s*>([^<]+)</gx:coord>", text)]
    if len(when) != len(coords):
        raise ValueError(f"{kml}: when/coord mismatch")
    return when, coords


def _enu(lat, lon):
    import numpy as np
    k = 111_320.0
    return (np.asarray(lon) - REF_LON) * k * np.cos(np.radians(REF_LAT)), (np.asarray(lat) - REF_LAT) * 110_574.0


def _hdist(px, py, xs, ys):
    """Minimum horizontal distance from (px, py) to the polyline (xs, ys); also returns the segment index."""
    import numpy as np
    ax, ay, bx, by = xs[:-1], ys[:-1], xs[1:], ys[1:]
    dx, dy = bx - ax, by - ay
    t = np.clip(((px - ax) * dx + (py - ay) * dy) / np.maximum(dx * dx + dy * dy, 1e-9), 0, 1)
    d = np.hypot(ax + t * dx - px, ay + t * dy - py)
    i = int(np.argmin(d))
    return d[i], i, t[i]


def trilaterate(passages):
    import numpy as np
    from scipy.optimize import least_squares
    tracks = []
    for p in passages:
        when, coords = read_track(p["kml"])
        w = np.array(when)
        sel = np.abs(w - p["t_ref"]) < 300
        c = np.array(coords)[sel]
        x, y = _enu(c[:, 1], c[:, 0])
        tracks.append((x, y, c[:, 2], p["L"]))

    def resid(v):
        return np.array([_hdist(v[0], v[1], x, y)[0] - L for x, y, _, L in tracks])

    grid = [(gx, gy) for gx in range(-15000, 15001, 500) for gy in range(-15000, 15001, 500)]
    start = min(grid, key=lambda g: float(np.sum(np.abs(resid(g)))))
    fit = least_squares(resid, start, loss="soft_l1", f_scale=50.0)
    r = resid(fit.x)
    alts = []
    for (x, y, z, _), p in zip(tracks, passages):
        _, i, t = _hdist(fit.x[0], fit.x[1], x, y)
        alts.append(z[i] + t * (z[i + 1] - z[i]) - p["H"])
    lat = REF_LAT + fit.x[1] / 110_574.0
    lon = REF_LON + fit.x[0] / (111_320.0 * np.cos(np.radians(REF_LAT)))
    return dict(lat=float(lat), lon=float(lon), alt_msl_m_median=float(np.median(alts)),
                residuals_m=[float(v) for v in r], alt_estimates_m=[float(a) for a in alts],
                rms_m=float(np.sqrt(np.mean(r ** 2))), median_abs_m=float(np.median(np.abs(r))))


def build():
    fetch()
    rows = list(csv.DictReader(open(ZDIR / "final_results_40_cases.csv")))
    passages = []
    for row in rows:
        d = ZDIR / "extracted" / row["category"] / row["case"]
        freq = list(csv.DictReader(open(d / "frequency_vs_time_used.csv")))
        t_first, t_last = _iso(freq[0]["time_utc"]), _iso(freq[-1]["time_utc"])
        passages.append(dict(row=row, dir=d, kml=d / row["track_file"], L=float(row["L_ADSB_m"]),
                             H=float(row["H_ADSB_above_recorder_m"]), t_first=t_first, t_last=t_last,
                             t_ref=(t_first + t_last) / 2))
    first = trilaterate(passages)
    inliers = [p for p, r in zip(passages, first["residuals_m"]) if abs(r) < 150]
    outliers = [p for p, r in zip(passages, first["residuals_m"]) if abs(r) >= 150]
    groups = {"main": (inliers, trilaterate(inliers))}
    if len(outliers) >= 3:
        groups["outlier_group"] = (outliers, trilaterate(outliers))
    write_json(ZDIR / "trilateration.json", {k: dict(t, cases=[p["row"]["case"] for p in ps])
                                             for k, (ps, t) in groups.items()})
    for name, (ps, tri) in groups.items():
        log(f"livermore {name} {tri['lat']:.6f},{tri['lon']:.6f} rms {tri['rms_m']:.1f} m n={len(ps)}")
        for p, res, alt in zip(ps, tri["residuals_m"], tri["alt_estimates_m"]):
            _case(p, dict(tri, group=name, n=len(ps)), res, alt)


def _case(p, tri, res, alt):
    row, d = p["row"], p["dir"]
    day = row["received_start_utc"][:10]
    case = CASES / f"livermore_{row['case']}"
    place(d / "audio.wav", case / "audio/audio.wav")
    place(d / "frequency_vs_time_used.csv", case / "source/frequency_vs_time_used.csv")
    place(d / "result_page.pdf", case / "source/result_page.pdf")
    place(p["kml"], case / "adsb" / row["track_file"])
    for lat, lon in S.DEM_TILES["livermore"]:
        if tile_path(lat, lon).exists():
            place(tile_path(lat, lon), case / "dem" / tile_path(lat, lon).name)
    dg = DOWNLOADS / "dopplergeist-extracted/results/real-feature-benchmark"
    for suffix in (".json", ".csv", ".npz", ".png"):
        if (dg / f"{row['case']}{suffix}").exists():
            place(dg / f"{row['case']}{suffix}", case / "baseline/dopplergeist-release" / f"{row['case']}{suffix}")
    day_clip = CASES.parent / "adsb" / day / "livermore.parquet"
    dur = _wav_duration(d / "audio.wav")
    ok = abs(res) < 150
    write_json(case / "truth.json", dict(
        case_id=case.name, kind="real", lat=round(tri["lat"], 6), lon=round(tri["lon"], 6),
        recorder_alt_msl_m=round(tri["alt_msl_m_median"], 1), recorder_alt_this_passage_m=round(alt, 1),
        location_source=(f"trilaterated from the {tri['group']} group's {tri['n']} ADS-B tracks and published "
                         "ground-track offsets L "
                         f"(data/_downloads/zenodo-{S.ZENODO_PATHAK}/trilateration.json)"),
        location_confidence=(f"{'medium-high' if tri['group'] == 'main' and ok else 'low'}: "
                             f"{tri['group']} fit over {tri['n']} passages, rms {tri['rms_m']:.0f} m, "
                             f"this passage residual {res:.0f} m"
                             + ("" if tri["group"] == "main" else "; recorded away from the main site")),
        location_residual_m=round(res, 1),
        recording_start_window_utc=[utc_iso(p["t_last"] - dur), utc_iso(p["t_first"])],
        recording_start_note=("audio sample-zero UTC not published; window = ridge lies inside the clip. "
                              "Ridge timestamps in source/frequency_vs_time_used.csv are UTC."),
        audio_duration_s=round(dur, 3),
        events=[dict(name="passage", ridge_start_utc=utc_iso(p["t_first"]), ridge_end_utc=utc_iso(p["t_last"]),
                     timing_difference_s=float(row["timing_difference_s"]))],
        flight_ids=[dict(registration=row["case"].split("_")[0], track_file=f"adsb/{row['track_file']}",
                         altitude_basis=row["altitude_basis"])],
        publisher_adsb_values=dict(V_closest_kmh=float(row["V_ADSB_at_closest_kmh"]), L_m=p["L"],
                                   H_above_recorder_m=p["H"], R_min_m=float(row["R_ADSB_m"])),
        category=row["category"], flags=row["flags"] or None,
        background_adsb=(f"data/adsb/{day}/livermore.parquet" if day_clip.exists() else None),
        source_url=f"https://zenodo.org/records/{S.ZENODO_PATHAK}", license=S.PATHAK_LICENSE))


def _wav_duration(path):
    import wave
    with wave.open(str(path)) as w:
        return w.getnframes() / w.getframerate()
