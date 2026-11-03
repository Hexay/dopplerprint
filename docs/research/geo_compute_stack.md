# doppler_print: geo + compute stack research

Scope: DEMs and geoids, fast distance math, candidate grid design, the Rust/Python crate stack, QGIS output, and
atmosphere data. Researched 2026-11-03. Crate versions come from the crates.io API on that date. Method context:
`docs/research/transcript/analysis.md`. Reference implementation: `reference/thunderprint`, which uses rusqlite 0.40
(bundled), rustfft 6.2, hound, rayon 1.10, and hand-written GPKG with GP-header WKB in EPSG:4326 and no rtree or styles.

Items marked **[verify]** were not confirmed from a primary source during this pass.

---

## 1. DEMs for CONUS

### 1.1 Candidate datasets

| Dataset | Posting | Surface | Vertical datum | Tiling / format | Access | Notes |
|---|---|---|---|---|---|---|
| USGS 3DEP 1" (~30 m) | 1 arc-sec | **bare earth** | NAVD88 (CONUS) | 1°×1° GeoTIFF, 3612² px (6 px overlap), `USGS_1_n40w106.tif`; seamless `USGS_Seamless_DEM_1.vrt` | `s3://prd-tnm/StagedProducts/Elevation/1/TIFF/current/` (public HTTPS, no auth) | Best ground truth for CONUS. About 900–1000 tiles, roughly 30–50 MB each, so ~40 GB in total **[verify]** |
| USGS 3DEP 1/3" (~10 m) | 1/3 arc-sec | bare earth | NAVD88 | 1° tiles, 10812² px, `.../Elevation/13/TIFF/` | same bucket | ~9× the 1" volume (hundreds of GB). Overkill for a grid spacing of 100 m or more |
| Copernicus GLO-30 | 1" (lon spacing widens above 50°N) | **DSM** (canopy and buildings included) | EGM2008 (EPSG:3855) | 1° COG, 3600² px, `s3://copernicus-dem-30m/Copernicus_DSM_COG_10_N40_00_W106_00_DEM/` | AWS Open Data, anonymous | Global and consistent. Canopy bias of +10–30 m in forest. GLO-90 is the 3" version (~1/9 the size) |
| SRTM GL1 v3 | 1" | mostly DSM (C-band, partly penetrates canopy) | EGM96 | 1° HGT | LP DAAC (Earthdata login), OpenTopography | Acquired Feb 2000, has voids. Superseded by NASADEM |
| NASADEM | 1" | mostly DSM | EGM96 **[verify]** | 1° HGT/NetCDF | LP DAAC, OpenTopography | Reprocessed SRTM with voids filled |
| FABDEM | 1" | bare earth (Copernicus with forests and buildings removed) | EGM2008 | 1° tiles | Univ. Bristol | **CC BY-NC-SA**, so not usable commercially |

Recommendation: use 3DEP 1" for CONUS and GLO-30 as the fallback outside CONUS (or near borders and coasts). The
observer is on the ground and the aircraft altitude is MSL or HAE, so a DSM in forest overstates ground height by
the canopy. That bias goes straight into slant range. 3DEP is bare earth, so it avoids this.

### 1.2 Does the vertical datum matter?

- NAVD88 vs EGM2008 orthometric heights: they differ by ~0.5–1.5 m across CONUS (NAVD88 is tilted). EGM96 vs
  EGM2008 differ by <~1–2 m in CONUS. **Irrelevant** at our error scale. ADS-B altitude is quantised to 25 ft
  (7.6 m), 100 ft for Gillham.
- **What does matter is the aircraft altitude reference** (see 1.4). Those errors run to tens or hundreds of metres.

### 1.3 Sampling billions of points fast

Key point: **the hot loop never touches a DEM.** Ground elevation is a property of a lattice node, so precompute it
once for the lattice and store it as a dense array aligned with the grid. During the CPA sweep the elevation lookup
is then a single indexed load.

- **Derived product:** an EPSG:5070 raster at 100 m with `-r average` (or `-r bilinear`), built from the 3DEP 1"
  seamless VRT. The 200 m lattice is every second node, and 50 m refinement uses bilinear interpolation of the
  100 m raster or a resample from source on demand.
  - Size: the 5070 CONUS bbox is ~4620 km × 2900 km, so 46.2k × 29.0k nodes = 1.34e9 at 100 m.
  - Encoding: u16 decimetres with a −100 m offset covers −86 m (Death Valley) to 4421 m (Whitney), max 45210 dm.
    That gives **2.7 GB at 100 m and 0.67 GB at 200 m** on disk.
  - Access: `memmap2`, so only the tiles you touch are resident.
  - Layout: store it tiled (e.g. 512×512 node blocks, ~0.5 MB each) rather than row-major so a 50 km work tile
    touches a few pages.
