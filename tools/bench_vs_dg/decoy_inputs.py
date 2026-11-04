"""Decoy input variant (diagnostic, NOT the benchmark protocol): the benchmark's start shifted by SHIFT_S, so the
ADS-B searched is from a time the recording was not made. Any match is chance: its score measures look-elsewhere.

usage: python3 -I decoy_inputs.py INPUTS.json OUT.json SHIFT_S
"""
import json
import sys
from datetime import datetime, timedelta


def main():
    src, out, shift = sys.argv[1], sys.argv[2], float(sys.argv[3])
    cases = json.load(open(src))
    fmt = "%Y-%m-%dT%H:%M:%SZ"
    move = lambda s: (datetime.strptime(s, fmt) + timedelta(seconds=shift)).strftime(fmt)
    for c in cases:
        c.update(start_minute_utc=move(c["start_minute_utc"]), window_utc=[move(w) for w in c["window_utc"]])
    json.dump(cases, open(out, "w"), indent=1)


if __name__ == "__main__":
    main()
