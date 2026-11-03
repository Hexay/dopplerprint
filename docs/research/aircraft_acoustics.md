# Aircraft acoustic sources for doppler_print

Scope: which aircraft produce Doppler-trackable tones, how loud they are and how far they carry, how stable
f0 is, how to label engine type from ADS-B, and how ground reflection shapes the spectrogram. Context:
`docs/research/transcript/analysis.md`.

Evidence tags: **[S]** stated in a cited source. **[D]** derived here from cited inputs (formula shown).
**[E]** engineering estimate or general acoustics knowledge, not verified this session. Calibrate every [E]
number on real recordings before relying on it.

## 1. Source spectra by class

### 1.1 Common physics
- Propeller and rotor rotational noise is a set of tones at the blade-passage frequency
  `BPF = N_blades * rpm / 60` and its integer multiples, sitting on broadband noise. The fundamental is
  usually the strongest tone **[S: Bucak et al. ICSV12; NASA annoyance studies]**.
- Conventional propeller aircraft have fundamentals of about **50–150 Hz**. Advanced turboprops can reach
  ~300 Hz **[S: NASA 19880008057 / 19920002540]**.
- Harmonic count and strength depend mainly on tip shape and tip Mach number **[S: NASA]**.
- SAE 760454 found that in forward flight the harmonics fall off quickly until they reach the broadband floor
  at about the **6th harmonic** **[S]**. Expect roughly 3–8 usable harmonics.
- NASA and DLR measured en-route twin turboprops at **17,000–21,000 ft**. Their spectra were "dominated by the
  propeller rotational noise fundamental", with pronounced **beats** because the two propellers ran at
  slightly different speeds **[S: Dobrzynski, NASA 19900015543]**. Props are therefore tonal enough to
  measure from 5–6 km.
- Ferguson & Quinn (JASA 96(2):821, 1994) tracked the turboprop blade-rate line with one ground microphone.
  From its Doppler history they estimated speed, altitude (250–1500 ft) and rest frequency, checked against
  onboard logs at 150–250 kn **[S]**. That is the single-event version of our method, 30 years earlier.

### 1.2 Piston props (C172, PA-28, SR22, Bonanza)
- **Direct drive**: prop rpm equals crank rpm, so every tone is a multiple of the shaft rate `f_s = rpm/60`.
- **Exhaust**: the exhaust is the dominant engine source, more than casing radiation
  **[S: DLR, Lycoming IO-360]**. Four-stroke firing frequency is `f_fire = n_cyl * rpm / 120`.
  - 4-cylinder (O-320/O-360): `f_fire = 2 f_s`, the same as a 2-blade BPF, so prop and exhaust tones
    **coincide** **[D]**.
  - 6-cylinder (IO-550/IO-540): `f_fire = 3 f_s`, the same as a 3-blade BPF **[D]**.
  - Half-order lines (`0.5 f_s` multiples) come from cylinder-to-cylinder irregularity in a four-stroke
    **[E]**.
  - A 1937 JASA study found exhaust tones may be absent or not harmonically related to firing frequency,
    depending on exhaust layout **[S]**. Don't hard-code the comb.
- **Typical values [D, rpm from POH ranges, E]**:
  - C172 (O-320, 2-blade fixed pitch, ≈2100–2700 rpm): BPF ≈ **70–90 Hz**, `f_s` ≈ 35–45 Hz.
  - PA-28-161/181: about the same, BPF ≈ **70–90 Hz**.
  - SR22 (IO-550, 3-blade CS, 2500–2700 rpm): BPF ≈ **125–135 Hz**, firing frequency the same.
- **Comb test**: a dense comb at `f_s` spacing (≈40 Hz) with a strong 2nd order is the piston signature.
  It is a useful class discriminator **[E]**.

### 1.3 Turboprops (King Air, PC-12, Dash 8, ATR)
- The propeller turns through a reduction gearbox, so the comb spacing is the BPF. Turbine and compressor
  tones are kHz-range and not harmonically related to it **[E]**.
- **Dash 8 Q400** (Dowty R408, 6 blades):
  - Take-off and climb at **1020 rpm** (100%) **[S: EASA/CAA TCDS]**, giving BPF **102 Hz** **[D]**.
  - Cruise at 850 rpm, giving BPF **85 Hz**, with harmonics clearly visible
    **[S: vibrationdata, secondary]**.
  - Synchrophasing locks the two props together **[S: Bombardier cabin-noise paper]**.
- **ATR 72** (6 blades, Ø 3.93 m) **[S]**:
  - NP is 100% for take-off and max continuous, and 82% for climb and cruise
    **[S: study aids, unofficial]**.
  - If 100% = 1200 rpm **[E, unverified]**, BPF is **120 Hz** at take-off and ~**98 Hz** in cruise **[D]**.
