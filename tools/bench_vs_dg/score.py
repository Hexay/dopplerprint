"""Score a bench_vs_dg run: per-case metrics for both tools, aggregates -> results.json, results.tsv, summary.md.

usage: python3 -I score.py RUN_DIR
Status mapping: ours found -> found, inconclusive with a location -> ambiguous, without -> failed;
DG succeeded -> found, inconclusive with candidates -> ambiguous, without -> failed. Crash -> failed.
Success = found and error <= 1000 m (DOPPLERGEIST's stated acceptance radius).
"""
import hashlib
import json
import math
import sys
from datetime import datetime
from pathlib import Path

SUCCESS_M = 1000.0
TOOLS = ("ours", "dg")


def dist_m(lat1, lon1, lat2, lon2):
    p1, p2 = math.radians(lat1), math.radians(lat2)
    a = math.sin((p2 - p1) / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(math.radians(lon2 - lon1) / 2) ** 2
    return 2 * 6371008.8 * math.asin(math.sqrt(a))


def unix(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def load(p: Path):
    try:
        return json.loads(p.read_text())
    except (OSError, ValueError):
        return None


def stdout_json(p: Path):
    try:
        text = p.read_text(errors="replace")
        return json.loads(text[text.index("{"):text.rindex("}") + 1])
    except (OSError, ValueError):
        return None


def tail(p: Path, n=3):
    try:
        return " | ".join(p.read_text(errors="replace").strip().splitlines()[-n:])[:300]
    except OSError:
        return ""


def inside_ring(lat, lon, ring):
    inside = False
    for (x1, y1), (x2, y2) in zip(ring, ring[1:] + ring[:1]):
        if (y1 > lat) != (y2 > lat) and lon < x1 + (lat - y1) * (x2 - x1) / (y2 - y1):
            inside = not inside
    return inside


def ours_region(results: Path, lat, lon, area_km2):
    """The 90% region is the cell set flagged in_cr90 (the 'region' polygons only outline the top modes)."""
    gj = load(results / "result.geojson")
    if not gj or area_km2 is None:
        return None, None
    cells = [f for f in gj["features"] if f["properties"].get("kind") == "cell"]
    hit = [f for f in cells if inside_ring(lat, lon, f["geometry"]["coordinates"][0])]
    return bool(hit and hit[0]["properties"].get("in_cr90")), f"{area_km2:.2f} km2"


def clock_err(start, window):
    if start is None:
        return None
    lo, hi = window
    return 0.0 if lo <= start <= hi else (start - hi if start > hi else start - lo)


def score_ours(d: Path, truth):
    m = load(d / "measure.json") or {}
    rep = load(d / "results/result.json") or stdout_json(d / "stdout.log")
    r = {"exit": m.get("exit"), "wall_s": m.get("wall_s"), "rss_mb": m.get("peak_tree_rss_mb")}
    if not rep:
        return dict(r, status="failed", raw="crash", note=tail(d / "stdout.log"))
    raw = rep["status"]
    post = rep.get("posterior")
    loc = (post or {}).get("answer") if raw == "found" and post else rep.get("location")
    st = {"found": "found", "needs_input": "needs_input"}.get(raw, "ambiguous" if loc else "failed")
    r.update(status=st, raw=raw, note=(rep.get("message") or "")[:200], events=rep.get("events_used"),
             conf=(rep.get("confidence") or {}).get("level"))
    if loc:
        r["lat"], r["lon"] = loc["lat"], loc["lon"]
        r["region_km2"] = (post or {}).get("area_km2")
        r["in_region"], r["region"] = ours_region(d / "results", truth["lat"], truth["lon"], r["region_km2"])
    ac = rep.get("aircraft") or []
    r["ids"] = [a["icao24"].lower() for a in ac]
    r["clock"] = (rep.get("clock") or {}).get("start_unix")
    return r


def dg_icao(fid, synthetic):
    base = fid.split("@")[0]
    base = base.rsplit(":", 1)[0] if ":" in base and not synthetic else base
    return "f" + hashlib.sha1(base.encode()).hexdigest()[:5] if synthetic else base.lower()


def score_dg(d: Path, truth, synthetic):
    m = load(d / "measure.json") or {}
    res = load(d / "dg_result.json")
    r = {"exit": m.get("exit"), "wall_s": m.get("wall_s"), "rss_mb": m.get("peak_tree_rss_mb")}
    if not res:
        return dict(r, status="failed", raw="crash", note=tail(d / "stdout.log"))
    raw, cands = res["status"], res.get("candidates") or []
    st = {"succeeded": "found", "needs_input": "needs_input"}.get(raw, "ambiguous" if raw == "inconclusive" and cands else "failed")
    r.update(status=st, raw=f"{raw}/{res.get('reason_code', '')}", note=(res.get("message") or "")[:200],
             events=len(res.get("events") or []))
    if cands:
        c = cands[0]
        r["lat"], r["lon"] = c["lat"], c["lon"]
        u = c.get("uncertainty")
        if u:
            r["in_region"] = dist_m(c["lat"], c["lon"], truth["lat"], truth["lon"]) <= u["radius_m"]
            r["region"] = f"r {u['radius_m']:.0f} m ({u['kind']})"
            r["region_km2"] = math.pi * u["radius_m"] ** 2 / 1e6
        r["ids"] = [dg_icao(f, synthetic) for f in c["flight_ids"]]
        r["clock"] = unix(c["recording_start_utc"])
    return r


def finish(r, truth):
    if "lat" in r:
        r["err_m"] = round(dist_m(r["lat"], r["lon"], truth["lat"], truth["lon"]), 1)
    ids = r.get("ids") or []
    r["id_top1"] = (ids[0] in truth["icaos"]) if ids else None
    r["clock_err_s"] = None if r.get("clock") is None else round(clock_err(r["clock"], truth["start_window_unix"]), 2)
    r["success"] = r["status"] == "found" and r.get("err_m", 1e18) <= SUCCESS_M
    return r


def pct(xs, q):
    xs = sorted(xs)
    if not xs:
        return None
    k = (len(xs) - 1) * q
    lo = math.floor(k)
    return xs[lo] + (xs[min(lo + 1, len(xs) - 1)] - xs[lo]) * (k - lo)


def aggregate(rows, tool, group):
    rs = [r[tool] for r in rows if group == "all" or r["group"] == group]
    errs = [r["err_m"] for r in rs if "err_m" in r]
    found_errs = [r["err_m"] for r in rs if r["status"] == "found" and "err_m" in r]
    reg = [r["in_region"] for r in rs if r.get("in_region") is not None]
    ids = [r["id_top1"] for r in rs if r.get("id_top1") is not None]
    clk = [abs(r["clock_err_s"]) for r in rs if r.get("clock_err_s") is not None]
    walls = [r["wall_s"] for r in rs if r.get("wall_s") is not None]
    rss = [r["rss_mb"] for r in rs if r.get("rss_mb") is not None]
    f = lambda x, d=0: None if x is None else round(x, d)
    return {"tool": tool, "group": group, "n": len(rs),
            **{s: sum(r["status"] == s for r in rs) for s in ("found", "ambiguous", "failed", "needs_input")},
            "success": sum(r["success"] for r in rs), "with_location": len(errs),
            "median_err_m": f(pct(errs, .5)), "p90_err_m": f(pct(errs, .9)),
            "median_err_found_m": f(pct(found_errs, .5)), "p90_err_found_m": f(pct(found_errs, .9)),
            "region_hits": f"{sum(reg)}/{len(reg)}",
            "median_region_km2": f(pct([r["region_km2"] for r in rs if r.get("region_km2") is not None], .5), 2),
            "id_top1": f"{sum(ids)}/{len(ids)}",
            "median_clock_err_s": f(pct(clk, .5), 1), "median_wall_s": f(pct(walls, .5), 1),
            "max_wall_s": f(max(walls, default=None), 1), "total_wall_s": f(sum(walls), 0),
            "median_rss_mb": f(pct(rss, .5)), "max_rss_mb": f(max(rss, default=None))}


def cell(r):
    err = f"{r['err_m']:.0f}" if "err_m" in r else "-"
    reg = {True: "in", False: "out", None: "-"}[r.get("in_region")]
    idt = {True: "ok", False: "wrong", None: "-"}[r.get("id_top1")]
    clk = "-" if r.get("clock_err_s") is None else f"{r['clock_err_s']:+.1f}"
    return f"{r['status']} | {err} | {reg} | {idt} | {clk} | {r.get('wall_s') or '-'} | {r.get('rss_mb') or '-'}"


def main():
    run = Path(sys.argv[1])
    inputs = {c["id"]: c for c in json.loads((run / "inputs.json").read_text())}
    rows = []
    for cid, c in inputs.items():
        if not (run / cid).is_dir():
            continue
        t, syn = c["truth"], c["kind"] == "synthetic"
        rows.append({"case": cid, "group": "synthetic" if syn else "real",
                     "ours": finish(score_ours(run / cid / "ours", t), t),
                     "dg": finish(score_dg(run / cid / "dg", t, syn), t)})
    aggs = [aggregate(rows, tool, g) for g in ("all", "synthetic", "real") for tool in TOOLS]
    (run / "results.json").write_text(json.dumps({"cases": rows, "aggregates": aggs}, indent=1))
    keys = ("status", "raw", "err_m", "in_region", "region", "id_top1", "ids", "clock_err_s", "wall_s", "rss_mb", "events", "note")
    with (run / "results.tsv").open("w") as f:
        f.write("case\ttool\t" + "\t".join(keys) + "\n")
        for r in rows:
            for tool in TOOLS:
                f.write(f"{r['case']}\t{tool}\t" + "\t".join(str(r[tool].get(k, "")) for k in keys) + "\n")
    hdr = "status | err m | truth in region | top-1 id | clock err s | wall s | peak RSS MB"
    md = [f"| case | ours: {hdr} | DG: {hdr} |".replace(" | ", " | "), "|---" * 15 + "|"]
    md += [f"| {r['case']} | {cell(r['ours'])} | {cell(r['dg'])} |" for r in rows]
    md += ["", "| " + " | ".join(aggs[0]) + " |", "|---" * len(aggs[0]) + "|"]
    md += ["| " + " | ".join(str(v) for v in a.values()) + " |" for a in aggs]
    (run / "summary.md").write_text("\n".join(md) + "\n")
    print("\n".join(md[-(len(aggs) + 2):]))


if __name__ == "__main__":
    main()
