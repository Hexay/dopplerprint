#!/usr/bin/env bash
# Dev runner: check | test [filter] | build | py | eval [experiment.toml] [eval args, e.g. --case N86LP]
# build: dp-cli at target/$DP_PROFILE/dopplerprint (default fastrel). Full gate: tools/gate.sh
# py/eval build the fastrel profile (DP_PROFILE=release for release); outputs are profile-independent: docs/research/iteration_loop_2026-11-03.md
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
MANIFEST="$ROOT/Cargo.toml"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
PROFILE="${DP_PROFILE:-fastrel}"

# Incremental dev builds; sccache only primes a cold target. Why: docs/research/iteration_loop_2026-11-03.md
incr() {  # mode cmd... (cargo or maturin)
  local mark="$TARGET/.dp-primed-$1"; shift
  if [[ -f $mark ]]; then env -u CARGO_INCREMENTAL RUSTC_WRAPPER= "$@"
  else CARGO_INCREMENTAL=0 "$@" && mkdir -p "$TARGET" && touch "$mark"; fi
}

# nextest rejects a binary() that matches no binary, so only add it when a test file name matches.
test_filter() {
  local e="test(/$1/)"
  ls "$ROOT/crates/dp-core/tests" | grep -qE -- "$1" && e="$e | binary(/$1/)"
  echo "$e"
}

build_cli() { incr "$PROFILE" cargo build --manifest-path "$MANIFEST" -q --profile "$PROFILE" -p dp-cli 2>&1 | tail -n 40; }

case "${1:-}" in
  check) incr check cargo clippy --manifest-path "$MANIFEST" --workspace --all-targets -q 2>&1 | tail -n 60 ;;
  test)  shift; incr test cargo nextest run --manifest-path "$MANIFEST" -p dp-core --cargo-quiet --status-level fail \
           ${1:+-E "$(test_filter "$1")"} 2>&1 | tail -n 60 ;;
  py)    (cd "$ROOT" && VIRTUAL_ENV="$ROOT/.venv" incr "py-$PROFILE" "$ROOT/.venv/bin/maturin" develop -q --profile "$PROFILE" 2>&1 | tail -n 20) ;;
  build) build_cli ;;
  eval)  shift; build_cli
         "$TARGET/$PROFILE/dopplerprint" eval ${1:+--experiment "$1"} "${@:2}" 2>&1 | tail -n 80 ;;
  lit)   shift; uv run --no-project -q --with pypdf python3 -I "$ROOT/tools/lit.py" "$@" ;;
  # Rebuild the committed PTX on the GPU box (needs nvcc; see crates/dp-core/src/search/gpu/mod.rs).
  ptx)   K=crates/dp-core/src/search/gpu
         "$ROOT/tools/gpu.sh" sync
         for k in sweep emission; do
           "$ROOT/tools/gpu.sh" run "nvcc -ptx -O3 --fmad=false -arch=compute_75 $K/$k.cu -o $K/$k.ptx"
           "$ROOT/tools/gpu.sh" fetch "$K/$k.ptx"
         done ;;
  *) echo "usage: tools/dev.sh check|test [filter]|build|py|eval [experiment.toml]|lit search|fetch ...|ptx" >&2; exit 2 ;;
esac
