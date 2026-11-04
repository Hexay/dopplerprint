"""Diagnostic input variant (NOT the benchmark protocol): start at the true start ±TOL s, to separate clock
ambiguity from everything else. Uses oracle time on purpose; label results as diagnostic.

usage: python3 -I diag_inputs.py INPUTS.json OUT.json TOL_S
"""
import json
import sys
from datetime import datetime, timezone


def main():
    src, out, tol = sys.argv[1], sys.argv[2], float(sys.argv[3])
    cases = json.load(open(src))
    for c in cases:
        w = c["truth"]["start_window_unix"]
        s = round(0.5 * (w[0] + w[1]))
        iso = lambda t: datetime.fromtimestamp(t, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        c.update(start_minute_utc=iso(s), tolerance_s=tol, window_utc=[iso(s - tol), iso(s + tol)])
    json.dump(cases, open(out, "w"), indent=1)


if __name__ == "__main__":
    main()