- **King Air** (PT6A, 3- or 4-blade Hartzell; 5-blade STCs exist) **[S]**:
  - Typical POH speeds of 1700–2200 rpm **[E]** give BPF ≈ **85–150 Hz** depending on blade count **[D]**.
- **PC-12** (4- or 5-blade, ~1700 rpm) **[E]**: BPF ≈ **113–142 Hz** **[D]**.
- **Twins**: two ridges a fraction of a Hz apart, or one ridge with amplitude beats at `|f1 − f2|` if the
  props are not synchrophased **[S: NASA en-route]**.

### 1.4 Helicopters (R44, EC135)
- Main-rotor lines sit at about **8–35 Hz** and tail-rotor lines higher. Detection systems match both with a
  ~2% rotor-speed tolerance **[S: EP0548689B1]**.
- **Robinson R44** (2-blade main, 2-blade tail):
  - Main rotor runs at 400 rpm nominal, power-on limits 101–102% (404–408 rpm) **[S: R44 POH]**.
  - Main BPF is **13.3–13.6 Hz** **[D]**.
  - Tail rotor: from the POH gear ratios (57/11 then 31/27), ≈2,380–2,430 rpm, giving BPF ≈ **80 Hz** **[D]**.
  - The tail-rotor tone and its harmonics are the practical tracking target. The main-rotor fundamental is
    below the high-pass filter of most consumer microphones, but its harmonics and blade-slap impulses
    reach hundreds of Hz.
- **EC135** (4-blade hingeless main, ~400 rpm **[S: secondary]**):
  - Main BPF ≈ **26.7 Hz** **[D]**.
  - The Fenestron has 10 rotor blades, unevenly spaced on purpose ("phase modulation"), at about 3,545 rpm
    **[S: informal]**. The nominal BPF is ≈590 Hz, but the uneven spacing spreads that energy into several
    lines **[S: yarchive/AIN]**.
  - DLR flight tests found the Fenestron mostly broadband in hover and tonal at high speed with sideslip
    **[S: elib.dlr.de/138687]**.
- **Strength**: rotor rpm is governed tightly, so f0 is very stable (best class for f0 stability).
- **Weakness**: the motion often isn't constant-velocity (hover, orbits, low speed). The constant-velocity
  fit breaks there, and only the full-trajectory Doppler model (section 8 of the transcript analysis) works.
- **Range**: processing raised R44 detection range from 4.1 km to 17.75 km (cyclic Wiener filtering), and an
  R22 kept a 77.8% detection rate at 11–13 km **[S: MDPI Appl. Sci. 12(13):6632 via search snippet]**.

### 1.5 Turbofans and jets: are there trackable tones?
Yes, but they behave differently:
- **Fan BPF** = `N1 * N_fan / 60`, typically **1.5–3.5 kHz** **[E]**.
  - TU Delft recovered N1 from >400 A330 flyovers by finding the Doppler-corrected fan BPF and its harmonics
    **[S]**.
  - A later method tracks fan harmonics in de-Dopplerized spectrograms with dynamic programming **[S]**.
  - At low fan speed the BPF tone may be cut off (it does not propagate), so a missing tone proves nothing
    **[S: NASA 20040027959]**.
- **Buzz-saw / MPT**:
  - Appears at supersonic fan-tip speed during take-off and climb. Approach is dominated by the subsonic
    BPF whine **[S: NASA 19770026222, Han 2007]**.
  - Blade-to-blade differences create tones at every shaft order. With `f_s` ≈ 50–90 Hz **[E]**, that gives a
    dense comb that looks prop-like.
  - It radiates forward from the inlet and decays nonlinearly with distance **[S]**.
- **Airframe tones**: the A320-family fuel over-pressure cavity whistle on approach. Its frequency depends on
  Mach number, and vortex generators reduce it 2–6 dB **[S: LAWA, Euronoise 2018]**. The often-quoted
  ~500–600 Hz is unverified **[E]**.
- **Why exclusion still holds**:
  - kHz fan tones lose **10–25 dB/km** to absorption (table in section 2.3), so they only survive near
    airports.
  - N1 changes continuously, so f0 is not stable.
  - Jets are usually high and fast.
  - Buzz-saw combs could still be mistaken for props, which is another reason to "exclude known jets" and
    not trust unknowns blindly.

## 2. Levels, directivity, propagation, audibility

### 2.1 Source levels (calibration anchors)
- **C172R noise-certification level**: **76.3 dB(A)** against a limit of 84.7 **[S: CAA/EASA sheet]**. The
  certification event is the 14 CFR 36 App. G take-off, with the microphone 2,500 m from brake release and
  the aircraft a few hundred metres up **[E for height]**.
- **A-weighting hides props**: weighting is ≈ −22 dB at 80 Hz and −13 dB at 160 Hz. The **unweighted** BPF
  tone is therefore ~10–20 dB above what the dBA figure suggests **[E]**.
