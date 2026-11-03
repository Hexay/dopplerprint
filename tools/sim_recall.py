"""Event-discovery recall on sim scenarios: render each scenario, optionally round-trip it through codecs
(tools/sim_codec.sh), run `dopplerprint discover`, and match discovered events to truth.
A truth event (CPA arrival inside the clip) is found if a discovered event has |Δt_cpa| ≤ --dt and its
f0 within --f0-tol of truth f0 × h for h in {0.5, 1, 2, 3, 4}. Prints one summary row per variant.
Usage: python3 -I tools/sim_recall.py <scenario_dir>... --work DIR [--codec] [--bin target/release/dopplerprint]"""

import argparse
import json
import pathlib
import statistics
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
HARMONICS = (0.5, 1.0, 2.0, 3.0, 4.0)


def run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def match(truth: dict, events: list[dict], dt: float, tol: float) -> tuple[list[dict], int]:
    rows, used = [], set()
    for t in (e for e in truth["events"] if e["in_recording"]):
        best = None
        for i, e in enumerate(events):
            b = e["fit"]["best"]
            dtc = abs(b["t_cpa_arrive"] - t["cpa_arrival_audio_s"])
            ok_f = any(abs(b["f0"] / (t["f0"] * h) - 1.0) <= tol for h in HARMONICS)
            if i not in used and dtc <= dt and ok_f and (best is None or dtc < best[1]):
                best = (i, dtc)
        row = {"found": best is not None}
        if best:
            used.add(best[0])
            b = events[best[0]]["fit"]["best"]
            row |= {"dt": best[1], "v_err": b["v"] / t["speed_mps"] - 1.0, "d_err": b["d_cpa"] / t["slant_m"] - 1.0}
        rows.append(row)
    return rows, len(events) - len(used)


def summarise(name: str, rows: list[dict], spurious: int, minutes: float) -> str:
    found = [r for r in rows if r["found"]]
    med = lambda k: statistics.median(abs(r[k]) for r in found) if found else float("nan")
    return (
        f"| {name} | {len(found)}/{len(rows)} | {med('dt'):.2f} | {100 * med('v_err'):.1f} | {100 * med('d_err'):.1f}"
        f" | {spurious / minutes:.2f} |"
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("--work", required=True)
    ap.add_argument("--bin", default=str(ROOT / "target/release/dopplerprint"))
    ap.add_argument("--codec", action="store_true")
    ap.add_argument("--dt", type=float, default=10.0)
    ap.add_argument("--f0-tol", type=float, default=0.08)
    a = ap.parse_args()
    work = pathlib.Path(a.work)
    print("| variant | recall | med abs Δt_cpa s | med abs v err % | med abs d err % | spurious/min |")
    print("|---|---|---|---|---|---|")
    for d in map(pathlib.Path, a.dirs):
        acc: dict[str, tuple[list, int, float]] = {}
        for sc in sorted(d.glob("*.toml")):
            out = work / d.name / sc.stem
            run([a.bin, "sim", str(sc), "--out", str(out)])
            variants = {"": out}
            if a.codec:
                run([str(ROOT / "tools/sim_codec.sh"), str(out)])
                variants |= {f"+{c}": pathlib.Path(f"{out}_{c}") for c in ("aac128", "opus135", "aac128_opus135")}
            truth = json.loads((out / "truth.json").read_text())
            for tag, vdir in variants.items():
                ev_path = vdir / "events.json"
                run([a.bin, "discover", str(vdir / "audio.wav"), "--out", str(ev_path)])
                rows, spur = match(truth, json.loads(ev_path.read_text()), a.dt, a.f0_tol)
                r0, s0, m0 = acc.get(tag, ([], 0, 0.0))
                acc[tag] = (r0 + rows, s0 + spur, m0 + truth["duration_s"] / 60.0)
        for tag, (rows, spur, minutes) in acc.items():
            print(summarise(d.name + tag, rows, spur, minutes))


if __name__ == "__main__":
    main()
