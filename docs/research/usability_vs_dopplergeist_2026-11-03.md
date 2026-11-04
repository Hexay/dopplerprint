# Usability: dopplerprint vs DOPPLERGEIST (2026-11-03)

Seen from a new user's side. DOPPLERGEIST (DG) is at `dg_upstream` HEAD `994d1ee` (merge PR #258, 2026-11-03). It was
installed with `uv pip install` from a `git archive` copy into a scratch venv (Python 3.13). That took 1.8 s,
but only because the uv cache was warm. dopplerprint (DP) was run as `target/release/dopplerprint` 0.1.0 (built 11:40).

**Caveats about our build.** This binary was built before `serve` landed (`bd08dc5`, 17:50), and
`dopplerprint serve --help` gives `error: unrecognized subcommand 'serve'`. The in-tree
`python/dopplerprint/_core.abi3.so` (10:29) predates `locate_json` (`7ed7d2e`), so
`dopplerprint.locate(...)` raises `AttributeError: module 'dopplerprint._core' has no attribute 'locate_json'`.
Both are stale dev artifacts and would not affect a wheel user, so the DP web UI and Python API below are
assessed from code and docs (`crates/dp-serve/src/web/*.js`, `crates/dp-cli/src/cmd/serve_backend.rs`, docs/USAGE.md).
Re-check after `tools/dev.sh py` and a release build. Heavy searches were not run on either tool.

Test inputs: `tone.wav` (20 s, 110 Hz sine), `tone_ct.m4a` (the same tone with MP4 `creation_time` 2026-05-14T17:02Z),
`notes.mp3` and `notes.txt` (text), and `missing.mp3` (does not exist). DP ran with `--offline` and an empty `DOPPLERPRINT_CACHE`.
DG ran without `--online`.

## Dimension table

