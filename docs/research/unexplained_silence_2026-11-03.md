# Unexplained events, abstain, and a likelihood silence term (backlog items 4 and 5), 2026-11-03

Branch `feat/unexplained-silence`, rebased onto main `b57cef3`. The λ_u and r50 sweeps ran on `7365433`
(the tip of fix/soft-assoc-gates at that point). The final A/Bs (dev base / λ_u −6 / r50 2 km, all40,
sim_snr) were re-run after the rebase with `[search] calibration = "auto"` on both sides (the
`experiments/unexplained/*_base.toml` copies). Their point errors, log scores, ranks and stages are
identical to the pre-rebase runs, so the search is unchanged by the rebase. Only the calibrated-region
columns move, because main brought new tables. Runs are under `runs/` of the worktree. Experiments are in
`experiments/unexplained/`.

## What changed

- **Unexplained events** (`AssocParams.unexplained_cost` = λ_u, default `None`, i.e. off). Any event may stay
  unexplained. It is then not scored and adds λ_u to the penalty. A hypothesis needs at least one explained
  event, and its clock is the mean over explained events only. Full hypotheses are enumerated and capped
  exactly as before. Partial hypotheses get their own enumeration budget and their own `max_hypotheses` cap,
  and are appended after the full ones. The first attempt mixed the two lists, and the cheaper partial
  hypotheses pushed full ones out of the cap (dev discovered: assoc 23 → 19). Output shape: `Hypothesis.tracks`
  is `Vec<Option<usize>>`. `Candidate.track_ids[i]` is `""` (`UNEXPLAINED_ID`) and `events[i]` is
  z = 0, f0 = 0 for an unexplained event, so the lengths are unchanged. The viewer shows an empty flight id.
- **Abstain.** `SearchResult.null_score = −N·λ_u` (serialised only when set) is the all-unexplained null. It
  assumes some cell in the region hears no flight, so its silence cost is 0. `SearchResult::abstains()` is
  true when no cell scores above the null. `app::run` then stops with an inconclusive "no location" message.
  Eval sets `abstained = true` and `best = None`, keeps the stage recall and score gap, scores the case with
  the **uniform coarse-cell log score −ln N_cells** (the rule `discovered::front::no_answer` already used),
  and leaves the region empty (`hpd90_*` and `coverage` = None). Abstained cases count as flights wrong. The
  summary line now reports `≤300 m` and the number of no-answers. Discovered eval no longer drops the
  weakest event after an abstain.
- **Flights correct** skips unexplained events: a dropped spurious event is not a wrong flight. At least
  one event must be explained.
- **Silence** (`search/silence.rs`, `[search.assoc.silence] mode = "flat" | "likelihood"`, default flat). The
  module docstring has the model. In short: P_d = 0.9·logistic(margin/σ), with
  margin = −20·log10(r/r50) − hp_loss, and a cost of −ln(1 − P_d) nats per unassigned flight whose CPA sound
  arrives during the recording within `silence_range_m`. The cost is converted with `GATE_Z_PER_NAT` (1.58)
  under zsum and stays in nats under power_llr. A CPA at a track's first or last point
  (`Cpa.at_track_end`, new) is never counted in likelihood mode, because the source left or entered
  coverage. Flat mode is unchanged.
- **HP corner** (`audio/highpass.rs`): the lower edge of the lowest octave band whose 10th-percentile
  band level is within 10 dB of the 200–500 Hz band's. On Livermore-40 it finds 32 files at 20 Hz (no
  high-pass), 5 at 80 Hz and 3 at 160 Hz. That gives 8 high-passed files, the same count as
  corpus_signatures.md §6. The corner is measured only in likelihood mode (`SilenceParams::measure_recording`,
  called by `search::locate` and `eval::run_case`). The global search path does not measure it.

### Assumptions (item 5)

- **Source:** one nominal source for every engine class: f_src = 90 Hz, with per-harmonic SNR
  [1, 1, 0.6, 0.4, 0.3, 0.2], the `LlrParams::snr` prior. hp_loss is the share of that harmonic energy below
  the corner, in dB.
- **Floor:** the absolute floor level is **not** used. Phone gain and noise suppression move the signal level
  and the floor together (corpus_signatures §6: "NS costs level, not TNR"). The floor's shape (the HP corner)
  is the only input that does not depend on gain.
- **Spread:** σ = 6 dB covers source-level and propagation spread. r50 is the only knob tuned.

## Item 4: λ_u (dev split, 28 cases, discovered + oracle)