- **Working anchor [E]**: a light single at full power puts its BPF tone at roughly **80–90 dB re 20 µPa at
  300 m**, about 35–40 dB at 1 m-equivalent above 110 dB. Twin turboprops are about +5–10 dB on that.
  Helicopter tail-rotor and main-rotor harmonics are comparable to or higher than a light single.
- **Power dependence**: helicopter detection range roughly halves for a 27% main-rotor rpm reduction
  **[S: NASA 19920003380]**. Levels are strongly power- and rpm-dependent, so expect ±5 dB between climb and
  cruise, and more on an idle descent **[E]**.

### 2.2 Directivity [E]
- **Propeller rotational noise**: peaks near the rotation plane, slightly aft (~100–110° from the forward
  axis), with a null on the axis. At long range the head-on approaching segment is weaker than the abeam
  segment, so ridges fade in toward CPA.
- **Moving source**: convective amplification adds roughly `(1 − M cos θ)^-2` to `^-3` on approach (+1–2 dB
  at M = 0.2).
- **Jets**: fan tones radiate forward from the inlet and aft from the bypass duct. Jet mixing noise peaks
  30–40° off the aft axis.
- **Modelling**: use a directivity term `D(θ)` per class in the audibility model. For the Doppler fit itself,
  directivity only reweights the curve.

