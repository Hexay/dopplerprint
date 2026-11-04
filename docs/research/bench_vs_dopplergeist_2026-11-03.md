# dopplerprint vs DOPPLERGEIST: end-to-end accuracy and speed (2026-11-03)

- **Ours:** `dopplerprint locate` from commit **c1777da** (clean worktree, release build; binary sha256
  `560a53e7…dab`, frozen at `runs/bench-vs-dg/prep/bin/`). Every case used this one binary.
- **Theirs:** DOPPLERGEIST **994d1ee** (latest upstream) in a fresh `dg_upstream/.venv` (Python 3.14, their
  `py314-linux.lock`). Both native C extensions (`_native_ridge`, `_native_acoustic`) were built and active.
  We ran the automatic product path (`analyze`, through its `JobManager`, because the CLI cannot pass a search
  box), and the legacy `run` path only for a functional check.
- **Raw output:** `runs/bench-vs-dg/acc/` (accuracy; `results.{json,tsv}`, `summary.md`),
  `runs/bench-vs-dg/diag-true-start-5s/` (diagnostic), `runs/bench-vs-dg/acc/perf/` (matched speed).
- **Harness:** `tools/bench_vs_dg.sh` (`prep | run | score | perf | perf-summary`) and `tools/bench_vs_dg/*.py`.

The scope was cut on request: Oyster Bay was not run for DOPPLERGEIST, and the speed comparison covers one case
with one repeat per thread setting.

## Protocol: same inputs for both tools

| Input | Both tools got | Difference |
|---|---|---|
| Audio | The case file: MP3 for `dg_multievent_*`, WAV for the rest | none |
| Start time | Truth start rounded to the nearest minute, **±2 min** (ours: `--start` and `--start-tolerance 120s`; DG: `start_utc`/`end_utc` = minute ∓ 120 s) | none |
| Area | 10 km around a public place centre: "Livermore, CA" 37.6819,-121.7680 (2.4 km from truth), "Oyster Bay, NY" 40.8715,-73.5318; synthetic: a point 5 km NW of truth | Ours searches circles and DG searches boxes. Ours got `lat,lon,10` (314 km²); DG got that circle's bounding box (400 km², a superset). |
| ADS-B | The same adsb.lol day archive: `v2026.05.17-planes-readsb-prod-0`. Ours reads its store (`ingest`, 34 s per day). DG reads its own FlightIndex, built by `tools/bench_vs_dg/dg_index.py` with DG's own `FlightIndex.build(archive_staging(...))`. | DG's `FlightCache` only fetches `staging-0` releases, so we built the `prod-0` index offline through the documented `flight_indexes` operator override. |
| ADS-B, synthetic | The case's finite track population (7 to 18 flights). Ours: converted to readsb traces and ingested. DG: `FlightIndex.build` straight from `tracks.parquet`/`tracks.csv`. | Synthetic flight IDs map to ICAO-like IDs (`f` + sha1[:5]) on both sides. |
| Terrain | Livermore: ours used 3DEP 30 m (`livermore_30m.u16`); DG used GLO-30 tiles + EGM96. Synthetic: ours a constant DEM (120 m − N, the same ellipsoidal ground); DG the fixture's `terrain.tif`/`geoid.tif`. | 3DEP vs GLO-30 in the accuracy run (GLO-30 for both in the speed run) |
| Atmosphere | Ours: archived Open-Meteo weather on real cases (the default), still air on synthetic. DG: still air, c 343 m/s (it has no weather model). | Ours had more information on real cases |
| Events | Automatic discovery on both sides. Ours keeps at most 4 passes (the default); DG has no cap. | — |
| CPU | Ours: 8 cores (`taskset 0-7`), alone. DG: 3 parallel lanes of 2 cores each, with `search_workers` at its default (cores − 1). | DG's search of a one-tile box runs in a single process, so lanes cost it little. Wall times include whatever load other sessions put on the machine. |
| Time cap | DG `max_runtime` 1800 s. Its default is 3600 s; its README suggests 14400 s. | Ours has no cap (it finished in ≤ 39 s). |

