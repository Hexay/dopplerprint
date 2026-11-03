# thunderprint as a template for doppler_print

Source: `/srv/agents/work/doppler_print/reference/thunderprint` (abbrev. `TP/`). Read 2026-11-03. Single squashed
commit (`8f6886b`), so dev history isn't recoverable from git. The process has to be inferred from `research/`,
`tools/`, `experiments/` and `eval/`.

**Corrections to the brief, up front:**
- **No PyO3 and no numpy interop.** Python ships via maturin `bindings = "bin"`. The wheel bundles the CLI binary
  and a pure-Python wrapper runs it as a subprocess and parses its JSON output (`TP/pyproject.toml`,
  `TP/python/thunderprint/_cli.py`).
- **No Cargo features, no explicit `crate-type`, no SIMD.** Parallelism is rayon `par_iter().map_init(...)` only.
  Speed comes from FFT correlation (rustfft) and per-thread scratch reuse.
- **No QGIS plugin or processing script.** QGIS support is a hand-written GeoPackage file with no layer styles.
  The interactive UI is a single self-contained Leaflet+Plotly HTML page.
- Directories missing from the brief: `src/audio/` (decode/filter/envelope/stereo/spectrogram), `src/strikes/`
  (catalogue sources), `eval/` (truth TOMLs), `experiments/` (override TOMLs).

---

## 1. Repo layout and module responsibilities

```
Cargo.toml  pyproject.toml  README.md  LICENSE(MIT)  .gitignore (/target .env /data /out /litreview /docs)
.github/{workflows/ci.yml, workflows/release.yml, dependabot.yml}
assets/viewer.html (include_str!'d template)  assets/screenshot.jpg (README hero)
src/  lib.rs (pub mod list only)  main.rs (clap CLI, 272 lines)
python/thunderprint/{__init__.py,_cli.py,results.py,py.typed}  python/tests/smoke.py
research/*.md  research/<Topic>.md + research/<Topic>/*.md (deep-research sub-reports)
tools/*.sh (experiment drivers)  tools/*.py (offline analysis/plots)
eval/truth.toml  eval/clips/<youtube-id>.toml     experiments/<name>.toml (SearchConfig overrides)
```
Every source file is ≤ 272 lines (total src ≈ 3.8k LOC). Modules are small and single-purpose.

