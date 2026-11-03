# Calibration on discovered events (2026-11-03)

This supersedes the fitted values of `calibration_2026-11-03.md`; the method (area-aware HPD, T by log score,
split-conformal q̂ per stratum) is unchanged and described there. That fit used oracle (truth-label) events;
`locate` searches discovered events, whose posteriors were overconfident: Livermore discovered 90% coverage at
the shipped calibration was 31/39 answered (0.79), with confident wrong answers (N231DT_R57, N2933A_R08,
N451EE_R45, N748SP_R36) at log score −3.5 to −8.5.

Shipped `crates/dp-core/src/search/calibration.toml`: **T = 2.17, ε = 0.28** (was 1.58, 0.01), q̂ below.
Full report: `runs/calibration_discovered.md` (gitignored; regenerate as below).

## What was fitted on what

| role | data | events | cases (answered) |
|---|---|---|---|
| T, ε (log score) | Livermore dev split (`livermore_dev_discovered.toml`) + Oyster Bay (`oysterbay_discovered.toml`) | discovered | 28 + 1 |
| q̂ | the same + sim cal bank (`calib_sim_discovered.toml`, `sim_cal_*`) | discovered | 29 real + 57 sim (3 unanswered) |
| held out | Livermore test split (`livermore_test_discovered.toml`), sim test bank (`sim_test_*`), `sim_chain_phone_livermore_like` (`simatm_discovered.toml`) | both | 11 (1 unanswered) / 32 (4) / 17 (1) |

- Sim bank: `experiments/sim_calib` (1/2/3 passes, each in its own wrong atmosphere) passed through the
  `phone_livermore_like` recording chain: `python3 -I tools/gen_sim_chain.py phone_livermore_like --src
  experiments/sim_calib --out experiments/sim_calib_chain_phone_livermore_like` (committed).
- Unanswered cases (nothing searched or no candidate) get no region from `locate`, so they are not
  calibration cases; the report counts them and gives coverage with them as misses.
- Strata count the passes actually searched (discovered events after selection), as `locate` does.
- ε is now fitted with T (coordinate ascent, `--fit-floor`). Persistence (2 nats) only shapes modes; it does
  not enter log score or coverage and was left alone.

## Decoupling the soft gates from T

`SoftGates.z_per_nat` used to read `calibration.toml`'s T, so refitting T rescaled every association gate
cost. It is now the constant `GATE_Z_PER_NAT = 1.58` (`search/hypothesis/soft.rs`, rationale in its module
doc). Checks: decoupled vs before (same T) is identical on Livermore dev/test in both modes (`compare`: every
metric 0 / 0 / n ties). After shipping T = 2.17, errors, flights, truth rank and stage recall are again
identical to the T = 1.58 runs (`compare runs/m2_* runs/cal_*`); only calibrated quantities move.

## Fit

| T | 0.5 | 1 | 2 | 3 | 5 | 8 | 12 | 20 |
|---|---|---|---|---|---|---|---|---|
| mean log score, real fit cases (ε = 0.28) | −1.99 | −1.75 | −1.67 | −1.70 | −1.86 | −2.07 | −2.28 | −2.55 |

| ε | 0.001 | 0.003 | 0.01 | 0.03 | 0.1 | 0.2 | 0.3 |
|---|---|---|---|---|---|---|---|
| mean log score (T = 2.17) | −2.59 | −2.40 | −2.18 | −1.98 | −1.77 | −1.68 | −1.67 |

ε ≈ 0.28 matches the share of discovered real cases whose truth sits where the posterior has essentially no
mass (9 of 28 dev cases have HPD score ≥ 0.988 at the old fit). At ε fixed to 0.01 the best T is 3.29; at 0.03, 3.04; at 0.1, 2.74.

| stratum | n | source | q̂50 | q̂80 | q̂90 | before (oracle fit) |
|---|---|---|---|---|---|---|
| 1 pass | 26 | real | 0.379 | 0.740 | 0.783 | 0.269 / 0.792 / 0.988 |
| 2 passes | 19 | real+sim (3 real, all misses) | 0.174 | 0.380 | 1.000 | 0.178 / 0.380 / 0.572 |
| ≥ 3 passes | 12 | sim | 0.192 | 0.485 | 0.722 | 0.188 / 0.303 / 0.522 |

## Results (before = shipped oracle fit T 1.58, ε 0.01; after = this fit)

Coverage k/n of answered cases; area = median km² of the region; log score = mean ln km⁻² at truth, paired Δ
with bootstrap 95% CI. Livermore 40 = dev (in-sample for the fit) + test.

