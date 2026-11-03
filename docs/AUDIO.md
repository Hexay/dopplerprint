# Audio front end and Doppler-event discovery

The code lives in `crates/dp-core/src/audio/` and `crates/dp-core/src/events/`. This file is the main record of why the design is the way it is. Code comments link here instead of repeating it.

## Pipeline

```
decode_band ─► log-f STFT (wins 0.5/1/2 s) ─► whiten ─► combine_max ─► comb image ─► peel Viterbi paths
  ─► analyze (harmonics, fundamental) ─► split_inconsistent ─► merge_related ─► stitch ─► merge_related
  ─► finalize (CV fit + validation) ─► same_source_keep ─► events sorted by t_cpa
```

## Decode (`audio/decode.rs`, `audio/resample.rs`)

- Symphonia decodes mp3, aac/m4a, flac, wav/pcm/adpcm and ogg/vorbis. Gapless decoding is on.
- Channels are averaged to mono f32.
- A corrupt packet is skipped and counted in `AudioInfo::skipped_packets`. A sample-rate change in mid-stream is an error.
- `decode_band(path, f_max)` decimates while it decodes, so a 1 h, 48 kHz file never sits in memory at full rate.
  - The factor `q` is the largest divisor of `sr` with `sr/q ≥ 2.5·f_max`. Because `q` divides `sr`, the reported rate is exact.
  - The filter is a Blackman-windowed sinc FIR, with the stopband starting at `out_sr − f_max`.

## Spectrogram (`audio/stft.rs`, `audio/spectrogram.rs`)

- realfft computes a Hann STFT with frames centred at `k·hop`.
- `nfft` is zero-padded so linear bins are at most 0.25 Hz apart.
- The log-frequency axis has `bins_per_octave` bins (96 by default) from `f_min` to `f_max`. Each log bin takes its value in one of two ways:
  - **Interpolation**: where the log bin is narrower than a linear bin (low frequencies), it interpolates linearly between the two neighbouring linear bins.
  - **Max-pool**: where the log bin spans several linear bins, it takes the maximum of them, so a narrow line keeps its peak height.
- Frames are computed in parallel with rayon. Each thread keeps its own FFT buffers, and memory is dominated by the output image, `n_t × n_f` f32 values. For 1 h at a 0.25 s hop and 542 bins (20 Hz to 1 kHz), that is about 31 MB per window.
- `log_power_spectrogram` gives dB power with no whitening, for plots and level checks.

## Whitening (`audio/whiten.rs`)

Whitening works on log power per bin:

1. **Floor.** A running 0.25-quantile over 60 s, evaluated every 2 s and interpolated, is subtracted from each bin.
   - A median floor of 45 s or less removed the flat asymptotes of slow passes, because a line that stays in one bin for more than half the window *is* the median. The lower quantile with a longer window keeps them.
2. **Scale.** Each bin is divided by a global per-bin MAD.
   - A running MAD inflates wherever an event sits, which suppresses the event.
3. **Frame centring.** The median of each frame is subtracted. This removes broadband transients such as wind gusts and handling noise.
4. **Clip.** Values are clipped to [−3, 8], so one loud line cannot dominate the comb sums.
5. **Mains notch (optional).** With `mains_hz` set, bins within ±`notch_hz` of every multiple of the mains frequency are set to 0.

Every step is relative, so the output does not depend on gain. A test checks that a ×1000 gain changes the output by less than 1e-3.

Stationary tones are removed on purpose. Doppler ridges move, so they survive.

`FrontEnd` computes several windows (0.5, 1 and 2 s by default), whitens each one, takes the element-wise maximum, and standardises the result again. Short windows follow the steep part around CPA; long windows resolve faint, slowly moving asymptotes.

## Comb image (`events/comb.rs`)

Harmonic `h` sits at a fixed log-axis offset `bpo·log2(h)` from the fundamental, whatever the Doppler factor. For each bin `u`, the comb image is:

`C[u] = max(e[u], Σ_{h≤H} e[u+s_h] / √n)`

`e` is clamped to [−2, 6], and each row of `C` is re-standardised per bin. The per-bin step is needed because fewer harmonics fit below `f_max` at high `u`, so the noise level of the sum changes with `u`.

- Using `max(single, sum)` instead of the best subset matters. A best-subset comb lets the tracker hop between lines that belong to different comb teeth.
- A **missing fundamental** still scores, because the sum is taken at `u` whether or not `e[u]` itself is present.

## Tracker (`events/track.rs`)

The tracker runs a local (Smith–Waterman-style) Viterbi over the comb image:

