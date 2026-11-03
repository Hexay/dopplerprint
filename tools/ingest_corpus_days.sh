#!/usr/bin/env bash
# Ingest every downloaded adsb.lol day into data/adsb/store (skips days already present). Sequential: ~2 min, ~2.5 GB RSS each.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --manifest-path "$ROOT/Cargo.toml" -q --release -p dp-cli
BIN="$ROOT/target/release/dopplerprint"
for d in "$ROOT"/data/_downloads/adsblol/v*-planes-readsb-*; do
  v="$(basename "$d" | sed -E 's/^v([0-9]{4})\.([0-9]{2})\.([0-9]{2}).*/\1-\2-\3/')"
  if [[ -d "$ROOT/data/adsb/store/date=$v" ]]; then echo "skip $v"; continue; fi
  echo "ingest $v"
  "$BIN" ingest "$d" --out "$ROOT/data/adsb/store" --typedb "$ROOT/data/typedb" 2>&1 | tail -n 3
done
