"""Run DOPPLERGEIST's automatic product path (`dopplergeist analyze`, via its JobManager so the request can carry
`search.bounds_wsen`, which the CLI does not expose) on one benchmark case. Run in the dg_upstream venv.

usage: run_dg.py INPUTS.json CASE_ID OUT_DIR [MAX_RUNTIME_S]
Writes OUT_DIR/jobs/<job>/... and OUT_DIR/dg_result.json (the job's final result.json).
"""
import json
import os
import sys
from pathlib import Path

from dopplergeist.jobs import JobManager

SEARCH_WORKERS = 7  # taskset gives each tool 8 cores; DG's default is one fewer than the CPUs it sees


def operator_config(case: dict, out: Path) -> dict:
    """DG_SEARCH_WORKERS=auto leaves DG's own default; a number sets it. case.dg.operator_extra is merged last."""
    workers = os.environ.get("DG_SEARCH_WORKERS", str(SEARCH_WORKERS))
    cfg = {"cache_root": str(out / "dg_cache"), "offline": True, "flight_indexes": case["dg"]["flight_indexes"],
           "terrain": case["dg"]["terrain"]}
    if workers != "auto":
        cfg["search_workers"] = int(workers)
    return {**cfg, **case["dg"].get("operator_extra", {})}


def request(case: dict) -> dict:
    lo, hi = case["window_utc"]
    return {"schema_version": 1, "search": {"coverage": "conus", "bounds_wsen": case["bbox_wsen"]},
            "time_window": {"start_utc": lo, "end_utc": hi, "source": "user",
                            "source_detail": f"start typed to the minute ({case['start_minute_utc']}) +-2 min"}}


def main():
    inputs, cid, out, *rest = sys.argv[1:]
    case = next(c for c in json.loads(Path(inputs).read_text()) if c["id"] == cid)
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    config = operator_config(case, out)
    (out / "dg_operator.json").write_text(json.dumps(config, indent=1))
    (out / "dg_request.json").write_text(json.dumps(request(case), indent=1))
    audio = Path(case["audio"])
    mgr = JobManager(out / "jobs", config, max_runtime_s=float(rest[0]) if rest else 3600.0,
                     max_upload_bytes=4 * 1024**3)
    try:
        with audio.open("rb") as src:
            queued = mgr.submit(request(case), src, audio.stat().st_size, suffix=audio.suffix)
        res = mgr.wait(queued["job_id"])
    finally:
        mgr.close()
    res["job_dir"] = str(out / "jobs" / queued["job_id"])
    (out / "dg_result.json").write_text(json.dumps(res, indent=1))
    print(json.dumps({k: res.get(k) for k in ("status", "reason_code", "message")}))


if __name__ == "__main__":
    main()
