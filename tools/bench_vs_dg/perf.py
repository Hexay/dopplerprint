"""Matched-work speed/memory runs: both tools, 2 thread settings, N repeats, /usr/bin/time + perf stat.

usage: python3 -I perf.py RUN_DIR [REPS] [case ...]     (cases: ob synth livermore; tools/bench_vs_dg.sh perf)
Each run: measure.py (tree RSS, threads) -> /usr/bin/time -> perf stat -> CMD. Raw files under RUN_DIR/perf/.
Matched settings and their limits: docs/research/bench_vs_dopplergeist_2026-11-03.md (matched section).
"""
import json
import math
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

P = Path(__file__).resolve().parents[2]
B = P / "tools/bench_vs_dg"
DGPY = "/srv/agents/work/dg_upstream/.venv/bin/python"
DGBIN = "/srv/agents/work/dg_upstream/.venv/bin/dopplergeist"
PREP = P / "runs/bench-vs-dg/prep"
DP = str(PREP / "bin/dopplerprint")  # frozen build; commit in prep/bin/COMMIT
DG_REF = P / "data/_downloads/dopplergeist-extracted/data/source/reproduce-local.json"
AUTO = {"synth": "dg_multievent_3", "livermore": "livermore_N172EL_R35", "known_minute": "dg_synthetic_known_minute"}
PERF_RADIUS_KM, PERF_TOL_S = 5.0, 5.0
ONE_THREAD = {"RAYON_NUM_THREADS": "1", "OMP_NUM_THREADS": "1", "OPENBLAS_NUM_THREADS": "1", "MKL_NUM_THREADS": "1",
              "NUMEXPR_NUM_THREADS": "1", "DG_SEARCH_WORKERS": "1"}


def iso(t):
    return datetime.fromtimestamp(t, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def perf_case(c: dict) -> dict:
    """Accuracy-run inputs narrowed to the matched protocol: known start ±5 s, 5 km box, still air."""
    w = c["truth"]["start_window_unix"]
    s = round(0.5 * (w[0] + w[1]))
    lat, lon = c["center"]
    dlat, dlon = PERF_RADIUS_KM / 111.0, PERF_RADIUS_KM / (111.0 * math.cos(math.radians(lat)))
    c = json.loads(json.dumps(c))
    c.update(start_minute_utc=iso(s), tolerance_s=PERF_TOL_S, window_utc=[iso(s - PERF_TOL_S), iso(s + PERF_TOL_S)],
             bbox_wsen=[round(lon - dlon, 5), round(lat - dlat, 5), round(lon + dlon, 5), round(lat + dlat, 5)],
             radius_km=PERF_RADIUS_KM, near_ours=f"{lat:.5f},{lon:.5f},{PERF_RADIUS_KM:g}")
    c["ours"].update(no_weather=True, config=str(B / "perf_matched.toml"))
    if c["kind"] == "livermore":
        c["ours"]["dem"] = str(PREP / "dem/livermore_glo30_30m.u16")  # same GLO-30 source as DG's rasters
    c["dg"]["operator_extra"] = {"search_budget": {"spacing_m": 200.0, "refine_spacing_m": 25.0}}
    return c


def commands(case: str, tool: str, out: Path, inputs: Path, c: dict):
    if case == "ob":
        if tool == "ours":
            return [DP, "eval", "--experiment", str(B / "perf_ob_parity.toml"), "--out", str(out / "eval")]
        return [DGBIN, "run", str(DG_REF), "--output", str(out / "dg_run")]
    if tool == "ours":
        return [DP, "locate", c["audio"], "--start", c["start_minute_utc"], "--start-tolerance", f"{int(c['tolerance_s'])}s",
                f"--near={c['near_ours']}", "--cache", c["ours"]["cache"], "--dem", c["ours"]["dem"],
                "--out", str(out / "results"), "--json", "--quiet", "--no-weather", "--config", c["ours"]["config"]]
    return [DGPY, "-I", str(B / "run_dg.py"), str(inputs), c["id"], str(out), "3600"]


def run_one(cmd, out: Path, env_extra: dict, cwd: Path):
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, **env_extra)
    if "DG_SEARCH_WORKERS" not in env_extra:
        env["DG_SEARCH_WORKERS"] = "auto"
    wrapped = ["python3", "-I", str(B / "measure.py"), str(out / "measure.json"), str(out / "stdout.log"), "--",
               "/usr/bin/time", "-f", "%e %U %S %M", "-o", str(out / "time.txt"),
               "perf", "stat", "-e", "instructions,cycles", "-x,", "-o", str(out / "perf.csv"), "--", *cmd]
    (out / "cmd.json").write_text(json.dumps({"cmd": cmd, "env": env_extra, "cwd": str(cwd)}))
    subprocess.run(wrapped, env=env, cwd=cwd, check=False, stdout=subprocess.DEVNULL)
    return parse(out)


def parse(out: Path) -> dict:
    m = json.loads((out / "measure.json").read_text())
    r = {"exit": m["exit"], "tree_rss_mb": m["peak_tree_rss_mb"],
         "threads": m.get("peak_threads", 0) - 2, "procs": m.get("peak_procs", 0) - 2}  # minus time + perf
    try:
        t = (out / "time.txt").read_text().split()[-4:]
        r.update(wall_s=float(t[0]), cpu_s=round(float(t[1]) + float(t[2]), 2), max_rss_mb=round(int(t[3]) / 1024))
    except (OSError, ValueError, IndexError):
        pass
    try:
        for line in (out / "perf.csv").read_text().splitlines():
            f = line.split(",")
            if len(f) > 2 and f[2].split(":")[0] in ("instructions", "cycles"):
                r[f[2].split(":")[0]] = int(f[0]) if f[0].isdigit() else None
    except OSError:
        pass
    return r


def main():
    run, *rest = sys.argv[1:]
    reps = int(rest[0]) if rest else 3
    cases = rest[1:] or ["ob", "synth", "livermore"]
    root = Path(run) / "perf"
    root.mkdir(parents=True, exist_ok=True)
    acc = {c["id"]: c for c in json.loads((PREP / "inputs.json").read_text())}
    pin = [perf_case(acc[cid]) for cid in AUTO.values()]
    inputs = root / "perf_inputs.json"
    inputs.write_text(json.dumps(pin, indent=1))
    by_id = {c["id"]: c for c in pin}
    variants = [("ours", "default", {}), ("ours", "1thread", ONE_THREAD), ("dg", "default", {}), ("dg", "1thread", ONE_THREAD)]
    if os.environ.get("DG_PYTHON_FALLBACK"):
        variants.append(("dg", "default-pyfallback", {"DOPPLERGEIST_ACOUSTIC_BACKEND": "python", "DOPPLERGEIST_RIDGE_BACKEND": "python"}))
    log = root / "runs.jsonl"
    for case in cases:
        c = by_id.get(AUTO.get(case, ""), {})
        for rep in range(1, reps + 1):  # interleave tools within a repeat so load drift hits both
            for tool, mode, env in variants:
                out = root / case / f"{tool}-{mode}" / f"rep{rep}"
                if (out / "measure.json").exists():
                    continue
                r = run_one(commands(case, tool, out, inputs, c), out, env, P)
                rec = {"case": case, "tool": tool, "mode": mode, "rep": rep, **r}
                with log.open("a") as f:
                    f.write(json.dumps(rec) + "\n")
                print(json.dumps(rec), flush=True)


if __name__ == "__main__":
    main()
