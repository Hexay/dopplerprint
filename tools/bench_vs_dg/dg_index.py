"""Build DOPPLERGEIST FlightIndex SQLite files from local inputs (run in the dg_upstream venv, python -I).

  day   DATE RAW_DIR TYPES_JS OUT   adsb.lol day archive -> all-type index, as FlightCache.acquire_day builds it
                                    (our raw days are prod-0 for May; acquire_day only knows staging-0).
  synth CASE_DIR TYPES_JS OUT       a synthetic case's finite track population (tracks.parquet or tracks.csv).
"""
import gzip
import json
import sys
import time
from pathlib import Path

import pandas as pd
from dopplergeist import tracks as normalizer
from dopplergeist.data_access import FlightIndex, archive_staging, sha256


def class_map(types_js: Path) -> dict:
    raw = types_js.read_bytes()
    text = (gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw).decode()
    types = json.loads(text[text.index("{"):text.rindex("}") + 1])
    out = {}
    for code, value in types.items():
        desc = value.get("desc", "") if isinstance(value, dict) else (value[2] if len(value) > 2 else "")
        out[code] = {"J": "jet", "T": "turbine", "P": "piston"}.get(desc[-1:], "unclassified")
    return out


def build_day(day: str, raw: Path, types_js: Path, out: Path):
    parts = sorted(str(p) for p in raw.glob("*.tar.*"))
    tag = raw.name
    day0 = pd.Timestamp(day, tz="UTC").timestamp()
    coverage = {"bounds": [-180, -90, 180, 90], "time_range_s": [day0, day0 + 86400], "all_aircraft": True,
                "provider": "ADSB.lol", "release_tag": tag, "license": "ODbL", "missing_receiver_data_possible": True,
                "normalizer_sha256": sha256(normalizer.__file__)}
    sources = [{"path": p, "bytes": Path(p).stat().st_size} for p in parts]
    cmap = class_map(types_js)
    return FlightIndex.build(out, archive_staging(parts, cmap), sources=sources, coverage=coverage,
                             class_map=cmap, prepared=True)


def synth_frame(case: Path) -> pd.DataFrame:
    pq, csv = case / "adsb/tracks.parquet", case / "adsb/tracks.csv"
    frame = pd.read_parquet(pq) if pq.exists() else pd.read_csv(csv)
    if "altitude_basis" not in frame:
        frame["altitude_basis"] = "ellipsoidal"  # synthetic world: geoid 0, so MSL == ellipsoid
    return frame


def build_synth(case: Path, types_js: Path, out: Path):
    frame = synth_frame(case)
    cov_file = case / "source/coverage.json"
    coverage = json.loads(cov_file.read_text()) if cov_file.exists() else {
        "bounds": [-180.0, -85.0, 180.0, 85.0], "all_aircraft": True,
        "time_range_s": [float(frame.time_s.min()), float(frame.time_s.max())],
        "source_kind": "complete_finite_synthetic_population", "real_receiver_coverage": False}
    src = pq if (pq := case / "adsb/tracks.parquet").exists() else case / "adsb/tracks.csv"
    return FlightIndex.build(out, [frame], sources=[{"path": str(src), "sha256": sha256(src)}],
                             coverage=coverage, class_map=class_map(types_js))


def main():
    mode, *args = sys.argv[1:]
    t0 = time.time()
    if mode == "day":
        idx = build_day(args[0], Path(args[1]), Path(args[2]), Path(args[3]))
    elif mode == "synth":
        idx = build_synth(Path(args[0]), Path(args[1]), Path(args[2]))
    else:
        sys.exit(__doc__)
    rep = idx.report
    print(json.dumps({"rows": rep.get("rows"), "segments": rep.get("segments"), "build_s": round(time.time() - t0, 1)}))


if __name__ == "__main__":
    main()
