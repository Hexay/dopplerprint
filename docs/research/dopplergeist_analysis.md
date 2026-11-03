# DOPPLERGEIST — algorithmic map (for Rust/PyO3 reimplementation)

Source: [DOPPLERGEIST](https://github.com/1337isnot1337/DOPPLERGEIST) @ `f6ba18a` (merge PR #141). Paths below are
relative to `src/dopplergeist/` unless prefixed. Python ≥3.11; deps numpy, scipy, pandas, pyproj, rasterio,
soundfile, pyarrow, requests, matplotlib; ffmpeg/ffprobe external (`pyproj.toml`).

It is an independent reconstruction of an algorithm shown in a YouTube video (`docs/METHODOLOGY.md`). There are
two code paths:
- **Legacy research path** `pipeline.run` (`pipeline.py:28`): manual event windows/bands per JSON config, a
  straight-pass fit gives speed/slant/f0 bounds, a corridor grid at 200 m → 100 m → 25 m, `pair_candidates`
  full association, and brightness/ridge scoring.
- **Automatic product path** `automatic.analyze` (`automatic.py:63`): whole-file discovery, `prepare_events`
  with broad physical bounds, ADSB.lol daily index, tiled CONUS search at 2 km → 100 m, subset association,
  `robust_ridge` scoring, Powell refinement, sensitivity profiling, conservative decision gate.
  Exposed by the CLI `analyze`, `serve` and `job` commands.

## 1. Pipeline stages end-to-end (automatic path)

| # | Stage | Entry point | Notes |
|---|---|---|---|
| 1 | Request validation | `contracts.validate_request` `contracts.py:155` | `AnalysisRequest{audio_path, time_window{start_utc,end_utc,source}, search{coverage:"conus", bounds_wsen?}}` |
| 2 | Intake / decode | `intake.intake_recording` `intake.py:97` | ffprobe, then ffmpeg pans every channel to mono at equal weight and resamples to **4000 Hz f32le**, streamed to `intake/recording.f32` on disk (bounded RAM). The SHA-256 of the source is recorded. Metadata time tags are reported only as hints, `usable_as_recording_clock: False` (`intake.py:57`). With no `time_window` the job returns `needs_input` (`automatic.py:135`). |
| 3 | Chunked spectrogram | `discovery._spectrum` `discovery.py:48`, `IntakeRecording.iter_chunks` `intake.py:81` | 120 s chunks with 60 s overlap, read from a memmap/file seek |
| 4 | Peak linking → ridges | `discovery._paths` `:64` | Per-frame `find_peaks` plus Hungarian assignment (`linear_sum_assignment`) of peaks to active paths |
| 5 | Ridge acceptance | `discovery._usable` `:111` | Duration, support, drop and monotonicity gates |
| 6 | Dedup / stitch / harmonics | `_merge_duplicates :154`, `_stitch_fragments :197`, `_harmonic_groups :175` | Gives `DetectedEvent` dicts, one per ridge, that share a `harmonic_group_id` |
| 7 | Event preparation | `matching.prepare_events` `matching.py:297` | Folds harmonics onto the lowest ridge (divides by the ratio) and gives each ridge equal total weight. Bounds are broad: speed [1, 0.9c], slant [1, 20 km], f0 [0.1·fmin, 1.9·fmax]. `cpa_tolerance_s` is half the event window. |
| 8 | Time-window gating | `automatic._earliest_emission :162`, `_latest_emission :196` | Emission query window = [start_lo + first event − R/300 − ~121 s, start_hi + duration/last event + ~121 s]. Capped at `max_acquisition_days` = 3. |
| 9 | ADS-B acquisition | `data_access.acquire_inputs :784`, `FlightCache.acquire_day :575` | ADSB.lol `globe_history_YYYY` GitHub release: split tar, then readsb traces, then a SQLite+R-tree index for each UTC day |
| 10 | Terrain | `data_access.CachedTerrain :647` | Copernicus GLO-90 COG tiles plus EGM2008 2.5′ geoid. DEM + N gives ellipsoidal height. |
| 11 | Tiled national search | `tiled_search.search_national :330` | EPSG:5070 integer grid. Per tile: query tracks, CPA table, association, clock sweep scoring, top-k merge. Then 100 m refinement around the global finalists. |
| 12 | Local refinement | `matching.refine_candidate :479` | Powell over (dx, dy, dclock) in a ±500 m / ±5 s box, run for the top `refine_finalists` (default 5) modes |
| 13 | Sensitivity profile | `matching.profile_candidate :557` | Leading mode only. Re-refines under frequency, tilt, clock, altitude, sound-speed and constant-source perturbations. |
| 14 | Decision | `confidence.decide_location :117` | Ordered gate. Outcomes: `succeeded`, `inconclusive` (with a `reason_code`) or `failed`. |
| 15 | Optional spectral cross-check | `spectral_matching.compare_spectral_modes :64` | Brightness ranking of the retained modes. Does not feed the decision. |

Orchestration, checkpoints and resume: `automatic._save_audio_checkpoint :28` stores a `resume.json` after
discovery. Tile checkpoints `search/tile-<ix>_<iy>.json` are keyed on a run hash, which hashes inputs, code
files and package versions (`tiled_search.py:366-390`). Jobs run in a subprocess (`jobs.execute :238`), and the
HTTP service lives in `service.py`.

## 2. Exact math

### 2.1 Physical Doppler model (emission/retarded time) — `geometry.py`
- Receiver `o` is static ECEF WGS84 (`ecef :26`, a=6378137, e²=6.69438e-3). Heights are ellipsoidal: DEM + geoid
  for the ground and readsb geometric altitude for the aircraft.
- Trajectory: `Track` (`:43`) builds a **scipy `CubicHermiteSpline`** in ECEF over relative time. Knot
  velocities come from ADS-B gs/track/vertical-rate (ENU→ECEF, `velocity_ecef :35`). Missing components use a
  segment-local `np.gradient` of positions (`tracks._segment_velocity`, tracks.py:86). `extrapolate=False`
  throughout. Segments split at gaps over **30 s** (`load_tracks max_gap_s`, index `max_gap_s=30`).
- Forward model (`Track.acoustic :165`): for each receive time t, solve `t = τ + |x(τ)−o|/c` by **Newton**:
  `τ ← τ − (τ − t + r/c)/(1 + u_r/c)`, at most 10 iterations, tolerance 1e-7 s, τ clipped to the track span.
  The initial guess is `τ0 = t − |x(clip(t)) − o|/c`.
  Then `u_r = (x−o)·v/r` (positive when receding) and **factor = c/(c+u_r)**, so `f_obs = f_src · factor`.
  Samples outside [arrival(start), arrival(end)] are NaN. A non-monotone arrival (1+u_r/c ≤ 0) raises.
  c = 343 m/s, constant. There is no wind, no temperature profile and no refraction.
- A subsonic check per spline interval (`_unsupported_motion_intervals :85`) uses a Bezier bound on |v|, then
  the exact quartic extrema of |v|² (cubic roots). The arrival intervals for flagged emission intervals use
  Bezier hull distances (`_check_motion_support :139`).
- Analytic oracle for a straight constant-speed pass (`straight_doppler :246`), with CPA at τ=0 and arrival at
  slant/c:
  `u = t − t_cpa_rx + d/c`, `β = v/c`, `τ = (u − sqrt((βu)² + (1−β²)(d/c)²))/(1−β²)`,
  `u_r = v²τ / sqrt(d² + (vτ)²)`, `f = f0·c/(c+u_r)`.
- CPA per track (`closest_approaches :207`): sample every 2 s, `find_peaks(−range²)`, then
  `minimize_scalar(bounded, xatol=1e-3)` on relative time. Endpoint minima are kept only if the range rate
  changes sign.

### 2.2 Legacy straight-pass fit (`audio.fit_flyover`, audio.py:99)
Uses `scipy.optimize.least_squares(loss="soft_l1", f_scale=1)` on params (f0, v, d, tc). Bounds: f0 ∈
[0.5·fmin, 1.5·fmax], v ∈ [10, min(250, 0.85c)], d ∈ [50, 20000], tc within the window. Initial values: median
f, 80 m/s, 1500 m, 0. The legacy pipeline derives speed ±25 %, slant [0.6, 1.5]×, and f0 ±max(3 Hz, 2 %) from
this fit (`pipeline.py:52-55`). **The automatic path does NOT use it**, because real tracks turn and accelerate
(MATCHING.md).

### 2.3 Spectrogram / ridge parameters
- Legacy `audio.spectrogram :41`: Hann window 2.0 s, hop 0.25 s, **nfft = 4·n** (zero-pad), PSD → dB.
- Discovery `_spectrum :48`: Hann 2.0 s, **hop 0.5 s**, nfft = n (0.5 Hz bins), band 40–min(1600, 0.48·sr) Hz.
- "Contrast" normalisation, shared by both:
  `W = dB − median_t(dB)` per frequency row (removes stationary tones). Per column, `center = median_f(W)` and
  `scale = 1.4826·MAD_f`. `contrast = clip((W−center)/max(scale, 1), −4, 12)`.
- Legacy Viterbi ridge (`trace_ridge :60`): state strength is `clip(contrast, −2, 7)`. Transitions allow
  downward slopes up to 12 Hz/s and upward slopes up to 1 Hz/s, with quadratic cost `0.12·(Δf/Δt)²·Δt`. Ends
  with parabolic sub-bin interpolation on dB. Weight is `clip(contrast, 0, 8)`.
- Discovery linking (`_paths`):
  - Peaks per frame need `height ≥ 2.5` contrast, `prominence ≥ 1.5`, `distance ≥ 2` bins, and the top 60 by
    height are kept. Parabolic sub-bin refinement follows.
  - Allowed slope is from −max(8, 0.08f) to +max(1, 0.008f) Hz/s.
  - Cost is |Δf − predicted (median slope of the last 8 points)·dt| / max(1, 0.01f). Assignment is Hungarian.
    A path dies after a 2 s gap.
- Acceptance (`_usable`):
  - Duration ≥ 12 s and support ≥ 0.65.
  - Drop ≥ max(4 Hz, 3 % of the median).
  - After a median-5 filter: fraction of non-increasing steps ≥ 0.75, and fraction of "moving" slopes ≥ 0.35.
  - Max jump ≤ max(1 Hz, 0.25·drop), and median |f − smooth| ≤ max(1, 1 % f).
- Duplicate merge across chunk overlaps: ≥ 8 shared 0.5 s samples, with ≥ 85 % of them within max(1 Hz, 0.6 %).
- Fragment stitch:
  - Gap ≤ 20 s and gap/span ≤ 0.35. Both endpoint slopes must descend by > 0.05 Hz/s.
  - The bridging rate must lie in [0.4·min, 2.5·max] of the endpoint slopes, and the continuation must be
    reciprocal-unique.
- Harmonic grouping: ratios {a/b : 2≤a≤8, 1≤b<a, ≥1.2} cover missing fundamentals. A ridge joins a group when the
  90th-percentile relative ratio error is < 0.8 % and the shared overlap is ≥ 60 % of the shorter span.
- Per-event `frequency_uncertainty_hz` = max(1/window, 1.4826·MAD(f − medfilt5(f))) (`discovery.py:291`).
  It becomes the robust loss scale, with a floor of 1 Hz.

### 2.4 Scoring a (receiver, flight-pass set, clock) hypothesis
Three modes exist in `search.score_candidate :346`. Only `robust_ridge` is used by the product.

**robust_ridge** (`matching._fit_ridge_uncached :358`, per event):
1. Compute `factor_i, τ_i` for the ridge samples at the receive times `T0 + t_i` (one shared clock T0 for all
   events).
2. Nuisance source model with linear drift in emission time: `f_src(τ) = f0 + a·u`, where
   `u = (τ − mid(τ))/span(τ) ∈ [−½, ½]`.
   - Drift cap: `cap = 0.02 · clip(median(y/factor), f0_bounds)`.
   - Gaussian prior on `a` with precision `(2σ/cap)²`.
3. Solve `min Σ w (factor·(f0 + a·u) − y)²` with `f0 ∈ bounds`, `|a| ≤ cap`. It is solved **exactly** as a 2×2
   normal-equation system, falling back to the 4 box edges (`_source_fit :109`).
   Wrapped in **6 IRLS iterations** with pseudo-Huber weights `w/sqrt(1 + (r/σ)²)`.
4. `loss = avg_w 2σ²(sqrt(1 + (r/σ)²) − 1)`, `penalty = (2σa/cap)²/N_indep`, and
   `event_score = −sqrt(loss + penalty)` (in Hz). σ = `frequency_loss_scale_hz`, default max(1, uncertainty).
   The weights are renormalised to sum to N_indep.
5. Joint full-association score: `Σ event_score`, kind `negative_robust_ridge_loss_hz`.
6. **Subset association score** (`association_score :282`): `Σ (1 − loss_i/(3σ_i))`. If any matched event has
   `loss > 3σ` the score is −inf. Kind: `matched_group_support_minus_normalized_residual`.
   - More matched independent flyovers give a higher score, each contributing at most 1.
   - Unmatched proposals cost nothing.

**ridge** (legacy): closed-form `f0 = Σ w·F·y / Σ w·F²`, then score = −weighted RMSE.
**brightness** (legacy, and the optional cross-check `spectral_grid.best_frequency :6`): sweep a constant f0
in 0.2 Hz steps across f0_range. At each step, take the bilinear `map_coordinates` sample of contrast along
`f0·factor(t)`. The score is the mean contrast with NaN counted as −4, and it requires ≥ 95 % in-band support.
The candidate score is the sum over events.

**Clock handling ("locked offset")**:
- All events share one recording start T0. There is never a per-event time shift.
- Candidate clock interval: for each matched pass, `start_k = receive_time_cpa_k − event.cpa_receive_s`.
  The intersection of the `[start_k ± (cpa_tolerance_k + offset_tol/2)]` intervals (`association_candidates
  :151`) is then expanded by ±`time_sweep_s` (5 s) and intersected with the trusted UTC window.
- That interval is swept on a **0.5 s grid** (`search.py:370-381`). The best trial wins.
- Since `cpa_tolerance` is half the event duration, the interval is typically tens of seconds, i.e. ~50–400
  trials per candidate.
- `batch_acoustic_trials` (`matching.py:440`) vectorises all trials of a candidate into one Newton solve over
  `unique(starts[:, None] + t[None, :])`, capped at 262,144 samples.

### 2.5 Candidate generation (geometry prefilter)
- `search.cpa_table :73` (production) and `cpa_table_dense :13` (dense oracle):
  - Sample the spline every `cpa_step_s` = 2 s.
  - Build a `cKDTree` on the track samples and run `query_ball_point(observers, R + 2·max_step)`.
  - Find local minima in the per-observer neighbour runs.
  - Project onto the two adjacent polyline edges to get the CPA time, distance and speed.
- Output rows: `(point_id, flight_id, emission_time, receive_time = t + d/c, distance_m, speed_mps)`. There is
  one row per local pass, so repeated passes by the same flight are kept.
- `filter_event :138`: speed ∈ speed_range ± tol and distance ∈ slant_range ± tol. With prepare_events defaults
  this is effectively "distance ≤ 20 km and subsonic".
- Association (`matching.association_candidates :151`) is a DFS over events per grid point:
  - Each event's options are bisect-indexed by its clock interval. A `None` option means "unmatched".
  - The shared-clock interval is intersected at every branch and the branch is pruned when it becomes empty.
  - A flight-pass cannot be reused within a hypothesis.
  - Optional **silence filter**: reject the hypothesis if another pass, closer than
    max(selected distances)·mult, arrives between the first and last matched passes (±60 s edge margin).
    This is the video's "no closer intervening prop" heuristic. It is off by default in the product.
  - Budgets: `max_candidates` 20,000 per tile and 20× work units.

### 2.6 Search grid & tiling (`tiled_search.py`)
`SearchBudget :38` defaults:

| Parameter | Default |
|---|---|
| `spacing_m` | 2000 |
| `tile_cells` | 32 (32×32 = 1024 points/tile; max 256) |
| `max_tracks_per_tile` | 2000 |
| `max_track_samples_per_tile` | 500k |
| `max_cpa_rows` | 100k |
| `max_candidates_per_tile` | 20k |
| `top_k` | 100 |
| `refine_spacing_m` | 100 |
| `refine_finalists` | 10 |
| `cpa_step_s` | 2 |

- Grid: one EPSG:5070 origin with integer cells. Tiles are half-open (`grid_tiles :95`), and points outside the
  WGS84 bounds are dropped (`tile_points :117`). Terrain is sampled per point.
- Track query per tile uses the projected tile box padded by `1.6·R + 15 km` (`_expanded_bounds :128`), with
  R = 20 km.
- A whole-track reject runs before the CPA step: `Track.outside_observer_radius` (geometry.py:119) does an
  axis-separation test against `R + c·max_gap/2`.
- Coarse pass: every tile runs `_score :174`, then a global `merge_candidates` keeps the top-k unique "modes".
  Mode identity is a hash of (event_indices, flight_ids, pass_emission_times, position, clock)
  (`finite_search.identify_mode :25`).
- Fine pass: for each of the top 10 distinct flight/approach sets, a ±spacing square at 100 m is cut into
  tile_cells² subtiles. Already-covered rectangles are skipped (`tiled_search.py:448-487`).
- Sizes: CONUS (−125, 24, −66, 50) at 2 km gives ~4.9 M receiver cells. A sample run with tile_cells = 16 had
  **19,278 tiles**. Each refinement square is 41×41 = 1,681 points.
- Legacy: `terrain.corridor_grid :94` only materialises cells within `1.45·R + spacing` of track samples that
  meet the speed/time bounds. Its spacing is 200 m coarse, then `refine_grid :163` at 100 m, then 25 m around
  the best point per flight combination (`pipeline.py:233-245`). This is the video's 200 m / 100 m scheme.

### 2.7 Optimisers used
| Where | Optimiser |
|---|---|
| Emission time | Newton, vectorised, ≤10 iterations |
| CPA (geometry path) | `scipy.optimize.minimize_scalar(method="bounded")` |
| CPA (search path) | Polyline projection |
| Source f0/drift | Closed-form 2×2 box least squares plus 6× IRLS (pseudo-Huber) |
| Clock | Exhaustive 0.5 s grid inside the association interval |
| Location (coarse/fine) | Exhaustive grid at 2 km, then 100 m |
| Local refinement | `scipy.optimize.minimize(method="Powell")` over (dx, dy[, dt]), bounds ±500 m / ±5 s, xtol 0.01, ftol 1e-7, maxiter 60, objective −score (terrain resampled at each evaluation) |
| Straight-pass fit (legacy only) | `least_squares` TRF with soft_l1 loss |
| Discovery linking | Hungarian assignment |
| Legacy tracing | Viterbi |

### 2.8 Physical refinement & sensitivity (`matching.profile_candidate :557`)
Each scenario reruns Powell refinement:

| Source | Scenarios |
|---|---|
| Observer height | ±150 m |
| Per-flight altitude bias | ±150 m (`_altitude_bias :131`, which shifts geodetic height and corrects the velocity derivatives) |
| Sound speed | ±5 m/s |
| Per ridge | ±σ constant frequency offset and ±σ linear tilt |
| Source model | Constant source (drift 0) |
| Clock | Fixed at each clock-window endpoint |

- `sensitivity.radius_m` is the maximum displacement from nominal.
- The profile is `complete` only if every scenario succeeds, every flight's altitude basis is `ellipsoidal` or
  `ellipsoidal_interpolated`, and the terrain datum is ellipsoidal.
- There is no wind model, no ray tracing and no terrain occlusion. Terrain is only used for receiver height.

### 2.9 Decision gate (`confidence.decide_location :117`, `DecisionPolicy :16`)
The checks run in this order:

1. Events exist, coverage is complete, and data are `available`.
2. ≥ 2 independent harmonic groups, or 1 if the clock is known exactly and was profiled.
3. Unmatched proposals were declared explicitly.
4. The clock is locked.
5. Per event: `rmse ≤ 3 Hz`, `outlier_fraction ≤ 0.25`, `support ≥ 0.95`, and drift not at its bound.
6. The finite-search ledger is complete (`finite_search.RetentionLedger :89`, an SQLite `retention.sqlite`
   recording the best discarded score). The gap to the best discarded mode, per group, must exceed **0.2
   support units**. For raw-loss scores the threshold is 0.5 Hz.
7. No near-tied competitor with a different flight association or more than 1 km away. A same-association
   near-tie is unioned into the radius.
8. Sensitivity covers {frequency, clock, altitude, sound_speed}, is complete and is not boundary-limited.
9. The union radius is ≤ `target_radius_m` = 1000.

Outcome: `succeeded/supported_by_evidence_checks`, otherwise `inconclusive` with a reason code. Scores are
explicitly "not probabilities".

## 3. Data sources & formats

- **ADS-B: ADSB.lol `globe_history_{year}`** GitHub releases, tag `v{YYYY.MM.DD}-planes-readsb-staging-0`
  (`data_access.py:575-645`).
  - Assets: a split `.tar.*` archive with GitHub sha256 digests. Download is resumable via HTTP Range
    (`download :134`), and `SplitReader :181` streams the concatenated parts into tar.
  - Members: gzip **readsb/tar1090 trace JSON** (`{icao, timestamp, trace: [[dt, lat, lon, alt_baro_ft|"ground",
    gs_kt, track_deg, flags, vrate_fpm, meta{t: type}, ?, alt_geom_ft, geom_rate, ...]]}`).
  - The normaliser `tracks.adsblol_frame` (tracks.py:44) chooses altitude in this order: flag bit 8 means alt
    is geometric; else `p[10]` geometric; else baro plus interpolated geom−baro offset; else labelled
    `barometric`.
  - Units are converted to SI (ft→m 0.3048, kt→m/s 0.514444, fpm→m/s 0.00508).
  - Null speeds and headings stay NaN and are inferred later.
  - ODbL licence. A whole day is downloaded (tens of GB, hours: see the CLI `--online` help).
- **Aircraft types:** `wiedehopf/tar1090-db icao_aircraft_types.js`. The last character of `desc` maps J→jet,
  T→turbine, P→piston. In the automatic path this is a **label only**: jets are retained. The legacy
  `load_tracks` excludes jets when given a doc8643 CSV (tracks.py:134).
- **Index:** one SQLite file per day (`all-types-v2-<normalizerhash12>.sqlite`).
  - Staging `samples(flight, time, row JSON)`. Per-flight gap split (30 s, ≥3 samples).
  - `segments(name, start, end, payload = gzip JSON rows, payload_hash)`.
  - An **R-tree `spatial(west, east, south, north)`** holds conservative continuous-curve bounds: a Bezier
    control-hull ball, then lat/lon padding (`_continuous_bounds :76`). There is also a time index on
    (start, end).
  - Build is parallel: (cores − 1) worker processes plus one SQLite writer, with 128 MB queue backpressure
    (`_parallel_flight_segments :347`).
  - Query returns whole segments, never clipped (`FlightIndex.query :499`). **Each query decompresses JSON and
    rebuilds a pandas frame and a CubicHermiteSpline Track** (`_frame_track :54`). There is no in-memory track
    cache across tiles.
- **Other track inputs (legacy):** KML gx:Track, single trace JSON, Parquet (with pushdown filters), CSV
  `time_s, lat, lon, alt_m [, east/north/up_mps, typecode, altitude_basis]` (`tracks.load_tracks :111`).
  FR24 (pyfr24) is mentioned in the video only. No OpenSky adapter exists.
- **Terrain:** Copernicus **GLO-90** COG tiles from `copernicus-dem-90m.s3.amazonaws.com`, with
  `tileList.txt` as the catalogue. A tile absent from the catalogue means ocean (0). Note the half-pixel
  north/west edge shift at degree boundaries (`CachedTerrain.sample :692`).
  **EGM2008 geoid** comes from `cdn.proj.org/us_nga_egm08_25.tif`, and height = DEM + N. Windowed rasterio
  reads with an LRU tile cache. Offline mode never fetches.
  Legacy `terrain.Terrain` (`terrain.py:8`) accepts arbitrary rasters or a constant plus an optional geoid
  raster, and uses nearest-pixel lookup.
- **Caching:**
  - Day index reused by the normaliser hash. Index identity is content-hashed (sources, coverage, normaliser
    hash, gap).
  - Every download carries a sha256 receipt.
  - Tile checkpoints and the retention ledger are keyed by `run_hash`.
  - Within-batch ridge-fit memo (`_RidgeFitCache`, 4096-entry LRU, ContextVar-scoped, `matching.py:45`).

## 4. Performance hotspots (Rust targets, ranked)

Work model per coarse tile: ≤ 1024 receivers, ≤ 2000 tracks, ≤ 500k track samples, ≤ 100k CPA rows,
≤ 20k association candidates. CONUS has ~4.8k tiles at 32 cells (19k at 16 cells). A synthetic CONUS run with
sparse traffic took 175 s and 278 MiB, but only tiles containing tracks get receiver points.

1. **Clock-sweep robust ridge scoring** (`search.score_candidate` → `batch_acoustic_trials` → `Track.acoustic`
   → `_fit_ridge_uncached`). Cost is
   candidates (≤ 20k per tile) × clock trials (50–400) × matched events (1–6) × ridge samples (24–400 per
   harmonic at 0.5 s, harmonics concatenated) × Newton iterations (≤ 10) × spline evaluations (position and
   derivative). Add 6 IRLS passes per trial. Order of magnitude: 10⁹–10¹¹ spline evaluations per busy tile.
   This is pure numpy with per-candidate Python overhead, and the dominant cost.
   - Rust plan: implement the cubic Hermite eval, the vectorised Newton solve and the closed-form 2×2 IRLS fit
     as one fused kernel, parallelised (rayon) over candidates or trials.
   - Exploit the fact that receive times on a 0.5 s clock grid with a 0.5 s ridge hop overlap heavily; the
     Python code already dedups with `np.unique`.
2. **CPA table** (`search.cpa_table`): tracks × (span/2 s) samples, a KD-tree query of 1024 observers at
   radius ≈ 20 km + 2·step, and a minima scan over the neighbour lists. Runs once per tile plus once per
   refinement subtile.
   - Rust plan: an R-tree or uniform grid on observers, then per-segment analytic point-to-segment minima.
     Reuse track sampling across tiles.
3. **Track materialisation** (`FlightIndex.query` → gzip + JSON + pandas + `CubicHermiteSpline`) is repeated
   for every tile and fine subtile, because overlapping padded query boxes are around 160 km wide versus 64 km
   tiles.
   - Rust plan: build splines once per query day and keep an in-memory, Arrow-backed segment store with an
     R-tree.
4. **Association DFS** (`association_candidates`): Python bisect/branching per grid point; combinatorial in
   events × passes. Easy to port, with bitset/interval pruning.
5. **Powell refinement + sensitivity:** each objective evaluation is a full `score_ridges` call plus a pyproj
   transform plus a terrain sample. ≤ 60 iterations × ~3·n_params line-search evaluations, times ~12–20
   scenarios (`profile_candidate`), for ≤ 5 finalists. Moderate cost, but Python-bound. A Rust objective with
   cached terrain would make it negligible.
6. **Discovery:** STFT, 2 s at 4 kHz = 8000-point FFT every 0.5 s → for a 1 h file, ~7.2k frames × 3.1k bins.
   - `_paths` is a Python per-frame loop with Hungarian assignment (≤ 60 peaks per frame).
   - Measured: a 3600 s file took **31 s at ~244 MiB** (AUDIO_DISCOVERY.md).
   - Rust plan: realfft plus a custom linker. A medium-priority gain.
7. Legacy brightness sweep (`best_frequency`): f0 grid (range/0.2 Hz) × time bins with bilinear interpolation,
   batched 64 at a time. Only needed for the optional spectral cross-check.
8. Index build: gzip/JSON/pandas per trace (~tens of thousands of aircraft per day). It is already
   multiprocess. Rust with serde + flate2 + rusqlite or Parquet would be much faster.

## 5. Outputs produced

- **Job directory** (automatic path, `automatic.py:87-96`):
  - `request.json`, `provenance.json`
  - `intake/{recording.f32, recording.json, intake.json}`
  - `discovery/{discovery.json, discovery.png, event-NNNN[-harmonic-k].{csv,png}}`
  - `resume.json`
  - `search/{manifest.json, tile-*.json, retention.sqlite, support/*.json, search-report.json}`
  - `data-provenance.json`, `refined-candidates.json`, `decision.json`, optional `spectral-comparison.json`
  - Job `result.json` (`JobResult` contract, `contracts.py:92`): status, coverage, events,
    `candidates[{lat, lon, score, score_kind, event_ids, flight_ids, recording_start_utc,
    uncertainty{radius_m, method, kind}}]`, reason_code, artifacts.
- **Legacy research run** (`pipeline.run`):
  - Per event: `event-N.{json,csv,npz,png}`
  - `coarse-grid.csv`, `filter-counts.json`, `ranked-grid.json`, `ranked.json`, `flight-combinations.json`,
    `report.json`, `model-support.json`
  - `location-scores.png`, `event-N-best-model.png` (`pipeline.plot_results :266`)
- **Web UI:** stdlib `ThreadingHTTPServer` on :8765 (`service.py:124`), with vanilla JS
  (`web/index.html`, `app.js`).
  - Endpoints: `GET /api/capabilities`, `POST /api/jobs` (raw octet-stream body plus an
    `X-Dopplergeist-Request` JSON header), `GET /api/jobs/{id}`, `POST .../cancel|resume`,
    `GET .../artifacts[/path]`.
  - Candidates are rendered as text cards with an **OpenStreetMap link** (`app.js:84`). There is no embedded
    map.
- **No GeoJSON, GPKG, KML or raster score surface is produced anywhere.** Scores exist only as JSON rows with
  lat/lon (and EPSG:5070 x/y) plus a matplotlib scatter PNG. A QGIS-friendly layer (candidate points,
  sensitivity radii, CPA/track lines, per-tile score raster) would be net new.

## 6. Known limitations & negative results

Doc paths below are relative to the repo root.

### 6.1 Accuracy evidence actually achieved
- **Original video case, 2 real events** (PC12 `a1dfeb:1` and B350 `ace391:3`, ADSB.lol 2026-09-08):
  - **43.86 m** from a mapped, unsurveyed reference (an osprey-nest platform).
  - Used manual windows/bands and a video-informed regional box (docs/PROGRESS.md:36-44).
  - Clean replay: 99 s and 811 MiB (docs/verification/CLEAN_REPLAY.md:64-73).
- **Faint third event** (CRJ9 jet `a38237`): **357 m**. Its clock was inherited from the two-event fit, not
  independent (docs/HANDOFF.md:87-88).
- The inferred start 20:17:29.66 UTC was never reconciled with the video's stated 20:37 UTC
  (docs/PROGRESS.md:46-49).
- **Fully automatic HTTP run on the same 1 h MP3** with a 20 s start window (benchmarks/README.md:346-364):
  - 224–254 s; 16 groups / 18 ridges; `inconclusive / search_incomplete`.
  - Leader **66 km** from the reference.
  - 19,190 of 19,278 tiles completed, but only 256 coarse and 243 refine points were actually scored, because
    the regional index left most tiles empty.
  - 86 tiles had no terrain and 2 tiles exhausted the association budget: 56 points each exceeded 20k
    associations.
  - **The end-to-end automatic product has never produced an accepted real-world location.**
- Synthetic national runs: one leader was 1,046 km off while the correct pair ranked 4th at 6 m. Others reached
  0.65 m, 5.6 m, 15 m and 22 m. All of them abstained under the decision gate (README.md:125;
  docs/PROGRESS.md:34).
- 40-passage Zenodo feature set (with annotated bands): 33/40 detected. Straight-fit median errors were **6.85 %
  speed and 15.4 % slant** (docs/PROGRESS.md:51-53). This set has no recorder coordinates and no sample clock,
  so it cannot be a location test.
- NASA sUAS (drones), WSU, AeroSonicDB, public webcams and bird cams: no usable location.
  - Most clips gave 0 proposals.
  - NASA Edge065 gave a conditional 474 m fit, likely to the wrong source (a background prop): the competing
    mode was 1.9 km away and the near-best region spanned 2.3 km.
  - WSU had 0 supported fits out of 4,992.
  - Sources: benchmarks/README.md:200-311, benchmarks/aerosonic-real-scan-20261103.md.

### 6.2 What didn't work and why
- **Straight-pass fit as a geometric filter.** On the reference case it said about 1 km slant, but the real track
  passed about 4.9 km away. Tight speed/slant filters therefore excluded the true curved path
  (docs/HANDOFF.md:84-86). The automatic path replaced them with broad bounds (≤ 20 km, subsonic), which
  massively inflates the candidate count.
- **Dropping jets.** The faint match is a CRJ9, so jets must be kept and type is only a label (docs/HANDOFF.md:82-83).
  This contradicts the video's jet removal.
- **Per-event clock shifts.** These manufacture false matches, so one shared clock is mandatory
  (docs/HANDOFF.md:97).
- **Discovery recall.**
  - The fixed 2 s / 0.5 s STFT and the ≥ 12 s, ≥ 3 % / 4 Hz drop gates miss short, faint, low-Doppler (slow
    drone / WSU, 3–4 % decline) and steep events.
  - A shorter window (0.5 s / 0.125 s) gave **zero** proposals on NASA.
  - All 10 s AeroSonic clips are below the 12 s minimum.
  - On the reference recording the documented faint pass was missed at threshold 3.0. At 2.5 it is found but
    the proposal count rises from 8 to 16, mostly unverified (docs/AUDIO_DISCOVERY.md).
  - Prominent passes fragment into several proposals, so stitching was added.
- **ADS-B trajectory quality** (research/trajectory-motion/README.md):
  - Modelled peak speeds reached 1,011 m/s and even 52 km/s (c01125: 14.6 km jump in 0.42 s).
  - Missing heading had been coerced to 0, which fabricated velocities.
  - Clipping before normalisation changed height by 137 m (baro vs ellipsoidal).
  - The fixes: a subsonic check per spline interval, `AcousticSupportError` isolation (whole track/stage marked
    unassessed, not dropped silently), and velocity inference only for genuinely missing components.
- **Newton emission solver.** It once returned τ about 293 s off. It now raises `emission_solver_not_converged`
  (docs/MOTION_SUPPORT.md:40-46).
- **Altitude datum.**
  - The old ingestion lost a 450 ft geom−baro calibration (1524 m vs 1661 m) (docs/HISTORICAL_INGESTION.md:26-36).
  - Barometric-only traces keep sensitivity "incomplete", so the decision abstains.
  - DATA_SOURCES.md still mentions EGM96 while the code uses EGM2008.
- **Clock provenance is the dominant real-world blocker.** Metadata, HLS captures and dataset WAVs lack a
  sample-zero UTC (docs/verification/REAL_DATA_TIMING.md, PUBLIC_CAMERA_BENCHMARK.md). An ffmpeg audio-only
  transcode dropped the A/V offset (8 s → 6 s).
- **Coverage and budget exhaustion.**
  - Tile association budgets overflow in busy areas, and smaller tiles cannot fix a single overloaded point.
  - Missing GLO-90 tiles or nodata cause hard failure.
  - Regional or cropped indexes can never claim complete border coverage.
- **Optimisations tried and not landed** (docs/HANDOFF.md:9-20):
  - Early rejection: 8.0–8.3 s down to 5.75 s uncached, but slower with the production fit cache.
  - Sample reuse: cut evaluations from 101,955 to 4,259 with no consistent wall-clock gain, because Python
    overhead dominates rather than the arithmetic. This is a strong signal for the Rust port.
- **Refinement duplication.** 8,405 fine visits covered only 5,863 distinct cells
  (docs/verification/REFINEMENT_UNION_01a19ec6.md).
- **Replay nondeterminism.** On replay the synthetic winner moved 24.8 m and ridge CSVs differed at 67/733
  frames. Cause unresolved (docs/verification/CLEAN_REPLAY.md:77-83).
- **Calibration.** No thresholds are calibrated, and there is no held-out real false-localization rate
  (docs/CONFIDENCE.md:35-37). Decision fixtures abstained 7/8 times, with 0/8 unsupported localizations.
- **Unmodelled physics:** wind and refraction (a single constant c, with ±5 m/s as a sensitivity only), terrain
  occlusion, microphone response, propeller RPM changes beyond ±2 % linear drift, and receiver motion.
- **Process overhead.** The repo is dominated by provenance and receipt machinery: run hashes, retention
  ledgers, "unknown trial" bounds. AGENTS.md now explicitly asks to cut it. Most of `tiled_search`,
  `finite_search` and `confidence` is accounting, not physics.

## 7. Implications for the Rust/PyO3 rebuild (distilled)
- **Port core (bit-for-bit semantics):**
  - `ecef` / `velocity_ecef`
  - Cubic Hermite ECEF track with gap split at 30 s and no extrapolation
  - Newton retarded-time solve and Doppler factor c/(c+u_r)
  - Subsonic interval check
  - Polyline CPA table
  - Closed-form drift-bounded IRLS ridge fit (§2.4)
  - Subset-association DFS with interval pruning
  - Clock sweep
  - Use `straight_doppler` as the test oracle.
- **Keep in Python, or swap for a crate:** ffmpeg decode, readsb ingestion (or Rust with serde/flate2), pyproj
  (→ `proj` crate), rasterio (→ `gdal` crate or direct COG reads), Powell (→ `argmin` or Nelder–Mead).
- **Biggest wins:**
  - A fused per-candidate kernel (trials × samples × Newton) parallelised with rayon.
  - A persistent in-memory track store instead of per-tile SQLite/JSON/spline rebuilds.
  - A terrain tile cache.
  - Grid-level parallelism (the Python driver is strictly sequential, `bounded_concurrency: 1`).
- **QGIS outputs to add:**
  - GeoPackage/GeoJSON layers: candidates (score, flights, clock, radius), the sensitivity-scenario points, and
    matched track LineStrings with CPA points.
  - A per-cell best-score raster (GeoTIFF in EPSG:5070) from the coarse and fine passes.
  - The search-extent and tile-status polygons.
