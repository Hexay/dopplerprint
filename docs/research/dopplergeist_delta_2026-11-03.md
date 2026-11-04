# DOPPLERGEIST delta: f6ba18a → 994d1ee (2026-11-03)

Read-only review. Theirs: `/srv/agents/work/dg_upstream` (not executed). Paths prefixed `dg:` are relative
to that repo (`src/dopplergeist/` abbreviated `dg:src/`). Unprefixed paths are ours, under
`crates/dp-core/src/`. Baseline map of their algorithm: `docs/research/dopplergeist_analysis.md`.
Engineering, CI and provenance comparison: `docs/research/engineering_vs_dopplergeist_2026-11-03.md`.

## 1. What they changed in 237 commits

Of 203 files changed (+36k lines), the large majority are **performance and accounting**. The physics and
the scorer are unchanged.

| Kind | Change | Evidence |
|---|---|---|
| Perf | Optional C backends for the IRLS ridge fit and the Newton emission solve | `dg:src/_native_ridge.c`, `_native_acoustic.c`, `matching.py:541` |
| Perf | Parallel resumable coarse tiles, with workers = CPUs − 1 | `dg:src/parallel_tiles.py`; `dg:docs/DATA_AND_SCALE.md:82-86` |
| Perf / coverage | Certified score ceilings: a pseudo-Huber dual lower bound on loss rejects clock trials that cannot reach the 3σ gate or the Top-K floor | `dg:src/acoustic_screen.py:10`; `dg:src/search.py:411-531` |
| **Coverage** | Overloaded tiles split into sub-rectangles. Overloaded single cells partition first-pass clock anchors into checkpointed ranges, so no association is dropped. | `dg:src/tiled_search.py:165,358`; `DATA_AND_SCALE.md:90-106` |
| **Coverage** | Tracks with unsupported (non-subsonic) motion are excluded only when a spline Bezier hull bound proves them out of range | `dg:src/geometry.py:148-178` |
| Model | Automatic CPA speed band widened from [1, 0.9c] to [0, c]. One real cell gained 1,166 associations. | `dg:src/matching.py:493`; `dg:docs/HANDOFF.md` (PR #196) |
| **Decision** | New abstention when two matched groups overlap at a rational harmonic ratio (q90 residual ≤ 2·(σ_hi + h·σ_lo)) | `dg:src/confidence.py:44-85,244-250` |
| Discovery | Algorithm unchanged; only the slope median and the plotting changed. Single-ridge groups stay eligible (14 of 15 groups on the 1 h upload are single-ridge). | `dg:src/discovery.py`; `dg:docs/AUDIO_DISCOVERY.md:20-37` |
| Timing | ENF tone screen only: peaks near 50/60 Hz ×1–3. No reference match and no UTC estimate. | `dg:src/enf.py:13,80`; `dg:docs/ENF_TIMESTAMPING.md` |
| Field evidence | Scanner-only surveys of real audio, and the first end-to-end real ranking (Livermore) | §2 |
| Review of us | They reviewed `dopplerprint` @9641802 | `dg:docs/DOPPLERPRINT_REVIEW_2026-11-03.md` (§5) |

These are unchanged since f6ba18a and remain their core: subset association with a free "unmatched" option,
drift-bounded IRLS ridge fit, Powell refinement, sensitivity profile, and the ordered decision gate
(`dopplergeist_analysis.md` §2.4, 2.7–2.9).

## 2. Their published accuracy numbers (verbatim)

No figure below is a surveyed-microphone error. "Inferred reference" means their reconstruction from tracks
and published offsets across the Livermore dataset (`dg:docs/LIVERMORE_DEMO.md:13-19`).

### 2a. Localisation on real recordings

| Claim (verbatim) | Source | Data | Priors / scope | Outcome |
|---|---|---|---|---|
| "Its top candidate landed **476 m from an inferred reference**" | `dg:README.md:9`; `dg:docs/LIVERMORE_DEMO.md:119` | Real Livermore N784MB_R04 MP3; ADS-B for 466 aircraft (all types) | Clock window ≈10 s; city box `-121.84,37.65,-121.70,37.73`; 250 m coarse grid | inconclusive. The table shows rank 10 at 152 m and rank 20 at 176 m. |
| "1 … 1,982 m"; "4 … 477 m"; "17 … 153 m" | `dg:docs/LIVERMORE_RANKED_RUN.md:17-21` | Same MP3 and same 466 aircraft | ≈10 s window; rectangle derived from the reference; 200 m grid | inconclusive / search_incomplete. All five representatives are aircraft `aaa077`. |
| "The top candidate scored 0.928432 and was **110 m from the inferred reference**" | `dg:docs/LIVERMORE_DEMO.md:72-73` | Same MP3; index holds **only the known aircraft** | ≈10 s window; city box | inconclusive |
| "about **158 m from the inferred reference**" | `dg:docs/LIVERMORE_DEMO.md:56` | Same MP3; index holds only the known aircraft | ≈10 s window; area derived from the reference | inconclusive |
| "The highest-scoring point is **481 m from an inferred reference**" | `dg:docs/LIVERMORE_DEMO.md:13` | WAV plus the supplied track, window and band (configured run) | ±5 s clock sweep; city prior | `accepted_location: false` |
| "placed its leading candidate 43.86 m from an approximate public reference" | `dg:docs/LIVERMORE_DEMO.md:40-42` | Oyster Bay, 2 events, configured | Manual windows; regional box | incomplete support. Predates f6ba18a. |
| Full-day 1 h reference upload (CONUS, 2026-09-08 00:00 to 09-09 12:00 window): "No final search result or location has been produced" | `dg:docs/HANDOFF.md` (Full-day section) | Real | No time prior; CONUS coverage | **still running at 16:30 UTC** |
| NASA edge_flyover_065: 474.0 m conditional; the other 3 of 4 cases gave "no_proposal" | `dg:benchmarks/nasa-session-results-20261103.csv:2-5` | Real UAV with the supplied track | Supplied trajectory only | Predates f6ba18a; likely the wrong source |

### 2b. Scanner-only results (no localisation)

| Claim (verbatim) | Source | Data |
|---|---|---|
| Livermore: "Thirty-five produced at least one moving-tone proposal and five produced none … 182 ridge proposals grouped into 137 harmonic groups" | `dg:docs/LIVERMORE_AUDIO_SCANNER.md:21` | Real, 40 whole WAVs |
| AeroSonicDB: "Aircraft labeled \| 125 \| 51 (40.8%)"; "Ambient/background … \| 30 \| 0 (0%)" | `dg:benchmarks/aerosonic-scanner-survey-20261103.md:23-24` | Real, 10 s clips |
| YouTube random sample: "**Total** \| **200** \| **198** \| **2** \| **24** \| **118**" (24 of 198 scanned clips had proposals) | `dg:benchmarks/youtube-random-sample-20261103.md:21` | Real |
| YouTube expanded: 47 with proposals, 226 without | `dg:benchmarks/youtube-expanded-scan-20261103.md:12-13` | Real |
| DLR RECORD 139 (A320, 88 m/s): "returned **no proposals**" | `dg:benchmarks/dlr-record139-scanner-check-20261103.md` | Real, 7 s |
| Hermes Cessna 421: two groups with "median high/low ratio of 1.5025"; "No flight matching or geolocation was run" | `dg:benchmarks/hermes-reference-real-scan-20261103.md` | Real |
| Eclipse field audio: "18 produced proposals and 27 produced none" (batch 1); 4 with proposals and 5 without (batch 2) | `dg:docs/ECLIPSE_FIELD_CORPUS.md:7` | Real |
| ENF screen: a persistent tone candidate in 2 of 24 site recordings and 0 of 26 others | `dg:docs/ENF_TIMESTAMPING.md:49-54` | Real; screen only |

### 2c. Synthetic results (unchanged since f6ba18a)

| Claim | Source |
|---|---|
| Discovery found 13/13 events on the 3/4/6-flyover fixtures | `dg:README.md:156` |
| National search led with a hypothesis 1,046,023 m from truth; the true pair ranked 4th at 6.16 m. A separate run's leader was 0.65 m from truth. Both runs were inconclusive. | `dg:README.md:157` |
| 33/40 annotated passages detected; median error 6.85 % in speed and 15.37 % in slant | `dg:README.md:158` |

Their performance headlines (`dg:README.md:106`: "62.03 → 37.19 seconds") are pipeline speed, not accuracy.

**Bottom line:** they have **one** real recording localised end to end, with no surveyed truth, a ±5 s clock
prior and a city-sized box. Their rank-1 result is 476 m or 1,982 m depending on the run, and every run abstains.

## 3. Capability comparison

Verdicts are on accuracy and algorithm, not engineering.

| Capability | Theirs @994d1ee | Ours | Verdict |
|---|---|---|---|
| Event discovery | Single 2 s/0.5 s STFT; Hungarian peak linking; gates ≥12 s and drop ≥3 %/4 Hz; harmonic grouping by ratio (`dg:src/discovery.py`; analysis §2.3). Finds 17 ridges on the 1 h upload. | Multiresolution 0.5/1/2 s whitened spectrogram; line mask; comb Viterbi with peeling; split, merge and stitch; gates ≥10 s and drop ≥2 % (`audio/frontend.rs:30,72`; `events/discover.rs:25-82,133-150`; `events/mod.rs:112-130`). Finds 47 proposals on the same file (their count). | **We're ahead** on recall and short-event handling. Precision is unmeasured on both sides. |
| Discovery negative control | 0/30 ambient AeroSonic clips produced proposals; wide surveys of real negatives (§2b) | No ambient or negative-control false-alarm rate recorded | **They're ahead** (evidence, not algorithm) |
| Unmatched events | Subset association: unmatched proposals cost 0 and each matched group adds ≤1 (`dg:src/matching.py:424`, `association_score`) | Every event must be explained. On failure we drop the weakest and retry (`app/run.rs:189`). There is no null option (backlog #4). | **They're ahead** |
| Hypothesis budget | Exhaustive DFS with interval pruning. Overloaded cells are partitioned by clock anchor rather than truncated (`dg:src/tiled_search.py:358`). | Cheapest 64 per cell (`search/hypothesis.rs:56`). Truncation is silent. | **They're ahead** on completeness |
| Association gates | Broad bounds: d ≤ 20 km and v ∈ [0, c] (`dg:src/matching.py:493`) | Student-t soft gates on ln d, ln v and clock, with hard outer bounds (`search/hypothesis/soft.rs:35-95`) | Different tradeoff. Ours is tighter, so recall risk on curved tracks (their HANDOFF lesson). Parity, pending a false-exclusion measurement. |
| Silence / intervening aircraft | Filter exists but is off by default (`dg:src/automatic.py:187`) | Flat 3 z per unexplained flight within 3 km (`search/hypothesis.rs:57-58`) | We're ahead (it is used), though crude |
| Scoring model | Ridge residual: f_src = f0 + a·u, with linear drift capped at 2 %; pseudo-Huber IRLS; closed-form 2×2 fit (`dg:src/matching.py:541`) | Spectrogram z-sum along the predicted curve, 6 harmonics, ±1 bin tolerance, constant f0 per event (`search/score.rs:50-55`) | **They're ahead on drift**: we have none (backlog #8). We're ahead on using spectral power directly and on harmonics. |
| Clock resolution | 0.5 s grid, then a continuous dclock in Powell (±5 s) | 0.25 s frames, ±5 s; continuous only inside `power_llr` | Parity |
| Spatial refinement | 2 km or 200 m grid, then 100 m, then Powell over (dx, dy, dt) in a ±500 m box for the top 5 (`dg:src/matching.py:685`) | Lattice 200 → 25 m, top 200 per level; no continuous optimiser (`search/pipeline.rs:47-49`) | Parity: our 25 m lattice is finer than their 100 m grid. They have a continuous polish; we don't. |
| Sensitivity / robustness | Re-refines under ±150 m altitude bias, ±150 m observer height, ±5 m/s sound speed, ±σ frequency offset and tilt, and the clock endpoints (`dg:src/matching.py:763`) | No altitude-bias study. c and wind probes not run on Livermore (backlog). | **They're ahead**, as a diagnostic |
| Uncertainty / decision | Ordered gate. Uncalibrated, and abstains on every real case. New harmonic-overlap abstention (`dg:src/confidence.py:44,244`). | Calibrated posterior with T and ε; conformal 90 % HPD per pass-count stratum; ToMATo modes; flag for the one-aircraft mirror (`search/surface.rs:3`; `app/posterior.rs:24-36`) | **We're ahead** on calibration. **They're ahead** on harmonic-ambiguity abstention, which we lack. |
| Multi-event | Unlimited events. Harmonics count once; the gate needs ≥2 independent groups. | Up to 4 events (`app/mod.rs:62`) | Parity. They use more events but abstain anyway. |
| Jets | Kept; type is a label only | Kept and flagged; penalised in `pass_p`; tracks above 9,000 m dropped (`app/run.rs:31-32`) | Parity. Our 9 km cut is a small risk. |
| Terrain / geoid | GLO-90 + EGM2008 (`dg:src/data_access.py`) | 3DEP 1 m, then GLO-30, + EGM2008; height + 1.5 m (`dem/tiles.rs:12-15`) | **We're ahead** |
| Atmosphere | c = 343 m/s; ±5 m/s only as a sensitivity | HRRR wind/temperature column, on by default. Livermore dev 334 → 250 m (`docs/research/weather_wind_2026-11-03.md:75`). | **We're ahead** |
| Time-window handling | Accepts any window up to 3 days; returns `needs_input` if none. The full-day CONUS run is in progress. | Default ±120 s (`app/when.rs:7`); file-time fallback | **They're ahead** in scope. Neither has a real result with a wide window. |
| Search coverage / size | CONUS tiling, parallel, resumable, with explicit incomplete-work accounting | ≤50 km radius in the app (`app/place.rs:9`). The CONUS `search::global` is eval-only and fails on Oyster Bay (537 km; `docs/research/global_search_2026-11-03.md`). | **They're ahead** in coverage. We're ahead on speed (Rust/CUDA). |
| ENF / time recovery | Tone screen only | Detector, trace, sample-rate check and splice detection (`audio/hum/mod.rs`); not matched to a grid | We're ahead. Neither does timestamping. |
| Trajectory support | Subsonic check; unsupported segments excluded (left Livermore inconclusive, 187 evaluations) | No per-interval motion check found | Mixed. Their check is safer against ADS-B glitches but costs coverage. |

## 4. Where they're ahead, ranked by likely accuracy impact for us

1. **Null / unmatched-event option in association.** Discovered events include spurious and non-aircraft
   ridges. Today one bad event forces a wrong match or a drop-and-retry. The gap from oracle to discovered
   events (dev median 334 → 443 m; `docs/research/calibration_discovered_2026-11-03.md:93-96`) is the
   largest error source we have measured, and this attacks it directly. Their form: score Σ(1 − loss/3σ) over
   matched groups, unmatched costs 0. For our posterior, use a clutter likelihood per event rather than 0.
2. **Source-frequency drift in the fit.** Their linear drift (±2 %, Gaussian prior, `dg:src/matching.py:541`)
   absorbs RPM and pitch changes. Our constant f0 per event pushes drift into geometry, mostly as along-track
   and clock error. Add one drift parameter to the per-event f0 sweep (backlog #8).
3. **No silent hypothesis truncation.** Our 64-per-cell cap (`search/hypothesis.rs:56`) can drop the true
   association in busy airspace and wide clock windows, with no record. Make the cap at least visible
   (count truncations per cell), then test 64 against 512 on all40 and the busy sims.
4. **Harmonic-overlap abstention.** Two "independent" events that are really harmonics of one pass give false
   multi-pass confidence (and the wrong conformal stratum). Port their check
   (`dg:src/confidence.py:44-85`): q90 |f_hi − h·f_lo| ≤ 2(σ_hi + hσ_lo) over ≥60 % overlap. Fold the pair or
   downgrade the stratum rather than abstain. Our backlog notes the published ridge is an overtone in 33/40
   Livermore cases.
5. **Nuisance sensitivity (altitude bias ±150 m, c ±5 m/s).** They use it as a diagnostic, not a correction.
   For us it would decide whether baro-only tracks and c errors explain the residual cases over 1 km. Run the
   unrun `atm_c337/349` probes and add an altitude-bias probe.
6. **Wide-window / national scope.** Their exhaustive design works with an unknown day-scale clock. Ours needs
   ±2 min and a place. This doesn't change accuracy on current benchmarks, but it limits which recordings we
   can attempt at all. Their run hasn't finished either, so this is unproven for both.
7. **Continuous polish (Powell).** It matters little at our 25 m lattice. It mainly helps the clock and
   sub-cell peaks (backlog #13).
8. **Negative-control evidence for discovery** (0/30 ambient). It isn't an algorithm, but without it our
   discovery precision is unquantified. The cheap fix: run `discover` on AeroSonic ambient and the YouTube
   no-aircraft categories.

## 5. Where we're ahead

- **Measured real accuracy.** Livermore: 34/40 right aircraft, median 207 m, oracle events, ±2 s
  (`runs/1793706036-livermore_all40/summary.md:3`). On discovered events: 443/465 m dev/test, with 39/40
  answered (`calibration_discovered_2026-11-03.md:64,93-94`). They have one Livermore recording at 476 m
  (rank 1, all aircraft).
- **Calibrated uncertainty.** Posterior with T and ε, plus conformal regions: 90 % coverage of 0.88 on all40.
  Their gate is uncalibrated and has never accepted a real location.
- **Physics inputs:** wind and temperature, a 1 m/30 m DEM, and harmonic comb scoring.
- **Speed:** a bit-identical CUDA sweep. Their native C buys 1.4–1.8×.

## 6. Their critique of our benchmark (valid; address before external comparisons)

From `dg:docs/DOPPLERPRINT_REVIEW_2026-11-03.md:27-49`:
- Our Livermore reference is also derived from tracks and published distances, not surveyed. They say this
  explicitly (the review cites our `tools/corpus/livermore.py`).
- The headline uses supplied flyover windows and a clock aligned from the published ridge, so it is not blind.
  Our README states the first part (`README.md:54-55`).
- The 64-hypothesis cap and count-based silence penalty "can omit candidates without explicit incomplete-work
  accounting" (cf. §4 item 3).
- They ran our `dopplerprint==0.1.0` wheel blind on the Oyster Bay MP3: 47 proposals, against their 17 ridges.
  Both overlapped all three reference windows. Neither precision nor recall was measured.

Like-for-like comparison: both projects score Livermore against an inferred reference with a narrow clock
prior. Ours covers 40 recordings with ±2 s and oracle events, or 39 with discovered events. Theirs covers 1
recording with ≈10 s and discovered events. Their 476 m sits near our discovered-event median (443–465 m).
