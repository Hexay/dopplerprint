# Calibrated output (stage 3), 2026-11-03

Stage 3 of `matching_mathematical_methods.md`: area-aware HPD, basin modes, T fitted by log score, split-conformal
q̂ per stratum. Fitted values ship in `crates/dp-core/src/search/calibration.toml`.
Reproduce: `python3 -I tools/gen_sim_calib.py`, the eval runs below, then

```
dopplerprint calibrate --calib runs/c_liv_dev --calib runs/c_oyster --calib runs/c_sim_cal [...] \
  --t-case livermore --t-case oyster --test runs/c_liv_test --test runs/c_sim_test \
  --write crates/dp-core/src/search/calibration.toml --out runs/calibration.md
```

## Method

- Posterior over the refined cell set (`search/surface.rs`). Leaves are the finest scored cells, and each one
  carries its own area. p(x) = (1 − ε)·exp(s/T)/Z + ε/A with ε = 0.01, A the region, and unscored ground at ε/A.
  `hpd_level(density, area, mass)` is area-weighted. `&[]` means equal areas, which is what the dp-py grid
  uses.
- Modes (`search/modes.rs`): ToMATo basins on leaf adjacency, merged when peak − saddle < 2 nats (score/T).
  Each mode has a peak, a mass, a centroid, and its share and area of the reported region.
- Log score = ln p(truth) in ln km⁻². HPD-split score = the mass at density ≥ p(truth). q̂ = the
  ⌈(n+1)·level⌉-th smallest score in the stratum. A stratum uses real cases alone when it has ≥ 9 of them,
  and real+sim otherwise.
- T is a golden-section fit of the mean log score over ln T ∈ [ln 0.2, ln 50]. The fit re-tempers the cached
  surfaces (`CaseResult.surface`) and never re-runs the search.

## Data

| set | runs | n | strata |
|---|---|---|---|
| calibration, real | `livermore_baseline.toml` (dev split), Oyster Bay parity | 28 + 1 | 1 pass: 28; 2 passes: 1 |
| calibration, sim | `calib_sim.toml` on `experiments/sim_calib/cal_*` | 60 | 20 per stratum |
| test, real | `livermore_test.toml` (frozen split, run once) | 12 | 1 pass |
| test, sim | `calib_sim.toml` on `sim_calib/test_*` | 36 | 12 per stratum |

Each sim recording is rendered in its own wrong atmosphere (c 337–349 m/s, wind ≤ 6 m/s). The model keeps
c = 343 and no wind. All passes are straight.

## Temperature

Mean log score at truth (ln km⁻²) against T:

| fit cases | 0.5 | 1 | 2 | 3 | 5 | 8 | fitted T |
|---|---|---|---|---|---|---|---|
| real (Livermore dev + Oyster Bay, 29) | −0.33 | 0.09 | 0.17 | 0.03 | −0.24 | −0.50 | **1.58** |
| sim cal (60), not used | 3.75 | 3.53 | 3.07 | 2.72 | 2.21 | 1.77 | 0.44 |

**T = 1.58 replaces the hand-set 3.0** (`likelihood::default_temperature()` reads it). The change is material:
test log score rises by +0.39 to +0.78 nats per stratum (table below). Sim wants a T 3.6× sharper than real,
so sim's model error understates reality's. T is therefore fitted on real cases only.

## q̂ per stratum (at T = 1.58)

| stratum | n | source | q̂50 | q̂80 | q̂90 | 5% floor of delivered 90% coverage (Beta) |
|---|---|---|---|---|---|---|
| 1 pass | 28 | real | 0.269 | 0.792 | **0.988** | 0.84 |
| 2 passes | 21 | real+sim (1 real) | 0.178 | 0.380 | 0.572 | 0.79 |
| ≥ 3 passes | 20 | sim | 0.188 | 0.303 | 0.522 | 0.78 |

On a single pass the 90% region has to hold 98.8% of the mass. Livermore's misses are along-track or
wrong-flight solutions 1.5–5 km away that carry almost no mass at any T. On multi-pass sim the raw posterior
over-covers, so q̂ < nominal and the region shrinks.

## Test (held out, once): coverage, k/n [95% Clopper–Pearson]

| stratum | nominal | raw at T = 3 (before) | raw at T = 1.58 | conformal q̂ at T = 1.58 | median area km² |
|---|---|---|---|---|---|
| 1 pass, real (12) | 0.5 | 0.83 [0.52, 0.98] | 0.58 | 0.50 [0.21, 0.79] | 0.08 |
| | 0.8 | 0.92 [0.62, 1.00] | 0.92 | 0.92 [0.62, 1.00] | 0.53 |
| | 0.9 | 0.92 [0.62, 1.00] | 0.92 | **0.92 [0.62, 1.00]** | 2.56 |
| 1 pass, sim (12) | 0.5 / 0.8 / 0.9 | 0.92 / 1.00 / 1.00 | 0.75 / 0.92 / 1.00 | 0.42 / 0.92 / 1.00 | 0.08 / 0.31 / 1.44 |
| 2 passes, sim (12) | 0.5 / 0.8 / 0.9 | 1.00 / 1.00 / 1.00 | 1.00 / 1.00 / 1.00 | 0.67 / 0.92 / 1.00 | 0.01 / 0.01 / 0.02 |
| ≥ 3 passes, sim (12) | 0.5 / 0.8 / 0.9 | 1.00 / 1.00 / 1.00 | 1.00 / 1.00 / 1.00 | 0.58 / 0.83 / 1.00 | 0.00 / 0.00 / 0.00 |

