#!/usr/bin/env bash
# Check two eval output dirs are identical apart from wall-clock fields: tools/eval_same.sh runs/A runs/B
set -euo pipefail
A=$1 B=$2; bad=0
strip() { jq -S 'walk(if type == "object" then del(.runtime_s, .timing) else . end)' "$1"; }
md() { awk -F'|' -v OFS='|' '$11 ~ /^ *[0-9.]+ *$/ {$11=""} {print}' "$1"; }  # column "s" is per-case runtime
diff <(cd "$A" && ls *.json) <(cd "$B" && ls *.json) || bad=1
for f in "$A"/*.json; do
  n=$(basename "$f")
  [[ -f $B/$n ]] && ! cmp -s <(strip "$f") <(strip "$B/$n") && { echo "differs: $n"; bad=1; }
done
diff <(md "$A/summary.md") <(md "$B/summary.md") > /dev/null || { echo "differs: summary.md"; bad=1; }
[[ $bad == 0 ]] && echo "identical: $(ls "$A"/*.json | wc -l) json files (timing ignored)"
exit $bad
