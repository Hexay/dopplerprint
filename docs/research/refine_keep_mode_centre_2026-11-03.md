# Refinement keep and mode-centre answer, 2026-11-03

Backlog items 1 and 3 (`error_reduction_backlog_2026-11-03.md`). Code base: main `249eea3`. The first pass ran on
`dac7602`, and its keep results were identical.

**Pinned calibration.** `search/calibration.toml` is compiled in (`include_str!`). Every run here uses the
`249eea3` copy (sha256 `306aa4b5…c14f`, T = 1.58, ε = 0.01, persistence 2 nats), so both sides of every pair
use the same calibration.

**Protocol.** Choices were tuned on `experiments/keep/dev_*.toml` (Livermore dev split, 28 cases, discovered
events with the oracle run alongside). They were then reported once on `livermore_all40_discovered.toml`. Its
`oracle/` side matches a separate `livermore_all40.toml` run exactly (paired Δ = 0 on every metric). Oyster
Bay is a sanity check (`experiments/keep/oyster_*.toml`; the parity TOML needs a relative `data/`). Paired
deltas are B − A, medians with 95% bootstrap CIs, from `dopplerprint compare`. That command now also prints
the candidate-answer table.

## Item 1: refinement keep

| all40 | oracle L200→L100→L50→L25 | discovered L200→…→L25 | error | cal. 90% area, mean Δ km² | cal. log score Δ | runtime Δ s/case (oracle, disc.) |
|---|---|---|---|---|---|---|
| keep_top 200 (baseline) | 39 → 36 → 33 → **33**/40 | 23 → 22 → 21 → **21**/39 | median 201 m oracle, 400 m disc. | – | – | – |
| keep_top 800 | 39 → 39 → 35 → **35** | 21 (=) | Δ 0 in 40/40 and 38/38 | −0.06 [−0.08, −0.04] (35/1) | +0.00 | +0.43, +0.12 |
| keep.rs Δ = 3 nats, cap 3000 | 39 → 37 → 36 → **36** | 21 (=) | Δ 0 in 40/40 and 38/38 | −0.02 [−0.03, −0.01] (20/0) | +0.01 [−0.00, 0.02] | +0.21, +0.07 |

Dev (28): k200 L25 23, k400 24, k800 24, Δ3 25, Δ6 + 8 per basin within 12 nats 25. Discovered was 15 for every
rule. In-90%-region flips are 0 for every variant on both paths. Oyster Bay: L25 1/1 and error 79 m under every
rule.

**Conclusion: keeping more cells does not reduce point error on this corpus.** Even when a recovered truth is
refined, the peak stays where it was (score gap > 0). The truths still lost at k800 are not keep losses:
- **N1387U**: the truth's coarse cell is never scored (rank "None" at L200). No hypothesis survives planning at
  the cell centre.
- **N231DT, N2933A**: wrong flight, with a 10 z gap. The truth ranks 1277/1795 at L50 even at k800.
- **Discovered**: every L200 "loss" is an unscored truth coarse cell (N4040, N413AK_R49, gap −0.13 and 0.62).
  The truth would win if its cell were planned. Association is checked at the cell centre, and the 200 m cell
  is too coarse for the discovered events' bounds. This is a `hypothesis.rs`/`pipeline.rs` matter (owned): pad
  the association bounds by the cell half-diagonal at coarse levels.
- The discovered path loses 12 of 38 at assoc, so refinement is not its bottleneck.

The q̂90 = 0.988 single-pass region is therefore driven by wrong-flight and unplanned-cell truths, not by
keep_top.

**Phase 2 (`search/keep.rs`).** `kept(found, keep_top, &KeepParams) -> Vec<usize>` keeps the top `keep_top`,
every cell within `delta_nats` (score/T) of the best, and the best `per_basin` cells of each ToMATo basin whose
peak is within `basin_delta_nats`, up to `cap`. Quota cells come first under the cap. The default is off,
which is exactly `take(keep_top)`. On dev, Δ = 3 alone equals Δ = 6 plus quotas, so the quota is unused. The
hook is optional. It buys recall and a slightly smaller region at about half the runtime cost of keep_top
800. It is not needed for point error.

