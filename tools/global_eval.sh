#!/usr/bin/env bash
# global_eval.sh EXPERIMENT.toml — run a region-free eval in this worktree; print the stage line, the
# top regional tuples and timings per case. See docs/research/global_search_2026-11-03.md.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-4}"
cargo run --release -q -p dp-cli -j4 -- eval --experiment "$1" 2>&1 | grep -E "global:|err " | cut -c1-160
R=$(ls -td "$ROOT"/runs/*/ | head -1)
for f in "$R"*.json; do
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
