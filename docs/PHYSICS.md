# Forward acoustic physics

Code: `crates/dp-core/src/{track.rs, physics/, sim/}`. Background and error budgets:
`docs/research/doppler_physics_dsp.md` §1–2, `docs/research/aircraft_acoustics.md` §2–5.

Conventions: positions ECEF metres, velocities ECEF m/s (ground-relative, as ADS-B reports), time UNIX
seconds for tracks. Wind `w` is a uniform ECEF vector, `c` the still-air sound speed.

**Precision gotcha.** f64 UNIX time near 1.7e9 has a 2.4e-7 s ulp. Solver tolerances scale with |t|
(`emission::tolerance`), CPA minimisation runs in segment-relative time, and the synthesiser works in
audio-relative time and shifts tracks by `start_unix` only for output.

## 1. Track interpolation

`Track::state_at` is cubic Hermite between the bracketing knots (binary search), using the reported
velocities as the end tangents. The returned velocity is the derivative of that polynomial, not an
interpolated report, so Newton's derivative is consistent with the position. Linear motion is reproduced
exactly. Outside `[t_start, t_end]` the result is `None`. Tracks are split at gaps upstream, so no gap is
ever bridged.

## 2. Emission (retarded) time with uniform wind

In the air frame the medium is still. The source is at `p(τ) − wτ` and the observer at `r − wt`. Sound
emitted at τ reaches the observer at t when

    |D(τ)| = c (t − τ),    D(τ) = r − p(τ) − w (t − τ)

Solve g(τ) = τ + |D(τ)|/c − t = 0 by Newton, with

    g'(τ) = 1 + k̂·(w − u)/c,    k̂ = D/|D|,    u = ṗ(τ) (ground velocity)

g' ≥ 1 − |u − w|/c > 0 for a subsonic source in the air frame, so g is strictly increasing and the root is
unique. That gives an exact out-of-track test. If g(t_start) > 0 the sound has not yet been emitted. If
g(t_end) < 0 the emission happened after the track ended. Either way the result is `None`, and
`doppler_factors` returns NaN.

- Cold start: τ₀ = t − |r − p(clamp(t))|/c.
- Warm start: τ₀ = τ_prev + (t − t_prev)·F_prev. The previous Doppler factor F is dτ/dt (below), so the
  guess is first-order exact. It converges in one or two updates.
- Iterates are clamped to the track span. The stopping rule is |g| < max(1e-9 s, 4·eps·|t|).

## 3. Doppler factor: stationary observer, moving source, uniform wind