| Module | Files | Responsibility | Doppler analogue |
|---|---|---|---|
| `geo` | `src/geo.rs` | `LatLon`, local ENU km (`enu_km`, `offset_km`, `dist_km`), hand-rolled civil date ↔ epoch (Hinnant), `parse_date/hms/iso`, `format_utc`. No chrono dep. | same; add altitude/ECEF for slant range to aircraft |
| `strikes` | `src/strikes/{mod,frost,csv,ualf}.rs` | Source catalogue: `Strike` struct, Frost API fetch + per-day cache (`$THUNDERPRINT_CACHE`), UALF parser, user CSV (`--strikes`), `StrikeIndex` (time-sorted + lat/lon hash buckets, `within(p, r, &mut ids)`). | `adsb/`: OpenSky/ADSBx/adsb.lol fetch+cache, CSV import, `TrackIndex` (spatial-temporal) |
| `audio` | `src/audio/{decode,filter,envelope,spectrogram,stereo}.rs` | ffmpeg subprocess decode to f32 at `ANALYSIS_FS`=2 kHz, zero-phase Butterworth bandpass 20–250 Hz, RMS envelope + Gaussian smooth + rolling-min floor removal, viewer spectrogram (8-bit dB, base64), stereo GCC lag track. `BandAudio` is the observation struct. | `audio/`: STFT/reassigned spectrogram, harmonic-line tracker giving an observed f(t) track |
| `model` | `src/model/{params,propagation,render,emg}.rs` | Forward physics. `ModelKind {Improved, Baseline}` (baseline reimplements the inspiring video). `Atmosphere` nuisance (c, wind, lapse, onset height). `Physics` priors (serde default). `arrival()` gives one `Arrival` per strike/listener. `render()` draws EMG pulses into an envelope buffer. | `model/`: `doppler.rs` f_obs(t)=f0·c/(c+v·r̂) with retarded time, `Atmosphere` (c, wind), `Physics` (blade/engine harmonics prior) |
| `sim` | `src/sim/{mod,channel}.rs` | **Independent, richer synthesiser** for the twin: tortuous 3-D channel segments, Ricker wavelets, directivity, refraction shadow, rain floor + car bursts, optional 2-mic stereo, `write_wav` (hound). `Scenario` struct. | synthesize aircraft audio from real ADS-B tracks: harmonic stack, ground reflection/comb, wind, noise, other aircraft |
| `search` | `src/search/{pipeline,scorer,ncc,grid,select,feature,robust,free_amp,doa,shape,trace,probe}.rs` | Inverse: coarse→mid→fine grids, per-cell scoring with FFT NCC over start-time lags, tempered evidence, posterior, credible region, ellipse, reliability flag, diagnostics. See §4. | same skeleton |
| `onset` | `src/onset/{detect,solve,residuals}.rs` | **Independent cross-check solver**: detects onsets (MAD threshold), runs consensus voting over (cell, start bin) with u64 bitmasks, then Gauss-Newton. `residuals` writes CSV residual-vs-distance diagnostics. | closest-point-of-approach (CPA) time picker + geometric solve |
| `output` | `src/output/{mod,geojson,gpkg}.rs` | `write_all`: heatmap.geojson, result.gpkg, candidates.json, trace.json, index.html (template string-replaced). | same |
| `eval` | `src/eval.rs` | `Truth {lat, lon, start?}` read only by eval. `report()` prints per-candidate error, start error, truth-in-CR90, credible level, coarse/mid truth rank, ellipse along/across offsets. | same |
| `config` | `src/config.rs` | `defaults(kind)` → `SearchConfig`. An `overrides!` macro declares the TOML-overridable fields once (`deny_unknown_fields`). List-valued keys (`lapse_k_per_km`, `cg_onset_alt_km`) are cross-producted into the nuisance grid. | same pattern |
| `main` | `src/main.rs` | clap derive. Subcommands `fetch / locate / sim / probe / eval`. `#[command(flatten)]` shared arg groups `ModelArgs` (`--model`, `--overrides`) and `StrikeArgs` (`--strikes`). `dotenvy` loads `.env`. Progress goes to stderr, results to stdout. | `fetch / locate / sim / probe / eval` |

Conventions visible in code: doc comments on every pub item, written as terse rationale. Gotchas are stated
inline (`main.rs:31` "video flash times give the frame clock…"). Constants carry their evidence
(`shape.rs:31` MIN_RELIABLE_STRIKES justified by sim-suite numbers; `config.rs:89-93` explains why features are
off by default, e.g. "Off: on the sim tier … no gain"). Physics constants point to `research/*.md`
(`params.rs`, `config.rs:54`).

## 2. Rust crate setup

**`TP/Cargo.toml`**: edition 2024, v0.1.1, MIT, `include = ["src/**", "assets/viewer.html", "README.md", "LICENSE"]`
keeps research/eval out of the published crate. Lib and bin coexist through Cargo's auto-detection: `src/lib.rs`
(crate `thunderprint`) plus `src/main.rs`, which `use thunderprint::{...}`. There is no `[lib]`/`[[bin]]` table and no features.
Deps (all plain): `anyhow, base64, clap(derive), dotenvy, hound, rand 0.8, rand_distr, rayon, rusqlite(bundled),
rustfft, serde(derive), serde_json, toml, ureq`. Audio decode shells out to **ffmpeg** (`audio/decode.rs`). It
does not use symphonia.
Profiles: `[profile.release] debug=1, lto="thin", codegen-units=1` and `[profile.dev] opt-level=2`, so dev
builds are fast enough to run the search.

