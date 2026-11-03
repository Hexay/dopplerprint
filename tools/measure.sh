#!/usr/bin/env bash
# Measurement harness runner: tools/measure.sh build | <module::test>, e.g. picket::picket_fence, fitbias::cv_fit_bias, drift::drift_bias, codec::codec_robustness, candan::candan_clean_tone_residual
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
ARGS=(--manifest-path "$ROOT/Cargo.toml" -p dp-core --release --lib)
case "${1:-}" in
  build) cargo test "${ARGS[@]}" --no-run -q 2>&1 | grep -E "^(error|warning)|-->" | head -n 40 || true ;;
  "") echo "usage: tools/measure.sh build|<test>" >&2; exit 2 ;;
  *) cargo test "${ARGS[@]}" -q -- --ignored --nocapture --test-threads=1 --exact "events::measure::$1" 2>&1 \
       | grep -vE "^(running|test result|$)" ;;
esac
