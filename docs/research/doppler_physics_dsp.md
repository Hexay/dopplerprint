# Doppler physics and signal processing for doppler_print

Scope: the physics and DSP behind matching aircraft Doppler curves in a spectrogram against ADS-B
trajectories. See `docs/research/transcript/analysis.md` for the pipeline this feeds. Numerical claims marked
[verified] were checked numerically with a throwaway numpy script, which is not committed (closed form
against a Newton solve, CRB example, wind asymmetry). Everything else comes from the cited sources or textbook acoustics.

Notation: observer at r (fixed, ENU metres). Source trajectory p(τ), velocity u(τ) = ṗ(τ), emission time
τ, reception time t, sound speed c, rest (source) frequency f0, Mach number M = |u|/c.

---

## 1. Moving-source Doppler with retarded (emission) time

### 1.1 Exact formulation (still air)

The retarded-time equation links reception time t to emission time τ:

    g(τ; t) = τ + R(τ)/c − t = 0,        R(τ) = |p(τ) − r|

The unit vector from source to observer is n̂(τ) = (r − p(τ))/R(τ). The approach (closing) speed is
v_r(τ) = u(τ)·n̂(τ), which is positive when approaching and equals −dR/dτ. The received frequency is:

    f(t) = f0 · c / (c − v_r(τ(t)))                                   (1)

This form is exact for a subsonic point source with a fixed observer in still, homogeneous air. f0 is the
frequency in the source frame (the blade-pass frequency, BPF = n_blades · RPS, for propellers). Evaluating
v_r at t, not τ, gives a different curve, not a time-shifted one: the approaching branch is compressed by
dt/dτ = 1 − v_r/c and the receding branch is stretched. The video reports that this mistake breaks the
search (analysis.md §8).

Amplitude: for a monopole, |pressure| ∝ 1/(R (1 − v_r/c)²) (convective amplification). This matters only if
we weight the score by predicted level. It does not change the frequency curve.

### 1.2 Solving for τ

g'(τ) = 1 − v_r(τ)/c ≥ 1 − M > 0, so g is strictly monotone and τ(t) is unique for M < 1. τ(t) is also
monotone in t, with dτ/dt = 1/(1 − v_r/c).

- **Fixed point** τ_{k+1} = t − R(τ_k)/c. The contraction factor is |v_r|/c ≤ M. At M ≈ 0.25 that is about
  log(1e-9)/log(0.25) ≈ 15 iterations to reach 1 ns from a cold start.
- **Newton** τ_{k+1} = τ_k − g(τ_k)/(1 − v_r(τ_k)/c). Convergence is quadratic and global here, because g
  is monotone with a bounded, slowly varying derivative. From a cold start τ_0 = t − R(t)/c it takes 2–3
  iterations. Warm-started from the previous sample's τ (sweep t forward) it takes 1–2.
- **Closed form per linear segment.** If p is piecewise linear, p(τ) = a + u τ on a segment, then
  |a + uτ − r|² = c²(t − τ)² is a quadratic in τ. Take the root with τ < t:

      (c² − |u|²) τ² − 2(c² t + u·q) τ + (c² t² − |q|²) = 0,   q = a − r

  This gives an exact, branch-free per-sample solve. The segment is found with a monotone pointer, since τ
  increases with t.

Recommendation: use Newton with a warm start and at most 4 iterations, stopping when |g| < 1e-6 s, on a C¹
trajectory (§1.3). The convergence argument does not depend on the interpolant, and 1e-6 s of timing error
is far below any spectrogram resolution.

### 1.3 Trajectory interpolation from ADS-B

ADS-B gives position (CPR-encoded, roughly 5 m resolution in airborne format), barometric altitude in 25 ft
steps (sometimes GNSS height too), groundspeed and track (1 kt and ~0.1–1° resolution), and vertical rate
(64 ft/min steps). Historical archives are often resampled to 1–10 s.

- **Linear interpolation** has a velocity discontinuity at every knot. v_r jumps, so f(t) shows steps
  of Δf ≈ f0 Δv_r / c. At 10 s spacing and 1° of turn, Δv_r ≈ v·0.017 ≈ 1.4 m/s, which is ≈0.4 % of f0 and
  lands right in the scoring tolerance. It is acceptable only for dense (≤1 s), straight segments.
- **Cubic Hermite** using the reported velocity at each knot (east/north from groundspeed and track, up from
  vertical rate) gives a C¹ path, so v_r is continuous and the derivative used in Newton is consistent.
  Gotcha: reported velocity and finite-differenced position disagree because of latency and quantisation.
  If |Δp/Δt − ū| > ~3 m/s, fall back to monotone (Fritsch–Carlson/PCHIP) interpolation of the positions,
  or run a constant-acceleration RTS (Kalman) smoother first.
- **Altitude**: convert barometric altitude to geometric altitude using QNH or the reanalysis surface
  pressure (§2.4). Prefer GNSS height when present. A 100 ft error at 1 km slant range changes the CPA
  distance by tens of metres and the curve slope by a few %.
- Sensitivity: δf/f0 ≈ δv_r/c. 1 kt (0.51 m/s) gives 0.15 %, which is 0.2 Hz at 130 Hz and below the
  bin width. Velocity quantisation is not limiting. Position and timing alignment are.
- ADS-B timestamps: use receiver or MLAT time (often ±0.1–0.5 s for aggregated feeds). This error is
  absorbed by the event-1 time-offset sweep but **not** by locked event-2 scoring. Allow ±1 s jitter there.

