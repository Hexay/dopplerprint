# doppler_print architecture

This tool works out where an audio recording was made. It matches the aircraft Doppler curves in the recording against historical ADS-B trajectories. The method is colsto's, summarised in `docs/research/transcript/analysis.md`. The structure follows thunderprint: a Rust core, a CLI and an evaluation harness. Python bindings are added with PyO3 and numpy so the algorithm can be iterated on interactively.

## Crates

| Path | Role |
|---|---|
| `crates/dp-core` | All computation. Has no Python dependency. |
| `crates/dp-cli` | `dopplerprint` command (lib + bin). `dp_cli::run(argv)` is the one parser; each subcommand is one file in `src/cmd/` and is a thin call into dp-core. `cmd/serve_backend.rs` adapts `dp_core::app` to dp-serve's `LocateBackend`. |
| `crates/dp-serve` | `serve` web app: tiny_http, one worker thread, on-disk jobs, embedded page. Calls a `LocateBackend` trait and does no computation itself. |
| `crates/dp-py` | `dopplerprint._core`, an abi3 wheel. Each file registers the bindings for one dp-core area; `_core.cli(argv)` runs `dp_cli::run`, so `pip install dopplerprint` gives the same command. Arrays are passed as numpy, and long calls release the GIL with `py.detach`. |
| `python/dopplerprint` | Python package with offline prep scripts (DEM, geoid, weather), notebooks and the QGIS plugin. Display and orchestration only. |
| `tools/` | Scripts for the corpus and the eval loop. |
| `experiments/*.toml` | One file per hypothesis. Each overrides `config` defaults. |

## dp-core modules (data flow order)

1. `adsb`: streams adsb.lol day archives into a Parquet store with a points table and a segments table. `load_tracks(TrackQuery)` returns cleaned, gap-split `Track`s. `typedb` maps a type code to an `EngineClass`.
2. `track`: a `Track` holds a list of `TrackPoint`s (UNIX seconds, ECEF position and velocity). `state_at(t)` interpolates with cubic Hermite.
3. `geo`: `Vec3`, WGS84 to ECEF conversion, and `albers` for EPSG:5070 and the nested `Cell` lattice (200 m to 100 m to 50 m).
4. `dem`: memory-mapped EPSG:5070 ground raster (MSL) and EGM2008 undulation. The observer's ellipsoidal height is `H_msl + N + mic_height`.
5. `physics`: emission-time Newton solve, `doppler_factors` (received/emitted frequency ratio), `cpa`, and `cv`, the closed-form constant-velocity curve.
6. `audio`: decode (symphonia) to a whitened `LogSpectrogram`. The log-frequency axis turns f0 and harmonic number into vertical shifts.
7. `events`: automatic ridge discovery, harmonic grouping and constant-velocity fitting. Fits produce generous ranges, which are used only for pruning.
8. `search`: the stages below.
9. `sim`: an independent time-domain synthesiser that serves as ground truth for tests and eval.
10. `output`: GeoPackage (QGIS), GeoJSON and a single-file HTML viewer.
11. `config`: all tunables, overridable from TOML.
12. `app`: the end-user `locate` pipeline (inputs → decode → discover → cached/fetched ADS-B → archived weather → `search::locate` → report and files). See `docs/USAGE.md`.
13. `weather`: Open-Meteo archive fetch/cache and profile → `physics::Column` (per-emission layer-mean c and wind vector, `docs/PHYSICS.md` §4a). Used by `app` and by eval (`[eval] weather = "archive"`).

## Search

The stages follow the video, plus fixes taken from prior art (see `docs/research/prior_art_landscape.md`):

1. **Window.** Load prop and helicopter tracks (jets flagged, not dropped) for the time prior and region.
2. **CPA table.** For each track segment and each 200 m cell within 10 km, compute the delayed-CPA arrival time and slant range in closed form per segment.
3. **Association.** Each event gets the flights whose speed and slant range fall within its constant-velocity bounds. These bounds are widened for the scale ambiguity between c, v and d. Event pairs must satisfy the audio offset between them, plus a tolerance.
4. **Silence.** A cell is penalised (not hard-cut) if it would have heard an extra flight. The penalty is weighted by predicted SNR and by ADS-B coverage.
5. **Locked joint score.** A cell's score is the sum, over events, of whitened spectrogram power along the predicted curves. Each event has its own f0, found by an FFT correlation along log-frequency, and harmonics are combined incoherently. All events share one clock offset.
6. **Refine.** Survivors are split into child cells, and the top modes are polished with Nelder–Mead over (x, y, clock).
7. **Confidence.** Report ambiguity modes such as the mirror and along-track solutions. A null distribution comes from re-running the search with ADS-B shifted in time. The output is a posterior, a 90% region and a reliable/inconclusive flag. Built: area-weighted posterior over refined cells (`search/surface.rs`), basin modes (`search/modes.rs`), fitted T and per-stratum conformal q̂ (`search/calibration.toml`, `dopplerprint calibrate`); see docs/research/calibration_2026-11-03.md. Not built: the time-shifted null.

## Iteration loop

- `dopplerprint eval` runs every case in `data/cases/*/` (truth in `truth.json`) and the sim suite.
- It writes `runs/<timestamp>/summary.{json,md}`. Columns are error in metres, truth rank, whether the truth falls inside the 90% region, runtime, and the stage where the truth was lost.
- Pass an experiment TOML to A/B any parameter. Defaults in `config.rs` point to the run that justified them.
