#!/usr/bin/env bash
# Wrong-flight diagnostic evals (liv_disc_w<W>.toml) on the frozen bench binary, pinned to cores 0-7.
#   [BIN=bin-evset] tools/bench_vs_dg/diag/run.sh [W...]   default: 5 120 (W = suffix after liv_disc_w)
set -euo pipefail
P="$(cd "$(dirname "$0")/../../.." && pwd)"
BIN=${BIN:-bin}
widths=("$@"); [[ ${#widths[@]} -gt 0 ]] || widths=(5 120)
for w in "${widths[@]}"; do
  echo "=== ±$w s"
  (cd "$P" && taskset -c 0-7 "$P/runs/bench-vs-dg/prep/$BIN/dopplerprint" eval --experiment "$P/tools/bench_vs_dg/diag/liv_disc_w$w.toml" 2>&1 | tail -n 40)
done
