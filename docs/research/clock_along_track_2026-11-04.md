# Default app scenario: error is the clock, along the track (2026-11-04)

`experiments/livermore_dev_discovered_w120_r10k.toml`: the 28 Livermore dev cases as `locate --near` runs them
(10 km radius, start ±120 s), at 81fba2c. Run: `runs/liv_w120_r10k_81fba2c`.

| | oracle events | discovered events |
|---|---|---|
| median error | 4482 m | 4905 m |
| ≤ 1 km | 8/28 | 2/28 |
| flights correct | 20/28 | 14/28 |
| calibrated 90 % region contains truth | 25/28 | 25/28 |

## Diagnosis

With the right flight (oracle, 20 cases), error tracks the start-time error: correlation 0.79. Median error
is 500 m when |clock error| < 10 s (n 10) and 7.4 km otherwise (n 10). Error ≈ v × clock error, along the
track. One pass fixes the cross-track distance and side; moving the recorder along the track and shifting
the start by the same flight time gives the same Doppler curve. Clock errors are spread evenly over ±120 s
(5/28 beyond ±100 s, ~4.7 expected), so the search isn't drifting to the window edge. It is a flat ridge,
and the argmax lands anywhere on it.

The calibrated region already covers this (25/28). The point estimate cannot do better from one pass
without more information.

## Not a fix

- **Pulling the point to the prior centre.** The eval centres the start prior on the true start
  (`eval/mod.rs` `t_start_prior`), so this would win the metric without helping a real user, whose clock
  error is unknown. Any estimator change here needs an eval with an off-centre prior first.

## Off-centre window and the silence term (2026-11-05)

`experiments/livermore_dev_discovered_w120_r10k_off.toml` (`time_prior_offset_frac = 1`: truth anywhere in
the ±120 s window) at 2997c7d. Discovered median 4752 m, 3/28 within 1 km; oracle 3699 m, 9/28. That is
close to the centred run, so the estimator did not lean on the centre. This is the baseline for clock and
along-track work.

The silence term's range model carries almost no along-track information (paired `dopplerprint compare`
against that baseline, 28 cases):

| variant | discovered error | reported-answer error, mean Δ | log score |
|---|---|---|---|
| silence off (flat, penalty 0) | 1 better / 0 worse / 27 tied | −442 m [−1148, 48] | +0.01 [−0.07, 0.12] |
| `silence_range_m` 3 → 6 km | 3 / 0 / 25 | **−427 m [−991, −19]** | +0.02 [−0.00, 0.05] |

**Adopted: `silence_range_m` default 6 km.** Standard gate against 3 km at the same base: Livermore dev
discovered neutral (reported answer −0.04 m [−0.78, 0.69], log +0.01 [−0.00, 0.02]); Oyster Bay −13 m, log
+0.21; global oysterbay_t5 unchanged (60.7 m, margin 30.9). It predicts detection from slant range only and
never looks at the spectrogram. Next: score every audible
unassociated flight on the spectrogram (similar_projects_2026-11-05.md idea 1).

## Ridge probe: other flights on the spectrogram, naive form (negative)

`dopplerprint eval --ridge-probe` (`eval/ridge_probe.rs`, run `runs/ridge_probe_v1`, 27 cases, oracle events):
slide the observer along the truth flight's ground track by v·Δ with the clock shifted by Δ (Δ ±120 s, 10 s
steps). Score the truth flight and every other flight within 8 km, with the clock locked (other flights:
whole recording, f0 50–300 Hz).

- Other flights' summed z peaks within 10 s of truth in **3/27** (chance ≈ 2/25 per case). Spans of 10–90 z
  are noise from maximising over a wide f0 range. Adding them to the score would add noise, not information.
- The truth flight alone peaks within 10 s on this straight line in 19/27, but the line isn't the real ridge.
  In the search, a cross-track nudge plus the ±5 s clock sweep keeps it flat. N456CS_R38 (right flight,
  8.6 km off): best z 22.95 against 21.66 at truth, a gap of 1.3 z.

Per flight, there is nothing for tighter f0 priors to recover. Of 99 other flights within 8 km, 6 peak within
10 s of truth (chance ≈ 12); only one has a clear peak (N748SP_R36 a26381, +32.8 z over its median). The
nearby traffic in these clips is inaudible, so spectrogram evidence from other flights (similar_projects
idea 1) cannot break the Livermore ridge. Only absence (the silence term) and a better clock can. Idea 1
is dropped for single-pass clips; it may still help long recordings with faint, undetected passes.

## One searched event per pass (`events::SAME_PASS_S`, adopted)

Discovered-vs-oracle losses at `select`/`assoc` had one shared cause. Searched events whose CPAs arrive
within 10 s of each other are the same aircraft in all 4 Livermore dev pairs: N394FX 355 + 846 Hz,
N500MG 113 + 53 Hz (2:1), N61NG 149 + 148 Hz, N814SR 66 + 61 Hz. Searching both forces a second flight.
`select` now keeps only the strongest (SNR) of such events.

| gate (vs 6 km silence base) | result |
|---|---|
| app scenario (10 km, off-centre ±120 s) | **log +1.03 [0.21, 2.05]**, truth-cell rank −598 [−1204, −128]; right flight now in all 4 changed cases (N394FX 7229 → 905 m, N500MG 8466 → 1362 m, N61NG 8490 → 8254 m, N814SR 1872 → 6872 m: was the wrong flight nearby, now the right one, 6.9 km along its track) |
| standard Livermore dev | 1 better / 0 worse (log +0.34 [0.00, 1.02]) |
| Oyster Bay discovered, global oysterbay_t5 | unchanged |

## Fitted d too small: manoeuvres, and two negative fixes

On labelled discovered events, d error follows v error squared (N451EE v −0.50 / d −0.75, N231DT −0.51 /
−0.85, N913TC −0.35 / −0.65). κ = v²/d stays right (N451EE fit 6.26, truth 6.32); v and d slide along it.
The truth KML tracks (scratchpad `manoeuvre.py`, ±30 s around CPA) show manoeuvres on the bad fits:

- N451EE: 82° climbing turn.
- N2933A: 136° turn.
- N3012N: circling.
- N231DT: ground speed 46–76 m/s.
- N413AK: ground speed 73–108 m/s.

The good fits (N8085W, N784MB, N9393T, N795MM) were steady to 1–2 m/s and < 1° of heading. N60RS is an
exception: ground speed 61–91 m/s, yet a good fit. The CV model is wrong for pattern traffic, not
under-determined.

- **Profile-likelihood v interval (reverted).** Stepping v and refitting the rest never widened the bad
  passes' bounds: the cost rises past k = 3 within one 10 % step, so the data prefers the wrong curve. It
  did widen spurious events to v 0–238 m/s.
- **Wider floors `v_floor` 0.2 → 0.5, `d_floor` 1 → 3, κ unchanged (not adopted).** App scenario:
  truth-cell rank +110 [4, 307], reported answer 2 better / 5 worse. Standard eval: neutral to worse.
  Association already admits the true flight (assoc 24/26 either way): the gates are soft, so a d/v miss
  costs a little, not the flight. Wider bounds only let more wrong flights compete.

## Levers that add information

- A second pass, or a faint one (the clips are 1–3 min).
- Negative evidence along the ridge: flights that should have been heard from each point on it (the
  regional silence term already applies this; how much it separates points along a ridge is untested).
- A tighter start: file timestamps, or a known event in the audio.
