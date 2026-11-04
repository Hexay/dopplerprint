#!/usr/bin/env bash
# Check two eval output dirs (with oracle/ and discovery/ if present) are identical apart from wall-clock
# fields: tools/eval_same.sh runs/A runs/B
set -euo pipefail
A=$1 B=$2; bad=0
for d in "$A" "$B"; do [[ -f $d/summary.md ]] || { echo "no eval output in $d" >&2; exit 2; }; done
strip() { jq -S 'walk(if type == "object" then del(.runtime_s, .timing, .discover_s, .load_s, .propose_s, .rerank_s, .locate_s) else . end)' "$1"; }
md() { awk -F'|' -v OFS='|' '$11 ~ /^ *[0-9.]+ *$/ {$11=""} {print}' "$1"; }  # column "s" is per-case runtime
list() { (cd "$1" && find . -name '*.json' | sort); }
diff <(list "$A") <(list "$B") || bad=1
n=0
for f in $(list "$A"); do
  n=$((n + 1))
  [[ -f $B/$f ]] && ! cmp -s <(strip "$A/$f") <(strip "$B/$f") && { echo "differs: $f"; bad=1; }
done
diff <(md "$A/summary.md") <(md "$B/summary.md") > /dev/null || { echo "differs: summary.md"; bad=1; }
[[ $bad == 0 ]] && echo "identical: $n json files (timing ignored)"
exit $bad