**Status mapping.** Ours: `found` → found; `inconclusive` with a location → ambiguous; without one → failed. DG:
`succeeded` → found; `inconclusive` with candidates → ambiguous (we score its leading candidate); without
candidates → failed. Success means found **and** ≤ 1000 m, DOPPLERGEIST's stated target. The uncertainty
region for ours is the calibrated 90% cell set (`in_cr90`) and its area. For DG it is the leading candidate's
`uncertainty.radius_m`, which DG labels `sensitivity` (uncalibrated). Top-1 ID means the first matched flight
is a truth flight. Clock error is the fitted start's distance to the truth start (or truth window).

Truth caveats: Livermore recorder coordinates are trilaterated (about 5 m RMS). The Livermore start is only a
10 s window. The Oyster Bay start was itself fitted by DOPPLERGEIST.

## Per-case results (accuracy run, `runs/bench-vs-dg/acc`)

Cells: status | error m | truth inside region | top-1 ID | clock error s | wall s | peak tree RSS MB.

| case | ours | DOPPLERGEIST |
|---|---|---|
| dg_synthetic_known_minute | found \| 10 \| in (314 km²) \| ok \| +0.2 \| 4.1 \| 85 | ambiguous \| **2** \| in (r 530 m) \| ok \| −0.0 \| 101 \| 429 |
| dg_synthetic_two_events | found \| 10 \| in (314 km²) \| ok \| +0.2 \| 4.0 \| 100 | ambiguous \| **2** \| in (r 530 m) \| ok \| −0.0 \| 84 \| 423 |
| dg_multievent_3 | found \| 7 \| in \| ok \| −0.2 \| 5.3 \| 69 | **timeout** at 1800 s (work_limit), no candidate \| 459 |
| dg_multievent_4 | found \| 5 \| in \| ok \| −0.1 \| 4.2 \| 84 | **timeout** at 1800 s, no candidate \| 527 |
| dg_multievent_6 | found \| 6 \| in \| ok \| −0.2 \| 4.6 \| 116 | **timeout** at 1800 s, no candidate \| 524 |
| livermore_N172EL_R35 | found \| 2618 \| out (314) \| wrong \| +119 \| 1.6 \| 164 | **timeout** at 1800 s \| 619 |
| livermore_N248LF_R14 | found \| 7396 \| in (71) \| ok \| +26.5 \| 12.4 \| 244 | ambiguous \| 6518 \| out (r 462 m) \| ok \| 0.0 \| 889 \| 601 |
| livermore_N4040_R12 | found \| 1562 \| out (71) \| ok \| +7.6 \| 2.3 \| 139 | ambiguous \| 9250 \| - \| wrong \| +132 \| 99 \| 498 |
| livermore_N413AK_R16 | found \| **218** \| in (71) \| ok \| 0.0 \| 2.3 \| 195 | **timeout** at 1800 s \| 652 |
| livermore_N456CS_R38 | found \| 5607 \| in (73) \| ok \| −47.8 \| 38.7 \| 173 | **timeout** at 1800 s \| 608 |
| livermore_N788BM_R03 | found \| **454** \| out (314) \| ok \| −0.7 \| 5.7 \| 160 | ambiguous \| 6556 \| out (r 407 m) \| ok \| −75.4 \| 427 \| 605 |
| livermore_N86LP_R01 | found \| 5363 \| out (71) \| ok \| −54.1 \| 6.0 \| 163 | ambiguous \| 8747 \| - \| wrong \| −107.7 \| 1557 \| 640 |
| livermore_N500MG_R17 | found \| 8579 \| in (314) \| wrong \| +26.6 \| 23.6 \| 164 | ambiguous \| **4788** \| - \| wrong \| +80.1 \| 891 \| 648 |
| livermore_N60RS_R11 | found \| 9817 \| out (314) \| wrong \| +111 \| 2.1 \| 176 | **timeout** at 1800 s \| 593 |
| livermore_N748SP_R36 | found \| 7587 \| out (73) \| wrong \| −79.6 \| 28.1 \| 241 | **timeout** at 1800 s \| 622 |
| oysterbay_20260908_colsto | found \| 6431 \| out (2.5) \| wrong \| −88.1 \| 25.6 \| 861 | **not run**: DG needs its own 2026-09-08 index, a 30+ min build (stopped) |

