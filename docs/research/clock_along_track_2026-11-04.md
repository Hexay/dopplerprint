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

## Levers that add information

- A second pass, or a faint one (the clips are 1–3 min).
- Negative evidence along the ridge: flights that should have been heard from each point on it (the
  regional silence term already applies this; how much it separates points along a ridge is untested).
- A tighter start: file timestamps, or a known event in the audio.
