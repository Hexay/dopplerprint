"""Scout xeno-canto GB clips near GA airfields for grid hum (ENF vs NESO) and aircraft passes.

Run: uv run --no-project --with numpy -- python -I tools/hum_scout.py STAGE [...]
Stages: select (GBIF + OurAirports → candidates.json), download (≤ --cap-gb), screen (hum, discover, ENF;
resumable results.jsonl), report (results.csv + results.md). Audio is untrusted: decoded by ffmpeg, read by
dopplerprint, never executed.
"""
import argparse
import csv
import json
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from corpus import common, neso, xenocanto  # noqa: E402

SCRATCH = Path("/tmp/claude-1001/-srv-agents-work-doppler-print/83679991-c801-44ae-bacc-c65574465eb6/scratchpad/humsrc")
BIN = Path(__file__).resolve().parents[1] / "target" / "fastrel" / "dopplerprint"
PASS_P = 0.5
GB_NOMINAL = 50.0
ENF_MAX_MHZ = 15.0


def stage_select(a):
    recs = xenocanto.gbif_gb(a.meta / "gbif_gb.json")
    xenocanto.nearest(recs, xenocanto.airfields(a.meta / "airports.csv"))
    sel = xenocanto.select(recs, max_km=a.max_km, per_recordist=a.per_recordist)
    common.write_json(a.meta / "candidates.json", sel)
    near = sum(r["af_km"] <= a.max_km for r in recs)
    rej = sum(r["af_km"] <= a.max_km and r["flag"] == "reject" for r in recs)
    pref = sum(r["flag"] == "prefer" for r in sel)
    common.log(f"{len(recs)} GB recs; {near} ≤ {a.max_km} km of an airfield; {rej} rejected by notes; "
               f"{len(sel)} selected ({pref} 'unprocessed'), {sum(r['dur_s'] for r in sel) / 3600:.1f} h")


def stage_download(a):
    sel = json.loads((a.meta / "candidates.json").read_text())
    have = xenocanto.download_all(sel, a.audio, int(a.cap_gb * 1e9))
    common.log(f"{len(have)} clips in {a.audio}")


def _run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=1800)


def _as_wav(src, tmp):
    if src.suffix.lower() == ".wav":
        return src
    dst = Path(tmp) / (src.stem + ".wav")
    p = _run(["ffmpeg", "-v", "error", "-nostdin", "-y", "-i", str(src), "-ac", "1", str(dst)])
    return dst if p.returncode == 0 else None


def _hum_summary(h):
    valid = [p for p in h.get("trace", []) if p.get("valid")]
    return {"present": bool(h.get("present")), "nominal": h.get("nominal_hz"), "f0": h.get("fundamental_hz"),
            "k": " ".join(str(x["k"]) for x in h.get("harmonics", [])), "snr_db": h.get("snr_db"),
            "detect": h.get("detect_frac"), "wander": h.get("wander_hz"), "frame_s": h.get("frame_s"),
            "ambiguous": h.get("ambiguous"), "rate": (h.get("rate") or {}).get("status"),
            "notes": "; ".join(h.get("notes", [])), "n_valid": len(valid)}, valid


def is_gb_grid(hum):
    # nominal 50 alone admits 37.5 Hz lines explained as a 0.75 sample-rate ratio; require f0 near 50 Hz too
    return bool(hum["present"] and hum["nominal"] == GB_NOMINAL and abs(hum["f0"] - GB_NOMINAL) < 0.5)