| λ_u (z) | path | answered | median m | ≤300 | flights | log score | in-90 | cal 90% | cal90 km² | L25 |
|---|---|---|---|---|---|---|---|---|---|---|
| off | oracle | 28 | 334 | 14 | 23 | −3.49 | 23 | 26 | 1.84 | 23 |
| −2 | oracle | 27 | 300 | 14 | 22 | −3.56 | 23 | 25 | 1.73 | 23 |
| −4, −6, −8 | oracle | 25 | 188 | 14 | 21 | −3.53 | 22 | 24 | 1.69 | 23 |
| −10 | oracle | 24 | 183 | 14 | 21 | −3.67 | 21 | 23 | 1.59 | 23 |
| off | discovered | 28 | 443 | 11 | 19 | −5.92 | 16 | 20 | 1.69 | 17 |
| −2 / −4 | discovered | 28 | 443 | 10–11 | 19 | −5.21 / −5.16 | 19 | 19–20 | 1.31 | 18 |
| **−6** | discovered | 28 | 379 | 11 | 19 | −5.13 | 19 | 20 | 1.31 | 18 |
| −8 | discovered | 28 | 379 | 11 | 19 | −5.12 | 19 | 20 | 1.31 | 19 |
| −10 | discovered | 27 | 388 | 11 | 18 | −5.21 | 18 | 19 | 1.25 | 19 |

λ_u is negative, so it acts as a credit: an unexplained event earns |λ_u| z, about the best score a
noise-only window reaches over f0, clock and cells. The search abstains when no cell beats N·|λ_u|. The paired
comparison at λ_u = −6 against off gives:

- discovered: log score mean Δ +0.79 [−0.05, 1.78]; error mean Δ −200 m [−544, +17]; 4 better, 3 worse;
  in-90 flips +3/−0;
- oracle: log score mean Δ −0.05 [−0.49, 0.42].

**Chosen: λ_u = −6** (−4 to −8 are equivalent on oracle; −6 and −8 are equal on discovered).

## Item 4: all40 (reporting only, rebased, calibration = auto)

| run | path | answered | median m | ≤300 | flights | log score | in-90 | cal 90% | cal90 km² | stages |
|---|---|---|---|---|---|---|---|---|---|---|
| base | oracle | 40 | 201 | 23 | 34 | −3.22 | 35 | 38 | 2.23 | L25 33/40 |
| λ_u −6 | oracle | 35 | 177 | 22 | 31 | −3.42 | 32 | 34 | 2.14 | L25 33/40 |
| base | discovered | 39 | 465 | 15 | 30 | −5.23 | 23 | 36 | 11.44 | select 36 → assoc 34/36 → L25 28/39 |
| λ_u −6 | discovered | 39 | 388 | 15 | 29 | −4.68 | 26 | 38 | 17.89 | select 35 → assoc 28/35 → L25 29/39 |
| λ_u −6 + silence r50 2 km | discovered | 39 | 388 | 15 | 29 | −4.65 | 27 | 37 | 17.76 | L25 30/39 |

The calibrated 90% region on the discovered path grows from 11.4 to 17.9 km² with λ_u. The new
discovered table (cec413d) was fitted on base surfaces, and partial hypotheses add near-best cells.
λ_u therefore needs its own calibration refit before region size can be judged. Before the rebase, at
T = 1.58, the area was 1.58 → 1.25 km².

Paired results (λ_u −6 against base):

- discovered: log score **+0.55 [−0.04, 1.25]**, 4 better, 4 worse; error mean Δ −143 m [−390, +12];
  in-90 flips +3/−0.
- oracle: log score −0.20 [−0.61, 0.18].

Per case:

- **Discovered gains come only from multi-event cases.** Before this change the search had to give a spurious
  event a flight (or drop-weakest removed an event):
  - N913TC_R15 3913 → 103 m (−11.75 → −1.99);
  - N563FX_R21 3483 → 1466 m (−11.74 → −4.87);
  - N61NG_R37 −10.97 → −4.12, same point.
- **Discovered loss:** N60RS_R11 83 → 316 m (−1.39 → −2.82). Drop-weakest used to remove its second event.
  Now that event is kept and left unexplained, and the clock comes from event 1 alone.
- **The four confident wrong answers do not change** (N231DT, N2933A, N451EE_R45, N748SP). Their discovered
  wrong flight explains the single event at z = 30–57, far above any credit that does not also abstain on
  correct cases (correct discovered single events start at z ≈ 10). A per-event constant null cannot catch them.
  That needs a relative null, i.e. the event's own free-CV-fit score minus a complexity cost, which the search
  does not compute today.
- **Oracle abstains on 5 single-event cases** (z ≤ 6.7):
  - N1387U, wrong, +4.6 nats;
  - N958JX, wrong, −2.5 (its wrong answer was still better than uniform);
  - N451EE_R51, N757AH_R41 and N814SR_R07, correct flights at 207–1868 m, −1.9 to −4.3.

  Abstaining loses whenever the posterior still puts more than 1/N_cells on the truth, and oracle events are
  weaker than discovered ones (shorter windows), so the same λ_u bites there. Discovered never abstained at
  λ_u ≥ −8.
- The stage-recall assoc drop (34/36 → 28/35) is a bookkeeping effect. Drop-weakest no longer fires, so more
  events are searched per case. The truth tuple then includes the labels of spurious events, which no
  hypothesis can match exactly.
- **Runtime:** discovered hypotheses scored 115.6 k → 162.7 k (+41%, the partial hypotheses). Summed score_s
  went 22.7 → 29.1 s. The box was shared, so timings are noisy (oracle score_s went down with identical
  hypotheses).

## Item 5: silence (dev split)