**Parallelism pattern** (copy this): `cells.par_iter().map_init(Workspace::default, |ws, c| scorer.score(c, atm, ws))`
(`pipeline.rs:211`). A `Workspace` (`scorer.rs:57`) holds every reusable Vec (ids, arrivals, rendered, prefix sums,
FFT scratch, rho, lls), so the hot loop doesn't allocate. `Workspace.phase_s[4]` accumulates per-phase timings
for `probe --bench`. f32 for signal buffers, f64 for accumulation and likelihoods.

**Python** (`TP/pyproject.toml`): `maturin>=1.7,<2`, `[tool.maturin] bindings="bin", python-source="python",
module-name="thunderprint", strip=true`, `dynamic=["version"]` (version from Cargo).
- `_cli.py`: `binary()` finds the exe in `sysconfig` scripts dirs (handles `--user` installs not on PATH) and
  falls back to `shutil.which`. `run(args)` sets the `THUNDERPRINT_CACHE` default to the platform user-cache dir.
  Non-zero exit raises `ThunderprintError(stderr)`.
- `__init__.py`: `locate()`, `simulate()`, `fetch()`, `load()`. Accepts str/date/time and strikes as a path,
  dict rows or a DataFrame (duck-typed `to_dict("records")`, so no pandas dependency), written to a temp CSV. Naive
  datetimes are treated as UTC (gotcha comment).
- `results.py`: frozen dataclasses `Region`, `Envelope`, `Spectrogram`, plus `Result` with `.best`, `.viewer`,
  `.geopackage` and `.envelope(rank)`. Parses `candidates.json` and `trace.json`. **The JSON outputs are the API
  contract between Rust and Python.**
- `python/tests/smoke.py`: offline end-to-end run. Synthetic strikes → `simulate` → `locate` → asserts error < 3 km,
  that the viewer and gpkg exist, and that a bad input raises.

**CI** `TP/.github/workflows/ci.yml`: job `check` (clippy --all-targets, cargo test, Swatinem/rust-cache) and job
`python` (apt ffmpeg, `maturin build --release`, pip install the wheel, run smoke.py).
**Release** `release.yml` runs on tag `v*`:
- `wheels` uses a maturin-action matrix: linux x86_64/aarch64 (native `ubuntu-24.04-arm`, because cross-compiling
  `ring` fails), windows x64, macos intel/arm.
- `sdist`.
- `binaries`: `cargo build --release --locked`, packaged tar.gz/zip.
- `github-release`: `gh release create --generate-notes`.
- `publish`: PyPI trusted publishing via `uv publish` + `actions/attest`.
- `crates`: an idempotent check whether the version already exists, then `rust-lang/crates-io-auth-action` OIDC.
- `dependabot.yml`: monthly, grouped (actions; cargo minor+patch).

**Decision for doppler_print:** keep `bindings="bin"` (zero ABI pain, one artefact serves CLI and Python) unless
we need in-process numpy arrays. If we do, add a separate `pyo3` feature/crate later. Don't start there.

## 3. QGIS / GIS integration

There is no plugin. `src/output/gpkg.rs` (164 lines) is a **dependency-light OGC GeoPackage 1.4 writer** on rusqlite.
It writes the core tables by hand (`gpkg_spatial_ref_sys` with EPSG:4326 WKT, `gpkg_contents`,
`gpkg_geometry_columns`; `application_id=1196444487`, `user_version=10400`) and builds GP-header + little-endian WKB
blobs for Point and Polygon. It has no R-tree, no `layer_styles` table and no QML/SLD.
- Driven from GeoJSON: `geojson::feature_collection` builds one FeatureCollection whose features carry
  `properties.kind`. `gpkg::write` splits it into layers via `LAYERS = [cell→cells, candidate→candidates,
  ellipse→ellipses, onset→onsets]`. Columns are inferred from JSON types. Nested objects are flattened to
  `parent_child` columns, arrays become JSON text.
