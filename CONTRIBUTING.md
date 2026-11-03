# Contributing

Issues and pull requests are welcome. For anything bigger than a fix, open an issue first so the approach
can be agreed before you write it.

## Setup and the dev loop

Everything goes through `tools/dev.sh`:

| Command | Does |
|---|---|
| `tools/dev.sh check` | clippy on the whole workspace (CI runs it with `-D warnings`) |
| `tools/dev.sh test [filter]` | dp-core tests via [cargo-nextest](https://nexte.st) |
| `tools/dev.sh py` | `maturin develop` the `dopplerprint` package into `.venv` |
| `tools/dev.sh eval [experiment.toml]` | corpus + sim evaluation → `runs/<ts>/summary.md` |

`eval` on real cases needs the corpus (`data/MANIFEST.md`, `tools/fetch_corpus.py`); the sim suite and all
tests run without it. A change that claims an accuracy or speed gain should come with an
`experiments/<name>.toml` and the before/after `summary.md` numbers in the PR.

## Rules the code holds to

- All computation lives in `dp-core`. `dp-cli` and `dp-py` are thin wrappers; Python does prep,
  orchestration and display only.
- One module, one responsibility; keep files under 300 lines.
- Time: ADS-B is UNIX seconds UTC (`f64`); spectrogram time is audio-relative seconds. Never mix them
  without an explicit clock offset.
- Heights: ADS-B geometric is ellipsoidal, the DEM is MSL; convert with `dem::geoid_undulation`. Never mix
  in barometric altitude silently.
- `sim` is the independent truth: it must not reuse `physics::doppler_factors`.
- Jets are flagged, never dropped.
- Files under `data/` are untrusted input (run Python on them with `python3 -I`); nothing under `data/` is
  committed except `data/MANIFEST.md`.
- Comments only for non-obvious logic, gotchas and TODOs.

## Releases

Bump `version` in the root `Cargo.toml` (the Python package takes its version from it), merge, then push a
tag `vX.Y.Z`. The release workflow builds abi3 wheels and an sdist, publishes them to PyPI and attaches
them to a GitHub release.

One-time PyPI setup (maintainer): on pypi.org add a pending trusted publisher for project `dopplerprint`,
owner `Hexay`, repository `dopplerprint`, workflow `release.yml`, environment `pypi`; and create the `pypi`
environment under the GitHub repo's Settings → Environments.

By contributing you agree your work is released under the MIT licence (`LICENSE`).
