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
| `dem/*.u16` | Optional ground models from `python/dopplerprint/prep/dem.py`; the first one covering the area is used |

ADS-B comes from the adsb.lol `globe_history` daily archives (ODbL). A day the cache doesn't have is
downloaded (~3.5 GB per UTC day the time window touches) after a yes/no question showing its size; `--yes`
skips the question, `--offline` never downloads, and `--max-cache-gb` (default 20) refuses a download that
would grow the cache past it. Downloads resume after an interruption. Without a DEM the ground is taken as
flat at the local geoid height, and the report says so.

<a id="output"></a>
## What you get

The terminal report gives the best location with an OpenStreetMap link, a confidence level, other spots
that score nearly as well (a single pass can't tell the two sides of its track apart: the "mirror
solution"), the aircraft each pass was matched to, and the recording's start time as fitted. `--json` prints
the same report as JSON.

Confidence is a heuristic on the score gap to the next distinct spot (z units): **high** ≥ 6 or nothing
close, **medium** ≥ 2, **low** below that. It is not a probability.

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