def screen_one(rec, path):
    out = {"id": rec["id"], "file": path.name}
    with tempfile.TemporaryDirectory() as tmp:
        wav = _as_wav(path, tmp)
        if wav is None:
            return {**out, "error": "decode"}
        p = _run([str(BIN), "hum", str(wav), "--json"])
        if p.returncode != 0:
            return {**out, "error": "hum: " + p.stderr.strip()[-200:]}
        hum, valid = _hum_summary(json.loads(p.stdout))
        ev_path = Path(tmp) / "events.json"
        p = _run([str(BIN), "discover", str(wav), "--out", str(ev_path)])
        events = json.loads(ev_path.read_text()) if p.returncode == 0 and ev_path.exists() else None
    out.update(hum=hum)
    out["grid_gb"] = is_gb_grid(hum)
    if events is None:
        out["error"] = "discover: " + p.stderr.strip()[-200:]
    else:
        pp = sorted((e["pass_p"] for e in events), reverse=True)
        out.update(n_events=len(events), n_pass=sum(x >= PASS_P for x in pp), max_pass_p=pp[0] if pp else 0.0,
                   pass_t=[round(e["fit"]["best"]["t_cpa_arrive"], 1) for e in events
                           if e["pass_p"] >= PASS_P and (e.get("fit") or {}).get("best")])
    if out["grid_gb"] and len(valid) >= 2:
        t0 = xenocanto.start_utc(rec)
        try:
            out["enf"] = neso.match([v["t_s"] for v in valid], [v["f_hz"] for v in valid], hum["frame_s"], t0)
        except KeyError as e:
            out["enf"] = {"error": str(e)}
    return out


def stage_screen(a):
    sel = {r["id"]: r for r in json.loads((a.meta / "candidates.json").read_text())}
    res_path = a.meta / "results.jsonl"
    done = set()
    if res_path.exists():
        done = {json.loads(l)["id"] for l in res_path.read_text().splitlines() if l.strip()}
    todo = [(sel[p.stem], p) for p in sorted(a.audio.glob("XC*")) if p.stem in sel and p.stem not in done
            and not p.name.endswith(".part")]
    common.log(f"screening {len(todo)} clips ({len(done)} already done)")
    with open(res_path, "a") as f, ThreadPoolExecutor(a.jobs) as pool:
        for i, r in enumerate(pool.map(lambda x: screen_one(*x), todo), 1):
            f.write(json.dumps(r) + "\n")
            f.flush()
            if r.get("grid_gb") or i % 50 == 0:
                common.log(f"{i}/{len(todo)} {r['id']} grid_gb={r.get('grid_gb')} n_pass={r.get('n_pass')} "
                           f"enf={r.get('enf')}")


def _row(r, rec):
    h, e = r.get("hum") or {}, r.get("enf") or {}
    enf_ok = bool(e.get("rms_mhz") is not None and e["rms_mhz"] < min(ENF_MAX_MHZ, 0.3 * e["median_mhz"]))
    enf_unique = enf_ok and e["alt_rms_mhz"] >= 1.5 * e["rms_mhz"]
    return {"id": r["id"], "date": rec["date"], "time_local": rec["time"], "dur_s": rec["dur_s"], "loc": rec["loc"],
            "lat": rec["lat"], "lon": rec["lon"], "airfield": rec["af"], "af_km": rec["af_km"], "who": rec["who"],
            "notes_flag": rec["flag"], "hum": h.get("present"), "nominal": h.get("nominal"), "k": h.get("k"),
            "f0": h.get("f0") and round(h["f0"], 4), "snr_db": h.get("snr_db") and round(h["snr_db"], 1),
            "detect": h.get("detect") and round(h["detect"], 2), "n_valid": h.get("n_valid"),
            "rate": h.get("rate"), "grid_gb": bool(h) and is_gb_grid(h), "enf_lag_s": e.get("lag_s"),
            "enf_rms_mhz": e.get("rms_mhz") and round(e["rms_mhz"], 1),
            "enf_median_mhz": e.get("median_mhz") and round(e["median_mhz"], 1),
            "enf_alt_rms_mhz": e.get("alt_rms_mhz") and round(e["alt_rms_mhz"], 1), "enf_alt_lag_s": e.get("alt_lag_s"),
            "enf_near_lag_s": e.get("near_lag_s"), "enf_near_rms_mhz": e.get("near_rms_mhz") and round(e["near_rms_mhz"], 1),
            "enf_ok": enf_ok, "enf_unique": enf_unique, "n_events": r.get("n_events"), "n_pass": r.get("n_pass"),
            "max_pass_p": r.get("max_pass_p") and round(r["max_pass_p"], 2), "pass_t": r.get("pass_t"),
            "error": r.get("error") or e.get("error"), "url": rec["url"]}