- Layer schemas (`output/geojson.rs`):
  - `cells` (Polygon): level, region, prob, log_ev, rho, start_utc, strikes, in_cr90. Only cells with log_ev>0
    or level=fine are written.
  - `candidates` (Point): rank, region_prob, log_ev, rho, start_utc, c_mps, wind_e/n, cr90_area_km2,
    shape_* (ellipse axes, bearings, audible_strikes, azimuth spread), reliable.
  - `ellipses` (Polygon, 48-pt ring): rank.
  - `onsets` (Point): start_utc, inliers, onsets, sigma_e/n_km, rms_resid_s.
- README claims QGIS/ArcGIS drag-and-drop and documents `geopandas.read_file(gpkg, layer="cells")`.

**`assets/viewer.html`** (178 lines) is embedded with `include_str!` in `output/mod.rs:13`, and Rust
string-replaces the placeholders `/*GEOJSON*/null`, `/*RUN*/null`, `/*TRACES*/null`, `/*SPEC*/null`. The result
is a single file that works from `file://`.
- Leaflet 1.9.4 and Plotly-cartesian load from CDN, over Esri World_Topo/World_Imagery tiles. The tiles are keyless;
  a comment notes that OSM 403s without a Referer.
- Right-hand panel: candidate list (click to switch the trace), level toggles (coarse/mid/fine), metric selector
  (posterior prob, log-scaled over 4 decades, vs rho), and a CR90-only toggle.
- Bottom 42vh: observed vs modelled envelope over UTC time, with the spectrogram on a shared time axis.
- Dark-mode CSS vars, turbo colour ramp, attribution line for the data provider.