### 2.3 Atmospheric absorption, ISO 9613-1
Pure-tone attenuation coefficient α(f, T, RH, p), valid 50 Hz–10 kHz and −20 to +50 °C **[S: ISO]**. Below
are values in dB/km computed here from the ISO 9613-1 equations (Sengpiel's implementation) **[D]**:

| T, RH | 63 Hz | 125 | 250 | 500 | 1k | 2k | 4k |
|---|---|---|---|---|---|---|---|
| 20 °C, 70% | 0.09 | 0.33 | 1.12 | 2.79 | 4.98 | 9.0 | 23.1 |
| 10 °C, 70% | 0.12 | 0.41 | 1.04 | 1.92 | 3.66 | 9.7 | 33.1 |
| 0 °C, 30% | 0.22 | 0.47 | 1.16 | 3.72 | 12.7 | 36.1 | 69.2 |
| 30 °C, 20% | 0.21 | 0.72 | 1.86 | 3.40 | 6.00 | 14.6 | 47.5 |

- The 10 °C row matches the legacy 9613-2 band table (0.4 / 1.0 / 1.9 dB/km at 125 / 250 / 500 Hz)
  **[S: windPRO appendix]**, which checks the implementation.
- **Below 150 Hz, absorption is negligible to ~10 km.** The prop fundamental is limited by spreading,
  ground effects, refraction and ambient noise, not absorption. Higher harmonics and fan tones are limited by
  absorption.
- Implement α in Rust from the closed-form equations. Use T, RH and p from reanalysis (ERA5/HRRR) for the
  event hour.

### 2.4 Other propagation terms [E]
- **Spherical spreading**: −20 log10(r/r0).
- **Ground** (section 5): about +3 dB on average for incoherent addition, with deep notches when coherent.
- **Refraction**:
  - Daytime lapse conditions cast a shadow zone at low elevation angles, costing 10–20 dB for aircraft
    below ~5–10° elevation.
  - Night inversions and downwind propagation do the opposite and enhance range.
  - Elevations above ~15° are barely affected.
- **Wind**: the effective sound speed shifts by `w·cos(azimuth)`, which also biases Doppler.
- **Sound speed**: c = 331.3 + 0.606·T(°C). Use the layer-mean temperature along the path.
- **Turbulence**: amplitude scintillation of a few dB, and spectral broadening that grows with frequency.

### 2.5 Receiver and ambient
- **MEMS self-noise**: equivalent input noise = 94 − SNR. A 65 dBA-SNR part has 29 dBA EIN, and common parts
  are 25–35 dBA **[S: Analog Devices MS-2348, patent]**. That is far below outdoor ambient at 50–200 Hz, so
  outdoors the **ambient sets the floor, not the mic**.
- **Camera audio chains**: these often add high-pass filters (100–300 Hz), AGC, noise suppression, 8–16 kHz
  sampling and lossy codecs **[E]**.
  - The fundamental may be attenuated, so harmonic stacking matters.
  - AGC makes absolute level unreliable. **Estimate N0(f) from the recording itself** and work in relative
    SNR.
- **Ambient spectral density near 100 Hz [E, order of magnitude]**:
  - Quiet rural night: 1/3-octave ≈ 25–35 dB, giving N0 ≈ **12–22 dB/Hz**.
  - Suburban day: ≈ 40–50 dB, giving **27–37 dB/Hz**.
  - Urban traffic: ≈ 50–65 dB, giving **37–52 dB/Hz**.
- **Human audibility (NPS)**: the audibility threshold is D'L = 7 dB, validated with attentive park
  listeners. The noticeability threshold is D'L = 17 dB **[S: NPS/Volpe]**. Air tours are clearly audible at
  **>10 km** in Hawai'i Volcanoes and Denali **[S: NPS 2023, Betchkal et al.]**. NPS-ActiveSpace is the
  open-source tool behind this.

### 2.6 Detection model (formula) [D/E]
```
L_k(r, t)  = L_k,ref(class, power) + D(θ) − 20·log10(r / r_ref) − α(k·f0, T, RH)·r/1000 + G(f, θ, h_rx)
SNR_bin    = L_k − [N0(k·f0) + 10·log10(Δf)]
SNR_track  = SNR_bin + 5·log10(N_frames_on_ridge) + 10·log10(K_eff)   # incoherent integration (approx.)
audible    ⇔ max over the event window of SNR_track ≥ θ_det   (θ_det ≈ 6–10 dB)
```
Worked example:
- Inputs: light single, fundamental tone 85 dB at 300 m; Δf = 1 Hz; a 30 s window integrates about 30
  independent frames, worth +7 dB.
- **Quiet rural** (N0 ≈ 20 dB/Hz): r = 3 km gives 65 dB (SNR ≈ 45). r = 10 km gives ≈ 54 dB (SNR ≈ 34).
  Refraction and directivity then decide. This agrees with NPS ">10 km audible".
- **Urban** (N0 ≈ 45 dB/Hz): SNR reaches 0 dB in a single bin near r ≈ 10 km, and integration brings the
  ridge to the visibility limit around 3–8 km. This matches colsto's "faint, only visible zoomed in".

Defaults to calibrate. These are slant ranges at which a prop ridge is still detectable in a spectrogram
**[E]**:

| class | quiet rural | suburban | urban |
|---|---|---|---|
| piston single | 6–12 km | 3–6 km | 1–3 km |
| turboprop twin | 10–20 km | 5–10 km | 2–5 km |
| helicopter | 10–18 km | 5–10 km | 2–5 km |
| jet, fan tone only | 2–4 km | 1–2 km | <1 km |

## 3. Is f0 constant through a flyover?

| prop / rotor type | within one pass (≈30–120 s) | between passes / phases |
|---|---|---|
| Fixed-pitch (C172, PA-28, many trainers) | Rpm follows throttle and airspeed. The prop is "sensitive to variations in airspeed" **[S: Kitplanes]**. Level cruise: ±1% **[E]**. Climb, descent or manoeuvring: ±3–10% **[E]**. | 2100–2700 rpm across phases (±12%) **[E]**. |
| Constant-speed (SR22, Bonanza, all turboprops) | The governor holds the selected rpm "irrespective of engine torque, airspeed or altitude" **[S: SKYbrary]**: ±0.5–1% **[E]**. Brief transients on power changes. | Discrete steps: Q400 1020 → 850 rpm (−17%) **[S]**; ATR 100% → 82% NP **[S]**. |
| Helicopter rotor | Governed within 101–102% (R44 power-on) **[S]**, so ±1%. Main and tail rpm are locked by gear ratio. | 90–108% range power-off **[S]**. |
| Turbofan N1 | Changes continuously with thrust, so the fan BPF drifts. TU Delft estimates N1 per flyover **[S]**. | Large. |

- **Twins**: two BPFs. Unsynchronised twins can differ by about 0.5–2%, which reads as beats or a doubled
  ridge.
- **Practical rule**: hold f0 constant within an event, but allow a slow linear drift
  `f0(t) = f0·(1 + β·t)` with |β| ≤ 2e-4 /s. Fit f0 freely per event. Never share f0 across events, even for
  the same aircraft, because the transcript's event 2 sweeps f0 independently.
- **Manoeuvring**: transcript event 1's left side didn't fit, and turns cause both v_r changes and rpm
  changes. The full-trajectory model handles the v_r part. Treat the rest as unexplained residual.

## 4. Engine type from ADS-B

### 4.1 ICAO Doc 8643 type description
The description has three characters: category, engine count, engine type.
- **Category**: L landplane, S seaplane, A amphibian, H helicopter, G gyrocopter, T tilt-wing/tiltrotor.
- **Count**: 1, 2, 3, 4, 6 or 8. `C` means coupled engines driving one propeller system.
- **Engine type**: P piston, T turboprop/turboshaft, J jet, E electric, R rocket. P/T/J are confirmed
  **[S: VATSIM KB, SKYbrary]**. E and R are from Doc 8643 conventions **[E]**.

Examples: C172 = L1P, B350 = L2T, DH8D = L2T, AT76 = L2T, R44 = H1P, EC35 = H2T, A320 = L2J.
- **Rule**: 3rd character in {P, T}, or 1st character in {H, G} → candidate. J → exclude. Unknown → keep, as
  colsto does.
- **Where to get it**:
  - ICAO sells Doc 8643 and offers data downloads via ICAOAPI@icao.int **[S]**.
  - FAA Order **JO 7360.1G** (PDF) mirrors the designators with engine type and count **[S]**.

### 4.2 Machine-readable hex → type → engine sources

| source | key | engine info | notes |
|---|---|---|---|
| **tar1090-db** (wiedehopf, from the Mictronics DB) | ICAO hex | type designator; long type; `desc` such as L2J via the types table | `aircraft.csv.gz` on the `csv` branch. readsb `--db-file` consumes it and `--db-file-lt` adds `desc` **[S: README, readsb man page]**. Column order is not verified here, so read the header and `csv.sh`. |
| **ADS-B Exchange basic-ac-db.json.gz** | ICAO hex | `icaotype` and **`short_type`** (e.g. L2J) **[S]** | Rebuilt daily at 23:30 UTC from downloads.adsbexchange.com **[S]**. Exact key spelling is unverified. |
| **OpenSky aircraftDatabase.csv** | icao24 | typecode, **icaoaircrafttype** (the L2J-style field), model, operator **[S: mirror]** | Monthly snapshots under `/datasets/metadata/` **[S]**. Coverage has gaps. When the aircraft is missing, the typecode is empty in OpenSky flight datasets **[S]**. |
| **FAA Releasable Aircraft DB** | `MODE S CODE HEX` in MASTER | ACFTREF via MFR-MDL-CODE gives type-aircraft, **type-engine**, engine count, weight class | Refreshed daily **[S]**. Type-engine codes **[E, from the ardata.pdf layout, which returned 403 here]**: 0 none, 1 reciprocating, 2 turbo-prop, 3 turbo-shaft, 4 turbo-jet, 5 turbo-fan, 6 ramjet, 7 2-cycle, 8 4-cycle, 9 unknown, 10 electric, 11 rotary. Authoritative for N-registered (US) aircraft and needs no type-designator mapping. |
| Mictronics readsb-protobuf DB | hex | same lineage as tar1090-db | Upstream of tar1090-db. |

**Recommended join**: the FAA registry (US, authoritative engine type), then tar1090-db or ADSBx `desc`, then
OpenSky `icaoaircrafttype`, then the emitter category, then unknown. Record which source supplied each label.

### 4.3 ADS-B emitter category (TC 1–4 identification message, DO-260B §2.2.3.2.5.2)
Set A:
- A0 none.
- A1 light, MTOW < 15,500 lb.
- A2 small, 15.5–75k lb.
- A3 large, 75–300k lb.
- A4 high-vortex large (B757).
- A5 heavy, > 300k lb.
- A6 high-performance (> 5 g and > 400 kt).
- A7 rotorcraft.

Set B: B1 glider, B2 lighter-than-air, B3 parachutist, B4 ultralight/hang-glider, B6 UAV, B7 space.
Set C: surface vehicles and obstacles **[S: ADSBx support, FlightAware]**.
- **Use**: A7 maps reliably to the helicopter class.
- **Limit**: A1 and A2 do **not** separate props from jets. Very light jets (C510, PC-24 class) are A1, and
  ATR72 / Q400 / CRJ200 are all A2 **[E from MTOWs]**. Use it only as a fallback prior, never to exclude.
- **Gaps**: the category is often missing or wrong on older transponders. Some decoders (VRS) mislabel the
  codes **[S]**.

## 5. Ground effects: Lloyd's mirror

**Model.** The microphone sits at height h, the source at height H, horizontal distance d, slant range R and
elevation angle θ. The reflected path is longer by
```
Δ = sqrt(d² + (H + h)²) − sqrt(d² + (H − h)²) ≈ 2·h·H / R = 2·h·sin θ   (H ≫ h)
Δτ = Δ / c ;   received |p|² ∝ |1 + Q·e^{−i 2π f Δτ}|²
```
Here Q is the spherical-wave reflection coefficient.

**Notches.**
- Rigid ground (Q ≈ +1, a reasonable model for steep angles and low frequency over most ground): nulls at
  `f_n = (2n + 1)·c / (4·h·sin θ)` and peaks at `m·c / (2·h·sin θ)` (up to +6 dB) **[D]**.
- Grass or soft soil: Q has finite impedance, so notches shift, shallow and smear at higher f and lower θ.
  Near grazing, Q → −1 gives the 200–600 Hz "ground dip" **[E]**.

**Scale for a camera on a pole [D].**
| h | 1st null, overhead (θ = 90°) | null spacing | at θ = 30° |
|---|---|---|---|
| 1.5 m | 57 Hz | 114 Hz | 114 Hz |
| 3 m | 29 Hz | 57 Hz | 57 Hz |
| 5 m | 17 Hz | 34 Hz | 34 Hz |

So the notches run straight through the 50–300 Hz prop band.

**Time–frequency shape.** As the aircraft passes, sin θ(t) is largest at CPA. Each notch therefore traces a
**∩-shaped curve whose lowest frequency is at CPA**, rising toward infinity at grazing. These are the
"interference fringes" of the time-frequency distribution **[S: Lo & Ferguson; Springer s11767-005-0215-6;
DRDO DSJ]**.
- Lo & Ferguson et al. estimated **speed, altitude and elevation angle** of low-flying aircraft from one
  sensor using these fringes, and also from multipath delays (IEEE TAES 39(1):259, 2003) **[S]**.
- Adaptive cepstral filtering can remove multipath artefacts and raise the SNR of aircraft signals
  (arXiv 2512.11165) **[S]**.

**Impact on ridge tracking.**
- The Doppler ridge crosses fringe notches, and each crossing fades it by 10–30 dB for one to a few seconds.
  A brightness-sum score then penalises the true curve unevenly.
- A wall or building next to the camera adds a second fringe family that depends on azimuth.
- Fringes are only visible where there is broadband source energy, so a faint tonal-only recording may show
  them only as ridge dropouts.

**Extra information.**
- The fringe curves constrain `h·sin θ(t)` independently of Doppler. With h roughly known (pole camera
  3–8 m), they give the elevation-angle history, so H/R and an altitude check.
- Ridge dropout times are a cheap version of the same thing: predict notch crossings per candidate and
  compare them with the observed fades.

## 6. Design implications for doppler_print

**Labelling (Rust `engine_class` module)**
1. Classes: `Piston`, `Turboprop`, `Heli`, `Jet`, `Unknown`.
   - Label from the join chain in section 4.2.
   - Exclude only `Jet`. Keep `Unknown`, as colsto did.
   - Store `label_source` and `n_engines`.
2. Map `H**` or emitter category A7 to `Heli`. Never exclude on emitter category alone.

**Spectrogram front end**
3. Band of interest: **10–1000 Hz** for prop and heli ridges, plus 1–4 kHz only if jet near-airport
   matching is ever wanted. Downsample to 4 kHz for the main pass.
4. Analysis window: `T ≈ 1/sqrt(max|df/dt|)` with `max|df/dt| ≈ f0·v²/(c·d_cpa)`.
   - C172 at 57 m/s, 500 m, 80 Hz: 1.5 Hz/s, so T ≈ 0.8 s.
   - Q400 at 70 m/s, 800 m: T ≈ 0.75 s.
   - Jet fan tone at 1.8 kHz, 75 m/s, 400 m: 74 Hz/s, so T ≈ 0.12 s.
   - Default **0.75 s Hann, 87.5% overlap, zero-pad to 0.25 Hz bins**. Add a long-window (2–4 s) pass for
     distant, slow-chirp events.
5. Whiten each frequency bin by its median over time. This handles AGC and camera EQ (the recording-derived
   N0(f)). Score log-power above the floor, not raw brightness.

**Doppler scoring**
6. Score harmonics jointly: sum the whitened power at `k·f(t)` for k = 1..K with K = 6 by default (props
   decay into noise around the 6th harmonic).
   - Use per-harmonic max-pooling over ±1 bin.
   - Allow missing harmonics, e.g. sum the top K−2 of K.
   - This also recovers fundamentals removed by the camera's high-pass filter.
7. f0 priors for the sweep, with a ±10% margin on each range:
   - Piston 55–140 Hz.
   - Turboprop 80–150 Hz.
   - Heli main 10–30 Hz and tail 50–120 Hz; for EC135-type Fenestrons, also 400–700 Hz.
   - Unknown 40–200 Hz.
   - Plus a drift term |β| ≤ 2e-4 /s.
   - Expected Doppler swing: ±15–25% about f0 (C172: 96 → 69 Hz for an 80 Hz f0).
8. Add a piston-comb discriminator: a significant line at `f_s = f0 / N_blades` multiples (shaft orders)
   suggests piston, and a clean BPF comb suggests turboprop. Use it as a soft prior on candidate flights,
   not a filter.
9. Make the scoring robust to Lloyd's-mirror fades:
   - Use a trimmed or median-based line integral (drop the worst 20% of frames).
   - Or multiply the expected-power model by `|1 + e^{−i2πfΔτ(t)}|²`, fitting h ∈ [1, 10] m as a per-site
     nuisance parameter shared across events. This gives an extra altitude/range check for free.
10. Use the temperature-dependent c and the wind along the path from reanalysis in the retarded-time solve.
    A 10 °C error shifts c by 1.8%, which maps directly into the speed fit.

**Silence / audibility model (Rust `audibility` module)**
11. Implement section 2.6 with the closed-form ISO 9613-1 α, per-class `L_ref` and `D(θ)` tables, a +3 dB
    incoherent ground term, and a refraction penalty of `−10 dB·max(0, 1 − θ/10°)` for elevations below 10°
    (by day).
12. Take N0(f) from the clip itself, as the median of the whitening baseline. Don't guess the environment.
13. Make the silence rule asymmetric. Disqualify a candidate only when a flight's predicted `SNR_track`
    exceeds θ_det + **margin 10 dB** in some window and no ridge is seen.
    - Losing the true location costs far more than keeping a false one.
    - Use the low end of the source-level range and the high end of the absorption/refraction penalties.
    - Apply this to the `Unknown` class too, with jet-like levels only for fan-tone bands.
14. Default "should have been heard" slant ranges: the table in section 2.6, scaled to the measured N0.
    Re-fit `L_ref` from the solved case (colsto-style ground truth) once one exists.

**Validation**
15. Build a synthetic-flyover generator from the same physics:
    - Inputs: trajectory, harmonic comb with drift, spreading, α, Lloyd's mirror, whitened ambient.
    - Use it for unit tests of the retarded-time solver and the scorer.
    - Add regression clips with known ADS-B, e.g. Ferguson-style straight passes recorded locally with a
      readsb receiver.

## Sources
- Bucak, Franjković, Galović, "Acoustical analysis of a light aircraft propeller noise" (ICSV12, 2005): https://www.bib.irb.hr/195232
- NASA propeller annoyance / fundamental-range studies: https://ntrs.nasa.gov/api/citations/19880008057/downloads/19880008057.pdf , https://ntrs.nasa.gov/api/citations/19920002540/downloads/19920002540.pdf
- NASA Cessna 172 two-blade propeller report: https://ntrs.nasa.gov/api/citations/19810070761/downloads/19810070761.pdf
- SAE 760454, Progress report on propeller aircraft flyover noise research: https://www.doi.org/10.4271/760454
- Dobrzynski, en-route turboprop noise (NASA 19900015543): https://ntrs.nasa.gov/citations/19900015543
- Ferguson & Quinn, JASA 96(2):821 (1994): https://researchers.mq.edu.au/en/publications/application-of-the-short-time-fourier-transform-and-the-wigner-vi/
- Ferguson ASA 1995 abstract: https://auditory.org/asamtgs/asa95stl/4pSP/4pSP10.html
- CAA/EASA light-aircraft noise certification sheet (C172R 76.3 dBA): https://caa.co.uk/Documents/Download/3934/ca692457-336e-4190-840e-cd26cf498935/3290
- DLR IO-360 exhaust-noise study: https://elib.dlr.de/36362
- Exhaust tones (JASA 1937, summary): https://labs.sonicfield.org/library/sound-frequencies-from-an-airplane-exhaust-system
- Light propeller aircraft noise (IOA, Chusseau et al.; propeller + exhaust sources): https://www.ioa.org.uk/system/files/publications/M%20CHUSSEAU%20D%20BIRON%20S%20PAUZIN%20P%20MATHARAN%20LIGHT%20PROPELLER%20AIRCRAFT%20NOISE%20STUDY%20EXPERIMENTAL%20APPROACH%20TO%20I.pdf
- Dowty R408 TCDS (EASA): https://www.easa.europa.eu/en/downloads/7905/en ; CAA: https://caa.co.uk/Documents/Download/3940/77d2f18a-4292-443a-8980-607aade5d45a/3401
- Q400 acoustics (85 Hz cruise BPF): https://blog.vibrationdata.com/?p=1346
- Dash-8 Q cabin noise / synchrophasing: https://jcaa.caa-aca.ca/index.php/jcaa/article/download/1288/1028
- ATR 72-600 factsheet: https://www.atr-aircraft.com/wp-content/uploads/2020/07/Factsheets_-_ATR_72-600.pdf ; NP settings (unofficial): https://brainscape.com/flashcards/power-plant-5338131/packs/7993095
- King Air 350 props: https://skybrary.aero/aircraft/b350 , https://nzdf.mil.nz/nzdf/our-equipment/aircraft/king-air-350
- Robinson R44 POH (rotor limits, gear ratios): https://robinsonheli.com/wp-content/uploads/2024/04/r44_poh_1.pdf , https://robinsonheli.com/wp-content/uploads/2024/04/r44_poh_full_book.pdf ; EASA TCDS: https://www.easa.europa.eu/en/downloads/7929/en
- Helicopter spectral-line detection patent: https://patents.google.com/patent/EP0548689B1/en
- Fenestron phase modulation: https://www.yarchive.net/air/helicopter_tail_rotor_noise.html , https://yarchive.net/air/fenestron.html , https://backend.ainonline.com/aviation-news/business-aviation/2008-05-02/oems-airframe-mods-key-noise-reduction ; DLR EC135 Fenestron flight test: https://elib.dlr.de/138687
- EC135 main rotor: https://www.globalair.com/aircraft-specifications/eurocopter/eurocopter-ec-135-t1-specifications/648 , https://natodays.cz/aircraft/ec135
- Helicopter long-range detection (R44 17.75 km): https://www.mdpi.com/2076-3417/12/13/6632 ; rotor-rpm vs detection range: https://ntrs.nasa.gov/api/citations/19920003380/downloads/19920003380.pdf
- Buzz-saw / MPT: https://ntrs.nasa.gov/api/citations/19770026222/downloads/19770026222.pdf , https://www.aere.iastate.edu/caal/assets/papers/han_2007_mpt.pdf , https://hal.archives-ouvertes.fr/hal-01067840
- Fan rpm from flyover recordings (TU Delft, A330): https://research.tudelft.nl/en/publications/calculation-of-the-fan-rotational-speed-based-on-flyover-recordin ; https://www.citedrive.com/en/discovery/estimation-of-the-fan-rotational-speed-using-flyover-audio-recordings/
- Fan tone cut-off at low speed: https://ntrs.nasa.gov/api/citations/20040027959/downloads/20040027959.pdf ; spectrogram Doppler reading: https://www.nap.edu/read/22606/chapter/18
- A320 FOPC whistle: https://www.euronoise2018.eu/index.php/component/contentbuilder/details/11/48/conference_programme-4-2-aircraft-helicopter-noise , https://www.lawa.org/media/26384 , https://airportnoise.torontopearson.com/a320-family-retrofit-program/
- ISO 9613-1: https://www.iso.org/standard/17426.html ; formula: https://sengpielaudio.com/AirdampingFormula.htm ; legacy band table: https://help.emd.dk/knowledgebase/content/windPRO4.1/Appendix_A_DECIBEL.pdf
- NPS audibility (D'L thresholds, NMSIM): https://rosap.ntl.bts.gov/view/dot/54900/dot_54900_DS1.pdf , https://www.govinfo.gov/content/pkg/FR-2003-11-07/pdf/03-28114.pdf ; Betchkal et al. 2023: https://home.nps.gov/articles/000/nsnsdarticle10_2023a.htm (doi:10.1016/j.jenvman.2023.119201)
- MEMS self-noise: https://www.analog.com/media/en/technical-documentation/technical-articles/Low-Self-Noise-The%20First-Step-to-High-Performance-MEMS-Microphone-Applications-MS-2348.pdf
- Constant-speed vs fixed-pitch props: https://skybrary.aero/articles/constant-speed-propeller , https://www.kitplanes.com/wind-tunnel-56/
- ICAO type description: https://knowledgebase.vatsim-germany.org/books/aircraft-knowledge/page/icao-type-description , https://skybrary.aero/articles/aircraft-description-icao-doc-8643 , https://www2023.icao.int/safety/OPS/OPS-Tools/Pages/aircraft-type-designators.aspx ; FAA JO 7360.1G: https://www.faa.gov/documentLibrary/media/Order/JO_7360.1G_Aircraft_Type_Designators_FINAL.pdf
- tar1090-db: https://github.com/wiedehopf/tar1090-db ; readsb man page: https://manpages.debian.org/unstable/readsb/readsb.1.en.html ; Mictronics readsb-protobuf: https://github.com/Mictronics/readsb-protobuf
- ADS-B Exchange DB fields / download: https://www.adsbexchange.com/?p=1650 ; emitter category: https://www.adsbexchange.com/?p=20078 , https://support.adsbexchange.com/hc/en-us/articles/44705224053517-Emitter-Category-ADS-B-DO-260B-2-2-3-2-5-2
- OpenSky aircraft DB (columns, monthly CSV): https://conormclaughlin.net/2024/03/making-large-csvs-queryable-via-rest-a-case-study-using-dbhub.io-to-create-a-serverless-api-for-aircraft-data/ , https://zenodo.org/records/4034521
- FAA Releasable Aircraft Database: https://www.faa.gov/licenses_certificates/aircraft_certification/aircraft_registry/releasable_aircraft_download/ ; layout: https://registry.faa.gov/database/ardata.pdf
- Emitter-category enum caveat (VRS): https://virtualradarserver.co.uk/SourceHelp/html/0d447530-d1c5-ae07-199c-299c31678c23.htm
- Lloyd's mirror flight-parameter estimation: https://link.springer.com/article/10.1007/s11767-005-0215-6 , https://publications.drdo.gov.in/ojs/index.php/dsj/article/download/3924/3750 , https://en.wikipedia.org/wiki/Lloyd%27s_mirror ; cepstral multipath filtering: https://arxiv.org/abs/2512.11165