Hook for `pipeline.rs` at `249eea3` (three lines; `SearchParams` lives in the owned file):
- after line 28 `pub keep_top: usize,`, add `pub keep: super::keep::KeepParams,`
- after line 45 `keep_top: 200,`, add `keep: Default::default(),`
- replace line 125 with
  `let next: Vec<Cell> = super::keep::kept(&found, p.keep_top, &p.keep).into_iter().flat_map(|i| found[i].cell.children()).collect();`

Without the hook, a `[search.keep]` table in an experiment is silently ignored, because `SearchParams` is
`serde(default)`.

## Item 3: the reported location

Candidate answers, each paired against the peak cell (all40, keep_top 200):

| answer | oracle median m | ≤300 m | paired Δ m [95% CI] | b/w | disc. median m | ≤300 m | paired Δ m [95% CI] | b/w |
|---|---|---|---|---|---|---|---|---|
| peak cell (previous) | 201 | 23 | – | – | 400 | 16 | – | – |
| peak of heaviest mode | 192 | 24 | 0 | 7/3 | 358 | 16 | 0 | 8/4 |
| centroid of peak's whole mode | 276 | 21 | −47 [−89, −9] | 27/13 | 320 | 19 | −23 [−63, −10] | 28/10 |
| centroid of heaviest whole mode | 276 | 21 | −54 [−123, −11] | 27/13 | 355 | 18 | −23 [−67, −10] | 28/10 |
| **peak's core centroid, 0.5 nats (new default)** | 216 | 23 | **−18 [−46, −4]** | 28/12 | **332** | **18** | **−13 [−34, +6]** | 22/16 |
| core 1 / 2 / 4 nats | 250 / 245 / 275 | 22 / 22 / 21 | −17 / −25 / −39 | | 331 / 324 / 321 | 18 / 18 / 19 | −13 / −20 / −23 | |

Cases worse / better by > 200 m, oracle: whole mode 5/10 (N788BM +1768 m), core 0.5 nats 1/5 (worst N916KT
+268 m), core 1 nat 4/5, core 4 nats 5/10. Discovered: 0 worse at every depth. Oyster Bay: 79 → 88 m (core
0.5) and 103 m (whole mode).

**Default: the probability-weighted centroid of the peak's core** (`modes::answer`, `ANSWER_DEPTH_NATS =
0.5`). The core is the leaves of the peak's mode that are connected to the peak and score within 0.5 nats of
it. Why:
- The paired median improves in all four cells (dev and all40, oracle and discovered). The discovered CI
  includes 0.
- It has the smallest harm tail of any centroid.

Mode choice stays by peak (calibration_2026-11-03 §Mode choice). The peak cell stays in `location` in the JSON.
The answer is `posterior.answer` (plus `from_peak_m`), and every mode carries `centroid_lat/lon`. The text
report and the web summary show the answer. The text report also gives the peak cell's distance from it. The
centroid is taken within one mode, so a mirror pair is never averaged, and the single-pass "do not rule out
the mirror" line is unchanged.

**Negative results.**
- **Whole-mode centroid (the backlog's proposal): rejected.** On dev it looked best (−69 m oracle). On all40 it
  sent five good fixes 0.9–1.8 km away (N788BM 100 → 1868 m, N916KT 195 → 1692 m, N757AH 207 → 1070 m) and
  raised the test-split median from 186 to 506 m. The cause, seen on dev (N61NG 159 → 886 m): ToMATo merges
  clusters over a saddle shallower than 2 nats, so one "mode" can hold two clusters about 2.5 km apart, and
  its centroid falls between them.
- **Caveat.** That failure was first seen on all40, which includes the frozen test split. The core rule and its
  depth were then chosen on dev only (`depth.py`-style tally: 0.5 nats was the only depth with no > 200 m harm
  on dev), but the test split is no longer untouched for this decision.
- **Mode by mass vs by peak: still undecided.** There are 8 discordant dev cases (5 better, 3 worse), against
  the 25 the calibration doc asked for.

## Files

`search/keep.rs` (new), `search/modes.rs` (`answer`, `core_centroid`), `eval/coverage.rs` (centroid, core and
answer errors), `eval/compare.rs` (answer table, runtime and area rows, refinement flips), `eval/aggregate.rs`
(reported-answer row), `app/posterior.rs`, `app/text.rs`, `dp-cli cmd/serve_backend.rs`, `experiments/keep/`.