**For doppler_print:** reuse gpkg.rs as-is (it's generic over GeoJSON). Add LineString support (WKB type 2)
for **aircraft track layers** (matched track, CPA point, Doppler-predicted segment). Optionally add a
`layer_styles` table with QML so QGIS opens the layers styled. That would be a cheap step beyond thunderprint.

## 4. Search algorithm structure (the pattern to mirror)

`search::pipeline::run(cfg, index, audio, window)` in `TP/src/search/pipeline.rs`:
1. **Observation prep**, once per stage: `observed_feature` = envelope at stage rate/smoothing → floor removal
   → `^gamma` compression → optional `Feature::Rise` transform. `Template::new` centres it, normalises it, and
   precomputes its FFT (`ncc.rs`). The *same* transforms are applied to the model (`feature.rs`: "Applied
   identically to model and observation").
2. **Coarse grid** over the bbox of all sources: `grid::cover` (constant-km equal-area-ish cells). Then
   `prune_inactive` drops cells that never see `min_strikes` audible sources inside a clip-length sliding
   window (a cheap geometric gate before scoring). Score at 2.5 km / 1 Hz / 2.5 s smoothing, single default
   atmosphere.
3. **Select** with `separated_peaks` (top 100, no separation at coarse, so near-duplicates can't crowd out truth).
   The coarse/mid key is `select_key` (peak-rho log-lik), because the day-long time prior swamps `log_ev`.
4. **Mid**: `grid::around` each peak (±2.5 km @ 0.75 km, 2.5 Hz). Pick `fine_regions=3` seeds separated by
   2·fine_half_km, each carrying its best start time.
5. **Fine** per seed: ±12 km @ 0.5 km, 10 Hz, start range ±120 s around the seed start. **Marginalise nuisance**
   (`score_cells_marginal`): score under each `Atmosphere` in the grid (3 temps × 9 winds × onset heights × lapse),
   `log_mean_exp` the evidence and keep the best atmosphere for reporting.
6. **Posterior**: `select::posterior` = softmax(log_ev + ln cell_area) (uniform-per-area prior).
   `credible_region` gives the CR90 mask and area. `polish`: ±1.5 km @ 0.1 km under the best atmosphere.
   `shape::shape` gives a Gaussian-moment 90% ellipse (√χ²₂(0.9)=2.146), storm bearing and azimuth spread.
   `reliable()` = audible_strikes ≥ 30 (evidence-justified constant).
7. **Rank regions** by `rank_key` (Improved: log_ev; Baseline: peak rho, as in the video). Region probabilities
   come from softmax over region log_ev.

**Scorer** (`scorer.rs`), per (cell, atmosphere):
1. Spatial query of sources within radius, filtered to the time window.
2. `arrival()` per source.
3. `render()` the model over the whole start-range plus clip length.
4. Feature transform.
5. Prefix sums p1/p2.
6. `Template::correlate`: overlap-save FFT gives the numerator at every lag. All-zero blocks are skipped via the
   prefix-abs check.
7. `lag_correlations`: Pearson rho per lag using the prefix-sum window energy. The lag is NaN when fewer than
   `min_strikes` onsets fall in the window.
8. Optional `robust` (Huber IRLS) or `free_amp` (Levenberg–Marquardt per-source log-gain refit with Gaussian
   prior penalty) rescoring of the top separated lags.
9. Per-lag log-lik `-½·n_eff·ln(1−ρ²) − penalty`.
10. `log_mean_exp_with_nulls`: invalid lags count as LL=0, giving **time-marginalised evidence vs a no-signal null**.
11. Optional `doa::log_lik`: stereo lag regressed on the bearing of the dominant source, as a BIC-penalised LLR
    added to log_ev.

`n_eff = duration / (2√π·smooth) · n_eff_factor`. That is **likelihood tempering** for model error; `n_eff_factor=0.5` was
calibrated by `tools/calibrate.sh` against CR90 coverage.

**Abstract pattern:** synthesize(model params, candidate, nuisance) → feature (same transform both sides) →
score every time lag at once (FFT NCC) → tempered log-evidence marginalised over lag and nuisance → coarse grid
(geometric pruning) → separated peaks → mid → fine with nuisance → polish → posterior/CR90/ellipse/reliability flag
→ independent cross-check solver (onset) → baseline model kept for comparison.

**Doppler mapping:** candidate = receiver (lat, lon[, alt]) and clock offset. Sources = ADS-B tracks
(interpolated position/velocity). Arrival = retarded time t_e with |r(t_e)−x|=c·(t−t_e). Predicted
f(t)=f0·(1/(1+v·r̂/c)) up to an unknown per-aircraft f0 (profile it out, like `free_amp` profiles loudness;
log-frequency makes f0 an additive offset). Observed feature = tracked harmonic f(t) or a log-freq spectrogram.
Score = NCC of log-f curves or a per-pass Gaussian LL over lags. Nuisance = c(T), wind (both shift the Doppler
curve). Reliability = number of matched passes + azimuthal spread of the CPAs (the Bondár gap criterion). The CPA
time/geometry solver plays the `onset` cross-check role. Baseline = the DOPPLERGEIST method as published.

## 5. Research process

Layout (`TP/research/`): flat topic files + one **deep-research report** with a same-named sub-folder of
sub-reports:
- `prior_art_receiver_location.md` (58 l, 2026-10-13): first quick prior-art sweep (web, Semantic Scholar, arXiv,
  Scholar, GitHub API, YouTube metadata). Sections: Direct precedent / Adjacent chronolocation / Adjacent
  receiver-from-catalogued-sources / mic self-localisation / forensic cues / Checked-nothing-relevant / **Verdict on
  novelty**.
- `thunder_physics_lit.md` (102 l, 10-13): physics lit vs the observed range error. Opens by citing the exact
  code lines being questioned (`propagation.rs:39`, `sim/mod.rs:77`). Sections by physical question, each entry
  with an *Implication:* line. Ends with **"Ranked model changes worth testing"** (7 items) and "not worth pursuing".
- `empirical_thunder_response.md` (74 l, 10-13/14): an **in-house experiment write-up** (method → tables →
  effect on location → truth-free nuisance fit → "Adopted default (date)" → "Ruled out" → "Caveats"). It produced
  the `cg_onset_alt_km ∈ {0,3,6}` default, cited from `config.rs`.
- `real_clip_candidates.md` (52 l): a table of found real recordings (URL, location+confidence, date+how known,
  duration, signal guess), plus second tier and excluded. These became `eval/clips/<id>.toml` and `tools/clips.sh`.
- `readme_conventions.md` (44 l, 10-20): survey of 7 comparable repos → README order, hygiene files, data licence
  terms (Frost NLOD/CC BY, don't redistribute raw data, attribution in viewer).
- `Thunder receiver geolocation prior art.md` (109 l, 10-20): a polished synthesis. Opens with a one-paragraph
  verdict, a **verification-label legend** (Full / Abstract / Snippet / Metadata / Self-report), a comparison table
  (Item | Method | Difference from ours | Verification), then "What the literature tells us to build next" and
  "Conclusion" (novelty split: method not novel, rigour is).
- `Thunder receiver geolocation prior art/{academic_thunder_catalogue, media_chronolocation, osint_hobbyist,
  receiver_from_catalogue}.md` (104–133 l each): per-angle sub-reports. Each has a header line "Searched <date>
  with <tools>; <paywalls hit>", then numbered sections with a fixed **Takeaway / Cited Findings / Inferences /
  Gaps** structure and inline `[abs]/[full]/[snippet]` tags.
- Also referenced but not committed: `memory/sim-suite-findings` notes, plus `/litreview` and `/docs` (gitignored).

**How research fed design:** prior art → `ModelKind::Baseline` that reimplements the precedent exactly, so the
README can claim "1.3 km vs 2.2 km baseline". The physics lit produced the ranked changes, which became
`experiments/*.toml` overrides (lapse_*, cg_onset_*, wx_*, neff_*, freeamp_*, robust_off, stereo,
min_strikes_*), run by `tools/*.sh` on the sim suite and real clip. Winners were adopted into `config.rs`
defaults with a comment and research pointer; losers stayed off, with a "no gain" comment. Bondár geometry
criteria → the reliability flag + ellipse. Readme survey → README order and badges.

**Equivalent research topics for doppler_print:**
1. `prior_art_receiver_location.md`: DOPPLERGEIST itself (method, claims, code?), acoustic aircraft
   localisation (passive acoustic tracking, Ferguson/Lo/Quinn Doppler+multipath work), receiver-from-known-emitter
   inverses (Doppler positioning: Transit/Argos/LEO signals of opportunity), ENF/media chronolocation.
   Verdict on novelty.
2. `aircraft_acoustics_lit.md`: source spectra (prop blade-pass and harmonics, turbofan fan tones/buzz-saw, helicopter
   BPF, jet broadband), tone stability vs throttle, retarded-time Doppler, ground-reflection Lloyd's mirror
   interference, atmospheric absorption by frequency, wind/temperature refraction, audibility range by type/altitude.
3. `adsb_data_sources.md`: OpenSky (Trino historical, state vectors 1–10 s, licence terms), ADS-B Exchange
   globe_history, adsb.lol, airplanes.live. Position/velocity/altitude accuracy (NACp/NIC, baro vs geo alt),
   coverage gaps, MLAT, licence/redistribution (the Frost-terms equivalent).
4. `empirical_doppler_response.md`: in-house fit of real passes against tracks (f0 drift, timing offsets,
   altitude reference), mirroring `empirical_thunder_response.md`.
5. `real_clip_candidates.md`: recordings with known place and time near busy airways/airports (YouTube plane-spotting,
   freesound geotagged).
6. `readme_conventions.md`: reuse thunderprint's, plus OpenSky/ADSBx attribution.
7. Deep-research report folder: `Aircraft Doppler receiver geolocation prior art/` with sub-reports
   (academic, OSINT/hobbyist, media chronolocation, receiver-from-catalogue).

## 6. Testing, eval, README, conventions worth copying

**Testing:** unit tests live next to the code (`#[cfg(test)]`). They are few and targeted:
- `ncc.rs` FFT correlation vs direct correlation
- `emg.rs` integrates to 1 and stays finite
- `geo.rs` civil date roundtrip and offset/ENU inverse
- `robust.rs` burst discounted
- `free_amp.rs` recovers swapped loudness
- `doa.rs` surrounding storm is informative and a one-sided storm gives 0

The accuracy testing is end-to-end:
- **Sim twin:** `sim` uses a *more detailed, independent* generator than the search model (3-D tortuous channels
  vs the parametric EMG pulses), so the inverse isn't tested against itself. It writes `audio.wav` +
  `truth.toml`. The truth is never read by the search (`eval.rs:6` "read only by evaluation").
- **Sim suite** `tools/sim_suite.sh`: 15 named cases (`name lat lon start temp wind_e wind_n noise seed`) in
  tiers. *side_\** cases are one-sided storms, *loc_\** cases are locatable (≥88 strikes heard, picked by
  `tools/pick_sim_cases.py`), and *real_twin* sims the real clip's geometry. The unlocatable tier exists to test
  the reliability flag. Suites can run both models, apply `OVR=` overrides and synthesise stereo with `MICS=`.
- **`eval` metrics**:
  - #k error in km and start error in s
  - truth inside a fine CR90
  - **credible level** (posterior mass above the truth cell; calibration target: <0.9 in ~90% of cases)
  - truth's rank at the coarse/mid levels, to diagnose which stage lost it
  - ellipse along/across error decomposition
- `tools/score_suite.sh` tabulates #1 error across override suffixes side by side without re-running and marks
  unreliable cases with `*`. `tools/calibrate.sh` sweeps `n_eff_factor` for coverage. `tools/real.sh` and
  `tools/clips.sh` run the real-clip holdouts. `tools/probe.sh` + `probe` print per-stage scores at a known point
  and dump `out/probe.csv` for `plot_probe.py`. `probe --bench` gives a per-phase ms/call profile.
  `tools/remote.sh` runs heavy suites detached on a bigger box.
- Python smoke test in CI (offline, synthetic sources).

**README structure** (`TP/README.md`, 176 l):
1. Badges (CI/PyPI/crates/License).
2. One-line pitch.
3. Hero screenshot of the viewer.
4. A 2-paragraph explanation of the physics and what the output is.
5. Install (pip / cargo / Releases, plus ffmpeg and the API key via `.env`).
6. **Quickstart with sim → locate → eval**, so a user can try it with no real recording ("should report ~0.4 km").
7. Python API.
8. Usage table, input CSV schema table and output file list.
9. Models and overrides.
10. How it works (5 numbered steps + "two physical findings drove the model", linked to research/).
11. **Accuracy and limitations** (error table for sim, twin and real-not-blind; failure modes stated plainly).
12. Prior art.
13. Data and attribution.
14. License.

**Conventions to copy:**
- Keep a **baseline model** of the precedent for head-to-head comparison.
- The **overrides TOML + `experiments/` dir** is the unit of experiment: one file per hypothesis, with a
  one-line comment stating it.
- Every default carries a justification comment pointing to the evidence.
- Name and document tiers of sim cases, including deliberately unlocatable ones.
- Hold out real clips, and label the real result "not blind" honestly.
- Outputs: JSON for machines (and the Python API), GeoPackage for GIS, self-contained HTML for humans.
- Progress goes to stderr and results to stdout, so scripts can `grep` the eval lines.
- Cache downloaded catalogue days, with an env override. Load credentials from `.env`, never commit them, and
  never redistribute raw data.
- Files ≤ ~270 lines; doc comments state rationale and gotchas.
- `research/` docs carry the search date and tools, verification labels, an *Implication*/*Inferences* line per
  finding, and end with ranked actionable changes.