- **Build it in Python, once:** `rasterio`/GDAL wheels with `gdalwarp` over `/vsicurl/` + the seamless VRT, or a
  lazy per-1°-tile cache that fetches a 3DEP tile only when a work tile needs it.
  - A full CONUS pass streams ~40 GB **[verify]**. The lazy cache fetches only areas near prop traffic. Write our
    own raw-tile format plus a small JSON header (origin, spacing, CRS, scale and offset).
- **Throughput if bilinear is needed in Rust:**
  - Bilinear on an in-RAM f32 tile is 4 loads + 3 lerps, ~10^8–10^9 samples/s/core when queries are sorted by tile
    (Morton or tile order) **[estimate]**.
  - Unsorted random access across GBs becomes TLB/page-fault bound, so always sort queries by tile first.
- **COG over HTTP from Rust:** `async-tiff` 0.4 (Development Seed) with `object_store` 0.14 can range-read COG tiles.
  `tiff` 0.11 is the pure-Rust baseline decoder (tiled, LZW/Deflate, predictors). `geotiff` 0.1 is thin.
  Not needed if preprocessing happens in Python.

### 1.4 Aircraft altitude to orthometric (MSL) height

ADS-B carries two altitudes:

- **Barometric (pressure altitude)**, referenced to 1013.25 hPa / 29.92 inHg.
  - QNH correction is ≈ +1000 ft per +1 inHg (≈ 8.3 m/hPa near the surface). A 10 hPa offset gives ~80 m.
  - Temperature correction scales height above the reporting station by T/T_ISA, about 4% per 10 °C. On a warm
    September day (ISA+15) an aircraft 1500 m above the station is ~75 m higher than indicated.
  - **Use baro only with QNH (from METAR) plus a temperature correction.**
- **Geometric (GNSS)**, which DO-260B/UAT report as **height above the WGS84 ellipsoid (HAE)**.
  - readsb/tar1090 expose it as `alt_geom`. OpenSky exposes it as `geoaltitude`.
  - Some v0/v1 transponders and some aggregators mix HAE and MSL **[verify per source]**.
  - Orthometric H = h − N, where the geoid undulation N runs from about −8 to −40 m across CONUS (more negative
    toward the east and north).
- **Geoid grids** (all on `cdn.proj.org` as GeoTIFF, readable by any TIFF reader):
  - `us_nga_egm08_25.tif` (EGM2008 2.5′, global, ~75 MB raw)
  - `us_nga_egm96_15.tif` (EGM96 15′, ~2 MB)
  - `us_noaa_g2018u0.tif` (GEOID18 CONUS, a hybrid that maps NAD83(2011) ellipsoid to NAVD88)
  - GeographicLib also ships `egm2008-5.pgm` and `egm2008-2_5.pgm`.
  - **Recommendation:** crop EGM2008 5′ or 2.5′ to CONUS once (25–50°N × 125–66°W at 5′ is 300×708 f32 ≈ 0.85 MB)
    and interpolate bilinearly. GEOID18 adds nothing at our tolerance: WGS84 vs NAD83 ellipsoid heights differ by
    ~1 m. GEOID2022/NAPGD2022 (NSRS modernisation) was still in beta in 2025, so ignore it.
- **Policy:** prefer `alt_geom − N`. Fall back to baro + QNH + temperature correction. Carry an altitude-σ per
  sample (e.g. 10 m GNSS, 30–60 m corrected baro) and propagate it into the slant-range tolerance.

---

## 2. Geodesy for 10 km-scale distance math

Numerical check (pyproj WGS84 geodesics, script at `/tmp/claude-1001/.../scratchpad/geo_err.py`, not kept). The
table gives the worst |error| over all azimuths for the horizontal distance between an observer and an aircraft d
apart. "Tile origin" means one projection origin per 50 km work tile, with the observer up to 25 km from it.

