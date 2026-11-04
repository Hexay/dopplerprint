#!/usr/bin/env bash
# Our side of the end-to-end benchmark across accuracy variants, sequential: runs/bench-vs-dg/var-<name>/, then scores each.
#   tools/bench_vs_dg/variants.sh [name...]   (default: all below)
set -euo pipefail
P="$(cd "$(dirname "$0")/../.." && pwd)"
PREP=$P/runs/bench-vs-dg/prep
V=$P/tools/bench_vs_dg/variants
declare -A BIN=([base]=bin [lu6]=bin [lu6_sil]=bin [evset]=bin-evset [evset_lu6]=bin-evset [evset_lu6_sil]=bin-evset)
declare -A CFG=([base]= [lu6]=$V/lu6.toml [lu6_sil]=$V/lu6_sil.toml [evset]= [evset_lu6]=$V/lu6.toml [evset_lu6_sil]=$V/lu6_sil.toml)
names=("$@"); [[ ${#names[@]} -gt 0 ]] || names=(base evset lu6 evset_lu6 lu6_sil evset_lu6_sil)
for n in "${names[@]}"; do
  echo "== $n"
  STAMP=var-$n ONLY=ours DP=$PREP/${BIN[$n]}/dopplerprint OURS_CONFIG=${CFG[$n]} "$P/tools/bench_vs_dg.sh" run > /dev/null
  cp "$PREP/${BIN[$n]}/COMMIT" "$P/runs/bench-vs-dg/var-$n/COMMIT"
  "$P/tools/bench_vs_dg.sh" score "var-$n" > /dev/null 2>&1 || echo "score failed: $n"
done
