#!/usr/bin/env bash
# Round-trip sim WAVs through real codecs (codec_measurements.md chains), keeping truth.json beside each.
# Usage: tools/sim_codec.sh <sim_out_dir>... ; for each <d>/audio.wav writes <d>_<chain>/{audio.wav,truth.json}
# for chain in aac128, opus135, aac128_opus135. Decoded back to s16 WAV at the source rate and channel count.
set -euo pipefail
command -v ffmpeg >/dev/null || { echo "ffmpeg not found" >&2; exit 2; }
ff() { ffmpeg -nostdin -hide_banner -loglevel error -y "$@"; }

for d in "$@"; do
  src="$d/audio.wav"
  [[ -f "$src" ]] || { echo "skip $d: no audio.wav" >&2; continue; }
  sr=$(ffprobe -v error -select_streams a:0 -show_entries stream=sample_rate -of csv=p=0 "$src")
  ch=$(ffprobe -v error -select_streams a:0 -show_entries stream=channels -of csv=p=0 "$src")
  tmp=$(mktemp -d)
  # AAC-LC encodes at 44.1 kHz, Opus at 48 kHz, as the platforms do; ffmpeg trims priming/pre-skip.
  ff -i "$src" -ar 44100 -c:a aac -b:a 128k "$tmp/a.m4a"
  ff -i "$src" -ar 48000 -c:a libopus -b:a 135k "$tmp/o.webm"
  ff -i "$tmp/a.m4a" -ar 48000 -c:a libopus -b:a 135k "$tmp/ao.webm"
  for pair in aac128:a.m4a opus135:o.webm aac128_opus135:ao.webm; do
    name=${pair%%:*}; file=${pair#*:}
    out="${d%/}_$name"
    mkdir -p "$out"
    ff -i "$tmp/$file" -ar "$sr" -ac "$ch" -c:a pcm_s16le "$out/audio.wav"
    [[ -f "$d/truth.json" ]] && cp "$d/truth.json" "$out/"
    [[ -f "$d/enf.csv" ]] && cp "$d/enf.csv" "$out/"
  done
  rm -rf "$tmp"
  echo "$d: aac128 opus135 aac128_opus135"
done
