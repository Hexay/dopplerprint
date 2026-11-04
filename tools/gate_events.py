"""Print discovered events whose CPA is within 10 s of a truth CPA: gate_events.py EVENTS.json T_CPA..."""

import json
import sys


def main(path: str, truth: list[float]) -> None:
    for e in json.load(open(path)):
        f = e["fit"]
        b = f["best"]
        if min(abs(b["t_cpa_arrive"] - t) for t in truth) >= 10:
            continue
        k = f.get("kappa_range") or (0, 0)
        print(
            f"  {e['id']} t {b['t_cpa_arrive']:.1f} v {b['v']:.0f} {[round(x) for x in f['v_range']]} "
            f"d {b['d_cpa']:.0f} {[round(x) for x in f['d_range']]} kappa {[round(x, 2) for x in k]} "
            f"f0 {b['f0']:.1f} {[round(x, 1) for x in f['f0_range']]}"
        )


if __name__ == "__main__":
    main(sys.argv[1], [float(t) for t in sys.argv[2:]])