| Approximation | d=1 km | d=5 km | d=10 km | d=20 km |
|---|---|---|---|---|
| Equirectangular, sphere R=6371 km, cos(lat_tile), observer at origin (lat 40–49) | 2.5–3 m | 12–15 m | 25–30 m | 51–65 m |
| Equirectangular, ellipsoid M,N at tile origin, observer at origin | 0.03 m | 0.6–0.8 m | 2.5–3.4 m | 10–13 m |
| Same, observer 25 km from tile origin | 2.3–3.2 m | 12–16 m | 23–32 m | 50–68 m |
| Equirectangular, ellipsoid M,N at **pair mid-latitude** | <1 mm | <1 mm | ≤2 mm | ≤14 mm |
| Flat 3D (horizontal geodesic + Δh) vs true ECEF chord, aircraft 1500 m up | 0.07 m | 0.56 m | 1.17 m | 2.34 m |

Error budget context:
- 1 m of path ≈ 2.9 ms of acoustic delay.
- The grid quantisation is up to 141 m (the 200 m grid half-diagonal) and ADS-B altitude quantisation is 7.6 m.
- Target is geometry error under ~1 m.

What the numbers say:
- Spherical equirectangular is **not acceptable**: 25 m at 10 km from radius-of-curvature error alone.
- Fixed-origin equirectangular breaks down because cos(φ) varies across a tile, adding about tan φ · Δφ per unit
  distance.
- Mid-latitude ellipsoidal equirectangular is excellent but needs a cos per pair.

**Recommendation: ECEF, relative to a work-tile origin, in f32.**
- Convert each ADS-B sample (lat, lon, HAE) to ECEF once in f64, subtract the tile origin, and store as f32. Values
  stay within ~±60 km, so the f32 ulp is ≈ 4 mm.
- Store observer lattice nodes the same way (lat/lon from the inverse Albers, plus HAE = DEM + N).
- Distance becomes `sqrt(dx²+dy²+dz²)`: exact straight-line acoustic path, no trig in the hot loop, and 8 lanes
  per AVX2 op.
- Velocity vectors live in the same frame, so the radial velocity v_r = (p−o)·v/|p−o| for the retarded-time
  Doppler model needs no frame juggling either.