DG's synthetic regions use an uncertainty radius of 530 m. Some of its Livermore candidates (R12, R01, R17)
carried no radius. The DG Oyster Bay lane exited at once with `failed/data_unavailable` because the index was
missing. That is recorded as not run, not as a DG failure.

## Aggregates

| | n | found | success (found, ≤ 1 km) | median err m | p90 err m | truth in region | median region | top-1 ID | median \|clock\| s | median wall s | max RSS MB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| ours, synthetic | 5 | 5 | 5 | 7 | 10 | 5/5 | 314 km² | 5/5 | 0.2 | 4.2 | 116 |
| ours, real | 11 | 11 | 2 | 5607 | 8579 | 4/11 | 73 km² | 6/11 | 47.8 | 6.0 | 861 |
| DG, synthetic | 5 | 0 | 0 | 2 (2 of 5 located) | – | 2/2 | 0.88 km² (r 530 m) | 2/2 | 0.0 | 1800 (3 timeouts) | 527 |
| DG, Livermore | 10 | 0 (5 ambiguous, 5 timeouts) | 0 | 6556 (5 located) | 9049 | 0/2 | 0.6 km² | 2/5 | 80.1 | 1557 | 652 |

What this shows:

- **Synthetic.** DG was more accurate where it finished: 2 m against our 5–10 m. It returned `inconclusive`
  every time, and with a ±2 min start it hit 1800 s on all three multi-event fixtures without a candidate.
  Ours located all five, with the correct flights, in 4–5 s.
- **Real (Livermore, Oyster Bay).** Ours is poor end to end: median 5.6 km, 2/11 within 1 km. It still reports
  `found`, and sometimes `high` confidence on a wrong answer (R35, R11), so the status is not a usable signal. DG is no better:
  0/10 within 1 km on Livermore, 5 timeouts, and 4.8–9.3 km on the 5 it located (median 6.6 km against our
  5.4 km on Livermore). It does at least label all of them `inconclusive`.
- **Calibration.** Our 90% region held truth on 9/16 cases (4/11 real). On real cases it is either very large
  (71–314 km², most of the search circle) or wrong. That is badly under-covered for a claimed 90%.
- **DG's region.** DG's sensitivity radius is not a probability; it held truth on the 2 cases that had one.

## Diagnostic: our losses on real cases come mostly from the start window

`runs/bench-vs-dg/diag-true-start-5s` reran ours with the **true** start ±5 s, an oracle clock that is not
part of the benchmark (`tools/bench_vs_dg/diag_inputs.py`). Everything else was unchanged:

| ours, real cases | ±2 min (benchmark) | true start ±5 s |
|---|---|---|
| median error | 5607 m | **1313 m** (Livermore only: 5485 → **1112 m**) |
| within 1 km (found) | 2/11 | **5/11** (Livermore 5/10: R14 141 m, R16 218 m, R11 222 m, R03 347 m, R01 910 m) |
| median \|clock error\| | 47.8 s | 0.7 s |
| top-1 ID | 6/11 | 6/11 |
| truth in 90% region | 4/11 | 5/11 |

The narrower start window cuts the real-case median error about 4× and more than doubles the within-1 km
count. A 4-minute window lets other flights explain a single pass well enough to win (the clock lands at the
window edge on R35, +119 s). With ±5 s, 5 cases still miss by 1–11 km. Those are the cases with discovery or
association failures (R35, R38, R17, R36 pick the wrong flight or the mirror), plus Oyster Bay at 8.9 km.

## Matched-settings speed and memory (one case)

