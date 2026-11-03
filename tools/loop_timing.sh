#!/usr/bin/env bash
# Time the local dev loop after a dp-core edit: clippy, test build, test run, box-style release builds.
# Usage: tools/loop_timing.sh [runs=3] [steps...]  (steps: clippy test_build test_run rel_cli rel_tests rel_one fast_cli fast_tests fast_one)
# Run it with `env -u CARGO_INCREMENTAL RUSTC_WRAPPER=` to match tools/dev.sh.
# The "edit" alternates a one-line body change in search/sweep.rs (a touch alone is free for incremental and sccache).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNS="${1:-3}"; shift || true
STEPS=("${@:-clippy test_build test_run}"); read -r -a STEPS <<< "${STEPS[*]}"
M=(--manifest-path "$ROOT/Cargo.toml" -j "${CARGO_BUILD_JOBS:-4}")
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" SCCACHE_RECACHE=1
SRC="$ROOT/crates/dp-core/src/search/sweep.rs" T="$CARGO_TARGET_DIR/.loop"
mkdir -p "$T"; cp "$SRC" "$T/a.rs"; trap 'cp "$T/a.rs" "$SRC"' EXIT
awk '{print} /^    acc.clear\(\);$/{print "    acc.reserve(0);"}' "$T/a.rs" > "$T/b.rs"
printf 'f=%q; t=%q; if cmp -s "$f" "$t/a.rs"; then cp "$t/b.rs" "$f"; else cp "$t/a.rs" "$f"; fi\n' "$SRC" "$T" > "$T/flip.sh"
EDIT="bash $T/flip.sh"

cmd_for() {
  case $1 in
    clippy)     echo "cargo clippy ${M[*]} --workspace --all-targets -q" ;;
    test_build) echo "cargo nextest run ${M[*]} -p dp-core --no-run --cargo-quiet" ;;
    test_run)   echo "cargo nextest run ${M[*]} -p dp-core --cargo-quiet --status-level fail" ;;
    rel_cli)    echo "cargo build ${M[*]} -q --release -p dp-cli" ;;
    rel_tests)  echo "cargo nextest run ${M[*]} --release -p dp-core --no-run --cargo-quiet" ;;
    fast_cli)   echo "cargo build ${M[*]} -q --profile fastrel -p dp-cli" ;;
    fast_tests) echo "cargo nextest run ${M[*]} --cargo-profile fastrel -p dp-core --no-run --cargo-quiet" ;;
    rel_one)    echo "cargo nextest run ${M[*]} --release -p dp-core --test search_pipeline --no-run --cargo-quiet" ;;
    fast_one)   echo "cargo nextest run ${M[*]} --cargo-profile fastrel -p dp-core --test search_pipeline --no-run --cargo-quiet" ;;
    *) echo "unknown step $1" >&2; exit 2 ;;
  esac
}

for s in "${STEPS[@]}"; do
  c=$(cmd_for "$s")
  prep=$([[ $s == test_run ]] && echo true || echo "$EDIT")
  bash -c "$c" > /dev/null 2>&1  # warm
  hyperfine -N -r "$RUNS" --prepare "$prep" -n "$s" "$c" --export-json "$CARGO_TARGET_DIR/.t.json" > /dev/null 2>&1
  jq -r '.results[0] | "\(.command)\tmedian \(.median*10|round/10) s\tmin \(.min*10|round/10)\tmax \(.max*10|round/10)\tn=\(.times|length)"' \
    "$CARGO_TARGET_DIR/.t.json"
done
