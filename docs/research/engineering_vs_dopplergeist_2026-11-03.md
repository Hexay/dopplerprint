# Engineering quality: doppler_print vs DOPPLERGEIST (2026-11-03)

Read-only comparison. Theirs = `/srv/agents/work/dg_upstream` (not executed). Counts via grep; no suites run.

## Summary

| Dimension | Verdict | Key evidence |
|---|---|---|
| Test breadth | Theirs: more tests. Ours: richer unit coverage per line | 110 files / 486 `def test_` vs 16 integration files + 66 inline `mod tests` / 220 `#[test]` |
| CI gating | **Theirs ahead** | 3 workflows incl. real-input regression + native-vs-baseline perf; ours clippy+nextest+ruff only |
| Provenance / reproducibility | **Theirs ahead** | `provenance.py` sha256 of inputs/code/config per run; `benchmarks/frozen-manifest.json` with dev/test splits |
| Crash / cancel / resume | **Theirs ahead** on runs; parity on downloads | `checkpoint_resume.py`, `jobs.py` cancellation; ours: `.part` + rename only |
| Caching ADS-B/DEM/indexes | Parity (different shape) | ours `app/cache.rs`, DEM tiles, ADS-B parquet stage→rename; theirs flight-index digests cached (55d97b1) |
| Performance engineering | **We are ahead on raw speed**; they are ahead on measurement discipline | Rust+CUDA bit-identical sweep vs 597 lines of optional C; their 1.96x claims gated on exact-output equality |
| Memory boundedness | Theirs better documented | `docs/DATA_AND_SCALE.md` bounded queues, VmHWM per tile, memory-limit retry; ours unmeasured |
| Field validation | **Theirs well ahead** | scanner-only surveys on AeroSonicDB, YouTube random sample, Risoux, Vireo, Eclipse |

## 1. Test strategy and coverage

- Theirs: `tests/` 110 files, 486 test functions. A distinct family `tests/test_real_*.py` (19 files listed in
  `.github/workflows/real-performance.yml`) runs on recorded real inputs: index columns, track cache, native
  ridge/acoustic parity vs Python fallback, batch-scalar parity, batch ceilings.
- Ours: `crates/dp-core/tests/*.rs` (16 files: adsb_*, physics_*, search_*, sim_chain, app_locate,
  dem_fetch_parity, eval_discovered) plus 66 inline test modules; 220 `#[test]`. Independent-truth `sim`
  (CLAUDE.md rule) is a stronger oracle than anything they have; `tools/gpu_parity.sh` gives bit-identity.
- Gap: we have no "real-recording regression" test that pins the end-to-end answer on a corpus case;
  `app_locate.rs` uses a synthetic fixture (`tests/common/locate_fixture.rs`).
- Verdict: they are broader. Our sim-based tests are stronger as an oracle. Real-input regression is missing on our side.

## 2. CI

- Theirs:
  - `tests.yml`: install matrix, compileall, CLI smoke, packaged assets.
  - `real-performance.yml`: restores public bundles (`scripts/restore_artifacts.py`), freezes real workloads,
    runs real-input regressions, checks the Python fallback separately, benchmarks against a checked-out baseline
    source tree (`--baseline-source`), and uploads failure diagnostics.
  - `checkpoint-replay.yml`: manual, replays a real regional case from checksum-verified release assets
    (`scripts/restore_ci_checkpoint.py` writes a receipt) and uploads evidence even on failure.
- Ours: `.github/workflows/ci.yml` runs clippy `-D warnings`, nextest and ruff E9/F. `release.yml` handles release.
  CUDA parity and evals are manual (`tools/gpu_parity.sh`, `tools/dev.sh eval`).
- Verdict: they are ahead. The cheap thing to adopt is one corpus-case regression job (below).

## 3. Reproducibility and evidence provenance

- Theirs: `src/dopplergeist/provenance.py` hashes the config, inputs and code at run start (sha256), and its
  docstrings say plainly what the hashes do *not* prove. `benchmarks/frozen-manifest.json` freezes
  case IDs with split (development/test) and scope. `benchmarks/truth` vs `truth-separated`. Scanner surveys
  keep the truth-bearing selection outside the scanner's view and join it after scanning.
- Ours: `data/MANIFEST.md` + per-case `SHA256SUMS` + `tools/fetch_corpus.py` (good). Eval writes
  `runs/<ts>/summary.md`, but grep finds no sha256/git-rev in `crates/`, so a run does not record which code,
  config or input bytes produced it. Experiments are TOMLs in `experiments/` with no frozen split file. The
  oracle-event eval issue (memory: eval-uses-oracle-events) is the kind of leak their separation protects against.
- Verdict: they are ahead.

## 4. Crash / cancel / resume

- Theirs: `checkpoint_resume.py` (`compatible_checkpoint_runs(output, run_hash)`, immutable payloads,
  atomic replace, legacy upgrade, 871c57e); `jobs.py` has job cancellation; `parallel_tiles.py` records
  per-tile failure and never marks a partial tile complete (`docs/DATA_AND_SCALE.md` ~l.238).