`V[t][u] = (C[t][u] − bias) + max(0, max_Δ V[t−1][u+Δ] − pen(Δ))`

- **Step penalties** come from `pen(Δ)`:
  - holding the same bin costs nothing;
  - each fall costs `lam_down`, plus `lam_steep` for every bin beyond the first;
  - a rise costs `lam_up`.
- **Maximum slope.** `Δ ≤ dmax`, derived from `max_rel_slope`.
- **Why it is local.** A path can start and end anywhere. This prevents the DOPPLERGEIST failure in which a global path is forced across the whole recording.
- **Peeling.** The tracker repeatedly:
  1. backtracks the best path;
  2. cuts it into segments;
  3. floors the harmonic family of every accepted candidate (multiples 1..2H, ±3 bins);
  4. recomputes the affected rows, and the Viterbi from the first changed frame onward. The recompute stops early once the scores stop changing.
- **Segment cuts** (`Path::segments`), so that one path cannot chain consecutive aircraft. A path is cut at:
  - a run of weak frames longer than 3 s;
  - a rise of more than 4 bins within 3 s;
  - a hop: a 3-frame fall much steeper than the local median slope.

## Candidate analysis (`events/candidate.rs`)

For each segment, three hypotheses are tested: the base track is harmonic `m` of the fundamental, for `m = 1..3`.

- **Presence of a harmonic.** For each harmonic `h ≤ 2H`, the on-track whitened power is max-pooled over ±1 bin and compared with two off-track controls. The controls sit `max(3, 0.4·shift((h+1)/h))` bins either side, which keeps them clear of the neighbouring harmonic. The null standard deviation comes from the *difference between the two controls*, not from the on-track samples. Otherwise a strong, flickering line would be penalised for its own amplitude variation.
- **Choosing the hypothesis.**
  - Harmonics with z ≥ `harmonic_z` count as present.
  - The fundamental is the gcd of the present orders. This handles a missing fundamental: if lines 2, 3 and 4 are present, the fundamental is line 1.
  - Each harmonic's z is the mean on-minus-control difference divided by its standard error, using `n / frames_per_indep` independent samples. The candidate score is the Stouffer combination of the present z values.
  - The best hypothesis wins. Ties within ×1.05 go to the smallest `m`.
- **Refined fundamental.** The fundamental track `fund_hz` is refined by parabolic peak interpolation on the strongest harmonic.

## Splitting, merging, stitching (`events/split.rs`, `events/merge.rs`)

- **`split_inconsistent`.** If a candidate's CV fit has an rmse above `split_rmse_rel·f0`, the candidate probably hopped from one aircraft to another.
  - It is split recursively: 16 trial cuts, depth 3.
  - A split is kept only if the combined rmse falls below half the whole-candidate rmse.
- **`merge_related`.** Two overlapping candidates are merged when they are the same source seen on different lines:
  - their log-ratio over the overlap is constant (robust spread ≤ 1.5 bins);
  - the ratio is either a duplicate (within 3%) or a small-integer ratio `a/b` with `a, b ≤ H`.

  The union is re-analysed and accepted only if its score is at least 0.6·max(parts). This is the over-merge guard: a line present in only one part dilutes the union score by about √2.
- **`stitch`.** Fragments are joined in mutually-best pairs. A pair is joined only if every condition below holds:
  - the gap is at most `max_gap_s`;
  - neither fragment is ragged on its own;
  - each fragment's own fit predicts the other's near end to within 3%, for some ratio in {1, 2, ½, 3/2, 2/3, 3, ⅓};
  - the joint fit's rmse is within a tolerance taken from the *stronger* fragment. A weak, noisy fragment must not loosen it.

  A rejected pair is remembered and never retried.
- **`same_source_keep`.** After the fits, the weaker of two events is dropped if the two lines keep a constant log-ratio through a fall of at least 6 bins. This catches piston shaft orders and exhaust-versus-prop ratios that are not small integers.

## CV fit (`events/fit.rs`, `events/stats.rs`)

The fit model is a constant-velocity straight pass:

`f(t) = f0·c²/(c²−v²)·(1 − v²s / √(d²(c²−v²) + c²v²s²))`, with `s = t − (t_arrive − d/c)`.

This is the received frequency at audio time `t`, with retarded time solved exactly. The private copy `cv_freq` carries a `TODO(merge)` to switch to `physics::cv::frequency`.

The parameters are `[f0, v, d, t_arrive]`. `t_cpa_arrive` is when the CPA sound reaches the mic.

**Solver.** Robust Levenberg–Marquardt with Huber IRLS weights (`huber_k` = 1.345, in units of σ = 1.4826·MAD of the residuals). The Jacobian is numeric.