| set | events | 50% before → after | 80% | 90% | 90% area | log score before → after (Δ) |
|---|---|---|---|---|---|---|
| Livermore test (held out) | discovered | 3/11 → 5/11 | 8/11 → 11/11 | 11/11 → 11/11 | 1.58 → 11.32 | 0.42 → 0.12 (−0.31 [−0.48, −0.11]) |
| Livermore 40 | discovered | 11/39 → 19/39 | 24/39 → 33/39 | **31/39 → 37/39** | 1.58 → 11.44 | −1.43 → −1.06 (+0.37 [−0.05, 0.84]) |
| Livermore 40 | oracle | 20/40 → 29/40 | 34/40 → 38/40 | 37/40 → 38/40 | 2.00 → 50.24 | 0.14 → −0.09 (−0.23 [−0.47, 0.11]) |
| sim test bank (held out) | discovered | 14/32 → 25/32 | 27/32 → 30/32 | 29/32 → 31/32 | 0.24 → 50.14 | 2.01 → 1.68 (−0.33 [−0.54, −0.04]) |
| sim test bank | oracle | 16/36 → 27/36 | 27/36 → 36/36 | 34/36 → 36/36 | 0.03 → 50.28 | 2.86 → 2.31 (−0.54 [−0.59, −0.49]) |
| sim_chain phone_livermore_like | discovered | 6/17 → 10/17 | 14/17 → 17/17 | 17/17 → 17/17 | 3.26 → 14.76 | 1.06 → 0.56 (−0.49 [−0.55, −0.43]) |
| sim_chain phone_livermore_like | oracle | 2/18 → 11/18 | 14/18 → 18/18 | 18/18 → 18/18 | 1.08 → 50.30 | 1.59 → 1.11 (−0.48 [−0.55, −0.40]) |
| Oyster Bay | discovered (1 pass, 2.8 km off) | 0/1 → 0/1 | 0/1 → 0/1 | 0/1 → 1/1 | 12.5 → 22.4 | −8.75 → −5.72 |
| Oyster Bay | oracle (2 passes) | 0/1 → 0/1 | 0/1 → 0/1 | 0/1 → 1/1 | 0.02 → 98.6 | 0.31 → 0.53 |

Unanswered (no region): Livermore 1/40 discovered (N958JX_R50), sim test 4/36, sim_chain 1/18. Counting
them as misses, Livermore discovered 90% after = 37/40.

Livermore discovered by stratum: 1 pass (35) 90% 30/35 → 34/35, area 1.89 → 11.37 km²; 2 passes (4: three
dev misses N563FX_R21, N913TC_R15, N814SR_R07 plus N788BM_R03) 1/4 → 3/4, area → 50 km² (q̂90 = 1).

"50 km²" is the whole eval search region: the region's HPD level drops to the floor density, so all unscored
ground is in. That is what a 90% region must do when roughly a quarter of comparable cases put the truth
outside the scored support. Oracle and sim surfaces are sharp, so their mass reaches 1 − ε = 0.72 in a tiny
area and q̂90 = 0.78 then has to take floor ground: oracle 90% regions become the whole search area. That is
the expected conservative side.

### Error at the peak and at the reported answer

The peak cell does not depend on T, so its error is unchanged everywhere (`compare`: 0 / 0 / n). The reported
answer (core centroid within 0.5 nats of the peak) does, since nats are score/T. Median error m (≤ 300 m count),
before → after:

| set | peak cell | answer |
|---|---|---|
| Livermore dev, discovered (28) | 443 (11) | 360 (12) → 365 (12); paired median Δ −1.3 m |
| Livermore test, discovered (11) | 465 (4) | 378 (4) → 403 (4); paired median Δ +1.3 m |
| Livermore dev, oracle (28) | 334 (14) | 231 (16) → 225 (15); paired median Δ −1.2 m |
| Livermore test, oracle (12) | 186 (9) | 208 (7) → 336 (6); paired median Δ +0.3 m, mean −45 m |
| sim test, discovered (32) | 98 (27) | 33 (27) → 35 (27) |
| sim_chain phone_livermore_like, discovered (17) | 259 (10) | 183 (12) → 184 (12) |

Per-case shifts are metres (paired median |Δ| ≤ 1.3 m); the Livermore test oracle median moves because
cases near the median swap order, not because answers move far.

### Confident wrong answers (discovered)