- Ours: atomic writes and resumable downloads (`app/http.rs` `.part` + resume, `dem/download.rs`,
  `dem/write.rs`, `adsb/ingest.rs` stage dir → rename). There is no checkpoint of search/eval progress and no
  cancellation token. A killed `eval` starts over.
- Verdict: we match them on fetches. They are ahead on long-run resume. This matters less for us because our runs are fast.

## 5. Caching

- Ours: a unified user cache (`app/cache.rs`, XDG layout in docs/USAGE.md#cache), DEM tiles
  (`dem/tiles.rs`, `dem/resolve.rs`), regional ADS-B parquet, and weather (`weather/openmeteo.rs`).
- Theirs: a managed flight-index digest cache (55d97b1), a track cache with isolation tests
  (`test_real_track_cache.py`), an acoustic-screen cache, and a bounded fit cache.
- Verdict: parity. Their cache-correctness tests on real inputs are worth copying as a pattern.

## 6. Performance engineering

Their perf commits from the last 60: a82f4b0 (native canonical ridge validation, columnar track decode),
41d0646 (reuse batch validation), 55d97b1 (index digest cache), 4abd58b (fused native acoustic propagation
`_native_acoustic.c` 214 lines, `_native_ridge.c` 383 lines), a4d27b2 (DEM-affine tile scheduling),
15e49b2 (avoid repeated CPA prep).

Their measurements (`docs/PERFORMANCE.md`): dense real point workload 63.8 → 32.5 s CPU median (1.96x).
Peak RSS is 516 → 498 MiB. HTTP upload end to end improves only 51.2 → 49.5 s (1.035x). Component gains are
1.03–1.40x. The method is sound: same MP3, index and affinity in both arms, profiling kept out of the timed
medians, native use confirmed by counts (228,240 ridge fits), and **no speedup is reported if any exact-output
comparison fails**.

Ours: Rust core with a CUDA sweep that is bit-identical through fixed point (`search/fixed.rs`), plus
`tools/measure.sh`, `loop_timing.sh` and `bench_dopplergeist.sh`. Profiling notes are in
`docs/research/profiling_2026-11-03.md`. We are faster in absolute terms. Our benchmarks are not in CI and do
not compare against a baseline commit with an output-equality gate.

Verdict: our throughput is ahead. Their benchmarking process is ahead.

## 7. Memory boundedness on long recordings

- Theirs: bounded worker queues, CPA-row and candidate caps per worker, per-tile `VmHWM` reporting, and a
  memory-limit retry that cut 1,929 → 614 tracks (`docs/DATA_AND_SCALE.md`). `test_real_batch_ceilings.py` gates this.
- Ours: no RSS measurement or ceiling test found. Spectrogram/frontend memory scales with recording length.
- Verdict: they are ahead, mostly because they measure it.

## 8. Field validation

- Theirs (`benchmarks/`):
  - AeroSonicDB scanner-only survey (`aerosonic-scanner-survey-20261103.md`), seed 20261103, stratified:
    proposals on 51/125 aircraft clips and 0/30 ambient clips. Labels joined only after scanning.
  - YouTube random sample (`youtube-random-sample-20261103.md`): 200 of 287 eligible videos, 24/198 with
    proposals, with a per-category table and a visual review.
  - Risoux, Vireo, Eclipse field screens, site leads and NASA session results.
  - Each report states its limits explicitly ("not a population sensitivity or false-alarm estimate").
- Ours: the parity corpus (`data/MANIFEST.md`), Livermore/Oyster Bay evals and sim chains
  (`experiments/sim_chain_*`). We have no detector-only survey on public audio datasets and no
  false-alarm rate on non-aircraft audio.
- Verdict: they are well ahead. Their 0/30 ambient result is the only false-alarm evidence available for
  either project.

## Cheap adoptions (ranked by value/cost)

1. **Run manifest in `runs/<ts>/`**: write `git rev-parse HEAD`, dirty flag, sha256 of the experiment TOML and
   of each input file (case `SHA256SUMS` already exist). About 50 lines in eval. This closes the provenance gap.
2. **Frozen split file**: `experiments/splits.toml` listing dev/test case IDs. Eval refuses to tune on test.
   Their model is `benchmarks/frozen-manifest.json`.
3. **Detector-only survey**: run our discovery (corpus_discovery harness) on a seeded stratified AeroSonicDB
   sample with labels joined afterwards, and report proposals per class, including false alarms on ambient
   clips. We can reuse their CSV case lists to get a direct comparison.
4. **Corpus regression test in CI**: one small committed case (with redistributable licence, or a sim case)
   that pins the `locate` answer within tolerance. Their model is `tests/test_real_*.py`.
5. **Perf gate with output equality**: in `tools/measure.sh`, compare against a baseline worktree and refuse
   to report a speedup unless the outputs are identical (we already have bit-identity tooling).
6. **Peak RSS in eval summary**: read `VmHWM` from `/proc/self/status` at exit and log it next to wall time.
7. Later, if long recordings show up: checkpoint per-event search results so an interrupted eval resumes.
