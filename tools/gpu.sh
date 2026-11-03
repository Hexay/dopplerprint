#!/usr/bin/env bash
# Rented GPU box: `tools/gpu.sh sync` pushes the tree (eval cases only, no ADS-B store), `tools/gpu.sh run CMD...`
# runs CMD in the remote repo, `tools/gpu.sh fetch PATH...` copies repo-relative files back. Host comes from $GPU_HOST or the gitignored .gpu-host ("user@ip [port]").
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Gitignored .gpu-host and data/ live only in the main checkout, not in worktrees.
MAIN="$(cd "$(git -C "$ROOT" rev-parse --git-common-dir)/.." && pwd)"
read -r HOST PORT <<< "${GPU_HOST:-$(cat "$MAIN/.gpu-host")}"
PORT="${PORT:-22}"
SSH=(ssh -o BatchMode=yes -p "$PORT" "$HOST")
REMOTE=/root/doppler_print

case "${1:-}" in
  sync)
    RS=(rsync -az --delete -e "ssh -o BatchMode=yes -p $PORT")
    # The main checkout also holds .uv-cache/, reference/ and .claude/worktrees/ (2.5 GB, 35k files); the box needs none.
    "${RS[@]}" --exclude /data/ --exclude target/ --exclude .git/ --exclude .venv/ --exclude runs/ --exclude .gpu-host \
      --exclude /.claude/ --exclude /.uv-cache/ --exclude /reference/ --exclude /research_notes/ \
      --exclude .ruff_cache/ --exclude __pycache__/ --exclude '*.so' --exclude /docs/research/samples/ \
      "$ROOT/" "$HOST:$REMOTE/"
    DATA="$MAIN/data"
    # ADS-B store: only the days of cases that read it (oysterbay parity); other cases carry their own adsb/.
    "${RS[@]}" --include /adsb/ --include /adsb/store/ --include '/adsb/store/date=2026-09-08/***' \
      --exclude '/adsb/**' --exclude /_downloads/ "$DATA/" "$HOST:$REMOTE/data/"
    ;;
  run)
    shift
    "${SSH[@]}" "export PATH=\$HOME/.cargo/bin:/usr/local/cuda/bin:\$PATH; cd $REMOTE && $*"
    ;;
  fetch)  # repo-relative paths back from the box
    shift
    for f in "$@"; do scp -q -o BatchMode=yes -P "$PORT" "$HOST:$REMOTE/$f" "$ROOT/$f"; done
    ;;
  *) echo "usage: tools/gpu.sh sync | run CMD... | fetch PATH..." >&2; exit 2 ;;
esac