def stage_report(a):
    sel = {r["id"]: r for r in json.loads((a.meta / "candidates.json").read_text())}
    rows = [_row(r, sel[r["id"]]) for r in map(json.loads, (a.meta / "results.jsonl").read_text().splitlines())]
    with open(a.meta / "results.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    hum = [r for r in rows if r["hum"]]
    gb = [r for r in rows if r["grid_gb"]]
    enf = [r for r in gb if r["enf_ok"]]
    prize = [r for r in gb if (r["n_pass"] or 0) > 0]
    lines = [f"# hum_scout: {len(rows)} xeno-canto GB clips near GA airfields", "",
             f"- screened {len(rows)} ({sum(r['dur_s'] or 0 for r in rows) / 3600:.1f} h); errors "
             f"{sum(bool(r['error']) for r in rows)}",
             f"- hum present {len(hum)}; GB grid (50 Hz) {len(gb)}; non-50 Hz (equipment) {len(hum) - len(gb)}",
             f"- ENF match (rms < {ENF_MAX_MHZ} mHz and < 0.3 × median over ±1 h) {len(enf)}; unique (next "
             f"minimum ≥ 120 s away is ≥ 1.5× worse) {sum(r['enf_unique'] for r in enf)}; best lag within ±300 s "
             f"of the listed time {sum(abs(r['enf_lag_s']) <= 300 for r in enf)}",
             f"- clips with any pass-like event (pass_p ≥ {PASS_P}) {sum((r['n_pass'] or 0) > 0 for r in rows)}",
             f"- GB-grid hum AND pass-like event {len(prize)}", "",
             "| XC | date | local | s | site | airfield (km) | k | f0 | snr | det | ENF lag s | rms/median mHz "
             "| ±300 s best (lag) | alt rms (lag) | passes (t_cpa) |", "|" + "---|" * 15]
    for r in sorted(gb, key=lambda r: -(r["n_pass"] or 0)):
        lines.append(f"| {r['id']} | {r['date']} | {r['time_local']} | {r['dur_s']} | {r['loc']} | {r['airfield']} "
                     f"({r['af_km']}) | {r['k']} | {r['f0']} | {r['snr_db']} | {r['detect']} | {r['enf_lag_s']} | "
                     f"{r['enf_rms_mhz']}/{r['enf_median_mhz']} | {r['enf_near_rms_mhz']} ({r['enf_near_lag_s']}) | "
                     f"{r['enf_alt_rms_mhz']} ({r['enf_alt_lag_s']}) | {r['n_pass']} {r['pass_t'] or ''} |")
    other = [r for r in hum if not r["grid_gb"]]
    if other:
        lines += ["", "Non-GB-grid hum: " + ", ".join(f"{r['id']} f0 {r['f0']} Hz k={r['k']}" for r in other)]
    (a.meta / "results.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines[:9]))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("stages", nargs="+", choices=["select", "download", "screen", "report"])
    ap.add_argument("--meta", type=Path, default=SCRATCH / "xc_gb_meta", help="candidates, results")
    ap.add_argument("--audio", type=Path, default=SCRATCH / "xc_gb", help="downloaded clips (untrusted)")
    ap.add_argument("--max-km", type=float, default=8.0)
    ap.add_argument("--per-recordist", type=int, default=40)
    ap.add_argument("--cap-gb", type=float, default=3.0)
    ap.add_argument("--jobs", type=int, default=3)
    a = ap.parse_args()
    a.meta.mkdir(parents=True, exist_ok=True)
    common.force_ipv4()
    for s in a.stages:
        globals()[f"stage_{s}"](a)


if __name__ == "__main__":
    main()
