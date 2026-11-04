#!/usr/bin/env bash
# gate.sh [-b BASE_PREFIX] [stage...] — gate a dp-core change on one incremental fastrel build.
# Stages (default: lint events liv oyster global):
#   lint    clippy + nextest dp-core (tools/dev.sh check/test)
#   events  Oyster Bay 1 h discover; events near the truth CPAs
#   liv     livermore_dev_discovered eval; oyster: oysterbay_discovered eval (compared with BASE_PREFIX{liv_dev,oyster} if -b)
#   global  experiments/global/oysterbay_t5.toml via tools/global_eval.sh
# DP_LOCK=FILE takes that flock around each heavy step. Outputs: runs/gate/. Timings: docs/research/iteration_speed_2026-11-04.md
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"  # eval resolves data/ against cwd
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-4}"
BIN="$CARGO_TARGET_DIR/${DP_PROFILE:-fastrel}/dopplerprint"
OUT="$ROOT/runs/gate"
WAV="${DP_OYSTER_WAV:-$ROOT/data/cases/oysterbay_20260908_colsto/audio/reference-recording.wav}"
BASE=""
[[ ${1:-} == -b ]] && { BASE=$2; shift 2; }
STAGES=("${@:-lint events liv oyster global}"); read -r -a STAGES <<< "${STAGES[*]}"
mkdir -p "$OUT"
failed=()

heavy() { if [[ -n ${DP_LOCK:-} ]]; then flock -o "$DP_LOCK" "$@"; else "$@"; fi; }

discovered_eval() {  # name experiment
  heavy "$BIN" eval --experiment "experiments/$2.toml" --out "$OUT/$1" > "$OUT/$1.log" 2>&1 || { tail -n 20 "$OUT/$1.log"; return 1; }
  grep -E "^discover [0-9]" "$OUT/$1/summary.md" | head -n 1
  [[ -z $BASE ]] || "$BIN" compare "$BASE$1" "$OUT/$1" 2>&1 | tail -n 25
}

stage() {
  case $1 in
    lint)   heavy tools/dev.sh check && heavy tools/dev.sh test ;;
    events) heavy "$BIN" discover "$WAV" --out "$OUT/oyster_events.json" > "$OUT/discover.log" 2>&1 \
              || { tail -n 20 "$OUT/discover.log"; return 1; }
            grep -m1 " events; " "$OUT/discover.log"
            python3 -I tools/gate_events.py "$OUT/oyster_events.json" 2155.7 3076.4 ;;
    liv)    discovered_eval liv_dev livermore_dev_discovered ;;
    oyster) discovered_eval oyster oysterbay_discovered ;;
    global) DP_BIN="$BIN" heavy tools/global_eval.sh experiments/global/oysterbay_t5.toml ;;
    *) echo "unknown stage $1" >&2; return 2 ;;
  esac
}

t=$SECONDS
heavy tools/dev.sh build || exit 1
echo "=== build $((SECONDS - t)) s"
for s in "${STAGES[@]}"; do
  echo "=== $s"; t=$SECONDS
  stage "$s" || failed+=("$s")
  echo "--- $s $((SECONDS - t)) s"
done
[[ ${#failed[@]} == 0 ]] && echo "gate: all passed" || { echo "gate: FAILED ${failed[*]}"; exit 1; }