| silence | oracle log score Δ | oracle error Δ (mean) | discovered log score Δ | discovered error Δ |
|---|---|---|---|---|
| likelihood r50 2 km (count ≤ 3 km) | +0.06 [−0.06, 0.24] | −82 m [−229, +2] | +0.04 [−0.07, 0.23] | +2 m |
| r50 3 km | +0.05 [−0.04, 0.19] | −79 m | +0.03 [−0.06, 0.18] | +6 m |
| r50 5 km (count ≤ 6 km) | +0.08 [−0.07, 0.26], 15 better / 6 worse | −103 m | +0.02 [−0.11, 0.20] | +8 m |

All three are within noise on Livermore, a sparse-traffic corpus that is mostly single passes. I picked
5 km on dev from the better/worse counts, then checked sim_snr, which strongly rejects it (below). The
recommendation is therefore **r50 = 2 km**.

## Item 5: sim_snr (120 cases) and all40

| run | median m | ≤300 | rank-1 truth | log score | in-90 | score_s |
|---|---|---|---|---|---|---|
| zsum, flat 3 z (default) | 894 | 41 | 81 | −3.03 | 94 | 84 |
| zsum, no silence | 900 | 37 | 74 | −2.63 | 120 | 131 |
| zsum, likelihood r50 5 km | 901 | 25 | 77 | −2.72 | 115 | 135 |
| **zsum, likelihood r50 2 km** | **640** | 37 | **89** | **−2.41** | 120 | 90 |
| power_llr, flat 3 | 534 | 50 | 90 | −2.54 | 83 | 311 |
| power_llr, no silence | 591 | 52 | 80 | −2.17 | 104 | 367 |
| power_llr, likelihood r50 5 km | 915 | 26 | 88 | −2.20 | 106 | 466 |
| **power_llr, likelihood r50 2 km** | 534 | 40 | 90 | **−2.04** | 106 | 311 |

The flat-default and r50 2 km rows were re-run after the rebase and reproduce the earlier values exactly.
Their score_s comes from those quiet re-runs; the other rows were timed on a busy box. Silence costs nothing
measurable at runtime.

Paired results against the flat default:

- **zsum, r50 2 km:** log score **+0.62 [0.43, 0.83]**; flights +8/−0; in-90 +26/−0; error mean Δ −99 m
  [−217, +11].
- **power_llr, r50 2 km:** log score **+0.51 [0.30, 0.73]**; flights +9/−9; error Δ −20 m [−235, +197].
- **r50 5 km** costs point error in both modes: ≤300 falls 41 → 25 and 50 → 26, with mean Δ +350 m.
  Penalising decoys at 3–6 km pulls cells away from the truth.

Under power_llr the flat 3 penalty is not "effectively off". Removing it costs 10 rank-1 truths (90 → 80),
even though truth scores are hundreds of nats.

all40 with r50 2 km (reporting only), paired against base:

- oracle: log score +0.03 [−0.06, 0.15]; error mean Δ −56 m [−160, +3];
- discovered: log score +0.02 [−0.06, 0.15]; error Δ +1 m;
- flights flips 0/0 in both paths, and none of the four confident wrong answers move.
- hypotheses are unchanged ±0.4%.

## Recommendations

1. **Silence: switch the default to `likelihood`, r50 = 2 km, `silence_range_m` 3 km.** Sim log score improves
   significantly in both score modes (+0.5–0.6 nats/case), and Livermore is neutral. Point error on sim:
   median improves (894 → 640 m under zsum), but ≤300 falls by 4 under zsum and by 10 under power_llr. Refit
   `search/calibration.toml` afterwards, because the score surface changes. **Done** (branch
   feat/likelihood-silence-default): default flipped and calibration refit per
   calibration_discovered_2026-11-03.md §Reproduce (runs m3_*): T 2.17 → 2.19, ε 0.28 → 0.25, q̂ within 0.1.
   It also fixes Oyster Bay nationally (global_search_2026-11-03.md).
2. **Unexplained events: keep off by default for now.** If turned on, the setting is λ_u = −6 on the
   discovered/locate path, where it is +0.55–0.79 nats/case (CI just crosses 0) and fixes km-scale
   multi-event errors. It hurts oracle eval through abstentions. It does not fix the confident wrong single
   events. Before making it default:
   - decouple "allow unexplained events" from "abstain" (an abstain margin, or abstain only when every
     event's best score is below the credit);
   - consider replacing `app::run::search_dropping_weakest` with it (it does the same job in one search).

## Open questions

- A relative null per event (free CV-fit score minus a complexity cost) is the only per-event null that can
  catch N231DT, N2933A, N451EE_R45 and N748SP. It needs a scoring pass of the event's own fitted curve.
- r50 is not anchored per recording. The SNR ladder shows the miss probability depends on source level,
  which the model holds fixed. A per-recording anchor from the explained events' measured strength at their
  CPA range would replace the nominal r50.
- The silence cost converts nats with `GATE_Z_PER_NAT` = 1.58, not the posterior T (2.17 discovered,
  1.54 oracle since b57cef3). The dev sweep tables were measured before the rebase, at T = 1.58, so their
  "cal" columns use the old table.
