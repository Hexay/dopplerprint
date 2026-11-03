#!/usr/bin/env bash
# Dev runner: check | test [filter] | py | eval [experiment.toml]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
MANIFEST="$ROOT/Cargo.toml"

case "${1:-}" in
  check) cargo clippy --manifest-path "$MANIFEST" --workspace --all-targets -q 2>&1 | tail -n 60 ;;
  test)  shift; cargo nextest run --manifest-path "$MANIFEST" -p dp-core --cargo-quiet --status-level fail ${1:+-E "test(/$1/) | binary(/$1/)"} 2>&1 | tail -n 60 ;;
  py)    (cd "$ROOT" && VIRTUAL_ENV="$ROOT/.venv" "$ROOT/.venv/bin/maturin" develop -q --release 2>&1 | tail -n 20) ;;
  eval)  shift; cargo run --manifest-path "$MANIFEST" -q --release -p dp-cli -- eval ${1:+--experiment "$1"} 2>&1 | tail -n 80 ;;
  lit)   shift; uv run --no-project -q --with pypdf python3 -I "$ROOT/tools/lit.py" "$@" ;;
  # Rebuild the committed PTX on the GPU box (needs nvcc; see crates/dp-core/src/search/gpu/mod.rs).
  ptx)   K=crates/dp-core/src/search/gpu
         "$ROOT/tools/gpu.sh" sync
         for k in sweep emission; do
           "$ROOT/tools/gpu.sh" run "nvcc -ptx -O3 --fmad=false -arch=compute_75 $K/$k.cu -o $K/$k.ptx"
           "$ROOT/tools/gpu.sh" fetch "$K/$k.ptx"
         done ;;
  *) echo "usage: tools/dev.sh check|test [filter]|py|eval [experiment.toml]|lit search|fetch ...|ptx" >&2; exit 2 ;;
esac
