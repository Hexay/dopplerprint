"""Summarise perf.py runs: median (min-max) per case/tool/thread mode, plus the work each tool reported.

usage: python3 -I perf_summary.py PERF_DIR   -> PERF_DIR/perf_summary.{json,md}
"""
import glob
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

METRICS = ("wall_s", "cpu_s", "max_rss_mb", "tree_rss_mb", "instructions", "cycles", "threads", "procs")


def load(p):
    try:
        return json.loads(Path(p).read_text())
    except (OSError, ValueError):
        return None


def work_ours(rep: Path) -> dict:
    r = load(rep / "results/result.json")
    if r:
        loc = (r.get("posterior") or {}).get("answer") or r.get("location") or {}
        return {"cells_scored": r.get("cells_scored"), "tracks": r.get("tracks_considered"),
                "events": r.get("events_used"), "status": r.get("status"), "lat": loc.get("lat"), "lon": loc.get("lon")}
    s = load(rep / "eval/summary.json")
    if s:
        c = s["results"][0]
        return {"cells_scored": c.get("cells"), "hypotheses": c.get("hypotheses"), "tracks": c.get("n_tracks"),
                "events": c.get("n_events"), "status": c.get("status"), "error_m": c.get("error_m")}
    return {}


def work_dg(rep: Path) -> dict:
    r = load(rep / "dg_run/report.json")
    if r:
        grid = rep / "dg_run/coarse-grid.csv"
        return {"coarse_points": sum(1 for _ in grid.open()) - 1 if grid.exists() else None,
                "coarse_pairs": r["coarse_filter_counts"], "scoring_pairs": r["scoring_filter_counts"],
                "ranked_points": r.get("ranked_points"), "flight_combinations": r.get("flight_combinations"),
                "model_support": {k: v for k, v in r.get("model_support", {}).items() if isinstance(v, (int, float))},
                "best": {k: r["best"].get(k) for k in ("lat", "lon") if k in r.get("best", {})}}
    res = load(rep / "dg_result.json")
    if not res:
        return {}
    out = {"status": f"{res['status']}/{res.get('reason_code', '')}", "events": len(res.get("events") or [])}
    job = Path(res.get("job_dir", ""))
    tiles = [(load(t) or {}).get("payload", {}) for t in glob.glob(str(job / "search/tile-*.json"))
             if not t.endswith(".error.json")]
    out["tiles"] = len(tiles)
    for key in ("hypotheses_examined", "cpa_rows", "candidates", "work_units", "evaluated_trials", "rejected_trials",
                "unknown_trials"):
        out["tile_" + key] = sum(t.get("counts", {}).get(key) or 0 for t in tiles)
    rep_json = load(job / "search/search-report.json") or {}
    for key in ("grid_points_searched", "refinement_points_searched", "refinement_points_skipped", "event_groups_proposed",
                "elapsed_s", "process_peak_rss_mib", "bounded_concurrency"):
        out[key] = rep_json.get(key)
    if res.get("candidates"):
        out.update(lat=res["candidates"][0]["lat"], lon=res["candidates"][0]["lon"])
    return out


def fmt(vals):
    vals = [v for v in vals if isinstance(v, (int, float))]
    if not vals:
        return "-"
    med = statistics.median(vals)
    big = abs(med) >= 1e6
    f = (lambda x: f"{x / 1e9:.1f}G") if big else (lambda x: f"{x:.1f}" if isinstance(x, float) else str(x))
    return f"{f(med)} ({f(min(vals))}-{f(max(vals))})" if len(vals) > 1 else f(med)


def main():
    root = Path(sys.argv[1])
    groups = defaultdict(list)
    for line in (root / "runs.jsonl").read_text().splitlines():
        r = json.loads(line)
        groups[(r["case"], r["tool"], r["mode"])].append(r)
    summary, md = [], ["| case | tool | threads mode | n | exit | wall s | CPU s | max RSS MB | tree RSS MB | instr | cycles | threads | procs |",
                       "|---" * 13 + "|"]
    for (case, tool, mode), rs in sorted(groups.items()):
        rep1 = root / case / f"{tool}-{mode}" / "rep1"
        work = (work_ours if tool == "ours" else work_dg)(rep1)
        row = {"case": case, "tool": tool, "mode": mode, "n": len(rs), "exits": [r["exit"] for r in rs],
               **{m: [r.get(m) for r in rs] for m in METRICS}, "work": work}
        summary.append(row)
        md.append(f"| {case} | {tool} | {mode} | {len(rs)} | {','.join(map(str, row['exits']))} | "
                  + " | ".join(fmt(row[m]) for m in METRICS) + " |")
    md += ["", "Work reported (rep1):", ""] + [f"- {r['case']} {r['tool']} {r['mode']}: `{json.dumps(r['work'])}`" for r in summary]
    (root / "perf_summary.json").write_text(json.dumps(summary, indent=1))
    (root / "perf_summary.md").write_text("\n".join(md) + "\n")
    print("\n".join(md[:len(summary) + 2]))


if __name__ == "__main__":
    main()