Differentiate |D(τ(t))| = c(t − τ) with respect to t, using dD/dt = −u τ' − w(1 − τ'):

    k̂·(−u τ' − w + w τ') = c (1 − τ')
    τ' (c + k̂·w − k̂·u) = c + k̂·w

The received/emitted frequency ratio is F = f_rx/f0 = dτ/dt, so

    F = (c + w·k̂) / (c + w·k̂ − u·k̂) = c_eff / (c_eff − u·k̂),    c_eff = c + w·k̂

- k̂ is the **air-frame** chord direction (the wave normal). It is D/|D|, not the geometric line of sight
  (r − p)/|r − p|. The two differ by O(w/c).
- u is the **ground** velocity. The same result follows from the textbook moving-source and
  moving-observer formula in the air frame, with source velocity u − w and observer velocity −w:
  f = f0 (c − (−w)·k̂)/(c − (u − w)·k̂).
- When w = 0 this reduces to F = c/(c − v_r), with v_r = u·n̂ the closing speed at the emission time.
- Check against the research example (c = 340, v = 80, 10 m/s tailwind): the asymptotes are 350/270 =
  1.2963 and 330/410 = 0.8049. Tested in `along_track_wind_asymptotes`. A second test,
  `zero_wind_equals_still_air_and_factor_is_dtau_dt`, checks that F equals the numerical dτ/dt of the
  solver in wind.

## 4. CPA

`cpa` minimises the geometric slant range |p(τ) − r| over the track. Candidates are every knot, plus one
interior minimum on each segment where the range rate changes from closing to opening. That minimum is
found by bisecting the range rate (p − r)·ṗ to 1e-9 s, in segment-relative time. Golden section on R²
was tried and rejected: in ECEF, R² is flat to f64 noise within about 1e-5 s of its minimum. It reports:

- `t_emit`, the minimising τ.
- `t_arrive` = t_emit + s, where s is the positive root of |q − w s| = c s, i.e.
  s = (−q·w + √((q·w)² + (c² − |w|²)|q|²)) / (c² − |w|²), with q = r − p.
- `slant_m`.
- `ground_speed`, the velocity component perpendicular to the local up at the aircraft.

With wind, the zero-Doppler instant is not exactly the geometric CPA. The search uses CPA only for
prefiltering, so this is acceptable.

## 4a. Height-layered medium (`physics::column`)

An `Atmosphere` may carry a `Column`: c(z) and the horizontal wind vector W(z) as piecewise-linear nodes
in ellipsoidal height (from `weather`). Each emission then uses §2–4 with the uniform medium of its own
layer [z_r, z_s] between observer and source:

    c̄ = (z_s − z_r) / ∫ dz/c,    W̄ = ∫ W dz / (z_s − z_r)

- Uniform weight in height is the straight-chord first-order kernel, independent of elevation angle
  (`docs/research/localization_deeper_factors.md`). An exact moving-medium ray trace on 460 passes found
  the full **vector** z-mean takes out 93–96% of the still-air CPA-range bias (jets median 113–133 → 5–6 m;
  GA 15–20 → 1–7 m); along-track-only is not enough, since crosswind moves the zero-Doppler point by up to
  139 m (summary and results: `docs/research/weather_wind_2026-11-03.md`).
- Heights come from the local vertical at the column origin with a spherical-earth drop
  (h ≈ h₀ + d·û + (|d|² − (d·û)²)/2R, < 1 m error within 20 km). Beyond the end nodes values are constant.
- The column stores departures from its reference layer (ground to 1 km); `Atmosphere.c`/`wind` hold that
  reference, so overrides and nuisance offsets on them shift every layer equally.
- The emission Newton derivative ignores dc̄/dτ and dW̄/dτ (altitude changes slowly); F is the §3 formula
  with the converged layer medium, which is exact for level flight (`tests/physics_column.rs`).
- Not modelled: curved-ray weighting (2–5× smaller residual under low-level jets), shadow zones and
  turning rays.

## 5. Constant-velocity closed form (`physics::cv`)

The path is straight and level at speed v, with CPA slant d and CPA emission time τc = t_c − d/c, where t_c
is the CPA arrival. In still air:

    f(t) = f0 · c²/(c² − v²) · [1 − v² (t − τc) / √(d²(c² − v²) + c² v² (t − τc)²)]

f(t_c) = f0 exactly. The curve is invariant under (c, v, d) → (kc, kv, kd). `cv::StraightLine` builds the
matching `Track` for tests.

## 6. Synthesiser (`sim`)

The synthesiser is independent of `physics::emission`, `doppler_factors` and `cv`. The only physics code it
shares is `Track::state_at`. Doppler is never computed. It emerges from evaluating the source phase at the
emission time:

    y(t) = Σ_paths Σ_k  G_path · a_k · (1 km / R) · e^(−α(k f0) R) · env(τ) · sin(k φ(τ) + θ_k)

- τ(t) comes from plain fixed-point iteration, τ ← t − |r − p(τ) − w(t − τ)|/c (contraction factor ≤ Mach),
  warm-started by linear extrapolation. R = c(t − τ). Past the track ends p is extrapolated linearly so the
  iteration always has a root. A 2 s raised-cosine `env` fades the source in and out at the track ends.
- φ(τ) = 2π∫₀^τ f0(s) ds, in closed form. f0(s) = f0 (1 + β s + Σ₄ a_j sin(2πν_j s + ψ_j)), with a slow
  rpm wander of RMS `jitter_frac` at ν ∈ [0.02, 0.3] Hz. φ is exact at any τ, so the direct and reflected
  paths stay mutually coherent and chunks render in parallel.
- α is ISO 9613-1 at 20 °C / 70 % RH (log-log table interpolation, ∝ f² outside 63 Hz–4 kHz), evaluated at
  the source-frame harmonic frequency.
- Ground: with `[ground] reflection = Q`, a second path goes to the microphone mirrored below the local
  tangent plane, with gain Q. That produces Lloyd's-mirror fringes. Assumes horizontal wind.
- Noise: white plus one-pole "red" Gaussian noise (`red_db` above white at DC, corner `red_corner_hz`).
  It is scaled analytically so an amplitude-1 tone at 1 km sits `snr_db` above the noise in a 1 Hz band at
  100 Hz. Optional mains hum at `hum_hz` plus its 3rd harmonic.
- Device: optional 2nd-order Butterworth high-pass, then normalisation to `peak_dbfs`, then `gain_db`, then
  hard clipping. RNG: seeded SplitMix64, so output is bit-for-bit deterministic.
- Source extras (`[aircraft.source]`, all default off): per-harmonic AM (`am_depth` RMS at rates around
  `am_rate_hz`), a ramped f0 step (`step_frac` at `step_time_s` over `step_ramp_s`, integrated in closed
  form like the drift) and a half-order line (`half_harmonic`, the Oyster Bay B350 0.5× line).

### Recording chain (`[chain]`, `sim/chain/`)

Optional; without the table the render is bit-identical to the idealised chain above. Each generator draws
from its own RNG stream (`chain::Stream`), so switching one on never changes another or the base noise.
Levels `tnr_db` are line power over the background PSD at 100 Hz in 1 Hz (amplitude `10^((tnr − snr_db)/20)`).
Measurements behind the defaults: `research_notes/Doppler localization verification/corpus_signatures.md`
and `codec_measurements.md`.

| Table | Model |
|---|---|
| `[chain.hum]` | ENF: OU walk (`wander_mhz` RMS, `wander_tau_s`, default 14 mHz / 60 s from Oyster Bay) + white `jitter_mhz` at 1 s steps; harmonic k follows k·ENF coherently. `harmonics` default favours k = 2, 4, 6 (fundamental 26 dB down); slow per-harmonic `level_wander_db`. Truth: `enf_hz` trace, also `enf.csv`. |
| `[[chain.lines]]` | Persistent local line `hz` (+ `harmonics`), `tnr_db`, slow `wander_hz`. E.g. 58.25 Hz motor slip, 25 Hz. |
| `[chain.wind]` | 3rd-order low-passed rumble (`corner_hz`) gated by exponential on/off gusts covering `fraction`, mean `gust_s`, lognormal strength; `level_db` is the rumble PSD below the corner over the 100 Hz floor. Truth: gust intervals. |
| `[chain.source]` | Per-aircraft draws merged into `[aircraft.source]`: drift N(0, `drift_pct_per_min_sd`), a step with `step_prob` of size N(0, `step_pct_sd`) %, within ±60 s of CPA arrival, `am_depth`/`am_rate_hz`, `half_harmonic`. Truth: effective values per aircraft. |
| `[chain.path]` | Ground-reflection coherence loss: a turbulent excess delay δt(t) on the reflected path (rate `turbulence_hz`), sized so E[cos 2π·250 Hz·δt] = `ground_coherence`; decorrelation grows with frequency. |
| `[chain.device]` | `highpass_order` (even) for `[device] highpass_hz`; `lowpass_hz` (4th order, upstream band limit); `ns` = {`depth_db`, `time_s`} WebRTC-like suppression (steady bins fall to depth within ~`time_s`, onsets pass); `quantise_bits`; `channels` = mono / dual_mono / stereo (second mic `stereo_baseline_m` along `stereo_axis_deg`, independent `self_noise_db`). |

Device order: high-pass, low-pass, NS, normalise, gain, clip, quantise; both channels share one gain.
`Rendered.samples` is the channel mean (what the decoder feeds the pipeline). Codecs are not modelled in Rust:
`tools/sim_codec.sh` round-trips rendered WAVs through ffmpeg (aac128, opus135, aac128→opus135).
Presets: `experiments/chain/*.toml`; `tools/gen_sim_chain.py <preset>` merges one over `experiments/sim_atm`
into `experiments/sim_chain_<preset>/`. `tools/sim_recall.py` measures event-discovery recall on them.

### Scenario TOML

Top level: `seed`, `sample_rate`, `duration_s`, `start_unix`, `adsb_dt_s` (track sampling, default 1 s).

| Table | Keys |
|---|---|
| `[observer]` | `lat_deg`, `lon_deg`, `h_ell_m` (ground, ellipsoidal), `mic_height_m` |
| `[atmosphere]` | `c`, `wind_enu = [e, n, u]` |
| `[ground]` | `reflection` (omit the table for no reflection) |
| `[noise]` | `snr_db`, `red_db`, `red_corner_hz`, `hum_hz`, `hum_db` |
| `[device]` | `peak_dbfs`, `gain_db`, `highpass_hz` |
| `[chain]` | optional recording chain, see "Recording chain" above |
| `[[aircraft]]` | `id`, `icao24`, `type_code`, `engine`, `amplitude` (fundamental at 1 km) |
| `[aircraft.source]` | `f0`, `harmonics = [a1, a2, …]`, `drift_per_s`, `jitter_frac`, `am_depth`, `am_rate_hz`, `step_frac`, `step_time_s`, `step_ramp_s`, `half_harmonic` |
| `[aircraft.straight]` | `heading_deg`, `speed_mps`, `alt_agl_m`, `cpa_offset_m` (+ puts the observer on the right), `cpa_time_s` (audio time of the geometric CPA), `half_span_s` |
| `[aircraft.waypoints]` | `points = [[t_audio_s, e, n, u], …]`. A C¹ spline with three-point tangents, resampled every `adsb_dt_s` |

Give each aircraft exactly one of `straight` or `waypoints`. Both are defined in the observer's ECEF
tangent plane, so a straight line stays exactly straight in ECEF. Examples are in `experiments/sim/`.

### Outputs

`dopplerprint sim scenario.toml --out dir` writes `dir/audio.wav` (16-bit, mono unless `[chain.device]
channels` says otherwise), `dir/truth.json` and, with `[chain.hum]`, `dir/enf.csv`.
`truth.json` contains:

- `observer`: lat, lon, ground `h_ell_m`, `mic_height_m`, `mic_ecef`.
- `sample_rate`, `duration_s`, `start_unix`, `seed`, `c`, `wind_ecef`, `output_scale`.
- `events[]`, one per aircraft: `track_id`, `icao24`, `engine`, `f0`, `harmonics`, `cpa_emit_audio_s`,
  `cpa_arrival_audio_s`, `cpa_arrival_unix`, `slant_m`, `speed_mps` and `in_recording`. The CPA is found
  by a direct scan, not by `physics::cpa`.
- `tracks[]`, as serialised `Track`s in UNIX seconds, the same schema the search consumes.
- `chain` (only with `[chain]`): `hum` (`mains_hz`, `enf_dt_s`, `enf_hz[]`, per-harmonic `amplitudes`),
  `lines[]`, `wind` (`fraction`, `gusts[]`), `sources[]` (effective drift, step, AM, half-order per aircraft)
  and `config` (the chain table as rendered).

## 7. Validation (crates/dp-core/tests/physics_*.rs, track.rs tests)

| Test | Checks |
|---|---|
| `cv_closed_form_matches_numeric_solve` | Closed form vs Newton on a Hermite straight track, worst relative error < 1e-6 |
| `newton_warm_start_converges_in_three` | ≤ 3 Newton updates warm-started (straight and 3°/s turn, with wind) |
| `zero_wind_equals_still_air_and_factor_is_dtau_dt` | w → 0 limit, and F = dτ/dt in wind |
| `along_track_wind_asymptotes` | Moving-medium asymptotes from §3 |
| `cpa_on_straight_track` | CPA time, slant, ground speed, arrival with and without wind |
| `hermite_reproduces_linear_motion` | Hermite exact for linear motion |
| `sim_ridge_matches_cv_closed_form` | STFT peak track of the sim output vs `cv::frequency`, within one bin (1 Hz) |
| `sim_ridge_matches_doppler_factors_in_wind` | Sim ridge vs `f0·doppler_factors` with 12.6 m/s wind: must agree far better with wind than ignoring it |
| `sim_truth_cpa_matches_physics_cpa` | Sim's scanned CPA vs `physics::cpa` on the emitted tracks |