**Seeds.** Nine starts, each run to convergence; the lowest robust cost wins.
- `v` and `f0` come from the 5–95% asymptotes, using `f_hi/f_lo = (c+v)/(c−v)`.
- `d` comes from the steepest local slope, using `|ḟ|max ≈ f0·v²/(c·d)`.
- The 3×3 grid multiplies `v` by 0.6/1/1.6 and `d` by 0.4/1/2.5.

**Bounds.** All bounds come from the covariance `σ²(JᵀWJ)⁻¹`:
- **Autocorrelation.** The covariance is inflated by `(1+ρ)/(1−ρ)`, where ρ is the lag-1 residual autocorrelation. Ridge errors are strongly correlated from frame to frame because windows overlap, so independent-error bounds would be too tight.
- **Width.** Each range is best ± `sigma_k`·σ_param, with `sigma_k = 3`.
- **c–v–d ambiguity.** `v` and `d` are also widened by ±`c_rel_unc` (3%), because the speed of sound is uncertain and it trades off against them.
- **Manoeuvre floors.**
  - `v` gets at least ±20%.
  - `d` gets at least [d/2, 2d].
  - `f0` gets at least ±1%.
  - `t_cpa` gets at least ±3 s.

  A real aircraft is not a perfect CV pass, and these bounds only prune a later search; they are never treated as truth.
- **Degenerate fits.** Too few points or a singular system give `rmse_hz = 1e9` and wide bounds. The value is finite on purpose so the JSON stays valid.

`rmse_hz` is computed over the Huber inliers (|r| ≤ 3σ).

## Validation (`events/discover.rs::finalize`)

An event is kept only if every check below passes:

| Check | Requirement |
|---|---|
| Duration | ≥ `min_duration_s` |
| Detection z | ≥ `min_snr` |
| Fit rmse | ≤ `max_rmse_rel·f0` |
| Speed | `v` ≤ `max_speed_mps`. Partial ridges extrapolate to the clamp. |
| CPA distance | `d` ≥ `min_cpa_m` |
| Fitted fall | ≥ `min_drop·f0`. This rejects flat hums that survived whitening. |

The fit is run on the frames with peak z ≥ 1, when there are enough of them.

## DOPPLERGEIST failure modes and what addresses each

| Failure | Countermeasure |
|---|---|
| Global path forced across the whole file | Local Viterbi plus segment cuts |
| Path hopping onto a crossing aircraft | Hop cut and `split_inconsistent` |
| Fundamental missing, so the event is lost or labelled at the wrong octave | Comb sum, gcd of present orders, sub-base hypotheses |
| One aircraft reported several times | `merge_related` and `same_source_keep` |
| Faint pass broken into pieces | `stitch` with a prediction check |
| Slow-pass asymptotes whitened away | Quantile floor over 60 s, global MAD |
| Garbage fits pinned at the limits | `max_speed_mps` and `min_cpa_m` |
| Gain or level dependence | Fully relative whitening |

## Parameters

Every field of `FrontEnd` and `DiscoverParams` (`events/mod.rs`, with a doc comment on each) can be set from one flat JSON object, `DiscoverConfig`. Missing fields take their defaults.

```json
{"wins_s": [0.5, 1, 2], "hop_s": 0.25, "f_min": 20, "f_max": 1000, "bins_per_octave": 96, "mains_hz": null,
 "harmonics": 6, "min_duration_s": 10, "min_snr": 6, "max_rmse_rel": 0.03, "fit": {"sigma_k": 3, "c_rel_unc": 0.03}}
```

## CLI

```
doppler-print discover file.mp3 --out events.json [--params cfg.json] [--wins 0.5,1,2] [--f-max 1000] [--mains 50]
```

The CLI prints timing and one line per event to stderr, and writes a JSON array of `Event` to `--out` (stdout if `--out` is omitted).

## Python (`doppler_print._core`)

Every call except `probe` releases the GIL while the Rust work runs.

```python
x, sr = decode(path, f_max=None)            # mono float32; with f_max, decimated by an exact integer factor
probe(path)                                 # dict: sample_rate, channels, n_frames, codec, skipped_packets
img, t, f = log_spectrogram(x, sr, win_s=1.0, hop_s=0.25, f_min=20, f_max=1000, bpo=96,
                            mains_hz=None, whiten=True)     # img is [t, f]
events = discover(x, sr, **params)          # params: any DiscoverConfig field; returns a list of Event dicts
fit = fit_cv(t, f, c=343.0)                 # CvFit dict
```
