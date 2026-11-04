# Region-free (CONUS) search: proposals from the aircraft side

`dp_core::search::global` locates a recording given only a start-time window. It inverts each event
from the aircraft side, proposes small regions, and hands the top K to the existing regional search.
Code: `crates/dp-core/src/search/global/`. Eval hook: `[eval] region_mode = "global"`
(`crates/dp-core/src/eval/global.rs`). Experiments: `experiments/global/*.toml`.

**Verdict.** It works when the recording has **two or more prop events with fitted d/v bounds**.
two_props_920s injected into real 2026-09-08 CONUS traffic ranks truth #1 of 56 proposals (±5 s)
and #1 of 407 (±60 s), with a 1 m final error and a 22 z margin. It does **not** work for single
events: truth ranks #19–#969 and the answer is >1000 km off, though both are correctly flagged
ambiguous. It also fails on Oyster Bay. The CV refit gave unusable d/v bounds there, and the oracle
bounds (300–8000 m, 25–170 m/s) are too loose: 2776 two-event coincidences, truth #122, answer
537 km off. That answer is flagged ambiguous (margin 0.24 z).

## Method

1. **Window tracks.** One store query per merged span (each event's emission window plus its scored
   audio window), with no region, inside a CONUS bbox (the store is worldwide: an un-boxed run proposed
   Cologne and Geneva), and capped at `d_max + ground_max` altitude. Jets are included.
2. **Footprint votes** (`footprint.rs`). For each (event, track) pair, the aircraft is sampled every
   ≤ 0.7 cell of track over the emission window t_emit ∈ [t_cpa + clk₀ − d_max/c, t_cpa + clk₁ − d_min/c].
   Ground speed must be in v_range. For both mirror sides and every horizontal offset
   L ∈ [√(d_min² − H_max²), √(d_max² − H_min²)], one vote goes to an EPSG:5070 cell (500 m) and a clock
   bin (4 s), with clock = t_emit + d/c − t_cpa. H spans an assumed ground height of 0–800 m MSL, since
   there is no national DEM. The weight is a log-normal d/v plausibility centred on the fit.
3. **Agreement** (`cluster.rs`). Per cell, find the clock window (±10 s) that maximises
   (events explained, Σ weight). Keep cells at n_max (n_max − 1 when n_max ≥ 3). Group them into
   8-connected patches with consistent clocks, then cover each patch greedily with regions of radius
   ≤ 5 km. This is Hough voting over (cell, clock bin), as in
   `docs/research/matching_mathematical_methods.md` §"Closed-form loci and clock-free voting" (item 3
   there). The closed-form 3×3 WLS loci and the EDT clock-free score from that section are not
   implemented.
4. **Spectrogram rerank** (`rerank.rs`). Score each proposal's supporting flights with one
   `score_joint` from a 3×3 grid (±300 m) around its centre. Sort by (events explained, z).
5. **Regional search** on the top K = 10. Each region's clock prior is narrowed to the proposal's
   clock range ± 10 s, and its tracks come from a bbox query. Regions are merged by best score.
   `ambiguous` = n_max < 2, or the best distinct (> 2 km) rival is within 3 z.

Sim cases put the scenario tracks on the last store day at the same UTC time of day (explicit
63 072 000 s shift for 2024-09-08 → 2026-09-08), so the competing traffic is real. Sim ground height
is the truth height, standing in for a national DEM. Oyster Bay uses its regional DEM where it covers
a cell, and geoid + 400 m elsewhere.

## Results

Release build, RAYON_NUM_THREADS=4, shared devbox (load ~30). "prop#" = rank of the first proposal
whose region contains the truth. "region#" = rank after the regional search (top 10 only).

| case | prior | events (n_max) | bounds | prop# / n | region# | error | margin z | ambiguous | wall s | peak RSS GB |
|---|---|---|---|---|---|---|---|---|---|---|
| sim two_props_920s | ±5 s | 2 (2) | sim ±60% d | **1** / 56 | 1 | **1 m** | 22.3 | no | 144 | 0.26 |
| sim two_props_920s | ±60 s | 2 (2) | sim | **1** / 407 | 1 | **1 m** | 21.6 | no | 272 | 0.30 |
| sim faint_single | ±5 s | 1 (1) | sim | 969 / 2156 | – | 2183 km | 0.04 | **yes** | 181 | 0.22 |
| sim manoeuvring_turn | ±5 s | 1 (1) | sim | 19 / 575 | – | 1838 km | 0.30 | **yes** | 141 | 0.20 |
| Oyster Bay | ±5 s | 2 (2) | oracle 300–8000 m | 122 / 2776 | – | 537 km | 0.24 | **yes** | 1323 | ~1.0 |

