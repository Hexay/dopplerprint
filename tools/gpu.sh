#!/usr/bin/env bash
# Rented GPU box: `tools/gpu.sh sync` pushes the tree (eval cases only, no ADS-B store), `tools/gpu.sh run CMD...`
# runs CMD in the remote repo, `tools/gpu.sh fetch PATH...` copies repo-relative files back. Host comes from $GPU_HOST or the gitignored .gpu-host ("user@ip [port]").
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
read -r HOST PORT <<< "${GPU_HOST:-$(cat "$ROOT/.gpu-host")}"
PORT="${PORT:-22}"
SSH=(ssh -o BatchMode=yes -p "$PORT" "$HOST")
REMOTE=/root/doppler_print

case "${1:-}" in
  sync)
    RS=(rsync -az --delete -e "ssh -o BatchMode=yes -p $PORT")
    "${RS[@]}" --exclude /data/ --exclude target/ --exclude .git/ --exclude .venv/ --exclude runs/ --exclude .gpu-host \
      "$ROOT/" "$HOST:$REMOTE/"
    # Gitignored data/ lives only in the main checkout, not in worktrees.
    DATA="$(cd "$(git -C "$ROOT" rev-parse --git-common-dir)/.." && pwd)/data"
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
