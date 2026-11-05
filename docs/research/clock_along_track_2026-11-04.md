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

## Levers that add information

- A second pass, or a faint one (the clips are 1–3 min).
- Negative evidence along the ridge: flights that should have been heard from each point on it (the
  regional silence term already applies this; how much it separates points along a ridge is untested).
- A tighter start: file timestamps, or a known event in the audio.
