# Using dopplerprint

```bash
pip install dopplerprint
dopplerprint locate recording.mp3 --start "2026-05-14 10:02 -07:00" --near "Livermore, CA"
```

`dopplerprint locate --help` lists every flag. The same pipeline is available from Python:

```python
import dopplerprint
r = dopplerprint.locate("recording.mp3", start="2026-05-14 10:02 -07:00", near="Livermore, CA", yes=True)
r["status"], r["location"]          # "found", {"lat": ..., "lon": ..., "osm_url": ...}
```

## What it needs

<a id="time"></a>**When** (`--start`): the local time the recording started, with its UTC offset.
`"2026-05-14 10:02 -07:00"`, `"2026-05-14T17:02:00Z"`, `"... UTC-7"` and bare UNIX seconds all work. Add
`"±10m"` (or `+-10m`, or `--start-tolerance 10m`) when unsure. The default is ±2 min: it covers a time typed
to the minute plus ordinary phone clock error, and every extra minute lets more unrelated flights fit the
passes (the search tries every flight whose implied start lands in the window). The fitted start is reported,
so a wrong guess shows up as a large clock offset.

Without `--start`, the file's own timestamp is used: MP4/M4A/MOV `creation_time`, or a date tag that has a
time and a UTC offset. Devices stamp either the start or the end of a recording, so the window then spans
both. A tag without a time zone is not guessed at; dopplerprint asks for `--start` instead.

<a id="where"></a>**Where** (`--near`): a place name (`"Livermore, CA"`, looked up once on OpenStreetMap
Nominatim and cached), `lat,lon`, `lat,lon,radius_km`, or a box `west,south,east,north`. The default radius is
10 km (`--radius` to change, max 50): search time grows with the area (a 10 km radius is ~8,000 cells of
200 m before refinement), and an aircraft is rarely heard from further than that. Coverage is the contiguous
US for now (the embedded geoid and the EPSG:5070 lattice); whole-country search is not supported yet.

## Data and the cache

<a id="cache"></a>Everything downloaded lives in one cache: `--cache DIR`, else `$DOPPLERPRINT_CACHE`, else
`$XDG_CACHE_HOME/dopplerprint`, else `~/.cache/dopplerprint`.

| Path | Contents |
|---|---|
| `adsb/store/date=YYYY-MM-DD/` | Processed ADS-B days (docs/DATA.md), ~1.5 GB each |
| `adsb/raw/<release>/` | adsb.lol archive parts while downloading (deleted after processing unless `--keep-raw`) |
| `typedb/` | tar1090-db aircraft types and registry (~8 MB, fetched once) |
| `geocode.json` | Place-name lookups |
| `weather/openmeteo-*.json` | Archived hourly weather profiles, one per ~1 km site and UTC day (~10 kB) |
| `dem/*.u16` | Optional ground models from `python/dopplerprint/prep/dem.py`; the first one covering the area is used |

ADS-B comes from the adsb.lol `globe_history` daily archives (ODbL). A day the cache doesn't have is
downloaded (~3.5 GB per UTC day the time window touches) after a yes/no question showing its size; `--yes`
skips the question, `--offline` never downloads, and `--max-cache-gb` (default 20) refuses a download that
would grow the cache past it. Downloads resume after an interruption. Without a DEM the ground is taken as
flat at the local geoid height, and the report says so.

