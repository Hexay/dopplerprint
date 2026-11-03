# Mains-hum (ENF) detector: corpus results and threshold choices (2026-11-03)

The detector is `dp_core::audio::hum`, described in docs/AUDIO.md under "Mains hum". To regenerate the tables, run `dopplerprint hum <file> --json` on each `data/cases/*/audio/*` file and on `sim` renders with and without `[noise] hum_*`. No grid or FNET data is used.

## Real corpus (66 files)

| Case | n | Hum present | Grid | Lines (k:dB) | Detectable | Rate check | Discontinuities |
|---|---|---|---|---|---|---|---|
| Oyster Bay (colsto, 3600 s, 48 kHz) | 1 | yes, `ambiguous` | 60 | 6:7 (360 Hz only) | 37% | ok, implied 48001 ± 16 Hz (−19 ppm) | none |
| Livermore N413AK_R49 | 1 | yes, `ambiguous` | 60 | 6:8 (360 Hz only) | 82% | ok, implied 47996 ± 16 Hz | none |
| Livermore, the other 39 | 39 | no | – | – | – | unknown | – |
| NASA UAV (mic2.wav, 20 kHz) | 16 | no | – | – | – | unknown | – |
| Seattle xc941260 | 1 | no | – | – | – | unknown | – |
| DOPPLERGEIST multievent 3/4/6 (wav and mp3) | 6 | yes | 60 | 1:23–24 | 100% | ok, −5 ppm | none |
| DOPPLERGEIST synthetic minute / two events | 2 | yes | 60 | 1:14 3:11 | 63% | ok | none |

Notes on the table:

- **Livermore rejections.** 19 had no candidate line at all. 20 had a candidate detectable in only 0–16% of frames: stray stationary tones, mostly single high harmonics at off-nominal ratios.
- **N759NC_R46** produced a two-line 50 Hz candidate (350.7 Hz and 501 Hz) that was detectable in only 14% of frames. It was rejected.
- **NASA 065/066** show a stable 89.97 Hz tone (40–71% of frames). It would only fit a 45 Hz grid through the 0.75 rate ratio, and it is a k=2 line, so the single-line rule rejects it. Without that rule these two files would be false "mislabelled" alarms.
- **Codec survival.** MP3 versions of the DOPPLERGEIST files give the same verdict as the WAVs, with line SNR 1 dB lower.
- **Real mislabel check.** `dg_multievent_3` relabelled from 8000 to 7350 Hz (i.e. ×44.1/48) is reported as `mislabelled`, implied 8000 ± 3 Hz, `likely_sr` 8000, and `ambiguous` because the rate claim rests on the fundamental alone.

## Sim (aircraft-only false-positive rate)

Each scenario in `experiments/sim/*.toml` was rendered with seeds 1–4, with and without hum.

| Renders | Present |
|---|---|
| hum removed (12) | **0 / 12** (false-positive rate 0) |
| two_props_920s with hum at −24 dB (4) | 4 / 4, ok. Three of them pass at 30–32% frame detectability, so they are borderline. |
| faint_single with hum at −12 dB behind a 120 Hz high-pass (4) | 0 / 4. The hum sits about 0 dB re noise in 1 Hz; 60 Hz loses 12 dB to the high-pass and 180 Hz is 6 dB down. Below threshold at 60 s. |
| manoeuvring_turn (no hum in the scenario) (4) | 0 / 4 |

## Thresholds and why

| Parameter | Value | Reason |
|---|---|---|
| `min_line_snr_db` | 6 dB over the ±4 Hz median floor (Welch, 4 s) | Noise peak/median with M ≥ 10 averaged segments is ≪ 6 dB. Lower values admit the Livermore stray tones. |
| `frame_snr_db` | 10 dB | With the ±0.12·k Hz window (about 3–5 independent bins), noise exceeds 10 dB over the median in about 0.5% of frames. At 6 dB it would be about 30%, the same as `min_detect_frac`. |
| `min_detect_frac` | 0.3 | Separates real hum (≥ 30%, typically ≥ 60%) from stray tones (≤ 16% in the corpus). Weak hum gets a second pass with 8 s frames. |
| `max_wander_hz` | 0.1 | Grid std over a recording is ≤ 0.05 Hz. Observed: DG 0.024, Oyster Bay 0.019, sim 0.05. |
| Single line, r ≠ 1 | rejected unless k = 1, ≥ 80% detectable and wander ≤ 0.05 | A lone tone falls in some (nominal, ratio) window by chance. NASA 065/066 are exactly this case. |
| `rate_tol` | 0.2% (or 3σ if larger) | Combines crystal error (≤ 1000 ppm) and grid mean deviation (σ 0.02 Hz ≈ 330 ppm). 44.1/48 is 8.8%, so it is unambiguous. |
| Fine search | ±0.4% around nominal·r | Covers clock and grid error. The windows of all (nominal, r) hypotheses stay disjoint. |
| `drift_tol_hz` | 0.1 Hz | Above any plausible grid trend within one recording. |
| Phase jump | ≥ max(8σ, 0.05 cycles); both 0.5 s sides coherent (\|mean z\| ≥ 0.6·median) | The first version used unwrap + line fit and produced ±½-cycle false jumps on DOPPLERGEIST (3–7 per file) and Oyster Bay. The cause was 2π slips and noise-only windows. The coherent-sum estimator with a coherence gate gives zero on the corpus. |
| Dropout | silence < 1% median RMS, or hum < ½ median and > 6σ for ≤ 5 s; not at either end of the file | Trailing silence at the end of Oyster Bay is padding, not a gap. |

## Measured accuracy (unit tests, synthetic)

- **Trace.** 60 Hz with ±0.02 Hz wander, three harmonics, 2 s frames.
  - At moderate SNR (about 15 dB per frame): rms 0.01–0.025 Hz, close to the CRB.
  - At high SNR: rms < 0.003 Hz.
  - Bias < 0.2·rms. The long-run mean is within 0.005 Hz of 60.
- **Mislabel.** 44.1↔48 kHz in both directions (60 and 50 Hz grids): implied rate within 0.1%, and `likely_sr` is correct.
- **Clock offset.** A 0.3% offset is reported as `clock_offset`, not as a mislabel.
- **Gaps and cuts.**
  - 0.5 s zero-fill: detected as a `dropout` within ±0.15 s. Its phase step is invisible, because 0.5 s is a whole number of cycles at both 50 and 60 Hz.
  - Removing 18.75 cycles: detected as a `phase_jump` of ¼ cycle at the right time.
- **Noise only.** Noise plus a sweeping tone, 5 seeds: never reported as present.

## Caveats

- Outdoors, hum is rare. 2 of 57 field recordings have any hum, and both show only the 360 Hz line, which is plausibly transformer magnetostriction. The self-check will usually report `unknown`, and that must not be read as "rate verified".
- A single-line detection is always `ambiguous`. The 360 Hz line could also be a 45 Hz grid × 8 or a machine tone.