### 1.4 Closed form for constant velocity (Ferguson & Quinn style)

Take a straight, level path at speed v, with CPA slant distance d and CPA emission time τ_c. Solving the
quadratic of §1.2 in closed form gives [verified to 3e-14 against the Newton solve]:

    f(t) = f0 · c²/(c² − v²) · [ 1 − v² (t − τ_c) / sqrt( d² (c² − v²) + c² v² (t − τ_c)² ) ]     (2)

- The CPA sound arrives at t_c = τ_c + d/c. At that moment f = f0 exactly, because the emission happened
  at CPA where v_r = 0. The fit can use t_c or τ_c. Report t_c, since it is what the spectrogram sees.
- Asymptotes: f(−∞) = f0 c/(c − v) and f(+∞) = f0 c/(c + v). Hence
  v = c (f₋ − f₊)/(f₋ + f₊) and f0 = 2 f₋ f₊/(f₋ + f₊).
- Peak slope near t_c: df/dt ≈ −f0 v²/(c d), with an O(M²) correction. At f0 = 100 Hz and v = 80 m/s this
  is 6.3, 1.9 and 0.63 Hz/s for d = 0.3, 1 and 3 km [verified]. The slope sets d, and the swing sets v.
- **Scale invariance (important).** Equation (2) is unchanged under (c, v, d) → (k c, k v, k d)
  [verified exactly]. A single microphone cannot separate c from the geometry, so the fitted v and d carry
  exactly the relative error of the assumed c. 1 % of c (≈6 K) gives 1 % of v and d, which is 30 m at
  3 km. This governs the candidate band width in pipeline step 6.
- The parameter vector for curve fitting is θ = (f0, v, d, t_c). Seed it from the asymptotes and the
  maximum-slope point, then refine by nonlinear least squares (Levenberg–Marquardt) on ridge samples, or
  directly by maximising the spectrogram score (§5).

### 1.5 Low-Mach approximations and their error

- First order: f ≈ f0 (1 + M cos θ). The relative error is M² cos² θ: 2.25 %, 6.25 % and 12.25 % at
  M = 0.15, 0.25 and 0.35 [verified]. That is 3–15 Hz at 120 Hz, many bins. **Never use it.**
- Ignoring retardation, i.e. f(t) = f0 c/(c − v_r(t)): the time error is R/c (3–30 s at 1–10 km) plus the
  shape distortion described in §1.1. Unusable for scoring. It is acceptable only for the coarse CPA-grid
  prefilter once the R/c delay is added (as "delayed CPA" does).
- Equation (2) on a curved or climbing real trajectory: fine for measuring (v, d) from the ridge. The
  final ranking needs the full-geometry Newton solve, as the video confirms for event 1's manoeuvring
  segment.

### 1.6 Moving medium (uniform wind w)

Move to the air frame. The source velocity becomes u − w and the observer velocity becomes −w. The
standard moving-source and moving-observer Doppler with wave normal k̂ then gives, using **ground**
velocity u (what ADS-B reports):

    f = f0 · c_eff / (c_eff − u·k̂),       c_eff = c + w·k̂                                (3)

The retarded-time equation becomes |p(τ) − w τ − (r − w t)| = c (t − τ). This is the still-air equation
of §1.2 with both endpoints displaced by the wind, so the same Newton solver applies unchanged. k̂ is the
wave normal and differs from the ray direction (c k̂ + w) by O(w/c). Example with v = 80 m/s and a 10 m/s
along-track tailwind: the approach asymptote f/f0 drops from 1.3077 to 1.2963 and the receding one from
0.8095 to 0.8049 [verified]. Most of this is a common −0.7 % scale, which the f0 sweep absorbs. The rest is
a ≈0.3 % asymmetry, about 0.4 Hz at 130 Hz.

---

## 2. Speed of sound, wind, refraction

### 2.1 c(T, humidity)

c ≈ 20.05 sqrt(T_v) m/s, with T_v ≈ T (1 + 0.61 q) the virtual temperature (K) and q the specific
humidity. This works out to ≈ 331.3 + 0.606 T(°C). Humidity adds 0.1–0.6 % at warm, moist conditions.
Pressure has no first-order effect.

Troposphere: lapse rate −6.5 K/km (standard), or near −9.8 K/km in a mixed afternoon boundary layer, so
dc/dz ≈ −4 to −6 s⁻¹ per km (that is, −0.004 to −0.006 s⁻¹). Evening and night stable layers invert this
near the surface (+0.02–0.1 s⁻¹ over the lowest 100–300 m).

### 2.2 Effective sound speed

c_eff(z) = c(T_v(z)) + w(z)·n̂_h, where n̂_h is the horizontal propagation direction. Wind shear near the
ground typically gives |dw/dz| of 0.02–0.1 s⁻¹ in the lowest 100 m, which is 5–20× the thermal gradient.
Upwind propagation refracts upward, downwind propagation refracts downward.

### 2.3 Size of the biases