| case | log score before → after | HPD score before → after | in 90% before → after | 90% area km² |
|---|---|---|---|---|
| N231DT_R57 | −8.52 → −5.19 | 1.000 → 1.000 (truth cell never scored) | no → no | 0.91 → 50.1 |
| N2933A_R08 | −8.52 → −5.19 | 0.991 → 0.740 | no → **yes** | 0.89 → 50.2 |
| N451EE_R45 | −6.03 → −4.24 | 0.989 → 0.734 | no → **yes** | 4.46 → 50.2 |
| N748SP_R36 | −3.45 → −2.56 | 0.977 → 0.683 | yes → yes | 2.23 → 11.3 |

All four are dev cases (in the fit). Each gains 0.9–3.3 nats; three of four are inside the 90% region, but
only because that region is now (nearly) the whole search area.

### Abstention

`locate` reports Inconclusive when the best spot's weakest event has match z < 4 (`app::report::MIN_EVENT_Z`).
It fires on none of the four (weakest z 30–57). A per-case feature that does separate misses in the dev data is
the lowest `pass_p` among the searched events: < 0.9 in 6 dev cases, all with HPD score ≥ 0.977 (N748SP,
N456CS, N563FX, N913TC, N814SR, N231DT), against 4 of 22 at ≥ 0.9 (N3012N, N451EE_R45, N61NG, N2933A). The
threshold was read off dev, and the test split has one such case (N788BM_R03, 41 m, correct), so it is **not**
adopted as a stratum or flag. The wide t_cpa band (half-width > 100 s) signal is 3 of 4 wrong on 4 cases:
too few. Revisit both with an independent real set.

## Choice of ε and the alternative

The log-score optimum is flat above 0.2, and the held-out Livermore test (chosen as medium-high-confidence
cases, so easier than dev) prefers a small ε. Coverage is conformal at any ε; ε trades log score on misses
against region size on hits:

| ε (T refit) | Livermore 40 discovered 90% / area km² | sim test discovered 90% area | in-sample log score |
|---|---|---|---|
| 0.03 (3.04) | 37/39 / 7.2 | 9.8 | −1.98 |
| 0.1 (2.74) | 37/39 / 9.8 | 50.1 | −1.77 |
| **0.28 (2.17), shipped** | 37/39 / 11.4 | 50.1 | **−1.67** |

ε = 0.28 is shipped because it is the honest density on the app path (the fit criterion); ε = 0.03 is the
choice if a smaller region on correct answers matters more than −0.3 nats on misses.

## Remaining issues

- The 90% region on the app path is a dichotomy: a tight core when the answer is right, the whole area when
  the posterior misses. Conformal q̂ per pass count cannot tell the two apart; a per-case reliability feature
  (pass_p, band width, margin) needs more real cases than the 40 Livermore ones.
- 2-pass q̂90 = 1 rests on three real dev misses; sim 2-pass alone would give a tight region.
- Oracle regions are the whole search area at 90%; oracle 50/80% are still informative (0.32 / 50 km²).
- Livermore is one site, one phone and pattern traffic; ε = 0.28 is a Livermore miss rate.

## Reproduce

```
python3 -I tools/gen_sim_chain.py phone_livermore_like --src experiments/sim_calib --out experiments/sim_calib_chain_phone_livermore_like
dopplerprint eval --experiment experiments/livermore_dev_discovered.toml --out runs/m2_liv_dev
dopplerprint eval --experiment experiments/livermore_test_discovered.toml --out runs/m2_liv_test
dopplerprint eval --experiment experiments/oysterbay_discovered.toml --out runs/m2_oyster
dopplerprint eval --experiment experiments/calib_sim_discovered.toml --sim-dir experiments/sim_calib_chain_phone_livermore_like --case sim_cal_ --out runs/m2_sim_cal
dopplerprint eval --experiment experiments/calib_sim_discovered.toml --sim-dir experiments/sim_calib_chain_phone_livermore_like --case sim_test_ --out runs/m2_sim_test
dopplerprint eval --experiment experiments/simatm_discovered.toml --sim-dir experiments/sim_chain_phone_livermore_like --out runs/m2_sim_liv
dopplerprint calibrate --events discovered --calib runs/m2_liv_dev --calib runs/m2_oyster --calib runs/m2_sim_cal \
  --t-case livermore --t-case oyster --fit-floor \
  --test runs/m2_liv_test --test runs/m2_liv_dev --test runs/m2_sim_test --test runs/m2_sim_liv --test runs/m2_oyster \
  --detail N231DT --detail N2933A --detail N451EE_R45 --detail N748SP \
  --write crates/dp-core/src/search/calibration.toml --out runs/calibration_discovered.md
```

Run the evals with the previous calibration in place (they record its coverage); the fit reads only the cached
surfaces. `--before <toml>` compares against another calibration.