- Earth curvature, which a flat ENU would miss by ~1 m at 10 km, comes for free.
- Use local ENU only where a human-facing azimuth or elevation is needed (rotate by the observer's R matrix).

---

## 3. Candidate grid design

### 3.1 Point counts and memory

Lower-48 area ≈ 8.08e6 km². The EPSG:5070 bbox has ~1.34e7 km² of nodes, of which ~60% fall on land in CONUS.

| Spacing | Nodes per km² | CONUS nodes | Dense f32 per field | u16 elevation |
|---|---|---|---|---|
| 200 m | 25 | 2.0e8 | 0.81 GB | 0.40 GB |
| 100 m | 100 | 8.1e8 | 3.2 GB | 1.6 GB |
| 50 m | 400 | 3.2e9 | 13 GB | 6.5 GB |

Never hold dense CONUS fields at ≤100 m inside an 8 GB session. The method is sparse anyway:
- One flight's 10 km disc is π·10² = 314 km², i.e. 7854 nodes at 200 m.
- The swath of a 1 h prop flight is ~200 km × 20 km = 4000 km², i.e. 1e5 nodes.
- Each node sees ~40 segments of 500 m (5 s at 100 m/s), so ~4e6 segment–node evaluations per flight. At 1e4–3e4
  prop flights per day that totals ~1e11 evaluations.
- At ~1e9–1e10 SIMD evaluations/s on 8–16 cores, that is **10 s to a few minutes** **[estimate]**.

### 3.2 Processing pattern (bounded memory)

1. **Work tiles.** Split the 5070 lattice into 50 km × 50 km tiles (62,500 nodes at 200 m). Build an `rstar` index,
   or a uniform bucket grid, of flight *segments* by 2D bbox expanded by 10 km.
2. **Per tile, in parallel with rayon** (one tile per task, scratch buffers reused per thread):
   - Gather the segments that intersect tile + 10 km.
   - Compute CPA per (node, flight) with the closed-form point-to-segment minimum in 3D ECEF, then the delayed CPA
     time `t_cpa + |p−o|/c`.
   - Emit a node-major CSR of `(flight_id: u32, t_delayed: f32 seconds-of-day)`.
3. **Filter immediately inside the tile:** pair offset 920 ± 5 s (sort each node's list by time, two-pointer), speed
   and slant-range gates, and silence. Only survivors (~1e4–1e5 nationwide per the transcript) leave the tile.
   Peak RAM is then roughly threads × one tile's CSR (a few MB to tens of MB) plus the segment index.
4. **Refinement.** Each survivor at 200 m spawns 2×2 (or 3×3 for safety) children at 100 m, then 50 m. Re-run the
   full retarded-time Doppler scoring only on the children. This is an implicit quadtree: node id = (level, ix, iy)
   in 5070, and child = (2ix + dx, 2iy + dy).

### 3.3 H3 vs projected lattice

- **H3** (`h3o` 0.11, pure Rust):
  - Average cell areas: res 9 is 0.105 km² (edge 174 m, centre spacing ≈ 300 m), res 10 is 0.0150 km² (edge 66 m,
    spacing ≈ 114 m), res 11 is 0.0021 km² (edge 25 m).
  - Aperture 7 means spacing steps by √7 ≈ 2.65×, which does not map onto the 200 → 100 → 50 m ladder, and parent
    and child cells do not nest exactly.
  - Every node needs a `cell → latlng` conversion. Rendering needs hexagon polygons, and there is no raster
    equivalent.
  - Advantages: global, near-equal area, nice for aggregation and for sharing.
- **EPSG:5070 (CONUS Albers, NAD83, standard parallels 29.5/45.5)**:
  - Equal-area. Linear scale error is ≲1.3% at the CONUS extremes **[verify]**, so a nominal 200 m step is ~197–203 m,
    which is harmless.
  - Integer (ix, iy) indexing. Exact 2× refinement. Trivially written as a GeoTIFF or GPKG raster or as square
    polygons.
  - Inverse Albers is closed form (~30 lines, or `proj` 0.31, which links libproj).
- **Recommendation:** use the **EPSG:5070 integer lattice** for search and refinement. Use H3 at most for
  output aggregation or interchange. Datum nuance: 5070 is NAD83 and ADS-B is WGS84, a ~1–2 m offset that does not
  matter here.

---

## 4. Rust / Python crate stack (crates.io, 2026-11-03)

| Need | Crate (version) | Verdict |
|---|---|---|
| Parallelism | `rayon` 1.12 | Yes. Use `par_chunks` over tiles and `with_min_len` to avoid over-splitting |
| SIMD | `wide` 1.7 (stable, portable f32x8), `pulp` 0.22 (runtime dispatch AVX2/AVX-512/NEON, used by faer) | Use **`pulp`** if we ship wheels to unknown CPUs (dispatches at runtime). Use `wide` plus `-C target-cpu=x86-64-v3` for local builds. `std::simd` is still nightly-only **[verify]** |
| Arrays | `ndarray` 0.17 + `numpy` (rust-numpy) 0.30 | `numpy` 0.30 tracks `pyo3` 0.30. Zero-copy `PyReadonlyArray1<f32>` in, `PyArray::from_vec` out |
| Python bindings | `pyo3` 0.30.1, maturin | Since 0.26, `allow_threads` is now `Python::detach` and `with_gil` is now `Python::attach`. **Release the GIL around every long Rust call** (QGIS UI and Ctrl-C) |
| abi3 wheels | pyo3 feature `abi3-py310` | One wheel per OS/arch for CPython ≥3.10 via `maturin build --release`, `manylinux_2_28`. Whether rust-numpy works under abi3 needs a spike build early **[verify]**. Fallback: pass `bytes`/buffer protocol or Arrow via PyCapsule |
| Columnar ADS-B | `arrow` / `parquet` 60.1 | Yes. Read only the needed columns (icao24, t, lat, lon, alt_geom, alt_baro, gs, track) with row-group pruning by time. Use `pyo3-arrow` or the Arrow PyCapsule interface to hand tables over zero-copy |
| Spatial index | `rstar` 0.14 | Fine for the segment bbox index. A uniform bucket grid over 5070 (cell = 10 km) is simpler and faster for fixed-radius queries |
| Geometry/geodesy | `geo` 0.33, `geographiclib-rs` 0.2.7 | `geographiclib-rs` for reference geodesics in tests only. The hot path is the hand-rolled ECEF math in section 2 |
| Projections | `proj` 0.31 (links libproj) | Avoid in the core wheel (native dependency). Hand-code Albers 5070 forward/inverse plus geodetic↔ECEF and test them against pyproj |
| Rasters | `gdal` 0.19 (links libgdal) vs `tiff` 0.11 / `async-tiff` 0.4 | **No GDAL in the Rust core.** It is painful to bundle in abi3 wheels and duplicates rasterio. Preprocess in Python (rasterio wheels bundle GDAL) into our raw tiles, then `memmap2` 0.9 in Rust |
| Audio decode | `symphonia` 0.6 (WAV/FLAC/MP3/AAC/Vorbis/ALAC, pure Rust) | Replaces thunderprint's `hound` (WAV only). Recordings will be MP3/AAC/M4A from phones and video. Video containers are out of scope; extract audio with ffmpeg first |
| FFT/STFT | `realfft` 3.5 (on `rustfft` 6.4) | Real-input STFT halves the work. Plan once per thread and reuse the scratch buffer. Long windows (e.g. 2–8 s, Δf ≤ 0.5 Hz) are needed for faint ridges |
| GPKG | `rusqlite` 0.41 (`bundled`) | As thunderprint does: write the GPKG tables directly. The bundled SQLite is needed for `gpkg_rtree_index` triggers; check that bundled libsqlite3-sys enables R*Tree **[verify]** |
| GRIB2 (optional) | `gribberish` 1.9 | Not needed; fetch weather in Python (section 6) |

Memory cap (8 GB per session):
- **Budget rule:** each rayon worker holds one work tile.
- Cap threads with `RAYON_NUM_THREADS` or a `ThreadPoolBuilder` set from a `--max-mem` option.
- mmap the DEM and the ADS-B Parquet-derived columns rather than reading them into the heap.
- Stream ADS-B by time window instead of loading the ~50 GB day dataset. A ±(920 s + 30 s) window around the
  candidate events covers the two-event case; the silence filter needs the inter-event span.
- Use `cargo build -j 4`, mold, and sccache, per machine policy.

---

## 5. QGIS integration

### 5.1 GPKG conventions

- **Feature layers:** thunderprint's writer is a good base (GP header, WKB, `gpkg_contents`,
  `gpkg_geometry_columns`). Add:
  - **SRS 5070** in `gpkg_spatial_ref_sys` (WKT from EPSG). Write lattice cells as exact squares in 5070 or keep
    points in 4326. Do not reproject squares to 4326 polygons.
  - **Spatial index** for layers over ~10k features: an R*Tree virtual table `rtree_<t>_geom` plus a row in
    `gpkg_extensions` (`gpkg_rtree_index`), populated with envelopes. QGIS works without it but pans slowly.
  - **Envelope in the GP header** (flags bit for the XY envelope) so readers can skip work. This is optional.
- **Styles:** QGIS reads table `layer_styles`, with columns:
  - `id`, `f_table_catalog`, `f_table_schema`, `f_table_name`, `f_geometry_column`
  - `styleName`, `styleQML`, `styleSLD`, `useAsDefault`, `description`, `owner`, `ui`, `update_time`
  - With `useAsDefault=1`, a style is applied automatically when the layer loads.
  - Register the table in `gpkg_contents` as `data_type='attributes'`.
  - **Workflow:** style the layers once by hand in QGIS, use "Save Style → in database", then ship those QML strings
    as `include_str!` templates and insert them per layer. Avoid generating QML in code.
- **Layers to emit:**
  - `candidates` (points: score_e1, score_e2, combined, flight pair ids, fitted f0 and offset)
  - `cells_100m` / `cells_50m` (squares, graduated by score)
  - `tracks` (LineString per flight: icao24, type, engine class, and time as M or as attributes)
  - `cpa_lines` (observer to aircraft-at-τ for the winner)
  - `events` (metadata, attributes table)
- **Heatmaps:** three options.
  - QGIS's point **Heatmap renderer** (kernel density in screen space, QML `renderer-v2 type="heatmapRenderer"`) is
    quick but zoom-dependent, so it is unsuitable for comparing scores.
  - **Graduated square cells** are exact.
  - A **score GeoTIFF** (float32, EPSG:5070, singleband pseudocolor) per event and combined is the best fit for
    rasters. Write it from Python (rasterio) or with the `tiff` encoder plus GeoKey tags.
  - The GPKG tiled gridded coverage extension is spec-heavy. Skip it.

### 5.2 QGIS Processing plugin

- **Structure:** plugin package with `QgsProcessingProvider` → one `QgsProcessingAlgorithm` per stage (search,
  refine, render).
- **Parameters:** `QgsProcessingParameterFile` (audio, Parquet), `...DateTime`, `...Extent` (spatial prior),
  `...Number`, `...FileDestination` (`.gpkg`).
- **Execution:**
  - `processAlgorithm` calls `doppler_print.search(...)`. Processing already runs it in a background thread.
  - The Rust side must `detach` the GIL and poll a cancel flag. Pass `feedback.isCanceled` through a callback or an
    `AtomicBool` handle, and report progress via `feedback.setProgress`.
  - Load results with `context.addLayerToLoadOnCompletion`.
- **Install:** `pip install doppler_print` into QGIS's own Python (OSGeo4W / QGIS 3.40+ ship Python 3.12). The abi3
  wheel means one binary covers whichever version QGIS uses. Keep the plugin pure Python and declare the dependency
  in `metadata.txt` (the QGIS plugin dependency mechanism is weak, so document the pip step).

### 5.3 HTML/Leaflet alternative

thunderprint ships `assets/viewer.html` (single file, data embedded as base64). Mirror it:
- Leaflet 1.9 + OSM/Esri imagery tiles.
- Candidates as GeoJSON, or for >50k points a `L.canvas` renderer or a pre-rendered **PNG score overlay**
  (`L.imageOverlay` with EPSG:3857 bounds, computed in Rust/Python).
- Tracks as polylines with a time slider.
- A spectrogram panel with the modelled curve overlaid for the selected candidate (PNG plus a JSON polyline).
- Computation stays in the backend; the viewer only displays.

---

## 6. Weather and atmosphere (speed of sound and wind)

### 6.1 What we need and how much it matters

- **Sound speed:** dry c = 331.3·√(1 + T_C/273.15) m/s, about 20.05·√T_K. Humidity adds about 0.1–0.6%; use
  c ≈ 20.05·√T_v with the virtual temperature T_v.
  - Errors: a 10 °C error is ~1.7% in c, which is ~170 ms over a 10 km path. A 5 m/s wind along the path is ~1.5%.
  - Both shift delayed-CPA times and the Doppler curve, since f = f0·c/(c − v_r).
- **Effective speed along a straight ray:** c_eff(z) = c(z) + **w**(z)·n̂.
  - Travel time t = ∫ ds / c_eff, evaluated by sampling the profile at ~10 heights between ground and aircraft
    altitude.
  - For the hot loop, precompute per tile a 1-D table of the mean slowness ⟨1/c_eff⟩ vs (aircraft height, azimuth
    bin). Refraction curvature is negligible at these elevation angles for scoring; ignore it.
- Temporal and spatial resolution: hourly and ~3–30 km is enough. Interpolate linearly in time between bracketing
  hours.

### 6.2 Sources

| Source | Resolution | Coverage | Access | Notes |
|---|---|---|---|---|
| **HRRR** (NOAA) | 3 km, hourly, CONUS | 2014-07-30 → now (path `conus/` from 2018-07-12) | `s3://noaa-hrrr-bdp-pds/hrrr.YYYYMMDD/conus/hrrr.tHHz.wrfprsf00.grib2` (+`.idx`), anonymous | Use the f00 analysis. The `.idx` gives byte ranges, so fetch only the TMP/UGRD/VGRD/HGT/SPFH messages at 1000–700 hPa (a few MB). **Gotcha: HRRR u/v are grid-relative (Lambert conformal); rotate to earth-relative.** `wrfsfcf00` has 2 m T, 10 m and 80 m wind, and surface pressure. Python: Herbie |
| **ERA5** (ECMWF) | 0.25°, hourly, 37 pressure levels | 1940 → ~5 days ago (ERA5T) | New CDS (legacy CDS shut down Sept 2024; new API keys, `cdsapi` ≥0.7 / `ecmwf-datastores-client`). Also **ARCO ERA5 Zarr**, anonymous: `gs://gcp-public-data-arco-era5/ar/full_37-1h-0p25deg-chunk-1.zarr-v3` | ARCO chunks are one global time step per chunk (~150 MB per variable), which is heavy for a single profile. The Google mirror stopped updating around 2023. The CDS point-request "timeseries" datasets suit single points better |
| **METAR/ASOS** (IEM) | station, 1–60 min | decades | `https://mesonet.agron.iastate.edu/cgi-bin/request/asos.py?station=XXX&data=tmpf,dwpf,drct,sknt,alti&sts=...Z&ets=...Z&tz=UTC&format=onlycomma` | Since 2026-04 there is a **1 req/s per-IP throttle** (429) and a 1000 station-year cap (422). `alti` is the QNH needed for baro altitude correction. ASOS 1-minute data is also available from IEM |
| Radiosondes (IEM RAOB, U. Wyoming) | 00/12 UTC, ~70 US sites | decades | IEM `raob` service | Spot check of lapse rate and inversions only |

### 6.3 Profile sampling recipe

1. Pick the hour bracket and the tile centroid (or survivor centroid).
2. Read HRRR prs f00 for the two bracketing hours via `.idx` byte ranges. Take the HGT, TMP, SPFH, UGRD, VGRD
   columns at the 4 nearest grid points and interpolate bilinearly. Interpolate in time.
3. Convert geopotential height to MSL height, compute T_v → c(z), rotate the winds, and prepend the surface
   (2 m / 10 m) values.
4. Pass `z[], c[], u[], v[]` (≤40 levels) to Rust as numpy arrays. Rust builds the slowness LUT.
5. Separately, take QNH and T from the nearest 1–3 ASOS stations at the event time for the baro-altitude correction
   (section 1.4).

Do all weather fetching in Python (Herbie / xarray / cfgrib, or `eccodes` wheels). It is low volume and keeps GRIB
out of the Rust wheel.

---

## Design implications for doppler_print

**Recommended crates (Rust core):**
- `rayon` 1.12, `pulp` 0.22 (or `wide` 1.7), `memmap2` 0.9, `arrow`/`parquet` 60
- `rstar` 0.14 (or a hand bucket grid), `symphonia` 0.6, `realfft` 3.5, `rusqlite` 0.41 `bundled`
- `pyo3` 0.30 (`abi3-py310`) + `numpy` 0.30, `anyhow`, `serde`
- Test-only: `geographiclib-rs`, `approx`
- **Exclude** `gdal`, `proj`, `h3o` and GRIB crates from the core.

**Python side:**
- `rasterio`/GDAL for the one-off DEM build and the score GeoTIFFs.
- `pyproj` for test oracles.
- `herbie-data` + `xarray`/`cfgrib` for HRRR, `cdsapi` for ERA5, `requests` for IEM.
- Optional `h3` for exports.

**Data formats:**
- ADS-B: Parquet, sorted by (time, icao24), row groups of about 1 h. A derived per-day "prop segments" Parquet
  holds ECEF f64 positions plus velocities, altitude source, and σ.
- DEM: our own tiled u16-decimetre raster in EPSG:5070 at 100 m (`dem_5070_100m.bin` + JSON header, 2.7 GB, mmap),
  built from 3DEP 1" and filled with GLO-30 outside CONUS.
- Geoid: cropped EGM2008 5′ f32 grid (~1 MB) embedded or shipped as a file.
- Lattice: implicit EPSG:5070 integer grid, (level, ix, iy) ids, 2× quadtree refinement 200 → 100 → 50 m.
- Distances: ECEF f32 relative to the work-tile origin. No trig in hot loops.
- Weather: a per-event profile JSON/NPZ `(z, c, u, v)` from HRRR (ERA5 before 2014-07), plus a METAR QNH/T table.
- Outputs:
  - GPKG (4326 points + 5070 cells, rtree, `layer_styles` with default QML), plus float32 COG score rasters in 5070.
  - Single-file Leaflet viewer with a PNG score overlay.

**Memory budget (8 GB session, target peak ≤ 5 GB):**

| Item | Budget |
|---|---|
| DEM mmap resident working set (tiles touched) | ≤ 0.5 GB (page cache, reclaimable) |
| ADS-B prop segments for the time window (~3e4 flights × ~1e3 samples × ~48 B) | ~1.5 GB, or stream by hour |
| Segment spatial index | ≤ 0.2 GB |
| Per-worker tile scratch (CSR + CPA buffers), × 8 workers | 8 × 50 MB = 0.4 GB |
| Survivors + refinement children + scores | ≤ 0.2 GB |
| Spectrogram (e.g. 60 s–20 min, 0.25 Hz bins to 2 kHz, f32) | ≤ 0.3 GB |
| Python/numpy overhead, GPKG writer | ≤ 0.5 GB |

Enforce the budget with a `--max-mem` that sizes the rayon pool and the tile size, and with time-window streaming
of ADS-B. Never materialise dense CONUS fields at 100 m or finer.

**Early spikes:**
1. abi3 + rust-numpy build.
2. Altitude-source audit on real data (`alt_geom` HAE vs MSL labelling).
3. HRRR grid-relative wind rotation check.
4. Bilinear/ECEF hot loop benchmark against the ~1e11 evaluations/day estimate.

---

## Sources

- USGS 3DEP 1" collection: https://www.sciencebase.gov/catalog/item/4f70aa71e4b058caae3f8de1
- 3DEP 1" on data.gov: https://catalog.data.gov/dataset/1-arc-second-digital-elevation-models-dems-usgs-national-map-3dep-downloadable-data-collec
- 3DEP 1/3" on data.gov: https://catalog.data.gov/dataset/1-3rd-arc-second-digital-elevation-models-dems-usgs-national-map-3dep-downloadable-data-co
- 3DEP staged products bucket: https://prd-tnm.s3.amazonaws.com/index.html?prefix=StagedProducts/Elevation/1/TIFF/
- 3DEP cloud VRT note: https://imiamaps.org/sponsor-corner-usgs-2026-jun3/
- 3DEP elevation index: https://index.nationalmap.gov/arcgis/rest/services/3DEPElevationIndex/MapServer
- OpenTopography 3DEP API: https://www.opentopography.org/node/3534
- Copernicus DEM, AWS registry: https://registry.opendata.aws/copernicus-dem/
- Copernicus DEM on AWS (forum): https://forum.sentinel-hub.com/t/copernicus-dem-data-available-on-aws/3027
- Copernicus GLO-30 (Earth Engine): https://developers.google.com/earth-engine/datasets/catalog/COPERNICUS_DEM_GLO30
- Copernicus DEM (Data Space): https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM
- GEOID18 vs NAD83/NAVD88: https://www.gpsworld.com/the-differences-between-geoid18-values-and-nad-83-navd-88-values/
- NAPGD2022 beta: https://beta.ngs.noaa.gov/NAPGD2022
- NAPGD2022 downloads: https://beta.ngs.noaa.gov/NAPGD2022/download.html
- GEOID2022 alpha downloads: https://alpha.ngs.noaa.gov/GEOID2022/download.shtml
- PROJ grid CDN (us_nga_egm08_25, us_nga_egm96_15, us_noaa_g2018u0): https://cdn.proj.org/
- GeographicLib geoid models: https://geographiclib.sourceforge.io/C++/doc/geoid.html
- H3 cell statistics: https://h3geo.org/docs/core-library/restable/
- EPSG:5070: https://epsg.io/5070
- Herbie intro (HRRR on AWS): https://herbie.readthedocs.io/en/latest/user_guide/tutorial/intro.html
- Herbie "Start your engines": https://herbie.readthedocs.io/en/stable/user_guide/start-your-engines.html
- NOAA HRRR, AWS registry: https://registry.opendata.aws/noaa-hrrr-pds/
- ARCO ERA5 (google-research): https://github.com/google-research/arco-era5
- ERA5 public dataset (Google Cloud): https://cloud.google.com/storage/docs/public-datasets/era5
- era5cli (legacy CDS shutdown notes): https://github.com/ewatercycle/era5cli
- ECMWF CDS announcements: https://forum.ecmwf.int/c/announcements/c3s-announcements/7
- IEM ASOS request API: https://mesonet.agron.iastate.edu/cgi-bin/request/asos.py?help
- IEM data services: https://mesonet.agron.iastate.edu/request/download.phtml
- PyO3 0.26.0 release notes: https://newreleases.io/project/cargo/pyo3/release/0.26.0
- PyO3 features (abi3): https://pyo3.rs/main/features
- Crate metadata (crates.io API, queried 2026-11-03): https://crates.io/api/v1/crates/{rayon,pulp,wide,pyo3,numpy,arrow,parquet,rstar,h3o,symphonia,realfft,rustfft,rusqlite,memmap2,tiff,async-tiff,gdal,proj,geo,geographiclib-rs,gribberish}
- OGC GeoPackage 1.4 spec (rtree extension, gpkg_extensions): https://www.geopackage.org/spec140/
- QGIS layer_styles in GPKG: https://docs.qgis.org/latest/en/docs/user_manual/managing_data_source/supported_data.html#geopackage
- QGIS Processing algorithm authoring: https://docs.qgis.org/latest/en/docs/user_manual/processing/scripts.html
- Leaflet: https://leafletjs.com/reference.html
- Reference implementation: [thunderprint](https://github.com/Hexay/thunderprint) `Cargo.toml` and `src/output/gpkg.rs`