Geometry-only ranking (the run before the rerank and the CONUS bbox; keep = 500):

| case | prior | prop# / n | error | note |
|---|---|---|---|---|
| sim two_props | ±5 s | 1 / 71 | 1 m | 120 s |
| sim two_props | ±60 s | 2 / 499 | 1 m | 490 s; the regional search still put truth first |
| sim faint_single | ±5 s | **150** / 2940 | 5825 km | the rerank made single-event ranking *worse* (→ 969) |
| sim manoeuvring_turn | ±5 s | **11** / 860 | 1768 km | rerank → 19 |
| Oyster Bay, oracle bounds | ±5 s | > 500 / 3188 | 607 km | not flagged ambiguous (margin 22.8): a false 2-event match in Ontario won |
| Oyster Bay, CV-refit bounds | ±5 s | 272 / 6996 | none | refit bounds unusable: event 2 d 10–42 m, v 11–16 m/s; event 1 d 3.2–13 km (truth ≈ 6 km); n_max 1 |

### Cost (stage seconds)

| case | window tracks | votes | load | propose | rerank | regional (K=10) |
|---|---|---|---|---|---|---|
| two_props ±5 s | 7251 | 93 k | 9.4 | 0.09 | 25 | 107 |
| two_props ±60 s | 8162 | 1.0 M | 2.0 | 0.10 | 177 | 92 |
| faint ±5 s (60 s audio) | 4292 | 73 k | 1.1 | 0.02 | 130 | 50 |
| Oyster Bay ±5 s (1 h audio, oracle d) | 9250 | 931 k | 8.1 | 0.27 | **1269** | 38 |

- The proposal stage itself is cheap: < 0.3 s and < 0.3 GB at ±60 s. Votes scale ~linearly with the
  prior width (93 k → 1.0 M from ±5 s to ±60 s) and with the d-range width (oracle d: 931 k at ±5 s).
  `vote_cell_m` coarsens the lattice so one footprint stays ≈ 2000 cells, which caps memory at wide
  priors.
- Window-track loading is 1–16 s (the store read dominates; it varies with cache state). Never more
  than ~9 k CONUS tracks.
- The **spectrogram rerank dominates**. One `score_joint` costs ~0.2 s CPU on an hour of audio with a
  ±10–20 s clock sweep. It is now capped by `rerank_max` (1000). The regional search costs 4–11 s per
  region.
- ±600 s was not run (scope cut). Expect ~10× the votes of ±60 s (~10 M, still < 1 GB). The number of
  2-event coincidences grows roughly linearly with prior width, so rerank cost and rank both degrade.

### Oyster Bay with truth-centred bounds: what limits real data

`[eval] global_truth_bounds = true` gives real cases `pose_sim`'s bound widths (d ×0.6–1.6, v ×0.75–1.3)
around each truth flight's CPA from its ADS-B track: a ceiling on what a good event fit would buy.
Oyster Bay CPAs: a1dfeb 6.2 km at 137 m/s, ace391 3.7 km at 102 m/s. All rows ±5 s,
`experiments/global/oysterbay_truthbounds*_t5.toml`.

| silence | prop# / n | region# | answer | error | flights | margin z | truth raw / score | winner raw / score |
|---|---|---|---|---|---|---|---|---|
| flat 3 z (default) | **1** / 180 | 2 | Purcellville VA | 414 km | wrong | 4.05 | 114.0 / 78.0 (12 silent) | 85.0 / 82.0 (1 silent) |
| off | 1 / 180 | 1 | Oyster Bay | **61 m** | ✓ | 23.85 | 114.5 | — |
| likelihood, r50 2 km | 1 / 180 | 1 | Oyster Bay | **79 m** | ✓ | 12.31 | 114.0 / 99.5 | Denver 90.6 / 87.2 |