At n = 12 per cell, every conformal coverage CI contains its nominal level, but each spans about ±0.3. The
stage-3 target was sim coverage within ±0.05 of nominal, and these n cannot confirm or refute it. Before
calibration, multi-pass regions over-covered at every level (1.00 at a nominal 50%).

Log score at truth (mean [bootstrap 95%], ln km⁻²):

| stratum | n | before (T = 3) | after (T = 1.58) | paired Δ |
|---|---|---|---|---|
| 1 pass (real + sim) | 24 | 0.18 [−0.71, 0.82] | 0.57 [−0.35, 1.23] | +0.39 [0.29, 0.50] |
| 1 pass, real only | 12 | −0.42 [−2.03, 0.66] | 0.02 [−1.68, 1.14] | +0.44 [0.26, 0.62] |
| 2 passes | 12 | 2.92 [2.64, 3.22] | 3.54 [3.21, 3.88] | +0.62 [0.56, 0.68] |
| ≥ 3 passes | 12 | 4.26 [3.88, 4.62] | 5.04 [4.67, 5.38] | +0.78 [0.71, 0.88] |

## Mode choice: by peak vs by mass (T = 1.58)

| set | n | > 1 mode | picks differ | truth in mode: mass | truth in mode: peak | mode log loss: mass | mode log loss: peak |
|---|---|---|---|---|---|---|---|
| calibration | 89 | 33 | 11 | 0.83 [0.74, 0.90] | 0.85 [0.76, 0.92] | 0.27 [0.16, 0.41] | 0.33 [0.19, 0.52] |
| test, all | 48 | 14 | 2 | 0.92 [0.80, 0.98] | 0.88 [0.75, 0.95] | 0.15 [0.08, 0.23] | 0.19 [0.10, 0.29] |
| test, real | 12 | 6 | 1 | 11/12 | 10/12 | 0.15 | 0.29 |

Mass has the lower log loss in every set, but the CIs overlap. Accuracy splits the other way between sets
(in-sample, peak wins 76 vs 74). **Not adopted.** Ranking, `best` and the reported location stay peak-based.
The report gives mode masses beside the peak. Adopting mass needs a paired test on more multi-mode cases (25
discordant so far would be the minimum).

## Mirror sides on single passes

A straight single pass carries no information about which side of its track the microphone was on
(`research_notes/.../wind_shear_raytrace.md`). Pairs below are the heaviest mode and its heaviest
same-flight rival ≥ 300 m away, counted when the truth lies in one of them.

| set | pairs | median heavier share | heavier side is true |
|---|---|---|---|
| sim cal, straight (T = 3) | 11 | 0.52 | 3/11 |
| sim test, straight | 8 | 0.51 | 5/8 |
| Livermore dev (T = 3) | 7 | 0.86 | 6/7 |
| Livermore test | 5 | 0.80 | 5/5 |

Sim behaves as theory says: about 50/50, and the side is a coin toss. The fitted T leaves those splits near
0.5. It does not make them lopsided. Livermore splits are lopsided and mostly right (11/12). The likely cause
is curvature in pattern-traffic tracks, but this is unverified. A turn decides the side only if the decision
survives a ~2% error in c, and that check is not done here (it needs `nuisance_c` runs). The locate report
therefore never calls the side decided when a single aircraft was heard (`side_resolved = false`). It always
lists the mirror and gives its mass.

## Locate report

The calibrated region replaces the heuristic confidence line in the text report. The test supports this: real
single-pass 90% coverage was 0.92 [0.62, 1.00], with a Beta 5% floor of 0.84 at n = 28. The report shows the
90% region (area, islands, and whether q̂ comes from real or sim), each mode's probability, and the mirror
caveat. `result.gpkg`/`geojson` now carry `in_cr90` cells and per-mode `credible90` outlines (convex hulls; the
cell set is the region). The JSON keeps `confidence` (heuristic) beside the new `posterior`.

## Not done / caveats

- Multi-pass q̂ comes from sim: one real 2-pass case, no real ≥ 3-pass case. All sim passes are straight
  GA flyovers in a uniform medium. The report labels these regions "calibrated on sim recordings".
- Livermore's single-pass q̂90 = 0.988 is near the 1.0 ceiling. Losing the truth at refinement (5/28 dev)
  shows up as HPD scores near 1, so the region is the posterior's whole support. Conformal scoring cannot repair
  search misses.
- No GPC. The 50% level under-covers slightly on test (0.42–0.67) and over-covers in-sample (0.40–0.50). Shape
  miscalibration that depends on level is possible but not resolvable at this n.
- The c ±2% robustness check on side decisions, and prediction-powered rectification of sim→real coverage,
  are open.
