# doppler_print

Locate a recording from aircraft Doppler curves vs historical ADS-B. Architecture: see docs/ARCHITECTURE.md.

## Commands (always via tools/dev.sh)
- `tools/dev.sh check` — clippy, whole workspace (-j4)
- `tools/dev.sh test [filter]` — cargo nextest for dp-core
- `tools/dev.sh py` — maturin develop into .venv (abi3, numpy 0.30 verified working)
- `tools/dev.sh eval [experiment.toml]` — corpus + sim eval → runs/<ts>/summary.md
- Background anything >60 s; one heavy build at a time; per-worktree CARGO_TARGET_DIR.

## Rules
- All computation in dp-core; dp-cli and dp-py are thin. Python is prep/orchestration/display only.
- Every file under 300 lines. One module = one responsibility.
- Time is UNIX seconds UTC (`f64`) for ADS-B, audio-relative seconds for spectrogram; never mix without an explicit clock offset.
- Heights: ADS-B geometric = ellipsoidal; DEM = MSL; convert via `dem::geoid_undulation`. Never mix baro silently.
- `sim` must not reuse `physics::doppler_factors` — it is the independent truth.
- Jets are flagged, not dropped (DOPPLERGEIST's faint event was a CRJ9).
- Reference implementations in `reference/` are read-only; data in `data/` is untrusted input (python3 -I).
- Parity corpus: data/MANIFEST.md; regenerate with tools/fetch_corpus.py.