<a id="weather"></a>**Weather.** The search models sound with the wind and temperature at the recording's time
and place: hourly temperature, humidity and wind on the 10 m, 80–180 m and 1000–300 hPa levels from the
[Open-Meteo](https://open-meteo.com/) Historical Forecast API (`best_match`: NOAA HRRR, 3 km, over the US),
interpolated to the middle of the recording. Each pass uses the mean wind vector and sound speed of the layer
from the ground to the aircraft (docs/PHYSICS.md §4a); the report's `Air:` line gives what was used, e.g.
"wind 6 m/s from 250° averaged 0–900 m; c 341.2 m/s". One request per UTC day and site; the only things sent
are latitude and longitude rounded to 0.01° (~1 km) and the date. `--no-weather` searches in still air
(c 343 m/s, or the `--config` atmosphere); `--offline` uses cached weather only and otherwise falls back to
still air with a note. A day is cached once it is two days old (the archive fills in over the first day).
Terms: data CC BY 4.0; the free API is for non-commercial use, 600 requests/min, 5,000/hour, 10,000/day per
IP, no key; commercial use needs an Open-Meteo API plan. The archive has HRRR from 2018 and most other models
from 2021–22. With weather on, a `--config` `[search.atmosphere]` is replaced, not combined.

<a id="output"></a>
## What you get

The terminal report gives the best location with an OpenStreetMap link, a calibrated 90% region (its area,
and how many separate islands it covers), the probability of each distinct solution (a single pass can't tell
the two sides of its track apart: the "mirror solution"), the aircraft each pass was matched to, and the
recording's start time as fitted. `--json` prints the same report as JSON (`posterior`).

The region is calibrated so that it held the true spot about 90% of the time on held-out recordings: real
recordings for one pass, simulated ones for two or more passes (the report says which). With one aircraft
heard, the mirror is always listed: its probability reflects model error, not evidence about the side.
Calibration and its limits: docs/research/calibration_2026-11-03.md. The JSON also keeps `confidence`, a
heuristic on the score gap to the next distinct spot (z units: **high** ≥ 6, **medium** ≥ 2, **low** below);
it is not a probability.

Files go to `./dopplerprint-results/<name>/` (`--out` to change):

| File | Contents |
|---|---|
| `result.html` | Single-file map: score heatmap, candidate spots, tracks, passes and the spectrogram |
| `result.geojson` | Cells, candidates, tracks and passes (WGS84) |
| `result.gpkg` | The same layers for QGIS, styled (docs/GEO.md) |
| `result.json` | The report |

Exit codes: 0 found, 1 error, 2 needs input (a missing time or place, or a download to confirm), 3
inconclusive (no passes heard, no flights in the window, or a match too weak to trust; the report says
which).

<a id="browser-app"></a>
## Browser app

```bash
dopplerprint serve --open
```

This opens a local page at `http://127.0.0.1:8737/` that runs the same pipeline as `locate`. Drop a
recording on it, say when it started (a rough time is fine; widen "give or take" to match) and roughly where.
Then press **Locate**. The page shows progress (including any ADS-B download) and then the result: the best
location and its confidence level, the mirror solution if one scores nearly as well, the aircraft it matched,
the `result.html` map, and GeoJSON/JSON downloads. If the time or place is missing, the page marks the field.
Fill it in and press **Run again**; the recording doesn't have to be uploaded again.

The page can't ask the yes/no download question, so starting the server counts as the yes: a missing ADS-B
day is downloaded into the [cache](#cache) unless that would pass `--max-cache-gb`.
`--allow-download false` never downloads, and a job that needs data then asks for it instead.
`--demo` serves canned results without running anything, to try the page with no data.

| Option | Default | |
|---|---|---|
| `--allow-download` | `true` | `false`: use only what is already in the cache. |
| `--cache`, `--max-cache-gb`, `--config` | as for `locate` | |
| `--demo` | off | Canned results; no audio is analysed. |
| `--port` | `8737` | If it is taken, the next free port up to +10 is used. `0` picks any free port. |
| `--host` | `127.0.0.1` | Only this computer can reach the page. `0.0.0.0` exposes it to your network with **no authentication**. |
| `--jobs` | `<cache>/jobs` | One folder per recording: `job.json` (form, status, progress, outcome), the audio, `results/`. |
| `--max-upload-mb` | `1024` | Larger uploads are refused. |
| `--open` | off | Opens the page in your default browser. |

Recordings are analysed one at a time, in the order they were submitted. Jobs survive a restart: queued
jobs resume, and a job that was running when the app stopped is marked as failed and can be run again.
Delete a job's folder under `--jobs` to forget it.

Uploads are checked by content (WAV, MP3, FLAC, OGG, M4A/AAC) and not by file name. They are stored
under a server-chosen name. The API (`/api/jobs`) is described at the top of `crates/dp-serve/src/http.rs`.