- Good bounds alone fix the proposal stage (#122 → #1, 2776 → 180 coincidences, 22 min → 3 min).
- The **flat silence penalty is unfair between regions**: NYC airspace has 12 audible-range flights,
  rural Virginia 1, so a 29-point worse Doppler fit wins. Locally every cell pays about the same.
  The likelihood term (`docs/research/unexplained_silence_2026-11-03.md`) charges by expected loudness
  against the recording's noise floor and keeps truth first.
- `sim_lsil_t5.toml` (likelihood silence on the sim suite) reproduces the flat-silence sim rows
  (two_props #1, 1 m, margin 22.3; singles ambiguous).
- Fixed: the global eval path never called `SilenceParams::measure_recording`, so likelihood silence
  ran without the high-pass corner.

### Oyster Bay with fitted bounds: solved (κ bounds, top_k 30)

Current discovery fits both events sensibly (the d 10–42 m event-2 fit above came from older
discovery). The fit now also bounds the CPA curvature κ = v²/d (`CvFit::kappa_range`: covariance +
window spread, floor ±0.3 ln), and the vote uses `EventSpec::d_range_at(ground speed)` = d_range ∩
v²/κ per candidate flight. Likelihood silence is the default. `oysterbay_t5.toml` (±5 s):

| build | top_k | prop# / n | answer | error | margin z | rerank s | wall s |
|---|---|---|---|---|---|---|---|
| no κ | 10 | 17 / 692 | Denver (wrong) | 2585 km | 6.1 | 306 | 428 |
| κ | 10 | 19 / 390 | Denver (wrong) | 2585 km | 39.4 | 146 | 238 |
| no κ | 30 | 17 / 692 | Oyster Bay ✓ | 79 m | 20.9 | — | 466 |
| **κ (default)** | **30** | 19 / 390 | **Oyster Bay ✓** | **79 m** | **21.6** | — | **329** |

- Fit bounds used: event 1 d 1.5–30 km, v 0.5–343 m/s, κ 0.46–20 (point fit good: v 143, d 6.7 km,
  κ 3.06 vs truth 137, 6.2 km, 3.02; sub-window refits disagree, so window spread blows every range).
  Event 2 d 1.1–4.4 km, v 74–111, κ 2.89–5.27 (truth κ 2.87, just outside, by ground vs 3-D speed).
- The **rerank is the bottleneck**, not the bounds: truth was #17–19, regional score 114 vs 78 for the
  best rival. top_k 10 → 30 is the fix; κ halves proposals and rerank time. Confident wrong answers at
  K 10 (margin 39 z) show the margin is only meaningful over regions actually searched.
- Sim suite at top_k 30 (`sim_t5.toml`): two_props #1, 1 m, margin 22.3 (246 s, was 59 s);
  manoeuvring_turn now 181 m, correct flight, still ambiguous (margin 1.6; 538 s, was 178 s);
  faint_single ambiguous. Cost ≈ 3× on the regional stage.

### Geometry-gated rerank: truth #1, top_k back to 10

`rerank.rs` now scores each proposal at its best voted cells (`rerank_cells` = 6) instead of centre
±300 m. At each cell it picks, per event, the supporting flight (≤ 3) whose CPA from that cell best fits
`d_range_at(gs)`, `v_range` and the CPA time (slack ×1.25 d/v, +5 s); a cell with no consistent flight
is skipped. One `score_joint` per cell, as before. Fitted bounds, ±5 s:

| case | prop# before → after | top_k | answer | margin z | rerank s | wall s |
|---|---|---|---|---|---|---|
| Oyster Bay | 19 / 390 → **1 / 390** | 10 | 79 m ✓ | 19.95 | 146 → **34** | 329 (K 30) → **289** |
| two_props | 1 / 56 → 1 / 56 | 10 | 1 m ✓ | 22.3 | 6 | 246 (K 30) → 101 |
| manoeuvring_turn | 19 → 32 / 575 | 10 | ambiguous | 0.30 | 49 | 178 |
| faint_single | 969 → 91 / 2156 | 10 | ambiguous | 0.40 | 55 | 141 |

Truth's quick score is 101.7 against 87.1 for the best rival (was 71.5, scored 1.5 km off truth, below
rivals' 76–98). The gate is what stops wrong pairs fitting noise with an unchecked flight. Singles
move both ways and stay ambiguous; only their candidate-list order changes.

### Window spread net of sub-fit σ: event 1 bounds honest

`events/window.rs` counted every sub-window disagreement as model error. Event 1's ridge (2124–2241 s,
CPA 2156 s) loses its approach side when the leading 30% is dropped; that refit is ill-conditioned and
its wander blew every range to the LM clamp. Now only |Δ| beyond `sigma_k` × the sub-fit's own σ counts.

| | event 1 d | v | κ | global prop# / n | wall s |
|---|---|---|---|---|---|
| before | 1.5–30 km | 0.5–343 | 0.46–20 | 1 / 390 | 289 |
| after | **3.3–13.3 km** | **114–171** | **2.27–4.13** | 4 / 37 | **143** |

Truth 6.2 km, 137 m/s, κ 3.02: all inside. Event 2 unchanged. Same answer (79 m, margin 21.6 z).
Regional discovered path, paired vs the `m3_*` runs (main 447dbed): Livermore dev (28) reported error
mean −167 m [−479, +78], flights +1/−0, calibrated log score +0.16 [−0.11, 0.51], in-90% flips +2/−2;
Oyster Bay discovered 826 → 668 m, log score +1.22. No calibration refit: the shift is inside noise
and goes the right way; the κ-gate step refits anyway. Sims use oracle bounds (unaffected).

### κ in the regional association gate: negative

`soft.rs` admitting on `d_range_at(ground speed)` (pure κ range when the intersection is empty),
paired vs the step-2 runs: Livermore dev reported error mean +92 m [−26, +301], 9 better / 16 worse,
flights +0/−1, calibrated log score +0.01; Oyster Bay discovered unchanged; global Oyster Bay
unchanged (prop #4, margin 20.8). Not shipped, so no calibration refit. κ stays a footprint and
rerank bound only. The regional gate already sees the flight's actual speed and slant range, so
the narrower d adds little information and cuts correct pairs whose fitted κ is biased.

### f0 drift as a consider parameter in the event fit

`events/cvlm.rs::covariance` adds S·σ_β²·Sᵀ, S = dθ/dβ for a linear source drift left at 0 in the fit
(fitting β is ill-conditioned on short spans). `drift_sigma_per_s` default 5e-5 (0.3 %/min).
`drift_bias` harness (`MEASURE_SUBSET=1`, 50 m/s): at 5 km and ±1 %/min, v −20 / +29 % was outside
its bounds; now v and d both inside at 1.5 and 5 km. Livermore dev discovered: reported error mean
+0.3 m [−0.07, +0.9], no flips; Oyster Bay discovered and global unchanged. σ_β 1e-4 also fixed the
harness but cost Livermore +17 m [+0.2, +50] and one calibrated-90 % flip, so the lower prior ships.

### f0 drift in the scorer: opt-in, off by default

`search/drift.rs`: each `ScoreParams::drift_grid` β adds bpo·log2(1 + β·(τ − τ_ref)) to an event's
offsets; the sweep keeps the best β less a Gaussian prior (`drift_sigma_per_s`, z = 1.58·½(β/σ)²).
CPU only: with a grid set, `Scorer::score` skips the GPU (TODO in backend.rs; port `sweep.cu`, then
`gpu_parity.sh`). Grid ±0.5/±1 %/min, σ 1e-4 unless noted.

| test | result |
|---|---|
| single pass, `drift_bias` (MEASURE_LOCATE), no prior | drifting cross-track 686–895 → 8–38 m, but steady 1.5 km −23 → +2709 m |
| single pass, σ 1e-4 / 2e-4 | steady fixed / not; drifting mostly unchanged, one case +2.5 km either way |
| `sim_drift` pairs (12, `simdrift_*.toml`) | error −11.5 m [−19.8, −4.7] (σ 2e-4: −21.5 [−35.7, −7.8]); flights unchanged |
| Livermore dev discovered | error +5 m [−7, +20], calibrated log score −0.07 [−0.14, −0.01], one calibrated-90 % flip lost |
| Oyster Bay discovered / global | +30 m, log −0.21 / 79 → 66 m, prop #4 → #2, wall 85 → 356 s |

Single passes can't separate β from geometry (their clock/along-track ridge absorbs it); pairs can.
On real data the max over β inflates every hypothesis's z, which the calibration (fit without drift)
reads as overconfidence. Turning it on needs the CUDA port (runtime) and a calibration refit on drift-on
runs; `search_score::true_observer_wins_and_recovers_f0_and_clock` also fails with the grid on (f0 reported
at τ_ref, not CPA).

### National DEM (Terrarium 1 km)

`data/dem/5070/conus_1000m` (docs/GEO.md) replaces the 0–800 m MSL band. Each footprint vote uses
the ground at its own point, the sweep is bounded by nadir ground ± `terrain_relief_m` 500, and
regions outside the case DEM get national ground instead of geoid + 400 m.

| | prop# / n | error | margin | runner-up | wall s |
|---|---|---|---|---|---|
| band 0–800 m | 4 / 37 | 79 m | 20.8 | – | 143 |
| national DEM | 4 / 36 | 79 m | 17.5 | 40.16 −104.39 (Denver, 82.0) | 103 |

Same answer, flights and clock. The lower margin is from the Denver rival being scored on its real
~1.6 km ground instead of 400 m: a more honest number, not a loss. Sims skip the national DEM
(rendered over flat truth ground); sim_t5 unchanged (two_props #1 at 1 m, margin 22.3).

## What works, what doesn't

- **Works:** ≥ 2 events with honest d/v bounds. Clock agreement across events 920 s apart is very
  selective: 56–407 CONUS-wide coincidences, and the true pair ranks first both by geometry and after
  the regional search.
- **Single events are hopeless nationally.** One pass fixes only f0, v/c, d and t_cpa, so every
  compatible flight gives two mirror ribbons: 575–2940 proposals. The quick spectral score can't
  separate them, because the curve shape is the same for any flight with the same v and d.
  Report "ambiguous" with the candidate list. Don't pick.
- **Bounds quality is everything.** Oracle d ∈ [300, 8000] creates thousands of chance two-event
  matches. The CV refit on Oyster Bay latched onto the wrong ridge for event 2. With real bounds (sim
  ±60% d, ±25% v) truth is #1. Fit d is also biased by f0 drift (+6–51% at 1 %/min, same methods doc).
  Loci should therefore be seeds, refit with the candidate flight's ADS-B speed.
- **Negative: the first rerank** (centre ±300 m, first supporting flight, no geometry gate). It hurt
  single events (150 → 969, 11 → 19) and left Oyster Bay truth at #122. Replaced by the gated rerank
  above.
- **Silence term not used in proposals.** The regional search applies it per region. A national
  silence pre-filter would need tracks over the whole recording, not just event windows (1 h for
  Oyster Bay). Not attempted.

## Recommended `dopplerprint locate` without `--near`

1. Require a start window. Run `search::global::locate_global` with the discovered events.
2. If n_max ≥ 2 and not `ambiguous`: report the best region's answer with its margin and the
   supporting flights.
3. If n_max = 1 (single event) or `ambiguous`: don't report a point. Say: "Only one Doppler event
   (or no consistent pair) was found, and N flights across CONUS fit it. Give `--near` (a city or
   state) or a tighter start time, or record longer to catch a second aircraft." List the top 10
   proposals (flight ids, places, quick scores) as candidates.
4. Done: likelihood silence default; κ bounds; geometry-gated rerank (top_k 10); window spread net of
   sub-fit σ; national 1 km DEM; hypothesis cap flagged (`Candidate::capped`, report warning, eval
   `capped_cells`: 0 of 99k scored cells in Livermore dev + Oyster Bay, so 64 is not binding); f0 drift
   consider covariance in the event fit. Already present, nothing to port: scorer charges uncovered
   window frames (`job.rs` norm over all frames); harmonic / same-source fold (`events/merge.rs`). Tried
   and dropped: κ in the regional gate. `tools/global_eval.sh
   EXPERIMENT` runs one global eval and prints ranks and bounds. Next: closed-form loci + EDT
   clock-free scoring (methods doc) instead of the lattice; weather for the top_k regions.
5. Shipped in the app, with a nationwide decoy check and a two-pass rule. On discovered events Oyster Bay is
   not solved (truth region z 60 vs 99.5 oracle; chance pairs reach 71): see nationwide_app_2026-11-04.md.
