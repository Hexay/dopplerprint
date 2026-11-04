# Iteration speed: events/global edit → gate (2026-11-04)

Loop: edit `events/*.rs` or `search/global/*` → build dp-cli → `discover` the 1 h Oyster Bay WAV → gate
(clippy, nextest, discover, `global_eval.sh oysterbay_t5`, `livermore_dev_discovered` and
`oysterbay_discovered` evals, `compare`). Base 8ddad78 (f0 floor 8 %). Worktree `-j4`, `RAYON_NUM_THREADS=4`.
The edit flips a one-line body change in `events/fit.rs`. Each step was run under `heavy.lock` with the
lock wait recorded separately.

## Where the 30 min went

| Step (after an events/fit.rs edit) | Before wall / user | After wall / user (2 runs) |
|---|---|---|
| build dp-cli | 94.5 s / 116 s (release: thin LTO, cgu 1, sccache, `CARGO_INCREMENTAL=0`) | 13.0, 16.0 s / 40–45 s (fastrel, incremental) |
| clippy | 3.8 s | 5.4, 2.8 s |
| nextest dp-core (build + run) | 65.3 s / 248 s (non-incremental via sccache) | 32.5, 34.6 s / 104 s (`dev.sh test`) |
| discover Oyster 1 h | 6.2 s / 16 s | 7.8, 7.3 s / 19 s |
| global `oysterbay_t5` | 76.1 s / 292 s (locate 67.5 s) | 45.1, 40.4 s / 144–155 s (locate 34.5 s) |
| eval `livermore_dev_discovered` | 80.6 s / 279 s | 57.5, 50.0 s / 157–168 s |
| eval `oysterbay_discovered` | 30.9 s / 106 s | 22.9, 21.2 s / 76–79 s |
| **sum of steps** | **357 s** | **~178 s; `tools/gate.sh` end to end 162 s** |
| `heavy.lock` waits, same session | 3260 s, 639 s, 89 s, 89 s, 23 s | 0 s (quiet period) |

Most of the observed 30 min was lock waits, not work. The main session's jobs and this one queue on one
`heavy.lock`, so wall time is set by everyone's heavy steps. Shorter steps also shorten everyone's waits.
Load was 13–31 for the "before" runs and lower for the "after" runs, so compare user CPU as well as wall.

Discovery was the suspect and isn't the cost: 6–8 s per 1 h discover (decode 2 s), 4.4 s inside the Oyster
eval, ~11 s across the 28 Livermore dev cases. `perf` on the Oyster eval put 70 % of samples in
`search::sweep::sweep_one`. That is the f0 sweep, 8× wider since 8ddad78.

## Changes

1. **Sweep (exact).** At `f0_step_bins = 0.25` the fixed-point step is 2^30, so steps s, s+4, … share one
   lerp fraction and read the row contiguously. The accumulator is laid out by s mod 4. `fixed::term_select`
   replaces the scalar `as i32` saturation (ucomiss/cmov, which blocked vectorisation) with selects. The
   body is also compiled for AVX2 and picked at run time. The f32 operations and exact i64 sums are the
   same, so the integers are the same and the CUDA kernel is untouched. On the Oyster eval (perf stat,
   2 runs each): 839/827 → 397/399 G user cycles and 2938 → 1167 G instructions. The sweep alone went from
   ~580 to ~205 G cycles.
2. **One fastrel build for the whole gate.** `tools/gate.sh` builds once via `dev.sh build` (fastrel,
   incremental, no sccache; see iteration_loop_2026-11-03.md), and lint uses `dev.sh check/test`.
   `global_eval.sh` no longer runs `cargo run --release` (a second LTO build target) and no longer hides
   eval failures behind its `grep`.
3. **Stage subsets.** `tools/gate.sh events` is about 21 s (build plus discover). `tools/gate.sh global`
   is about 55 s.

## Exactness

- Unit tests: `sweep_tests::layout_sweep_matches_strided_reference` checks the old loop against the new
  one on random planes, with NaN, saturation, partial rows, NONE frames and steps 0.125/0.25/0.3/1.0.
  `fixed::term_select_equals_term` covers edge values, 4 M random bit patterns and a dense ±130 range.
- Eval JSON compared with `tools/eval_same.sh`, timing ignored, against the pre-change release build:
  `oysterbay_discovered` (with oracle/ and discovery/), `livermore_dev_discovered` (86 files) and global
  `oysterbay_t5` are identical. The fastrel discover JSON is byte-identical (`cmp`) to release.
  `gate.sh -b runs/b_` compare shows 0 deltas on every metric except runtime.
- Rebased onto dff4fda: fastrel with main's sweep vs this one, `oysterbay_discovered` and global
  `oysterbay_t5` are identical. Wall/user went 81.0/170 → 22.4/80 s and 84.5/322 → 38.3/139 s, both
  runs back to back under the same load. 216 tests pass and clippy is clean.

## Fast loop

```
tools/gate.sh events                  # edit events/*: build + Oyster discover, events near truth CPAs (~21 s)
tools/gate.sh global                  # edit search/global/*: build + oysterbay_t5 (~55 s)
tools/gate.sh -b runs/<base>_         # full gate vs a baseline (~160 s)
DP_LOCK=<lock> tools/gate.sh ...      # take a shared flock around each heavy step
tools/eval_same.sh runs/A runs/B      # exactness check for perf changes
```

Re-measure with a step timer around each command after flipping a one-line edit (as above), and use
`perf stat -e cycles:u,instructions:u` when the box is loaded.

## Measured, not changed

- **Discovery cache** (events keyed by audio + params + code): it saves ≤ 30 s per full gate, and only
  when editing outside `events/`, `audio/` and `physics/`. A missed code dependency would make it silently
  stale. Not done.
- **Sweep, further:** the vector loop takes ~60 % of sweep samples, the scalar tail of each ~22-step phase
  run ~20 %, and per-run setup ~20 %. i32 partial sums across harmonics or 8-wide f32 would need a
  value-range guard to stay exact (term saturation). Left alone.
- **Front end in evals:** after the sweep change, STFT/whiten (`rayon` chunk helper ~15 %), `IfRefiner`
  ~9 % and median selection ~4 % of the Oyster eval. These are shared with discovery.
- **Nationwide CLI `locate`** (940–1200 s per the main session) is sweep-bound, so expect about 2× less.
  Not re-measured here.
- **Scoring plane:** 0.4–0.6 s per case. Not worth caching.
