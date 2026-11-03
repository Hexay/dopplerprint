# dopplerprint

[![CI](https://github.com/Hexay/dopplerprint/actions/workflows/ci.yml/badge.svg)](https://github.com/Hexay/dopplerprint/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Work out where an audio recording was made from the aircraft you can hear in it.

![Left: spectrogram of a light aircraft flyover with its Doppler-shifted engine tone and the fitted curve. Right: relative-likelihood heatmap over aerial imagery, peaking next to the true recording location, with a faint mirror patch across the flight track.](docs/img/hero.png)

<sub>A real flyover at Livermore, CA. The heatmap peaks next to the true microphone position (white dot); the faint
patch across the flight track is the mirror solution that a single pass cannot rule out.</sub>

A passing propeller aircraft or helicopter leaves a tone in the recording whose pitch falls as it goes by. The
shape of that curve fixes the aircraft's speed, its closest distance and the moment it was closest. dopplerprint
fits those curves, looks up which aircraft were flying at that time in public ADS-B archives, and searches the
ground for the point that would have heard every one of them at the right time and pitch.

**Status: early research software.** The method works end to end on real recordings when the start time is known
to within seconds and the area to within a few kilometres (see [Accuracy](#accuracy)). A one-command `locate` tool
is in progress; today the pipeline runs through the evaluation harness.

## Accuracy

All numbers are distance from the best-scoring point to the known microphone position, on real recordings and
real ADS-B data.

| Benchmark | Setup | Result |
|---|---|---|
| **Livermore, CA**: 40 recordings, one light-aircraft flyover each ([Pathak 2026](https://zenodo.org/records/23424278)) | Start time known to ±2 s; 4 km search radius, not centred on the microphone | Right aircraft in **34/40**. Median error **207 m**; 5 within 100 m, 27 within 500 m. |
| **Oyster Bay, NY**: colsto's recording, two flyovers | Same setup as DOPPLERGEIST's published replay (±5 s, 5.6 km radius) | **79 m** (DOPPLERGEIST: 52 m on the same inputs) |

What these numbers do and don't show:

- **The flyover's position in the recording is supplied**, not discovered: these runs test the location search, not
  automatic detection. Automatic discovery exists (`doppler-print discover`) but is not yet part of the benchmark.
- **One flyover leaves two mirror solutions**, one on each side of the flight track, and trades position along the
  track against the clock. Most errors over 1 km are one of these. A second aircraft usually resolves them.
- **The 6 misses** picked a different aircraft and land 2–5 km away.
- **Each case runs in 3–25 s** on a desktop CPU.
- **The Oyster Bay reference is a public approximate coordinate**, not a surveyed position.

## Install

dopplerprint is not on PyPI yet. Build from source with Rust ≥ 1.90:

```bash
git clone https://github.com/Hexay/dopplerprint
cd dopplerprint
cargo build --release              # CLI: target/release/doppler-print
pip install .                      # Python package (Python ≥ 3.11), optional
```

WAV, FLAC, MP3, AAC/M4A and Ogg Vorbis are decoded natively; no ffmpeg needed.

## Quick start

```bash
# Find aircraft flyovers in a recording
target/release/doppler-print discover recording.wav --out events.json

# Build an ADS-B track store from one adsb.lol daily archive (see docs/DATA.md)
target/release/doppler-print ingest v2026.09.08-planes-readsb-prod-0.tar.aa --out data/adsb/store

# Run the benchmark (needs the validation corpus: data/MANIFEST.md)
tools/dev.sh eval experiments/oysterbay_dg_parity.toml
```

The eval writes a summary table and per-case results to `runs/<timestamp>/`.

## How it works

1. **Audio.** Decode, whiten and build a log-frequency spectrogram. Find tonal ridges and fit a constant-velocity
   Doppler curve to each: rest frequency, speed, closest-approach distance and time.
2. **ADS-B.** Stream adsb.lol daily archives into a compact Parquet store. Jets are kept but flagged, since their
   tones are rarely visible.
3. **Search.** Score candidate locations coarse to fine (200 m down to 25 m). Each candidate must explain every
   flyover under one shared clock offset, and loses score for loud aircraft it should have heard but didn't.
4. **Output.** Ranked locations with a relative-likelihood map, as GeoPackage (with QGIS styles) and GeoJSON.

Details: [architecture](docs/ARCHITECTURE.md), [physics](docs/PHYSICS.md), [audio](docs/AUDIO.md),
[ADS-B store](docs/DATA.md), [geodesy and terrain](docs/GEO.md). The research behind each choice is in
[`docs/research/`](docs/research/).

## Responsible use

This tool can place a recording, and so the person who made it, on a map. Use it on your own recordings, on
material published for this purpose, or with the recordist's consent. Don't use it to find, track or expose people.

## Credits and data

- Method: colsto's video [*"This background sound leaks your exact location"*](https://www.youtube.com/watch?v=UytYrhYLOKI).
  [DOPPLERGEIST](https://github.com/1337isnot1337/DOPPLERGEIST) (MIT) is an independent implementation and our
  comparison baseline.
- ADS-B: [adsb.lol](https://adsb.lol), © adsb.lol contributors, [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/).
  Track stores built from it, and the test fixtures in `crates/dp-core/tests/fixtures/adsb/`, stay under ODbL.
- Aircraft types: [tar1090-db](https://github.com/wiedehopf/tar1090-db).
- Terrain: USGS 3DEP (public domain); fallback Copernicus GLO-30, © DLR e.V. 2010–2014 and © Airbus Defence and
  Space GmbH 2014–2018, provided under COPERNICUS by the European Union and ESA.
- Geoid: NGA EGM2008 (public domain) via [PROJ-data](https://cdn.proj.org/); `assets/geoid/` is a CONUS crop.
- Livermore recordings: Praveen Pathak, [Zenodo 23424278](https://zenodo.org/records/23424278), CC BY 4.0.
  Header image imagery: USGS The National Map / NAIP, public domain.

## Licence

MIT, see [LICENSE](LICENSE). Data keeps its own licence (above). Contributions welcome: [CONTRIBUTING.md](CONTRIBUTING.md).
