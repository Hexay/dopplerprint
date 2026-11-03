# Geodesy, terrain and GIS outputs

Design rationale: `docs/research/geo_compute_stack.md` ("Design implications"). This file is the contract for the formats.

## Projection: EPSG:5070

- `dp_core::geo::albers::{forward, inverse}`: closed-form ellipsoidal Albers (Snyder PP 1395), GRS80, lat_0 23°,
  lon_0 −96°, standard parallels 29.5°/45.5°, no false origin. WGS84 lat/lon go in as if NAD83 (~1 m, ignored).
- The lattice `Cell { ix, iy, step_m }` covers `[ix·step, (ix+1)·step) × [iy·step, (iy+1)·step)`.
- Validate: `.venv/bin/python -I tools/geo_validate.py` (needs `tools/dev.sh py` and pyproj) compares 1000
  random CONUS points with pyproj `EPSG:4269 → EPSG:5070`. Gates: forward < 1 mm, inverse and round trip < 1e-9°,
  geoid < 1 cm. Hard-coded pyproj values live in the `albers` and `dem::geoid` unit tests.

## Geoid asset

- `assets/geoid/egm2008_conus_2p5.bin`, embedded with `include_bytes!` by `dem::geoid_undulation`.
- Source: NGA EGM2008 (public domain), as PROJ-data's `us_nga_egm08_25.tif` (2.5′ grid, pixel-is-point,
  WGS84 → EGM2008 height, from `https://cdn.proj.org/us_nga_egm08_25.tif`, file dated 2018-10-08 from
  `egm08_25.gtx`). Cached at `data/dem/egm2008/`.
- Built by `python -I python/dopplerprint/prep/geoid.py <src.tif> assets/geoid/egm2008_conus_2p5.bin`.
- Crop: 24–50° N, 125.5–66° W at 2.5′ (625 × 1429 nodes). N ranges −52.5 to −7.1 m in the crop.
- Layout, little-endian: `b"DPGEOID1"`, f64 lat_south, f64 lon_west, f64 step_deg, u32 nrows, u32 ncols,
  f32 scale, f32 offset, then `nrows × ncols` u16, row 0 = south, N = raw·scale + offset (1 mm steps).
- Lookup is bilinear, the same as PROJ's, so it matches pyproj up to quantisation (≤ 0.5 mm). Outside the crop it
  clamps to the edge, which is wrong; the corpus is all inside CONUS.

## DEM raster (`dp-dem-u16-v1`)

- Built by `python -m dopplerprint.prep.dem` (needs the `prep` extra). Run with `DP_DATA=<repo>/data`:
  - case region: `--case data/cases/<id> --radius-km 25` (default step 30 m, bilinear)
  - CONUS: `--bbox -125 24 -66 50 --step 100 --name conus_100m` (average resampling, ~2.7 GB, streams ~40 GB of
    tiles; processed in 1024-row strips so memory stays at a few hundred MB)
- Sources, per 1° tile in preference order (`--sources`):
  1. USGS 3DEP 1″ bare earth (NAVD88), `prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1/TIFF/current/`,
     cached in `data/dem/3dep1/`.
  2. Copernicus GLO-30 DSM (EGM2008), `copernicus-dem-30m.s3.amazonaws.com`, cached in `data/dem/glo30/`. It
     includes canopy and buildings, so it reads high in forest.
  - A 403/404 leaves a `.missing` marker (open ocean); those nodes stay nodata.
- Outputs `<out-dir>/<name>.u16` and `<name>.json` (default out-dir `data/dem/5070/`).
  - Row-major little-endian u16, row 0 = north. Node (row, col) = `(x0 + col·step, y0 − row·step)`, and
    x0/y0 are multiples of step, so 200/100 m lattice centres land on 100/50 m nodes.
  - Height (m MSL) = raw·scale_m + offset_m with scale 0.1 and offset −100 (−100 to 6453 m); raw 0 = nodata.
  - Row-major rather than tiled: one 4 KB page is 2048 nodes (205 km at 100 m), so a 50 km work tile touches
    ~500 pages.
- `dp_core::dem::Dem::open` memory-maps it. `height_msl(Xy)` is bilinear; nodata neighbours are dropped and the
  weights renormalised; `None` off-raster. Also `height_msl_latlon` and `height_ellipsoid_latlon` (H + N).

## GeoPackage (`output::gpkg`)

Approach from thunderprint's writer (MIT): hand-written GPKG 1.4 core tables and GP/WKB blobs on bundled rusqlite.

- SRS 4326 and 5070. Non-point geometries carry an XY envelope in the GP header.
- Every feature layer has an R*Tree `rtree_<t>_geom`, a `gpkg_extensions` row and the spec's maintenance
  triggers (created after the bulk insert).
- `layer_styles` holds a default QML per feature layer (`assets/qml/*.qml`). Cells and candidates are graduated
  on `score_norm` (min–max normalised score, so the class breaks are fixed). Tracks are categorised by `engine`;
  regions by `kind`.

| Layer | Geometry | SRS | Columns |
|---|---|---|---|
| `cells` | Polygon (cell square) | 5070 | ix, iy, step_m, level, score, score_norm, in_cr90, score_e1..N |
| `candidates` | Point | 4326 | rank, score, score_norm, posterior, clock_offset_s, reliable, flight_ids, score_e1..N, flight_e1..N |
| `tracks` | LineString Z (ellipsoidal m) | 4326 | track_id, icao24, type_code, engine, t_start, t_end, n_points |
| `events` | none (attributes) | none | idx, name, t_cpa_s, f0_hz, band_lo_hz, band_hi_hz, snr_db, flight_id |
| `regions` | Polygon | 4326 | rank, kind (`credible90` / `ellipse`), mass, area_km2 |

- Rows come from `output::features::Results`, which the search stage fills.
- `output::write_all(dir, &results, spectrogram)` writes `results.gpkg`, `results.geojson` and `viewer.html`.
- Check a file with `.venv/bin/python -I tools/gpkg_check.py <file.gpkg>`. It runs sqlite3 checks, then opens the
  file with GDAL via pyogrio if that is installed. Demo file:
  `cargo test -p dp-core write_demo_outputs -- --ignored` writes `target/demo_outputs/`.

## Viewer

- `assets/viewer.html` is a single-file Leaflet 1.9 page (adapted from thunderprint) with Esri tiles.
  `output::viewer` fills its `/*DATA*/null` slot with `{fc, spectrogram}`.
- Cells are coloured by combined or per-event score, plus tracks by engine, candidates with popups, regions, and an
  event list.
- A `SpectrogramImage` PNG goes in the bottom panel when one is supplied.

## QGIS plugin

`python/dopplerprint/qgis/` is a Processing provider skeleton. Install it by copying the directory into the QGIS
plugins folder under a name other than `qgis` (e.g. `doppler_print_qgis`), and `pip install doppler-print` into
QGIS's Python. "Locate recording" calls `doppler_print.search(...)` (not implemented yet) and loads the GPKG layers.
