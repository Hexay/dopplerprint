#!/usr/bin/env bash
# Re-run DOPPLERGEIST on the checkpoint inputs and store outputs as parity baselines in each case.
# Needs: reference/.venv-dopplergeist (see data/MANIFEST.md) and `fetch_corpus.py dopplergeist` done.
# Sequential on purpose: each run peaks < 1 GB RSS, 15-210 s.
set -euo pipefail
P=/srv/agents/work/doppler_print
DG=$P/reference/.venv-dopplergeist/bin/dopplergeist
SRC=$P/data/_downloads/dopplergeist-extracted/data
STAMP=${STAMP:-$(date -u +%Y%m%d)}

run() {  # case_id config_path run_name
  local out=$P/data/cases/$1/baseline/rerun-$STAMP/$3
  if [[ -f $out/report.json ]]; then echo "skip $1/$3"; return; fi
  rm -rf "$out"; mkdir -p "$(dirname "$out")"
  echo "run $1/$3"
  /usr/bin/time -f "elapsed_s=%e max_rss_kb=%M" -o "$out.time" \
    "$DG" run "$2" --output "$out" > "$out.log" 2>&1 || { echo "FAILED $1/$3 (see $out.log)"; return; }
  cat "$out.time"
}

run dg_synthetic_known_minute "$SRC/synthetic/known-time-minute.json" synthetic-known-minute
run dg_synthetic_two_events "$SRC/synthetic/two-events.json" synthetic-two-events
run oysterbay_20260908_colsto "$SRC/source/reproduce-local.json" reference-local
run oysterbay_20260908_colsto "$SRC/source/reproduce-faint-alltypes-local.json" reference-faint-alltypes-local
