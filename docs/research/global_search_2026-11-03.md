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
- **Negative: the rerank as built.** It helps nothing on the 2-event sims (already #1). It hurts
  single events (150 → 969, 11 → 19), and on Oyster Bay it only lifted truth from > 500 to #122 at
  20 min CPU. Scoring at a 500 m-coarse centre with only the first supporting flight is too blunt.
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
4. Next steps, in order: closed-form loci + EDT clock-free scoring (methods doc) instead of the
   lattice; refit d per candidate flight with its ADS-B speed; a coarse national DEM (≤ 1 km) for H;
   rerank with all supporting flights at the vote-cell resolution, or skip it when n_max ≥ 2.
