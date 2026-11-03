# Roadmap experiments, 2026-11-03

Top 7 items of `localization_deeper_factors.md` checked against the code, item 1 built, items 2–4 probed.
All numbers: estimate [95% percentile bootstrap over cases]; paired Δ = variant − baseline via
`dopplerprint compare A B [--case s]`. Runs were on a heavily loaded devbox, so wall times are not comparable.

## A. Items 1–7 vs the code (base e315903)

| # | Exists | Missing | Size |
|---|---|---|---|
| 1 Measurement | error, flights correct, tuple rank, margin, clock error. ARCHITECTURE.md's "90% region" and "stage where truth was lost" did not exist; search has no posterior, null, refine or confidence stage. Only **1** real case (Oyster Bay) could be posed | done here, see B | S–M |
| 2 Temperature | one global uniform `Atmosphere.c` in `SearchParams`, TOML-overridable | per-case/per-sample c̄ from weather; z-uniform mean on [z_r, z_a]; σ_c nuisance; weather prep | M |
| 3 Wind | uniform ECEF wind through emission/Doppler/CPA; sim takes `wind_enu` | per-case source, per-sample along-ray mean, night σ_w inflation. A global ECEF wind is wrong for multi-site runs (eval now converts per case) | M |
| 4 Joint score | one clock shared by all events (locked sweep, `clock_tol_s` 8 s). The ±`clock_sweep_s` sweep is not clipped to the start prior | shared c/w nuisance (eval-level profile prototype here), marginal instead of max over clock, clock rate | M in `search::score` |
| 5 Detection | silence = flat 3 z per unassigned flight with CPA ≤ 3 km in the clip; no SNR, coverage, P_d, shadow, LOS or deaf state | all of it | M |
| 6 Unknown aircraft | none: each event must take a distinct catalogue flight | per-event unknown/clutter option, inconclusive flag | M |
| 7 Sim realism | sim independent of `physics`; uniform c and wind, coherent ground mirror, rpm jitter | layered c(z)/w(z), shadows, coherence loss, turbulence, untracked traffic | M–L |

## B. Measurement (item 1)

- `eval/livermore.rs`: the 40 Livermore passages become real cases (start from ridge-vs-spectrogram
  alignment, N-number → ICAO, oracle CPA from the truth ADS-B). Real corpus n: 1 → 41.
- `eval/metrics.rs`: stage recall (catalog → near → assoc → L200…L25), score gap, truth-cell rank,
  log score and 90%-region coverage/area from a tempered coarse-cell posterior (β 1, ε 0.01).
- `experiments/split.toml`: 12 frozen test cases with truth fingerprints; `eval.split` defaults to dev.
- Summary: old columns kept, new columns, bootstrap CI table, stage recall line; `compare` for paired Δ.

Baselines:

| run | n | median error m | ≤ 300 m | flights | log score | in 90% | recall lost at |
|---|---|---|---|---|---|---|---|
| Oyster Bay parity | 1 | 78.9 | 1/1 | 1/1 | −0.53 | yes (0.16 km²) | none |
| Livermore dev (`livermore_baseline`) | 28 | 334 [152, 1308] | 0.50 [0.32, 0.68] | 0.82 [0.68, 0.93] | −3.49 [−4.70, −2.55] | 0.82 [0.68, 0.96] | L200 1, L100 3, L50 1 |
| Livermore all 40 (`livermore_all40`) | 40 | 207 (upper median) | – | 34/40 | – | – | (run before metrics) |
| sim_atm, model c 343 / no wind | 18 | 171 [79, 668] | 0.56 [0.33, 0.78] | 1.00 | −1.94 [−2.36, −1.46] | 1.00 | L50 2 |

Livermore: truth is never lost before the lattice (catalog/near/assoc 28/28); all losses are at refinement.
The three 2026-05-09 "low confidence" truths account for three of the five wrong-flight cases (2–5 km).
Median score gap 0.59 [0.22, 1.26] z: the best cell beats the truth cell by little, i.e. errors are mostly
flat ridges (along-track/clock within the ±2+2 s window), not search misses.

## C. Experiments (items 2–4)

`experiments/sim_atm/` (tools/gen_sim_atm.py): 12 single + 6 two-aircraft flyovers rendered with c = 337 m/s
(≈ 10 K colder than the default 343) and a 6 m/s east wind. Uniform medium: a sensitivity test, not realism.

| variant vs reference (stratum) | n | Δ error m, median | Δ log score, mean | Δ score gap, mean | verdict |
|---|---|---|---|---|---|
| c_true vs default (pairs) | 6 | −34 [−247, −2]; mean −94 [−235, −12]; 5/1 better | +0.25 [0.05, 0.49] | −0.65 [−0.93, −0.32] | **10 K of temperature error costs ~35–95 m on two-event cases** |
| c_true vs default (singles) | 12 | +77 [−32, 121]; 4/8 | +0.07 [−0.01, 0.16] | −0.52 [−0.73, −0.34] | point error swamped by mirror/along-track flips (1–2 km either way); fit quality still improves |
| all_true vs c_true (pairs) | 6 | −12 [−24, 8]; mean −9 [−20, 4] | −0.02 [−0.06, 0.00] | −0.05 [−0.10, −0.01] | 6 m/s wind: ~10 m, CI includes 0. Too small to tell at n = 6 |
| all_true vs c_true (singles) | 12 | −24 [−739, 361] | −0.01 | −0.04 [−0.10, −0.01] | inconclusive |
| nuisance_c (c ∈ 331/337/343, profile) vs default (pairs) | 6 | −34 [−247, −2], identical to c_true | −0.02 [−0.17, 0.13] | −0.65 | **profile picks c = 337 in 6/6 and recovers the c_true error** |
| nuisance_c vs c_true (pairs) | 6 | 0 | −0.27 [−0.44, −0.11] | 0 | profiling costs ~0.3 nats of log score (more cells near their max) |
| nuisance_cw (c 337/343 × wind E −6/0/+6) vs all_true (pairs) | 6 | 0 [−8, 12]; 4 ties | −0.16 [−0.30, −0.06] | 0.00 | shared c+wind profile matches the known-atmosphere result; picks c right 6/6, wind right 4/6 (else 0, never wrong sign) |

Not run (time): Livermore c sweep (`atm_c337`, `atm_c349`), along-track wind probes
(`atm_wind_along_{p10,m10}`) and `nuisance_c_profile`. TOMLs are committed and paired against
`livermore_baseline`.

Corpus verdicts: with n = 28 dev real cases the hit@300 m CI is ±0.18, so only effects of roughly that size
(or paired shifts of ≳100 m median) are detectable on real data. Every atmospheric effect seen in sim
(≤ 100 m) is below that, so items 2–3 must be judged on sim and on the continuous scores (score gap, log
score), which moved with p < 0.05 where the point error did not.

## Notes

- Point error on single events is dominated by the degeneracies (mirror, along-track/clock), not atmosphere;
  stratify every atmosphere comparison by event count.
- Log score is coarse-cell (200 m) mass; it rewards sharp posteriors and penalises profiles. Use score gap
  and paired error alongside it, and fit β before reading coverage as calibration.
- The Livermore audio start comes from aligning the publisher's ridge, so clock error there is relative to
  that alignment, not an independent clock.