Case: **dg_synthetic_known_minute** (its cheapest for DG), 1 repeat per setting, `taskset 0-7`, machine shared
with other sessions (DG accuracy lanes ran on cores 8–13 at the same time). Raw files:
`runs/bench-vs-dg/acc/perf/known_minute/*/rep1/` (`time.txt`, `perf.csv`, `measure.json`, `cmd.json`). Summary:
`perf/perf_summary.md`. Inputs: `perf/perf_inputs.json`.

Matched settings:

| Setting | ours | DOPPLERGEIST | Matched? |
|---|---|---|---|
| Area | circle r 5 km (78.5 km²) | its bounding box, 10 × 10 km (100 km²) | Not identical: DG searches 27% more area |
| Coarse grid | 200 m | `search_budget.spacing_m` 200 | yes |
| Refinement | 200 → 100 → 50 → 25 m quadtree, top 200 cells per level | 200 m → one 25 m refinement around finalists (`refine_spacing_m` 25, top_k 100, 10 finalists) | Finest step matched; the schedules differ structurally |
| Clock window | true start ±5 s | `[start−5, start+5]` | yes |
| Clock step | 0.5 s (`[spec] hop_s = 0.5`; ours steps one spectrogram hop, default 0.25 s) | 0.5 s (`time_step_s`) | Yes. Ours also gets a coarser spectrogram. |
| Clock sweep | ±5 s (`clock_sweep_s`) | ±5 s around the CPA-implied start, clipped to the window | yes |
| Events | automatic discovery (2 used) | automatic discovery (2 groups) | yes |
| ADS-B, terrain, atmosphere | same 7-track population; constant ground; still air (`--no-weather`) | same | yes |
| Config | `tools/bench_vs_dg/perf_matched.toml` | operator `search_budget` in `perf.py` | |

| | wall s | CPU s (user+sys) | peak RSS MB (largest process / process tree) | instructions | cycles | threads (procs) used | error m |
|---|---|---|---|---|---|---|---|
| ours, default (rayon, 8 cores) | **3.63** | 4.53 | 101 / 119 | 66.4 G | 20.9 G | 9 (1) | 52 |
| ours, 1 thread (`RAYON_NUM_THREADS=1`) | 4.94 | 4.47 | 98 / 116 | 66.3 G | 20.9 G | 2 (1) | 52 |
| DG, default (`search_workers` auto = 7) | 22.1 | 16.7 | 329 / 365 | 99.6 G | 78.1 G | 31 (3) | **2** |
| DG, 1 thread (`search_workers` 1, OMP/OPENBLAS/MKL/NUMEXPR = 1) | 20.6 | 16.2 | 326 / 365 | 98.6 G | 76.1 G | 17 (3) | **2** |

Work done, from each tool's own counters:

- **Ours:** 3306 cells scored, 2 tracks, 2 events.
- **DG:** 2514 coarse grid points plus 1369 refinement points (1521 skipped) across 6 tiles; 3658 hypotheses;
  4980 CPA rows; 7278 evaluated, 13185 rejected and 51010 bounded-unknown clock trials; 26155 work units.
  Its search report says `bounded_concurrency 1` even at the default worker count, so it searched serially.

On matched settings, ours used **3.6× less CPU time** (4.5 vs 16.2 s, single thread), **1.5× fewer
instructions** and **3.6× fewer cycles**, and **3.3× less memory**. DG runs at about 1.3 instructions per cycle
against our 3.2. DG also searched 27% more area and about 17% more points (3883 vs 3306), so per point evaluated
DG costs about 3× our CPU time. DG's own search stage took 10–11 s of the 21 s. The rest is decode, discovery,
refinement and decision. DG was more accurate on this case: 2 m against our 52 m at the matched 0.5 s hop. Our
default hop gives 10 m (accuracy run).

Not done because of the time cut: repeats (n = 1, so treat ±20% as noise), the Oyster Bay `reference-local` and
Livermore matched cases, and the pure-Python DG fallback. For reference, functional checks during setup gave
33.4 s wall and 812 MB for DG `run reproduce-local.json`, and 10.4 s and 966 MB for our
`eval tools/bench_vs_dg/perf_ob_parity.toml`, which reproduced its 78.9 m with 4321 cells and 1875
hypotheses. Both ran on 2–4 cores under load, are not part of the matched table, and are not repeated.

