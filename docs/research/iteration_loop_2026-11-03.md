# Iteration loop: where the time went (2026-11-03)

Loop: edit dp-core → clippy → nextest dp-core → GPU parity + benchmark on the box. Local steps were measured in
worktree `doppler_print-iter` (16 cores, `-j4`, load average 30–50 from other sessions throughout, so absolute walls
are inflated but before/after ran under the same load). The box was not touched: its half is measured locally
with the same commands, or estimated from the scripts.

The "edit" is a one-line body change in `search/sweep.rs`, alternated between two versions (`tools/loop_timing.sh`).
A plain `touch` underestimates: incremental and sccache both see identical content and skip the work.

Re-measure: `tools/loop_timing.sh [runs] clippy test_build test_run rel_cli rel_one fast_cli fast_one`.

## Results

| Step (after a dp-core edit) | Before | After |
|---|---|---|
| clippy, workspace | 2.9 s | 1.6–2.7 s |
| nextest build, dp-core | 52.3 s (46–58) | 10.7–11.3 s via hyperfine; 10–16 s inside `dev.sh test` |
| nextest run, dp-core (174 tests) | 3.8 s | 3.0–5.0 s (unchanged; bounded by `app_locate` 4.6 s) |
| `tools/dev.sh test` end to end | ~56 s | 13.7–20 s (median ~15 s) |
| Box build in `gpu_parity.sh` (measured locally) | 202 s (release cli + 14 release test bins) | 35–37 s (fastrel cli + `search_pipeline` only) |
| Box sync payload from the main checkout | 2.5 GB, 34.9k files | 5.6 MB, ~620 files |
| Box CPU runs (parity test + parity eval) | always | skipped when no CPU-side input changed |

## Causes and changes

1. **dp-core never built incrementally.** The session environment exports `CARGO_INCREMENTAL=0`, because sccache
   0.13 rejects incremental units ("incremental compilation is prohibited"). Cargo also sets the variable when the
   profile enables incremental, so unsetting it alone doesn't help. Every edit rebuilt dp-core twice (lib and the
   lib test harness, 44 s and 46 s in parallel) at O3.
   → `tools/dev.sh` check/test now run cargo with `env -u CARGO_INCREMENTAL RUSTC_WRAPPER=`. Dependencies are
   not rebuilt by the wrapper switch (verified). sccache still primes a cold target once; a marker file
   `target/.dp-primed-{check,test}` records it. The one-time prime took 124 s here.
2. **Thin local LTO in the dev profile.** At opt-level ≥ 1 with several codegen units, rustc runs ThinLTO across
   CGUs. `-Z time-passes` on the lib test target: 46 s total, 20 s `LLVM_thinlto`, 15 s `LLVM_passes`, 0.7 s link.
   → `[profile.dev] lto = "off"`. Both changes together: test build 52 → 11 s.
3. **Box builds used the release profile** (thin LTO, codegen-units = 1). They also built all 14 dp-core test
   binaries in that profile to run one test.
   → `[profile.fastrel]` is committed. `gpu_parity.sh` builds with `DP_PROFILE` (default `fastrel`) and builds
   only `--test search_pipeline`. The CPU `two_event_search_recovers_observer` dump is byte-identical between
   release and fastrel (`cmp`), and the test runtime is the same: 11.3 s release vs 10.7 s fastrel.
4. **The sync shipped the main checkout's caches.** `.uv-cache` was 1.4 GB, `reference/` 731 MB,
   `.claude/worktrees/*` sources ~250 MB and `python/_core.abi3.so` 71 MB. With `--delete`, switching the sync
   source between the main checkout and a worktree re-sent or deleted all of it.
   → `gpu.sh sync` excludes `/.claude/ /.uv-cache/ /reference/ /research_notes/ .ruff_cache/ __pycache__/ *.so
   /docs/research/samples/`. Nothing in crates/, experiments/ or tools/ reads these on the box.