| Error source | Typical size | Effect on fitted v, d (single-mic fit) | Effect on full-geometry scoring |
|---|---|---|---|
| Wrong layer-mean T, ±5 K | ±0.9 % c | ±0.9 % v and d (scale invariance) | Doppler swing ±0.9 %, about 0.3 Hz at f0 = 130 Hz and M = 0.25. Arrival time ±r δc/c² = ±0.13 s at 5 km |
| Along-track wind, 10 m/s | ±3 % c_eff, sign flips across CPA | f0 bias −f0 v w/c² ≈ −0.7 %. Asymmetric curve, so fitting residuals appear on one side | ≈0.4 Hz asymmetry, travel time ±(r w/c²) ≈ 0.4 s at 5 km |
| Cross wind | O(w²/c²) on Doppler | negligible | ray drift ≈ w r/c ≈ 150 m lateral at 5 km (arrival-time and geometry shift) |
| Ray curvature (§2.5) | elevation-angle change ≈ L/(2R_c) | negligible for v_r unless the ray is near grazing | path-length excess L³/(24 R_c²) is a few m, so timing is negligible. Shadow zones matter (§2.5) |

Conclusion: thermal and wind errors are 0.3–1 Hz on the curve and ≤0.5 s in timing. That is comparable
to one STFT bin and to the ±1–2 s locked-time tolerance. They do **not** break the method. They do degrade
peak scores, and they bias the step-6 distance band by ~1–3 %. Handle them with a small c_eff nuisance
parameter or from reanalysis data, not by ignoring them.

### 2.4 Reanalysis data

- **HRRR** (NOAA): 3 km, hourly, CONUS, with an archive on AWS Open Data from 2014. It provides T, q, u,
  v on hybrid and pressure levels plus surface pressure. This is the best fit for the CONUS use case: one
  profile per event at the candidate-cluster centroid is sufficient.
- **ERA5** (ECMWF/Copernicus): 0.25° (~31 km), hourly, 1940–present, 37 pressure levels and 137 model
  levels, global. Use it as the fallback outside CONUS and for older dates.
- Use the profile for (a) the path-averaged c for travel time, as the harmonic mean of c_eff along the
  straight chord, (b) the c_eff at the source for equation (3), (c) barometric-to-geometric altitude
  correction, and (d) the silence/audibility model (shadow zones).
- The cost is negligible: one profile per event per ~10 km region.

### 2.5 Refraction and curved rays (1–10 km)

For a linear gradient g = dc_eff/dz, rays are circular arcs with radius R_c ≈ c/(|g| cos θ0).

- Thermal lapse only (|g| ≈ 0.004 s⁻¹): R_c ≈ 85 km. The chord deviation (sagitta) over horizontal range
  L is L²/(8 R_c): 15 m at 3 km, 150 m at 10 km.
- Upwind shear (|g| ≈ 0.02 s⁻¹): R_c ≈ 17 km. The sagitta is ~0.7 km at 10 km.
- **Shadow zone** for upward refraction, source height H and receiver height h:
  x_s ≈ sqrt(2 R_c)(sqrt(H) + sqrt(h)). For lapse only: H = 300 m gives ≈ 7.7 km and H = 1 km gives
  ≈ 13.6 km. Upwind with strong shear: H = 1 km gives ≈ 6 km. Beyond x_s the level drops by 10–30 dB
  (turbulent scattering fills it partially).
- Implications: (1) ray curvature does not matter for the Doppler curve or the timing at ≤10 km, so use
  straight rays with a path-mean c. (2) It **does** matter for the *silence* filter (step 7). A "should
  have heard it" flight 6–10 km away upwind, at low altitude, can be legitimately inaudible. Make the
  audibility threshold range-, altitude- and wind-direction-aware, or cap silence evidence at ≈5 km.
