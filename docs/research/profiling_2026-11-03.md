# Profiling, 2026-11-03

Release profile (`debug = "line-tables-only"`), 16-core devbox, `-j4` builds. Tools: `/usr/bin/time -v`,
`perf stat`/`perf record` (paranoid = 1, user space only), heaptrack. DWARF call graphs gave no caller chains
on this box, so hot spots come from flat samples with `--inline`, plus temporary stage timers.

**Wall-time caveat.** From ~10:15 the box load average was 50–95 (other sessions), so later wall times are
starved. Compare CPU-s and instruction counts; the "before" walls and the 8.3 s / 8.4 s / 17.9 s walls are
from quiet periods.

## Summary

| Area | Before wall / CPU-s / RSS | After wall / CPU-s / RSS | Outputs |
|---|---|---|---|
| 1. `discover` Oyster Bay 1 h | 245.0 s / 249 / 220 MB | 8.3 s / 11.5 / 222 MB | JSON byte-identical (also livermore_N67WL_R59) |
| 2. `two_event_search_recovers_observer` | 20.9 s / 317 / 25 MB | 8.4 s / 109 (commit 1); 74 CPU-s after commit 2 | ranking JSON byte-identical |
| 3. Oyster Bay parity run (±5 s prior) | 34.5 s / 443 / 0.88 GB | 17.9 s / 157 (commit 1); 117 CPU-s after commit 2 | case JSON identical minus `runtime_s` |
| 4. ADS-B ingest 2026-05-09 | 148 s* / 157 / 2.48 GB | 99 s* / 136 / 2.48 GB | 266 store files byte-identical |

\* already under load. DOPPLERGEIST on job 3: 18.4–19.4 s, ~21 CPU-s, 0.87 GB.

| Commit | Change |
|---|---|
| search: frame-outer f0 sweep | hoist `bpo·log2 h`, truncating lerp (no libm `floor`), rows read once per clock step |
| events: cache stitch pair fits | pair results cached and recomputed only for the merged fragment; per-fragment fits cached |
| search: restrict the f0 sweep | in-row steps are contiguous (monotone positions): two binary searches, branch-free body |
| adsb: FxHash | coverage / neighbour-offset bin maps |

## 1. Event discovery (1 h, 48 kHz stereo)

| | Before | After |
|---|---|---|
| wall / user | 245.0 s / 249.4 s (101 % CPU) | 8.29 s / 11.5 s (144 %) |
| instructions / cycles | 3294 G / 1331 G | 141 G / 56 G |
| peak RSS | 220 MB | 222 MB |

Stage timers (before, one run under load): peel 0.8 s, split 0.6 s, merge 0.01 s, **stitch 373 s**, merge +
finalize 0.15 s. `stitch` re-ran `stitch_fit` (two single CV fits plus up to seven joint fits) for all n² pairs
(541 fragments) after every join or rejection, serially.

| Hot (before) | % | Hot (after) | % |
|---|---|---|---|
| `fit::Problem::normal` | 48.8 | `fit::Problem::normal` | 20.7 |
| `fit_cv_with` residual closure | 26.2 | `select_nth_unstable` f32 (whiten medians) | 12.1 |
| `linalg::solve4` | 11.6 | `fit_cv_with` residual closure | 11.4 |
| `select_nth_unstable` f64 | 5.2 | `resample::Decimator::push` | 9.2 |
| `stats::median_abs` | 4.4 | `track::Tracker::run_from` | 6.5 |

Heap (after, heaptrack): 4.56 M allocation calls (3.22 M temporary), peak heap 243 MB = spectrogram 125 MB +
decimated audio 86 MB + rest. Top call site by count: `Vec` growth in `fit_cv_with` (LM residual vectors),
4.2 M calls.

Tried and dropped: vectorising the Viterbi row (`Tracker::run_from`) – it was < 1 % before the stitch fix.

## 2/3. Search scoring (test and parity run)

Before, both spent ~99 % in `score::best_f0` (inlined into `score_joint`): `__log2_fma` 29 % (`bpo·log2 h`
recomputed per lerp), libm `floor` ~30 %, the rest the lerp. Per hypothesis the work is clock steps (41) × f0
steps (~69) × frames (~1060) × harmonics (6) ≈ 18 M lerps; 1874 hypotheses over 4321 cells.

| | Test before | after c1 | after c2 | Parity before | after c1 | after c2 |
|---|---|---|---|---|---|---|
| CPU-s | 317 | 109 | 74 | 443 | 157 | 117 |
| instructions | 3171 G | 1169 G | 842 G | 4414 G | 1614 G | 1340 G |
| peak RSS | 25 MB | 26 MB | 26 MB | 876 MB | 876 MB | 875 MB |

After: `score_joint` is still 95 % of the parity run (scalar f64→int conversions plus two gathers per lerp). The
non-search part (decode, spectrogram, ADS-B load) is 4.3 CPU-s (same run with a 1 m region).
Heap (parity run): peak 806 MB, of which **691 MB is the full-rate 48 kHz decode** in `eval::case::pose_real`
(`audio::decode`); discover uses the band-limited `decode_band` (86 MB).

Tried and dropped: an AVX2 multiversioned kernel (with and without unchecked indexing) – no change, because
LLVM keeps f64→i64 scalar without AVX-512DQ.

## 4. ADS-B ingest (adsb.lol 2026-05-09, 3.2 GB tar)

Phases (before): stream 124 s, points finalize 12.7 s, segments 7.8 s, coverage 3.0 s. The main (absorb)
thread is 21 % of all samples; the parse batch and the serial absorb alternate instead of overlapping.

| Hot (before) | % |
|---|---|
| serde_json parse (sum of parse/ignore/seq frames) | ~15 |
| `miniz_oxide` inflate | 9.3 |
| zstd compress (finalize workers) | 6.7 |
| parquet Int32 encoders | ~5 |
| SipHash on `CovKey` (absorb thread) | 4.8 |
| `memmove` / `malloc` | 4.9 / 2.8 |

After FxHash: cycles 764 G → 665 G, instructions 1469 G → 1429 G. Not heap-profiled: tens of millions of
allocations under heaptrack on a loaded box.

## Next

- Parity-run RSS: decode band-limited in `pose_real` (`decode_band` at `cfg.spec.f_max`). The decimator is
  chunk-invariant, so the spectrogram should be unchanged, but `Audio::duration_s` changes when the sample count
  is not a multiple of q. Keep the container duration. Expected RSS 0.88 → ~0.2 GB.
- Search CPU: still ~18 M lerps per hypothesis. Further exact gains need fewer evaluations, not faster ones:
  cache `prepare()` per (event, track) across hypotheses that share a clock, or bound-and-skip clock steps
  whose upper bound can't beat the cell's best (this needs a proof that `ranked` is unchanged).
- Ingest: pipeline the absorb with the next batch's parse (a writer thread fed in order). The output is then
  unchanged and the serial absorb is hidden.
- Discover: reuse residual buffers in `fit::Problem::lm` (4.2 M temporary allocations).
