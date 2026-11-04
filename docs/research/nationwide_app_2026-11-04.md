# `dopplerprint locate` without `--near`: discovered events (2026-11-04)

The app runs `search::global::locate_global` when `--near` is left out (`app/nationwide.rs`), then the regional
locate around the answer. Every earlier global result (global_search_2026-11-03.md) used **oracle** events
(truth-fitted). This is the first test on events the app discovers itself.

## Oyster Bay, discovered events

`experiments/global/oysterbay_discovered_t{5,30,120}.toml` (eval, `oracle_events = false`) and the CLI offline
against `data/` (`--start 2026-09-08T20:17:30Z`).

Discovery finds 54 events in the hour, 4 of them real passes. `events::select` keeps the 4 strongest: both
labelled passes, one ground source (d 130 m, d range ×10⁴) and one other spurious pass. With all four voting,
the ±120 s run gave a confident wrong answer 3 928 km away (margin 4.66 > 3).

| change | ±5 s | ±120 s |
|---|---|---|
| none | – | wrong, 3 928 km, margin 4.66 |
| vote only informative events (d_max ≥ 300 m, d ratio ≤ 30, v ratio ≤ 5) | wrong (Denver), margin 11.2 | ambiguous, margin 1.7 |
| + f0 floor 1 % → 3 % (reverted: regional and nationwide unchanged) | wrong, margin 8.6 | ambiguous, margin 0.6 |
| + nationwide decoy check (CLI) | Denver passes both decoys | **needs `--near`** (decoy at 74 min +6.3) |
| + two-pass rule (CLI) | **inconclusive** (1 pass matched) | needs `--near` |

±30 s: truth proposal #25 of 148, not searched; wrong (Denver), margin 9.6.

Why truth loses: the truth proposal has the true flights (a1dfeb, ace391), but the regional search scores it
z 60.1 against 99.5 with oracle events. Discovered event 1 has f0 37.6 Hz (oracle 106–119: a ⅓ subharmonic
label) and event 2 134–137 Hz (oracle 118–134, +7 %). Chance flight pairs somewhere in CONUS reach z 60–71.

## Guards in the app

- `GlobalParams::{min_event_d_m, max_event_d_ratio, max_event_v_ratio}`: events that place nothing don't vote.
  Oracle runs (Oyster Bay ±5 s, sim_t5) are unchanged.
- Nationwide decoy: the same `locate_global` with the start window shifted (`decoy::shifts`, ~±70 min). The
  regional decoy only replays one area, while a nationwide match is the best of ~13 k tracks.
- Two-pass rule: without `--near`, a regional answer explaining < 2 passes is Inconclusive (the policy in
  global_search §Recommended).

Limits: two decoys reject a chance winner only ~⅔ of the time. More shifts cost ~50–60 s each on an hour of
audio. Runtime: 89 s (±5 s, decoys + regional), 109 s (±2 min).

## f0 from the lowest detected harmonic (`EventSpec::from_event`)

Event 1's f0 label is right (37.6 Hz, error 0.3 %), but its lines are harmonics 3–6, so the scorer's comb
(h 1–6, weights 1.0 … 0.4) put its two heaviest teeth on empty bins. The spec now uses the lowest detected
harmonic as f0 (event 1 → 112.8 Hz, where the oracle had it).

| gate | result |
|---|---|
| Livermore dev, discovered (28) | neutral: error +0.2 m [−2.8, 3.3]; truth-cell rank −0.43 [−0.89, −0.07] |
| Oyster Bay regional, discovered | error −16 m, log score +0.8, truth-cell rank −13 |
| Oyster Bay global, oracle | unchanged (78.9 m, margin 17.5) |
| nationwide discovered ±5 s | truth region #4 → **#2**, z 60 → 84.8 (raw 95.9); Denver 88.2 (raw 88.7) |
| nationwide discovered ±120 s | ambiguous (margin 1.9); chance pairs now reach z 97 |

The remaining gap is the association penalty (`score = raw − penalty`): 11 z on the true flights against 0.5
on Denver's pair, because the truth lies outside event 2's d/v bounds (f0 +7 %, CPA slant −62 % vs ADS-B).

## Next

Why event 2's fit is biased (far pass, 2.2 km; harmonics [1, 2, 5, 6] look mislabelled), and whether the
penalty should be softer for events with poor fits.