- Ground reflection (Lloyd's mirror): a microphone at height h_r sees direct and reflected paths with
  path difference Δ ≈ 2 h_r sin φ (φ is the elevation angle). The first null is at f ≈ c/(4 h_r sin φ):
  h_r = 4 m and φ = 30° give ≈ 42 Hz, with further nulls every 85 Hz. As φ changes during the pass, the
  tone fades in and out along the curve. Scoring must tolerate gaps (§5). The fringe pattern is in
  principle an h_r estimator.

---

## 3. Prior art: single-microphone flight-parameter estimation

- **Ferguson (1992), JASA 92(3):1403–1407**, "A ground-based narrow-band passive acoustic technique for
  estimating the altitude and speed of a propeller-driven aircraft". It tracks the propeller blade-rate
  line at short intervals and fits the constant-velocity model by least squares to estimate speed,
  altitude (the CPA range for an overflight) and f0. The aircraft's onboard records served as ground
  truth.
- **Ferguson & Quinn (1994), JASA 96(2):821–827**, "Application of the short-time Fourier transform and
  the Wigner–Ville distribution to the acoustic localization of aircraft". It estimates the turboprop
  blade-rate IF from STFT and WVD over short intervals for constant altitude and speed overflights at
  ~250–1500 ft, then derives speed, altitude and f0. The WVD gave better IF resolution at high SNR, at the
  cost of cross-terms.
- **Ferguson (1995, ASA St. Louis)**: narrow-band single-sensor localisation (v, CPA range, f0) for
  turboprops and helicopters. It also covers broadband two-microphone cross-correlation for transit
  detection, where faster aircraft need Doppler compensation and short integration.
- **Lo & Ferguson (2000), IEEE TAES 36(1):163–175**, "Broadband passive acoustic technique for target
  motion parameter estimation". This uses the time history of the time delay between two closely spaced
  microphones (broadband cross-correlation), which applies to jets that lack strong lines. It
  complements the narrow-band method.
- **Ferguson & Lo (2000), JASA 108(4):1763–1771**: turboprop and rotary-wing flight-parameter estimation
  with both narrow-band (Doppler IF) and broadband (multipath/time-delay) methods. Helicopter main- and
  tail-rotor harmonics give multiple lines that are fitted jointly.
- **Lo (2017), JASA**: flight-parameter estimation from IF plus direction-of-arrival at a single acoustic
  sensor node. Adding DOA resolves the CPA-range/heading ambiguities.
- **Sadasivan et al. (2002), Defence Science Journal**: UAV altitude, speed and engine RPM from
  ground-microphone spectrogram harmonics using an LMS fit to the IF history. It used higher harmonics
  because the fundamental was weak.
- **Yu, Xiao & Sun (2022), Frontiers in Marine Science** and **Zhang et al. (2023), JEIT**: single-
  hydrophone 3-D versions with air–water refraction. Yu et al. obtain f0 and v in closed form from the
  asymptotes and (h, CPA) from the first and second IF derivatives. They also propose an accumulated
  phase-difference IF tracker for low SNR. Field example: 68.5 Hz, 101 m/s, 150 m.
- **Rife & Gray (2004)**: CRBs for airborne-source tracking with hydrophones (bearing plus Doppler,
  unknown f0).
- Reported accuracies in this literature are a few % on v and altitude for cooperative low overflights
  (from abstracts; the full-text numbers were not retrieved, so treat them as order-of-magnitude).

### 3.1 Cramér–Rao bound (derived here)

For one complex tone in white noise over N samples, the Rife–Boorstyn bound on frequency in Hz is
var(f̂) ≥ 6 f_s² / ((2π)² η N (N² − 1)), where η = A²/σ² is the per-sample SNR. For a frame of length T
with coherent SNR ρ = η N:

    σ_f ≥ sqrt(6) / (2π T sqrt(ρ))            (≈0.39 Hz at T = 1 s, ρ = 0 dB)

Treat the per-frame IF estimates as independent with this variance. The Fisher information for
θ = (f0, v, d, t_c) is then F = σ_f⁻² Σ_k J_kᵀ J_k with J_k = ∂f(t_k)/∂θ from equation (2). Example:
f0 = 100 Hz, v = 80 m/s, ±60 s of track, 1 s frames [verified numerically]:

| d | ρ per frame | σ(f0) | σ(v) | σ(d) | σ(t_c) |
|---|---|---|---|---|---|
| 0.5 km | 0 dB | 0.05 Hz | 0.15 m/s | 7 m | 0.04 s |
| 1.5 km | 0 dB | 0.06 Hz | 0.25 m/s | 20 m | 0.09 s |
| 4 km | 0 dB | 0.12 Hz | 0.9 m/s | 120 m | 0.33 s |
| 4 km | 10 dB | 0.04 Hz | 0.3 m/s | 39 m | 0.10 s |

Caveats:
- The bound holds only above the IF-estimation threshold, roughly ρ ≳ 10–13 dB per frame for
  peak-picking. Below it, outliers dominate, and tracking or model-based integration (§4–5) is the remedy.
- Real errors are dominated by **model error**, not noise: c (§2), manoeuvres, f0 drift (governor
  hunting), and ADS-B timing. The CRB says the faint-tone noise floor is not the bottleneck once a ridge
  is detectable. The d-error grows roughly ∝ d^1.5–2.

---

## 4. Spectrogram ridge detection for faint tones

### 4.1 STFT choices for 50–300 Hz, ±10–20 % drift

- The chirp rate α peaks at CPA: α ≈ f0 v²/(c d), typically 0.3–6 Hz/s. The window that balances chirp
  smear against resolution is T_opt ≈ 1/sqrt(|α|). For a Gaussian window, σ_t = 1/sqrt(2π|α|). This gives
  T ≈ 0.4–2 s. Away from CPA the curve is nearly flat, so longer windows (2–4 s) win.
- Recommendation: resample to 2–4 kHz (anti-aliased) and compute a **multi-resolution stack** of Hann or
  Gaussian windows with T ∈ {0.5, 1, 2, 4} s. Hop T/8 (bounded to ≥ 50 ms), zero-padded to Δf ≤ 0.1 Hz.
  Score each candidate on the window whose T matches its predicted local α (≈1/sqrt(α)), or take the
  max/average across the stack per frame.
- Long windows reduce the variance of the noise per bin but not its mean. The detection statistic gain
  from a longer window is the coherent gain 10 log₁₀(T₂/T₁) dB per bin, as long as the tone stays within
  one bin over T.

### 4.2 Sharpening: reassignment and synchrosqueezing

- Reassignment (Auger & Flandrin 1995) moves energy to the local centroid. Synchrosqueezing (Daubechies,
  Lu & Wu 2011, plus the STFT-SST variants) does the same along frequency only and stays invertible. Both
  give near-ideal concentration for a single chirp at high SNR.
- At low SNR they scatter noise energy into spurious ridges and give **no detection gain**. Their value
  here is visual and for precise IF read-out once a ridge is detected. Use them for the human-facing plot
  and for refining (v, d) after detection. Do not use them for the scoring image.
- The WVD and its pseudo/smoothed variants (as in Ferguson & Quinn) have the same caveat, and their
  cross-terms between harmonics are harmful.

### 4.3 Background normalisation (whitening)

1. Compute the power spectrogram P(t, f).
2. Divide by a noise floor N(t, f): the running median over ±5–10 s in time **and** ±2–5 Hz in frequency
   (a 2-D median or the separable product). Then take z = log(P/N) or P/N.
3. Robust standardisation per frequency row: subtract the median and divide by 1.4826·MAD.
4. Clip to [−3, z_max ≈ 6–8] so speech bursts and clicks cannot dominate a line sum.
5. Notch stationary lines: mains 50/60 Hz and harmonics, plus fixed-frequency hum (HVAC, refrigerator).
   Any row whose time-median exceeds a threshold is a stationary tone. Doppler tones are never stationary
   for more than ~10–20 s near CPA.

Expected gain: this turns coloured, non-stationary noise into ~unit-variance noise, which is what makes the
sums of §5 approximately optimal. Against raw dB brightness it is typically worth 3–10 dB in effective
detectability on real recordings with speech or traffic.

### 4.4 Harmonic enhancement

A propeller emits BPF·h, h = 1..H, with a Doppler factor common to all harmonics. Summing H harmonics of
comparable whitened SNR gives ≈ 5 log₁₀ H dB incoherently (H = 4 gives +3 dB). The gain is larger when
the fundamental is attenuated by a codec high-pass or a Lloyd-mirror null (§2.5, §6). With a log-frequency
axis u = log f, harmonics are fixed shifts log h, so the comb sum is S_H(t, u) = Σ_h w_h S(t, u + log h).
It is a shift-invariant filter, computed once (§5.3). Weights: w_h ∝ the estimated per-harmonic SNR, or
uniform weights with a max-of-(sum, best single) guard against non-harmonic lines.

### 4.5 Ridge tracking (for the measurement tool, not the scoring)

- Viterbi / HMM line tracking (Streit & Barrett 1990): states are frequency bins, transitions allow
  |Δf| ≤ α_max·hop, and the emission is the whitened power. It is far more robust than peak-picking at low
  SNR, operating on unthresholded data. The continuous-gravitational-wave HMM trackers (Suvorova, Sun,
  Melatos et al.) are a mature reference implementation of the same idea, with Viterbi over frequency
  bins and summed log-likelihood along the path.
- Better for us: a 2-state Viterbi over (f, ḟ), or a DP whose transition penalty follows the Doppler
  model, i.e. prefers monotone decreasing f with |ḟ| ≤ f0 v²/(c d_min). The CW literature reports that
  tracking frequency and its derivative improves sensitivity by "a factor of a few" at a large compute
  cost. For us the cost is trivial (a few hundred bins × a few thousand frames).
- Output: the ridge polyline, its per-frame confidence, and an automatic (v, d, f0, t_c) LM fit to it.
  This automates the video's hand-traced step 11 and feeds the distance-to-polyline scorer.

### 4.6 Coherent versus incoherent integration and achievable gains

- Incoherent summation of K independent frames along a known curve: the deflection SNR grows as sqrt(K),
  i.e. 5 log₁₀ K dB. For a 60 s track at 0.5 s independent frames (K = 120) that is ≈ +10 dB over a single
  frame. Together with whitening and H = 3–4 harmonics, curves 15–20 dB below single-frame visibility are
  detectable when the model is right.
- Coherent integration (dechirping along the model phase φ(t) = 2π∫f dt, then one long FFT) grows SNR
  as 10 log₁₀ K dB. It is limited by the coherence time T_coh of the received tone: governor RPM hunting
  (≈0.1–1 % over seconds), turbulence phase scintillation at 1–5 km, and moving Lloyd-mirror nulls. At
  100 Hz, a T_coh of a few seconds is realistic. Semi-coherent (stack-slide) processing, with coherent
  segments of T_coh and incoherent sums over segments, is the practical optimum. That is exactly what the
  multi-resolution STFT plus the curve sum implements. Full-track coherent dechirping is not worth
  building.

---

## 5. Scoring a modelled curve against a spectrogram

### 5.1 Statistics

Model: x(t) = a(t) e^{iφ(t)} + n(t), with unknown slowly varying amplitude and phase. Per frame, the GLRT
for a tone at a known frequency f_k is |X(t_k, f_k)|²/σ²(t_k, f_k). With independent frames, the
log-likelihood ratio of the whole curve is

    Λ(θ) = Σ_k ψ( P(t_k, f_k(θ)) / N(t_k, f_k(θ)) )

- ψ(x) = x gives the incoherent energy detector, the classic "track-before-detect" sum. It is optimal for
  a constant-amplitude tone in Gaussian noise.
- ψ(x) = log(1 + x) or clip(x) is robust to impulsive outliers and amplitude fades.
- The video's "sum of brightness under the curve" uses ψ = dB of P, without whitening. It is the same
  family, but one loud speech burst crossing the curve can outweigh the tone, and per-candidate curve
  lengths bias the total.
- Model mismatch: real f_k differs from the model by ε ≈ 0.3–1 Hz (§2–3). Use a **tolerance band**: take
  the max (or soft-max, or a Gaussian-weighted sum) over |f − f_k| ≤ δ, with δ ≈ max(1 bin, 0.5 % f_k).
  The max-pooling costs a small, computable increase in the null mean, which is fine after normalisation.
- Matched filter in the strict sense (coherent correlation with the synthesized waveform) needs phase
  coherence (§4.6). Use it only for final verification on top candidates, on short semi-coherent segments.

### 5.2 Normalisation so that scores compare across candidates

The raw Λ depends on the number of frames K inside the clip, on the frequency band (noise colour) and on
the tolerance δ. Convert every score to a null-referenced z-score:

    Z(θ) = (Λ(θ) − μ₀(K, band)) / σ₀(K, band)

- Analytic option: for whitened, clipped z-values with per-pixel mean m and variance s² under H0, the
  null is approximately μ₀ = K m and σ₀ = sqrt(K) s. With max-pooling, use the extreme-value-adjusted m
  and s, estimated once per δ.
- Empirical option (more robust): evaluate the same curve shape at frequency offsets outside any plausible
  f0 (e.g. f0 ± 15–30 %) and at circular time shifts. Take the median and MAD as μ₀ and σ₀. This is cheap
  with the log-f shift trick below.
- Also correct for the look-elsewhere effect in the nuisance sweep. The max over N_f0 × N_t0 cells of
  null noise is ≈ μ₀ + σ₀ sqrt(2 ln(N_f0 N_t0)). Subtract it, or compare all candidates with the same sweep
  size.
- Combined two-event score: Z₁ + Z₂ for independent events (equivalently a Fisher/Stouffer combination).
  A locked event 2 has a smaller sweep and so a smaller look-elsewhere penalty, which is the reason the
  locked score is so discriminative.
- Report Z together with its components (K, frames with tone above threshold, best f0, best t0) so the
  outlier test ("20.48 vs ~15") is calibrated, not ad hoc.

### 5.3 Efficient nuisance sweeps (f0, t0)

- **Log-frequency trick.** f_k(θ) = f0·D_k(geometry), so log f_k = log f0 + log D_k. Resample the whitened
  spectrogram onto a uniform log-f axis u, e.g. 0.05 % per bin (≈0.06 Hz at 130 Hz), between 40 and
  1200 Hz. On that axis an f0 change is a pure shift. The (t0, log f0) sweep is then a 2-D correlation of
  S_H(t, u) with the curve's sparse mask:

      Λ(Δt, Δu) = Σ_k S_H(t_k + Δt, u_k + Δu)

  (a) Direct evaluation costs K × N_f0 × N_t0 additions over contiguous memory (SIMD-friendly). With
  K ≈ 500, N_f0 ≈ 200 and N_t0 ≈ 40 that is 4·10⁶ adds ≈ 1 ms per candidate per core. (b) A 2-D FFT
  correlation is better only when N_f0·N_t0 is much larger than log(TF), i.e. for blind full-range sweeps.
- **Harmonics** are shifts too, so precompute S_H once (§4.4). Harmonic summation then adds nothing to the
  per-candidate cost.
- **Tolerance band in O(1):** keep a per-frame cumulative sum along u, C(t, u) = Σ_{u' ≤ u} S(t, u'). A
  band sum is C(t, u+δ) − C(t, u−δ), an integral image along frequency. For max-pooling instead of sums,
  precompute a sliding-max image per δ, using the van Herk/Gil–Werman algorithm in O(1) per pixel.
- **Coarse-to-fine:** build a pyramid by max-pooling 2× in t and u. Score all candidates at the coarse
  level with a wide δ. Keep the top ~5 % plus everything within ~3σ₀ of the best. Refine on finer levels
  with a narrower δ. This works together with the spatial coarse-to-fine scheme of 200 m → 100 m → 25 m.
- **Curve synthesis reuse:** for a spatial grid point, the curve depends only on (trajectory, r). Neighbours
  on a 100 m grid have nearly identical τ(t), so warm-start Newton from the neighbour's τ array. In
  locked mode, cache the event-2 mask for each flight, because only r changes.
- Cost budget: 4·10⁴ points × 2 events × 1 ms ≈ 80 core-seconds, which is trivial with rayon. The real
  costs are CPA/grid generation and curve synthesis (≈K Newton solves per candidate, ~10 µs total).

---

## 6. Codecs and resampling

| Codec / path | Low-frequency behaviour | Risk to 50–300 Hz tones | Timing |
|---|---|---|---|
| MP3 (Layer III) | 32-band polyphase + 18-line MDCT. At 44.1 kHz the long-block line spacing is ≈ 38 Hz. No intrinsic LF cutoff | Psychoacoustic quantisation: a faint tone below the band's masking threshold can be quantised to zero in some granules (spectral holes), worse at ≤ 96 kbps and in joint stereo. The tone survives as an intermittent, noisy line, and gap tolerance in scoring handles it | Encoder delay/padding ≈ 1105 samples (LAME, ≈ 25 ms), a constant offset |
| AAC-LC / HE-AAC | 1024-line MDCT (≈ 21.5 Hz lines at 44.1 kHz). SBR and PNS act on HF bands, so the LF core is plain AAC | Same masking holes. Generally better than MP3 at equal bitrate | 2112-sample priming (≈ 48 ms) |
| Opus, application=AUDIO | `dc_reject` first-order high-pass at **3 Hz** (libopus `opus_encoder.c`) | Benign for 50+ Hz. CELT band energy is preserved, fine structure is quantised | 312-sample pre-skip at 48 kHz (6.5 ms) |
| Opus, application=VOIP (WebRTC, calls) | **Variable 2nd-order high-pass, 60–100 Hz** (`VARIABLE_HP_MIN/MAX_CUTOFF_HZ`, adapted to pitch). SILK LPC/LTP coding below 8 kHz | Fundamentals at 50–90 Hz attenuated by roughly 6–20 dB, so rely on harmonics. SILK represents non-speech tones with noise-like excitation, which lowers the line SNR | — |
| AMR-WB (VoLTE HD voice) | 16 → 12.8 kHz, then a **50 Hz high-pass** in pre-processing (TS 26.190 §5.1), then ACELP to 6.4 kHz | Fundamentals ≥ 70 Hz pass. **DTX/comfort noise:** during non-speech, VAD-off frames are replaced by SID-parameterised noise, which erases the tone between speech bursts | 20 ms frames |
| AMR-NB / G.711 PSTN | 300–3400 Hz band (with a ~80 Hz HP inside AMR-NB) | Fundamentals lost. Only harmonics ≥ 300 Hz remain, so the harmonic comb is essential | — |
| Phone noise suppression / AEC | Spectral-subtraction or Wiener style, with noise estimates over 1–3 s | Stationary tones are suppressed. Drifting Doppler tones partially survive (the video's claim). The gain is time-varying, so whitening must be local | — |

Notes:
- Resampling with a proper polyphase or windowed-sinc filter is transparent for 50–300 Hz. Watch out for
  **mislabelled sample rates** (44.1 vs 48 kHz is an 8.8 % error). They scale frequency, which f0 absorbs,
  and also time, which no sweep absorbs. A wrong rate stretches 920 s inter-event gaps to ≈ 845 s and
  biases d/v by 8.8 %. Add a time-scale nuisance s ∈ {44.1/48, 1, 48/44.1} or a check against the video
  frame clock.
- Device clock error (≤ 50 ppm) is 46 ms per 920 s, which is negligible. Dropped or duplicated audio
  buffers in phone or CCTV recorders are not negligible: detect them (phase jumps in mains hum,
  container timestamp gaps) before using locked inter-event timing.
- AGC and compressors in cameras and phones modulate the gain with foreground speech. Per-frame whitening
  (§4.3) neutralises this.
- Double compression (high-pass, MP3, broadband boost, as in the video's test) does not remove a tone that
  survived the first pass. A high-pass removes the fundamental only if it has a steep slope above f0, and
  harmonics remain.
- The mains hum (ENF) line, if present, gives a free sample-rate and drop-out check. ENF forensics
  (Grigoras 2007, Ojowu et al. 2012) is a mature reference for tracking 50/60 Hz lines through codecs.

---

## Design implications for doppler_print

**Physics core (Rust)**
1. `retarded_time`: Newton with a warm start, ≤ 4 iterations, tolerance 1e-6 s. Same code for the wind
   case via the air-frame shift r → r − w t and p → p − w τ (§1.6). Property test against closed form (2)
   to 1e-9 relative.
2. `trajectory`: cubic Hermite through ADS-B fixes using reported ENU velocity. Fall back to PCHIP or an
   RTS-smoothed path when velocity and position disagree by more than 3 m/s. Convert baro to geometric
   altitude using HRRR/ERA5 surface pressure, and prefer GNSS height. Expose a ±1 s ADS-B time-jitter
   nuisance.
3. `doppler_curve`: f_k = f0 · c_eff/(c_eff − u·k̂) at the emission time. Keep the full form, never the
   first-order one (6 % error at M = 0.25).
4. `cv_fit`: closed form (2) with θ = (f0, v, d, t_c), seeded from the asymptotes and the max slope, then
   LM. Report the CRB (§3.1) and the c-scale caveat: the v and d bands passed to step 6 must be widened by
   ±(δc/c + 2 %).

**Atmosphere**
5. Fetch one HRRR profile per event region (ERA5 outside CONUS or before 2014). Derive the path-mean c for
   travel time, c_eff at the source for Doppler, and the shadow-zone range for the silence model.
   Fallback: c = 331.3 + 0.606 T_surface − 0.003·z_mean, plus a c_eff nuisance of ±1.5 %.
6. Silence model: audibility falls off with range and has an upwind shadow at x_s ≈ sqrt(2R_c)(√H + √h).
   Treat "should have heard" evidence beyond ~5 km as weak.

**Spectrogram front end**
7. Resample to 4 kHz. Multi-resolution STFT with Hann/Gaussian windows T ∈ {0.5, 1, 2, 4} s, hop
   min(T/8, 0.25 s), zero-padded to ≤ 0.1 Hz. Pick the window per candidate frame by the predicted α.
8. Whitening: a 2-D running-median floor (±8 s, ±3 Hz), then a log ratio, a robust per-row z-score, and
   clipping to [−3, 7]. Automatic notching of stationary lines (mains and harmonics).
9. Resample to a log-f axis (0.05 % bins, 40–1200 Hz). Precompute the harmonic-comb image S_H (H = 1..4,
   SNR-weighted), the per-frame cumulative sum along u, and a 2× max-pool pyramid.

**Scoring**
10. Score Λ = Σ_k clip(S_H) along the model curve with a tolerance band of max(1 bin, 0.5 %). Tolerate gaps
    by summing, not multiplying, so codec holes and Lloyd nulls are harmless.
11. Sweep nuisances as 2-D shifts on the log-f image: f0 is a shift in u, t0 a shift in t. Use direct SIMD
    summation, or FFT correlation only for blind wide sweeps. Locked event 2 sweeps f0 only, plus ±1 s.
12. Normalise to Z = (Λ − μ₀)/σ₀ using an empirical null from off-band f0 offsets, and apply a
    look-elsewhere correction for the sweep size. Combine events by summing Z. Report K, best (f0, t0)
    and the per-harmonic contributions.
13. Coarse-to-fine in both domains: spatial grid 200 → 100 → 25 m and spectrogram pyramid coarse → fine.
    Warm-start Newton from neighbouring grid points.

**Measurement tool**
14. A model-aware Viterbi ridge tracker (states over (f, ḟ), transition prior from the Doppler model)
    produces a polyline plus confidence and feeds `cv_fit`. It replaces hand tracing. Use reassignment or
    synchrosqueezing only for display and for post-detection IF refinement.

**Input hygiene**
15. Detect codec and sample rate. Add a sample-rate-scale nuisance (44.1/48). Detect buffer drop-outs
    before trusting locked inter-event timing. For VOIP/AMR-WB/PSTN sources, weight harmonics up and
    expect DTX gaps.

---

## Sources

- Ferguson & Quinn 1994, JASA 96(2):821–827 (record): https://researchers.mq.edu.au/en/publications/application-of-the-short-time-fourier-transform-and-the-wigner-vi/
- Ferguson 1995 ASA abstract (narrow-band/broadband aircraft localisation): https://auditory.org/asamtgs/asa95stl/4pSP/4pSP10.html
- Ferguson 1992 JASA 92:1403 (record): https://synapsesocial.com/papers/W2001852194
- Lo & Ferguson 2000 IEEE TAES 36(1) (citation record): https://cs.reviewer.ly/app/expert/kam-w-lo
- Lo & Ferguson, "Extracting tactical information from acoustic signals": https://bioacoustics.myspecies.info/el/node/95
- Yu, Xiao & Sun 2022, Frontiers in Marine Science (single-hydrophone aircraft IF, closed-form f0/v): https://www.frontiersin.org/articles/10.3389/fmars.2022.996493/full
- Zhang et al. 2023, JEIT (helicopter, single hydrophone; cites Lo 2017 JASA): https://www.jeit.ac.cn/en/article/doi/10.11999/JEIT220693
- Sadasivan et al., Defence Science Journal (UAV parameter estimation from harmonics): https://publications.drdo.gov.in/ojs/index.php/dsj/article/view/2238
- Optimal window length for PWVD in passive acoustic parameter estimation: https://bvbvf1.bib-bvb.de/vufind/PrimoRecord/cdi_ieee_primary_389814
- Remarks on the CRB for Doppler-based target parameter estimation (ANU): https://researchportalplus.anu.edu.au/en/publications/remarks-on-the-cramer-rao-inequality-for-doppler-based-target-par/
- Rife & Gray 2004 / Rife–Boorstyn 1974 single-tone CRB (context): https://arxiv.org/pdf/0907.1147
- Single-tone ML threshold characterisation (ANU): https://openresearch-repository.anu.edu.au/entities/publication/693c1a0e-7a9e-4efc-80af-bc2b2372aaf2
- Frequency-line tracking survey incl. Streit & Barrett HMM: https://www.ncbi.nlm.nih.gov/pmc/articles/PMC6891505/
- Dynamic-programming frequency tracking (IOA): https://www.ioa.org.uk/system/files/proceedings/cr_walters_frequency_tracking_based_on_a_dynamic_programming_search_of_potential_tracks.pdf
- Multi-trace carving for frequency tracking (forensics): https://arxiv.org/pdf/2005.06686
- HMM tracking of CW gravitational waves (f, ḟ tracking sensitivity/cost): https://arxiv.org/abs/1710.00460
- HMM phase+frequency tracking, Melatos et al.: https://arxiv.org/abs/2107.12822
- Phase-continuous track-before-detect of slowly varying tone: https://researchportalplus.anu.edu.au/en/publications/phase-continuous-frequency-line-track-before-detect-of-a-tone-wit/
- Auger & Flandrin 1995, reassignment: https://doi.org/10.1109/78.382394
- Daubechies, Lu & Wu 2011, synchrosqueezing: https://doi.org/10.1016/j.acha.2010.08.002
- Flandrin, time-frequency/reassignment tutorial: https://perso.ens-lyon.fr/patrick.flandrin/Fourier_DAFx11c.pdf
- Effective sound speed and refraction (wind-turbine noise): https://arxiv.org/pdf/1306.2753
- Shadow-zone scattering by turbulence (NAP): https://nap.nationalacademies.org/read/25226/chapter/9
- NASA ray tracing in sound-speed gradients: https://ntrs.nasa.gov/api/citations/19950008471/downloads/19950008471.pdf
- HRRR archive (AWS Open Data): https://registry.opendata.aws/noaa-hrrr-pds/
- ERA5 (Copernicus CDS): https://cds.climate.copernicus.eu/datasets/reanalysis-era5-pressure-levels
- libopus encoder source (dc_reject 3 Hz, VOIP hp_cutoff): https://github.com/xiph/opus/blob/main/src/opus_encoder.c
- libopus SILK tuning (VARIABLE_HP 60–100 Hz): https://github.com/xiph/opus/blob/main/silk/tuning_parameters.h
- AMR-WB pre-processing, 50 Hz HP (TS 26.190 §5.1 mirror): https://itecspec.com/3gpp/26.190/s/5.1
- 3GPP TS 26.190 v11: https://www.freecalypso.org/pub/GSM/3GPP/archive/26_series/26.190/26190-b00.pdf
- AMR-WB overview: https://en.wikipedia.org/wiki/Adaptive_Multi-Rate_Wideband
- ENF extraction with adaptive tracking (Ojowu et al. 2012): https://people.kth.se/~johan79/papers/OjowuKLL2012.pdf
- ENF and compression (Grigoras 2007, AES): https://aes2.org/publications/elibrary-page/?id=14331
- ENF timestamping evaluation (Routtenberg group): https://www.bgu.ac.il/en/researcher/tirza-routtenberg/publications/326329618/
