#!/usr/bin/env bash
# Time DOPPLERGEIST on the Oyster Bay configs (N repeats each) → runs/bench-dopplergeist/<stamp>.tsv
set -euo pipefail
P=/srv/agents/work/doppler_print
DG=$P/reference/.venv-dopplergeist/bin/dopplergeist
SRC=$P/data/_downloads/dopplergeist-extracted/data/source
N=${N:-3}
OUT=$P/runs/bench-dopplergeist; mkdir -p "$OUT"
TSV=$OUT/$(date -u +%Y%m%dT%H%M%S).tsv
printf "config\trep\twall_s\tcpu_pct\tmax_rss_mb\n" > "$TSV"
for cfg in reproduce-local reproduce-faint-alltypes-local; do
  for rep in $(seq 1 "$N"); do
    tmp=$(mktemp -d "$OUT/run.XXXX")
    /usr/bin/time -f "%e\t%P\t%M" -o "$tmp/time" "$DG" run "$SRC/$cfg.json" --output "$tmp/out" > "$tmp/log" 2>&1
    IFS=$'\t' read -r wall cpu rss < "$tmp/time"
    printf "%s\t%s\t%s\t%s\t%s\n" "$cfg" "$rep" "$wall" "${cpu%\%}" "$((rss / 1024))" | tee -a "$TSV"
    rm -rf "$tmp"
  done
done
echo "wrote $TSV"
