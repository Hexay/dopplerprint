#!/usr/bin/env bash
# dopplerprint `locate` vs DOPPLERGEIST `analyze` end to end on the same inputs.
#   prep            inputs.json, synthetic ADS-B/DEMs, both tools' flight data (slow once: DG day indexes)
#   run [case...]   both tools per case, sequentially -> runs/bench-vs-dg/<stamp>/ (STAMP= to resume)
#   score stamp     metrics + aggregates -> <stamp>/results.{json,tsv}, summary.md
#   perf stamp ...  matched-work speed/memory (tools/bench_vs_dg/perf.py); perf-summary stamp
# Write-up: docs/research/bench_vs_dopplergeist_2026-11-03.md
set -euo pipefail
P="$(cd "$(dirname "$0")/.." && pwd)"
B=$P/tools/bench_vs_dg
DGPY=${DGPY:-/srv/agents/work/dg_upstream/.venv/bin/python}
PREP=$P/runs/bench-vs-dg/prep
DP=${DP:-$PREP/bin/dopplerprint}  # frozen copy, one build for every case (commit in $PREP/bin/COMMIT)
INPUTS=${INPUTS:-$PREP/inputs.json}  # diagnostics: tools/bench_vs_dg/diag_inputs.py writes variants
CORES=${CORES:-0-7}           # same 8 cores for both tools
DG_MAX_RUNTIME=${DG_MAX_RUNTIME:-1800}
measure() { python3 -I "$B/measure.py" "$@"; }
field() { python3 -I -c 'import json,sys; c=next(c for c in json.load(open(sys.argv[1])) if c["id"]==sys.argv[2]); v=eval("c"+sys.argv[3], {"c": c}); print(" ".join(map(str, v)) if isinstance(v, list) else v)' "$INPUTS" "$@"; }

prep() {
  mkdir -p "$PREP/dg_index"
  python3 -I "$B/cases.py" "$P/data" "$PREP" "$INPUTS" | tail -n 20
  "$DGPY" -I "$B/prep_synth.py" "$INPUTS" "$P/data" "$PREP"
  local real=$PREP/ours_cache_real
  mkdir -p "$real/adsb/store" "$real/weather"
  ln -sfn "$P/data/typedb" "$real/typedb"
  for d in "$P"/data/adsb/store/date=*; do ln -sfn "$d" "$real/adsb/store/$(basename "$d")"; done
  cp -n "$P"/data/weather/*.json "$real/weather/" 2>/dev/null || true
  for c in $(python3 -I -c 'import json,sys; print(" ".join(c["id"] for c in json.load(open(sys.argv[1]))))' "$INPUTS"); do
    if [[ $c == dg_* ]]; then
      local cache; cache=$(field "$c" '["ours"]["cache"]')
      if [[ ! -d $cache/adsb/store ]]; then
        mkdir -p "$cache"; ln -sfn "$P/data/typedb" "$cache/typedb"
        "$DP" ingest "$PREP/synth_traces/$c" --out "$cache/adsb/store" --typedb "$P/data/typedb" | tail -n 3
      fi
      [[ -f $PREP/dg_index/$c.sqlite ]] || "$DGPY" -I "$B/dg_index.py" synth "$P/data/cases/$c" "$P/data/typedb/icao_aircraft_types.js" "$PREP/dg_index/$c.sqlite"
    else
      local day raw; day=$(field "$c" '["dg"]["day"]'); raw=$(field "$c" '["dg"]["raw_day"]')
      [[ -f $PREP/dg_index/$day.sqlite ]] || taskset -c 0-3 measure "$PREP/dg_index/$day.measure.json" "$PREP/dg_index/$day.log" -- \
        "$DGPY" -I "$B/dg_index.py" day "$day" "$raw" "$P/data/typedb/icao_aircraft_types.js" "$PREP/dg_index/$day.sqlite"
    fi
  done
}

run_ours() {  # case outdir
  local c=$1 o=$2/ours
  mkdir -p "$o"
  local args=(locate "$(field "$c" '["audio"]')" --start "$(field "$c" '["start_minute_utc"]')"
    --start-tolerance "$(field "$c" '["tolerance_s"]' | cut -d. -f1)s"
    --near="$(field "$c" '["near_ours"]')" --cache "$(field "$c" '["ours"]["cache"]')"
    --dem "$(field "$c" '["ours"]["dem"]')" --out "$o/results" --json --quiet)
  [[ $(field "$c" '["ours"]["no_weather"]') == True ]] && args+=(--no-weather)
  [[ -n ${OURS_CONFIG:-} ]] && args+=(--config "$OURS_CONFIG")
  echo "$DP ${args[*]}" > "$o/cmd.txt"
  taskset -c "$CORES" python3 -I "$B/measure.py" "$o/measure.json" "$o/stdout.log" -- "$DP" "${args[@]}" | tail -n 1
}

run_dg() {  # case outdir cores
  local o=$2/dg
  mkdir -p "$o"
  DG_SEARCH_WORKERS=auto taskset -c "$3" python3 -I "$B/measure.py" "$o/measure.json" "$o/stdout.log" -- \
    "$DGPY" -I "$B/run_dg.py" "$INPUTS" "$1" "$o" "$DG_MAX_RUNTIME" | tail -n 1
}

# DG's automatic search is one serial process per tile (a 20 km box is one 2 km-spaced tile), so its cases run in
# DG_LANES parallel lanes of 2 cores each (~0.5 GB per lane); ours runs alone on $CORES first.
DG_LANES=${DG_LANES:-3}
run() {
  local stamp=${STAMP:-$(date -u +%Y%m%dT%H%M%S)} cases=("$@")
  local R=$P/runs/bench-vs-dg/$stamp
  mkdir -p "$R"; cp "$INPUTS" "$R/inputs.json"; cp "$PREP/bin/COMMIT" "$PREP/bin/SHA256" "$R/" 2>/dev/null || true
  [[ ${#cases[@]} -gt 0 ]] || mapfile -t cases < <(python3 -I -c 'import json,sys; [print(c["id"]) for c in json.load(open(sys.argv[1]))]' "$INPUTS")
  echo "stamp $stamp"
  for c in "${cases[@]}"; do
    [[ ${ONLY:-} == dg || -f $R/$c/ours/measure.json ]] || { echo "ours $c"; run_ours "$c" "$R/$c"; }
  done
  [[ ${ONLY:-} == ours ]] && return
  for ((l = 0; l < DG_LANES; l++)); do
    ( for ((i = l; i < ${#cases[@]}; i += DG_LANES)); do
        c=${cases[i]}
        [[ -f $R/$c/dg/measure.json ]] || { echo "dg   $c (lane $l)"; run_dg "$c" "$R/$c" "$((8 + 2 * l)),$((9 + 2 * l))"; }
      done ) &
  done
  wait
}

case "${1:-}" in
  prep) prep ;;
  run) shift; run "$@" ;;
  score) python3 -I "$B/score.py" "$P/runs/bench-vs-dg/${2:?stamp}" ;;
  # perf STAMP [reps] [ob|synth|livermore...]: matched-work speed/memory runs into <stamp>/perf/ (needs prep)
  perf) shift; s=${1:?stamp}; shift; python3 -I "$B/perf.py" "$P/runs/bench-vs-dg/$s" "$@" ;;
  perf-summary) python3 -I "$B/perf_summary.py" "$P/runs/bench-vs-dg/${2:?stamp}/perf" ;;
  *) sed -n 2,7p "$0"; exit 2 ;;
esac
