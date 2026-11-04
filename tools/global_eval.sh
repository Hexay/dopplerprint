#!/usr/bin/env bash
# global_eval.sh EXPERIMENT.toml — run a region-free eval in this worktree; print the stage line, the
# top regional tuples and timings per case. See docs/research/global_search_2026-11-03.md.
# Uses $DP_BIN if set, else builds fastrel via tools/dev.sh build (DP_PROFILE=release for release).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-4}"
BIN="${DP_BIN:-}"
[[ -n $BIN ]] || { "$ROOT/tools/dev.sh" build; BIN="$CARGO_TARGET_DIR/${DP_PROFILE:-fastrel}/dopplerprint"; }
R="$ROOT/runs/global/$(basename "$1" .toml)"
rm -rf "$R"; mkdir -p "$R"
"$BIN" eval --experiment "$1" --out "$R" > "$R.log" 2>&1 || { tail -n 20 "$R.log"; exit 1; }
echo "out: $R"
grep -E "global:|err " "$R.log" | cut -c1-160
for f in "$R"/*.json; do
  [[ $f == *summary.json ]] && continue
  python3 -I - "$f" <<'EOF'
import json, sys
c = json.load(open(sys.argv[1])); g = c['global']
print(f"{c['id']}: prop# {g['truth_proposal_rank']}/{g['n_proposals']} region# {g['truth_region_rank']} "
      f"margin {c.get('margin') or 0:.2f} rerank {g['rerank_s']:.0f}s locate {g['locate_s']:.0f}s")
for e in g['events']:
    print(f"  event t {e['t_cpa_audio']:.1f} d {[round(x) for x in e['d_range']]} v {[round(x, 1) for x in e['v_range']]}")
for t in c['tuples'][:3]:
    print(f"  {t['lat_deg']:.4f} {t['lon_deg']:.4f} {t['track_ids']} score {t['score']:.2f} raw {t['raw']:.2f}")
EOF
done