## Where DG beats us, and why we think so

1. **Synthetic precision: 2 m vs 5–10 m (52 m at hop 0.5 s).** DG ends with a jointly constrained
   position/clock refinement at 25 m and continuous optimisation of its finalists, and it fits a robust ridge.
   Our answer is a probability-weighted centre of the peak's core cells, so its resolution is the grid. A final
   continuous polish of the top cell would likely close most of the gap.
2. **Not claiming answers it cannot support.** DG returned `inconclusive` on every case. That is unhelpful on
   synthetic, but on real data it never asserts a wrong location. Ours said `found` on all 11 real cases while
   being wrong by more than 1 km on 9, twice with `high` confidence. Our confidence and region do not yet react
   to how many flights fit inside a wide start window.
3. **Real locations: two narrow wins.** DG returned a candidate on 5 of 10 Livermore cases, all 4.8–9.3 km off.
   It beat ours on R17 (4.8 vs 8.6 km, both picked the wrong flight) and on R14 (6.5 vs 7.4 km, both picked
   the right flight). On R12, R03 and R01 ours was closer, by up to 7.7 km. Neither tool is usable on single
   real passes with a ±2 min start. DG's R14 clock error was 0.0 s with the right aircraft and still 6.5 km
   off, so the error is position along a single pass's ambiguity (the mirror/along-track ridge), not the
   association.

## Where we beat DG

- **Multi-event with a ±2 min start.** DG timed out on all three multi-event fixtures at 1800 s. Its clock bounds
  span the window, and its refinement and decision work grows with every clock trial. With a ±5 s window the
  same fixture took 38 s (probe). Ours did all three in 4–5 s at 5–7 m.
- **Data preparation.** DG's offline day index took **25 min** (1517 s wall, 3 workers, 974 MB tree RSS) and
  produced **4.3 GB** of SQLite, with ~37 GB of transient staging. Our ingest of the same day takes 34 s. This is
  why Oyster Bay (2026-09-08) was not run for DG.
- **Wall time per case:** ours took 2–39 s (median 6 s). DG took 84–1800 s, with 8 of 15 runs stopped at
  the 1800 s cap.

## Failures and not-run

- DG, `dg_multievent_{3,4,6}`: `inconclusive/work_limit` at 1800 s, with 6, 8 and 12 discovered event records and
  no candidates. Not re-run. An earlier 900 s smoke run of `dg_multievent_3` (a 10 km box) also timed out.
- DG, Oyster Bay: not run. DG needs a 30+ min index build for 2026-09-08, which was stopped when the scope was
  cut.
- DG, Livermore: 5 of 10 hit the 1800 s cap with no candidate (R35, R16, R38, R11, R36). The other 5 took
  99–1557 s and all returned `inconclusive/search_incomplete`. The ±2 min window is the likely cost driver:
  `dg_multievent_3`, which timed out at ±2 min, took 38 s with ±5 s. Livermore was not probed at ±5 s.
- Ours: no crashes. The first run gave ours the box (`--near w,s,e,n`). Ours turns a box into its
  **circumscribed** circle (631 km²), a usability trap. That run was redone with centre+radius and archived in
  `acc/_ours_box_as_circle/`. The real-case median was 5.6 km in both runs, but single cases moved by up to 6 km (Oyster Bay 12.6 → 6.4 km).

## Reproduce

```bash
tools/bench_vs_dg.sh prep                                   # inputs, synthetic ADS-B/DEMs, DG indexes (slow)
STAMP=acc DG_MAX_RUNTIME=1800 tools/bench_vs_dg.sh run      # ours (8 cores) then DG in 3 lanes
tools/bench_vs_dg.sh score acc
tools/bench_vs_dg.sh perf acc 3 known_minute synth livermore ob && tools/bench_vs_dg.sh perf-summary acc
INPUTS=runs/bench-vs-dg/prep/inputs_diag_true_start_5s.json STAMP=diag-true-start-5s ONLY=ours tools/bench_vs_dg.sh run
```