| Dimension | DP vs DG | Evidence |
|---|---|---|
| Install path | **ahead** (on paper) | DP: `pip install dopplerprint`. abi3 wheels for Linux x86_64/aarch64, macOS x86_64/arm64 and Windows x64 come from `.github/workflows/release.yml`, and the only runtime dependency is numpy. Audio is decoded natively, with no ffmpeg. PyPI availability was not verified from the devbox. DG: git clone + venv + `pip install .`. It also needs ffmpeg/ffprobe on PATH, a C compiler for the optional native backends, and heavy wheels (scipy, pandas, rasterio/GDAL, pyproj, matplotlib, pyarrow). |
| Reproducible dev env | **behind** | DG ships `flake.nix` (`nix develop` devshell with ffmpeg, GDAL, PROJ etc., plus a `package.nix` app) and a pinned `requirements/py314-linux.lock`. DP has `uv.lock`, but no Nix flake and no devshell. |
| Platform support | **ahead** | DP builds wheels for 3 OSes. DG CI is Linux only (`tests.yml`, `checkpoint-replay.yml`, `real-performance.yml`), and the README says "Native Windows execution is unverified". Geography: both are CONUS only. |
| Required inputs | **ahead** | DP needs audio, `--start` and `--near`. DG needs audio plus `--start-utc`/`--end-utc`/`--time-source`, and the CLI and web form have **no place input**: the search is always all of CONUS (`requested_area: {coverage: "conus"}`). A bounded area needs an operator `--config` JSON. DP's `--near` takes a place name, `lat,lon`, `lat,lon,r` or a bbox, so a user can narrow the search themselves. |
| Time parsing | **ahead** | DP accepts a local time with an offset, `Z`, `UTC-7`, `-0700` and UNIX seconds, plus `±10m`/`--start-tolerance`. DG accepts strict ISO 8601 with a timezone, plus a start/end window. Neither accepts `PDT`, `May 14 2026 10:02am -07:00` or a time without a date. DG's web form takes **UTC only**. DP's web form has a time-zone picker (`web/index.html:70`). |
| No time given | **ahead** (different stance) | DP falls back to MP4/M4A/MOV `creation_time` (it used `"source": "MP4 creation_time in the file"` and widened the window to cover either a start stamp or an end stamp), and asks only when the file has no time. DG always refuses to use file time and returns `needs_input / recording_time_required`, even for the m4a with `creation_time`. DG's line is more defensible, but it costs a new user a round trip. |
| Bad-input errors | **ahead**, with one weak spot | See [Exact messages](#exact-error-messages). DP's messages say what to type. Its weak spot is non-audio files, where it leaks decoder internals (`unsupported feature: core (probe): no suitable format reader found`). DG's errors are terse ("Timestamp must be ISO 8601…") and come after a full argparse usage banner. |
| Exit codes | **ahead** | DP returns 0 found, 1 error, 2 needs input, 3 inconclusive (all observed). DG `analyze` returns **0 for `failed`, `needs_input` and `inconclusive`**. Only argparse-level errors give 2. `resume` of a non-resumable job printed an error and also exited 0. |
| Validation order | **behind** (partly) | DP checks time and place before it decodes, which is fast, but it reports **one missing input per run** (time first, then place). DG decodes and runs discovery *before* it asks for time, so the user already has `discovery.png` and the event proposals when asked. |
| First-run data & cache | **ahead** | DP has one cache (`--cache` > `$DOPPLERPRINT_CACHE` > XDG). It asks yes/no and shows the size (~3.5 GB per UTC day), and offers `--yes`, `--offline`, and `--max-cache-gb` (default 20). Downloads resume, and the place name is geocoded once and cached (the online `--near "Livermore, CA"` worked). DG uses `--online` with `~/.cache/dopplergeist`. Its docs warn "tens of gigabytes or more and hours" and "no automatic disk-space limit is currently enforced", and there is no prompt. |
| Progress reporting | **parity** | DP CLI prints stage lines ("Reading…", "Listening for aircraft passes…") and a download `\r` percentage (`app/ui.rs:48`). The DP web UI has a progress bar, a stage list and the queue position. DG `analyze` prints the job id and then blocks. `job --watch` prints `inconclusive (complete, 100.0%): 0 local checkpoint pieces, status updated 9s ago` with checkpoint rate and age, and the web UI has a progress bar and stage. DG's tile-checkpoint telemetry suits multi-hour runs. DP runs are minutes, so it needs less. |
| Cancellation | **behind** | DG has `dopplergeist cancel ID [--jobs|--server-url]`, `POST /api/jobs/{id}/cancel` and a web **Cancel** button that kills the process group. DP has no cancel anywhere (grep `cancel` in dp-serve/dp-cli/app: none). The only option is Ctrl-C on the CLI, and the web UI has no way to stop a queued or running job. Also observed in DG: `cancel` on a finished job silently printed the job JSON and exited 0. |
| Resume / checkpoint | **behind** | DG writes `resume.json` after discovery and per-tile search checkpoints. `resume ID` (CLI, HTTP or button) reuses completed discovery and tiles and checks integrity. DP resumes **downloads** only. In the DP server, a job that was running when the server stopped is marked failed and has to be rerun from scratch. DP's needs-input re-run avoids a re-upload ("Run again"), but DG's `needs_input` explicitly asks for a *new* analysis. |
| Job persistence | **parity** | Both keep a folder per job that survives restart (DP `--jobs <cache>/jobs` with `job.json`; DG `--jobs jobs` with `request/provenance/result/resume.json` and `worker.log`). DG also has `job ID` on the CLI to read a saved result without rerunning. DP has no CLI equivalent (the CLI writes `result.json` to `--out`). |
| Server robustness | **ahead** | DP: Host-header guard against DNS rebinding (`http.rs:36`), the next free port if the default is taken, uploads checked by content. DG: a second `serve` on the same jobs dir crashed with a raw traceback (`ValueError: Another job manager is using this directory`). Uploads without `Content-Type: application/octet-stream` get a clear 4xx. Both bind to loopback and give JSON `{"error": …}`. |
| Output formats | **ahead** | DP writes `result.html` (a single-file map with heatmap, candidates, tracks, passes and spectrogram), `result.geojson`, `result.gpkg` (styled for QGIS) and `result.json`. DG writes JSON artifacts, `discovery.png` and diagnostic PNGs/CSVs, and has no map: candidates are OSM links only (`web/app.js:84`). Gap: **DP writes no files for an inconclusive or needs-input run** (`"files": []`), while DG always leaves `discovery.png` + `discovery.json` to inspect. |
| Evidence / provenance | **behind** | Every DG job has `provenance.json` (source sha256, request, versions), `intake.json`, `worker.log` and a `reason_code`. The DP report has `start.source` and the `Air:`/terrain lines, but no input hash, version/commit stamp or log file. |
| Result & confidence explanation | **ahead** (CLI) / **behind** (web) | DP CLI/JSON gives a calibrated 90% region (area, islands), the probability of each distinct solution, the mirror solution explained, the matched aircraft and the fitted clock. USAGE explains the calibration and that `confidence` is a heuristic and not a probability. DG uses `reason_code`s with a documented decision policy (`docs/CONFIDENCE.md`). Its cards say "Score … not a probability" and "Sensitivity radius" versus "Calibrated uncertainty". DG says what a number is *not* more consistently. **DP web gap**: `serve_backend.rs:94-98` sets `radius_m: None` and shows the *heuristic* `confidence` level, so the calibrated region the CLI reports never reaches the browser ("Probably within about …" never renders). |
| Machine-readable status | **behind** | DG has `schema_version`, `status`, `reason_code` (e.g. `recording_time_required`, `invalid_media`, `no_supported_events`) and `required_fields: ["time_window"]`. DP JSON has `status` and a prose `message`, with no reason code, no list of missing fields and no schema version. With `--json`, a missing file still prints plain text `error: no such file: …` to stderr. |
| Documentation | **ahead** | DP: a 99-line README (quick start, accuracy table with caveats, how it works) and a 131-line USAGE covering every flag, the cache layout, weather terms, outputs and exit codes. DG: a 214-line README where about 60% is benchmark prose with commit hashes. Install is clear (Quickstart), but run instructions are spread across PRODUCT.md (313 lines) and 40+ dated docs. HANDOFF.md says it "is not a current feature inventory". DG's strengths are honest status tables and a reason-code reference. |
| Web UI | **ahead** | DP: drop a file, give the time with a time zone and "give or take", give a place. It shows progress and the queue, embeds the map with a copy-coordinate button, explains the mirror solution and lists the aircraft, offers GeoJSON/JSON downloads, has "Run again" without a re-upload, and has a `--demo` mode with no data. DG: upload, UTC datetime-local inputs, a time-source note, a coverage card, candidate cards with OSM links, the artifact list with inline diagnostic PNGs, cancel/resume buttons and recent jobs in localStorage. DG gives no place input, no map and no demo, and its upload limit defaults to 100 MB (DP's is 1 GB). |
| Python API | **ahead** (design) | DP: `dopplerprint.locate(path, start=, near=, **opts) -> dict`, the same pipeline as the CLI, documented in USAGE (currently unrunnable in-tree, see caveats). DG has no documented single-call API: modules (`service`, `jobs`, `confidence.decide_location`) are importable, but the user flow is CLI or HTTP. |

## Exact error messages

DP is `dopplerprint locate --offline …` and DG is `dopplergeist analyze --jobs J …`. Each DG argparse error is preceded by
the two-line `usage: dopplergeist [-h] [--version] {run,measure,analyze,serve,job,cancel,resume} ...` banner.

| Case | DP (exit) | DG (exit) |
|---|---|---|
| Missing file | `error: no such file: …/missing.mp3` (1) | `dopplergeist: error: Audio file is unavailable or not a regular file: …/missing.mp3` (2) |
| Malformed time `yesterday at 10` | `Can't read --start "yesterday at 10"; use e.g. --start "2026-05-14 10:02 -07:00" or "2026-05-14T17:02:00Z".` (2) | `dopplergeist: error: Timestamp must be ISO 8601 with an explicit timezone` (2) |
| Impossible date `2026-13-40 …` | same "Can't read --start …" (2) | (same ISO message) |
| Time without offset `2026-05-14 10:02` | `--start "2026-05-14 10:02" has no UTC offset; add one, e.g. --start "2026-05-14 10:02 -07:00" (or Z for UTC).` (2) | `dopplergeist: error: Timestamp requires an explicit timezone` (2) |
| Reversed window | n/a (single start ± tolerance) | `dopplergeist: error: Recording-start window is reversed` (2) |
| No time | `Need the time the recording started: add --start "2026-05-14 10:02 -07:00" (your local time and its UTC offset; append "±10m" if unsure).` (2) | JSON `status: needs_input`, `reason_code: recording_time_required`, `required_fields: ["time_window"]`, `message: "Supply a defensible UTC window for the start of this recording. File creation and upload times are not recording clocks."` (**0**) |
| No time, m4a with `creation_time` | used the file time and ran: `"source": "MP4 creation_time in the file"` (3, no passes) | same `needs_input` as above (**0**) |
| No place | `Need roughly where the recording was made: add --near "Livermore, CA" or --near 37.68,-121.77 (within ~10 km; whole-country search isn't supported yet).` (2) | n/a: always CONUS |
| Radius 200 km | `A 200 km search radius is too wide; narrow --near to within 50 km (whole-country search isn't supported yet).` (2) | n/a |
| Outside coverage (Paris) | `48.8500, 2.3500 is outside the area dopplerprint covers so far (the contiguous US).` (2) | n/a |
| Unknown place, offline | `"Nowhereville, ZZ" isn't in the place cache and --offline is set; give --near as lat,lon instead.` (2) | n/a |
| Non-audio (`notes.mp3`, `notes.txt`) | `error: unrecognised container: …/notes.mp3: unsupported feature: core (probe): no suitable format reader found` (1) | JSON `status: failed`, `reason_code: invalid_media`, `message: "Media probe failed; file is unsupported or corrupt"` (**0**) |
| Audio with no flyover | `Inconclusive: no aircraft pass found in the audio; dopplerprint needs at least one propeller or helicopter tone that drops in pitch as it goes by.` (3) | `status: inconclusive`, `reason_code: no_supported_events`, `"No supported moving-tone Doppler events were discovered in the recording."` (**0**) |
| Bad job id | n/a | `dopplergeist job: error: argument job_id: job_id must be the 32-character job identifier` (2) |
| Resume a needs_input job | n/a | `dopplergeist: error: Could not resume job …: Only stopped, failed, or inconclusive jobs can be resumed` (0) |
| HTTP bad time | n/a (DP serve not runnable here) | `{"error": "Timestamp must be ISO 8601 with an explicit timezone"}` |
| HTTP missing content type | n/a | `{"error": "Upload raw audio as application/octet-stream"}` |
| Second server, same jobs dir | n/a | Python traceback ending `ValueError: Another job manager is using this directory` |

The time-format matrix, where "accepted" means the parse passed:

| Input | DP | DG |
|---|---|---|
| `2026-05-14T10:02-0700`, `2026-05-14 17:02Z`, `…+00:00` | accepted | accepted |
| `2026-05-14 10:02 UTC-7`, `1778778120` (UNIX) | accepted | rejected |
| `2026-05-14 10:02 PDT`, `May 14 2026 10:02am -07:00`, `10:02 -07:00` | rejected | rejected |

## Usability gaps to close, ranked

1. **Cancel in the web UI and HTTP API.** DP has none. Add `POST /api/jobs/{id}/cancel`, a Cancel button for
   queued and running jobs, and a cooperative check in the pipeline's progress callback. Ctrl-C on the CLI should
   also leave a clear "cancelled" line, not a partial dump.
2. **The web result must show the calibrated region, not only the heuristic label.** Fill `radius_m` (or the 90% region
   area and island count) in `serve_backend.rs` from `rep.posterior`, and show the per-solution probabilities that the CLI
   prints. Today the browser drops the one number we calibrated.
3. **Write diagnostics on inconclusive results.** When no pass is found, or matching is too weak, emit at least the spectrogram
   with detected or candidate ridges (`result.html` without a map, or `spectrogram.png`) plus `result.json`. DG always leaves
   `discovery.png`. "0 aircraft passes" with nothing to look at is the most common first-run outcome.
4. **Machine-readable status.** Add `reason` (e.g. `time_required`, `place_required`, `no_passes`, `no_flights`,
   `weak_match`, `bad_audio`), `missing: ["start","near"]` and `schema_version` to the JSON report, and make
   `--json` emit JSON for every error, including a missing file. The web form already needs this to mark fields
   without parsing prose.
5. **Report all missing inputs at once.** With neither `--start` nor `--near`, the user currently needs two runs.
6. **Friendly non-audio error.** Map the symphonia probe failure to: `"notes.mp3" isn't audio dopplerprint can read
   (WAV, MP3, FLAC, Ogg, M4A/AAC); if it plays elsewhere, convert it to WAV.`
7. **Resume after a server restart.** Re-queue a job that was running when the server stopped (or offer one-click
   Resume) instead of marking it failed. The cheaper version: cache the decoded and discovered events per job so a rerun
   skips the audio stage. DG's tile checkpoints are overkill for minute-scale runs.
8. **Time-zone abbreviations.** Accept `PST/PDT/MST/MDT/CST/CDT/EST/EDT/AKST/AKDT/HST` (we are CONUS only, so there is no
   ambiguity) and `am/pm`.
9. **Provenance in the report.** Add the input sha256, the dopplerprint version and git commit, the cache data days used,
   and a `log.txt` in the result dir, so a shared `result.json` can be reproduced.
10. **A CLI view of saved jobs.** For example `dopplerprint jobs [ID]` to read server jobs without the browser (DG `job ID --watch`).
11. **Nix devshell (optional).** A small `flake.nix` with rustup, uv and maturin would match DG for Nix users. It is low
    priority, since wheels already avoid native dependencies.
12. **Dev-loop hygiene.** The in-tree `_core.abi3.so` and `target/release/dopplerprint` were stale relative to the wrapper and
    CLI. A version or ABI check in `python/dopplerprint/__init__.py` (compare `_core.__version__`/a feature list) would
    turn `AttributeError` into "rebuild with tools/dev.sh py".

## Where DG is worth copying, and where it is not

- Worth copying: reason codes with `required_fields`, cancel and resume as first-class features, provenance per job,
  diagnostics on every outcome, and wording that says what a number is *not*.
- Not worth copying: exit 0 for failed and needs-input runs, CONUS-wide search without a place input, no disk cap
  on downloads, UTC-only web time entry, and README space spent on benchmark history.