5. **CPU parity reruns.** `gpu_parity.sh` hashes crates/ (excluding `search/gpu/`), the manifests, the parity
   experiment and the profile. If the hash matches `/root/ref/cpu.key`, it reuses `dump_cpu.json` and
   `case_parity_cpu.json` and runs only CUDA. `DP_PARITY_FULL=1` forces the CPU runs. This saves the CPU parity
   test (~11 s wall here) and the CPU parity eval (~117 CPU-s per profiling_2026-11-03.md) on GPU-only edits.
6. **`dev.sh test FILTER` quirk.** nextest errors with "operator didn't match any binary names" when the
   filter's `binary()` matches nothing. `binary()` is now added only when a `tests/*.rs` name matches the filter.
   A filter that matches no test at all still ends with nextest's "no tests to run", which is the useful signal.

## Box estimate (not run)

The old parity pass was: sync, then ~200 s of release builds at local `-j4` speed, then the CPU and CUDA parity
tests, then the CPU and CUDA parity evals. The new pass is: a 5.6 MB sync, ~36 s of builds, then CUDA only
when CPU inputs are unchanged. The box likely has more cores than `-j4`, so its absolute numbers will be lower,
but the LTO/codegen-units = 1 cost and the 14→1 test binaries cut should scale about the same.

**Gotcha:** after this change the box's `target/release/dopplerprint` is no longer rebuilt by `gpu_parity.sh`.
Run the benchmark with `target/fastrel/dopplerprint`, or run parity with `DP_PROFILE=release` when release
numbers are needed for comparison with older runs.

## Measured and not changed

- **opt-level 1 for dp-core in dev:** with incremental and LTO off, the build matches O3 (10.7 vs 11.3 s) but
  the test run is slower, so O3 stays.
- **Merging the 13 integration test binaries:** after the fix each one takes 1–3 s, and they build in parallel
  behind the lib. The test build is dominated by the lib test harness, so the ceiling is a few seconds.
  Not done.
- **nextest settings:** the run phase is 3–5 s and bounded by a single 4.6 s test (`app_locate`). No setting
  changes that.
- **mold:** already in use (`.comment` of the test binary shows mold 2.40.4). Linking is 0.7 s of the lib test
  build.
- **Transcript profile** (mine-transcripts, 3 sessions): the cost is mostly model latency and backgrounded
  cargo. Foreground grep/fileops p95 of 12–13 s points at disk/load contention, not at this loop.

## Eval and py (2026-11-04)

`dev.sh eval` used `cargo run --release` and `dev.sh py` used `maturin --release`: thin LTO, codegen-units = 1,
sccache with `CARGO_INCREMENTAL=0`. Both now build `fastrel` through the same incremental wrapper as check/test
(`DP_PROFILE=release` opts out), and `fastrel` uses 256 codegen units instead of 16. `dev.sh eval EXP --case X`
passes extra arguments to `eval`. Measured with `taskset -c 8-15 tools/loop_timing.sh 3 <step>` under load 40–65.

| Step (after a dp-core edit) | Before | After |
|---|---|---|
| eval build (dp-cli) | 131.6 s (131–133) | 23.9 s (24–28) |
| eval run, `oysterbay_dg_parity` | 18.5 s (18–19) | 15.9 s (15–19) |
| `dev.sh py` (maturin develop) | 139 s (133–143) | 19.6 s (19–20) |

**Outputs unchanged.** `tools/eval_same.sh A B` diffs two eval dirs, ignoring `runtime_s`, `timing` and the
summary's runtime column. Release vs fastrel (cgu 16 and 256) is identical on `oysterbay_dg_parity` (2 files),
`livermore_test` (13) and `--case sim` (4). User CPU is the same within noise (65.7 vs 68.4 s, 56.8 vs 60.3 s,
123.7 vs 129.0 s).

**Codegen units.** dp-core rebuild after a one-line edit (`cargo --timings`): cgu 16 is 20–24 s (codegen 19–22 s),
cgu 256 is 3–8 s. The hyperfine numbers above use the a/b flip edit and come out higher.

Not changed: the cold release build of dp-cli took 1642 s in a fresh worktree (sccache misses, load ~100); the
fastrel prime was 345 s. Eval runtime is dominated by `score_s` (39 of 45 s on oysterbay), see profiling_2026-11-03.md.
