# Prior art, validation data, robustness, statistics, ethics: aircraft-Doppler receiver geolocation

Surveyed 2026-11-03. Sources: web, GitHub, Zenodo, NTRS/data.nasa.gov, and the local DOPPLERGEIST release in
`data/_downloads/dopplergeist-release/` (manifest + extracted `independent-reimplementation-notes.md`).
Method summary: [transcript/analysis.md](transcript/analysis.md).

Markers:
- **[UNV]**: from search snippets or background knowledge, not opened this session. Check before quoting.
- No marker: page or file read this session.

## 1. Prior art

### 1.1 Direct precedent: the same inversion (unknown receiver from aircraft Doppler + ADS-B)

1. **colsto, "This background sound leaks your exact location"** (YouTube). https://www.youtube.com/watch?v=UytYrhYLOKI
   - Origin of the method; see analysis.md.
   - Reported result: ~100 m, with a winner score of 20.48 against a runner-up of ~15.
   - No code, paper or uncertainty quantification.
   - The same channel published the thunder videos that thunderprint reconstructs (SnbkIm56TfI, 6a7Z0I3LAnE). The pattern each time is a public OSINT demo with no released code.
2. **1337isnot1337/DOPPLERGEIST** (MIT). https://github.com/1337isnot1337/DOPPLERGEIST
   - Pipeline:
     - harmonic-grouped track extraction plus an automatic moving-tone "discovery" stage
     - adsb.lol yearly archive for a window around the claimed time
     - forward model with retarded time, propagation delay, terrain and geoid
     - grid search, flyover reconciliation, and competing solutions kept
   - Reported errors, all marked as historical and not validated:

     | Case | Error | Status |
     |---|---|---|
     | Synthetic, two events | 21.9 m | |
     | Synthetic, known time | 40.9 m | |
     | Original clip | 43.9 m | Against an unsurveyed public reference; flagged `inconclusive/search_incomplete` and not accepted |
     | Faint flyover | 357 m | |
     | NASA Edge065 | 474 m | Inconclusive |

   - A synthetic national search ranked the true pair **4th**.
   - Explicit status: "no accepted blind real localization". The target is a supported fix within 1 km, preferably 100–500 m.
3. **mojomast, "aircraft-doppler-geolocation" agent skill v0.1.1** (MIT). It ships inside the DOPPLERGEIST release as `data/source/independent-reimplementation-notes.md`.
   - Small Python scripts:
     - `doppler_track.py`: ridge tracker that writes f0, speed, CPA slant distance, `ridge_snr_z` and `rejected_because`
     - `score_candidates.py`: RMS-Hz misfit ranking
     - a synthetic self-test
   - Useful numbers:
     - real tones score `ridge_snr_z` ≈ 2.7–3.6; noise scores below 2.5
     - the synthetic test recovers slant distance to 1 m with a known time
     - without a time, the synthetic test lands **~1.4 km along-track**. This is the documented single-event along-track ambiguity, plus left/right mirror ambiguity for straight tracks.
   - Reports "top-k + margin over 2nd" and treats a result as non-proof until visually confirmed.
4. GitHub searches ("doppler adsb geolocate audio" etc.) found no other reconstruction.
   - Neighbouring tools: clorth0/aetherscope (spectrograms tagged with ADS-B) and the iOS "Extra Ears" app (flyover prediction from OpenSky). Neither inverts for the receiver.
5. **Closest non-acoustic analogue: US10234536 "ADS-B geolocation"** [UNV detail]. It locates a ground receiver from the *RF* Doppler of 1090 MHz ADS-B messages against the aircraft's broadcast positions and velocities: Bayesian receiver localisation from aircraft Doppler, but in the radio domain.

**Verdict:** the core idea is not novel. colsto demonstrated it, and DOPPLERGEIST and the mojomast skill reproduce it. I found no peer-reviewed paper that solves for an unknown acoustic receiver from ADS-B. As with thunderprint, the open ground is rigour:
- calibrated false-localisation rate
- null/decoy controls
- blind real-world validation
- uncertainty regions

### 1.2 Forward-model physics: flight parameters from one microphone (receiver known)

- **Ferguson & Quinn 1994**, JASA 96(2):821–827. "Application of the short-time Fourier transform and the Wigner-Ville distribution to the acoustic localization of aircraft".
  - A turboprop flies straight and level over one microphone. From the blade-rate Doppler curve they get speed, CPA range, CPA time and the rest frequency.
  - This is exactly our constant-velocity event fit (analysis.md step 5).
  - https://researchers.mq.edu.au/en/publications/application-of-the-short-time-fourier-transform-and-the-wigner-vi/
  - Follow-ons:
    - Ferguson 1995 ASA abstract, https://auditory.org/asamtgs/asa95stl/4pSP/4pSP10.html
    - Lo & Ferguson 2000, IEEE TAES 36(1), broadband motion-parameter estimation [UNV]
    - Ferguson & Lo 2004, ISSNIP [UNV]
- **Pathak 2026, Zenodo 10.5281/zenodo.23424278** (CC BY 4.0). "Aeroplane data full analysis and raw files".
  - 40 smartphone flyovers in Livermore CA, May 2026.
  - Each pass has WAV, harmonic f–t CSV with UTC, an ADS-B KML track (geometric MSL) and a result PDF. Passes are binned into V/H constant/varying.
  - The companion paper "Aircraft speed and height from Doppler-shifted sound and flight-track data" was not found indexed. This is the dataset behind DOPPLERGEIST's "40 smartphone recordings". **See §2: the best ready-made validation set.**
- Single-microphone Doppler aircraft-state papers [UNV authorship]:
  - UPC theses: https://upcommons.upc.edu/entities/publication/9ae18a41-e532-4d46-95ea-581283c44d9e and https://upcommons.upc.edu/handle/2117/95089
  - DRDO DSJ spectrogram Doppler histories: https://publications.drdo.gov.in/ojs/index.php/dsj/article/download/2238/1198/7489

### 1.3 Seismic, DAS and infrasound networks that record aircraft Doppler

- **Meng & Ben-Zion 2018**, JGR Solid Earth 123(6):4783–4797. "Characteristics of airplanes and helicopters recorded by a dense seismic array near Anza, California".
  - 1,816 air-traffic events on the 2014 Sage Brush Flats nodal array, with a Doppler-modelling Matlab dataset: https://data.mendeley.com/datasets/kpftptxdj5
  - The receivers' positions are known; the source is estimated.
  - Follow-up: Zhang et al. 2022, ML detection of aircraft in seismic array data [UNV venue].
- **Eibl et al. 2017**, GJI 209(2):901–908. "Helicopter location and tracking using seismometer recordings".
  - From seismometers alone they recover blade count, RPM, speed, heading, altitude and position, and validate against the helicopter's GPS. This is the cleanest "acoustic/seismic Doppler vs GPS truth" precedent.
  - Companion: Eibl et al. 2015, JVGR 304:108–117. Tremor once attributed to Hekla turned out to be helicopters. https://researchrepository.ucd.ie/entities/publication/a6c4c008-8937-4b93-b045-6623d442d8cc
- **UAF / Alaska Earthquake Center (Nov 2025)** [UNV, press release only].
  - Seismic aircraft signals matched to Flightradar24 tracks. Removing the Doppler shift gives a per-type frequency signature.
  - ADS-B labels seismic data, which is the inverse of our direction. https://www.gi.alaska.edu/news/seismic-data-can-identify-aircraft-type
- **DAS:**
  - Sensors 21(15):5094 (2021): φ-OTDR on telecom fibre near Qinhuangdao airport shows aircraft Doppler. https://www.mdpi.com/1424-8220/21/15/5094
  - EAGE 2024: propeller Doppler on a buried telecom cable. https://www.earthdoc.org/content/papers/10.3997/2214-4609.202475013
  - Patent US10352763B2. https://patents.google.com/patent/US10352763
- **Infrasound arrays (IMS, sonic booms):** not searched this pass. They locate and characterise sources, not receivers.
- Thunderprint's survey has the receiver-from-catalogue analogues: station mislocation from catalogue residuals (Haase 1995) and clock drift from teleseisms. See `reference/thunderprint/research/prior_art_receiver_location.md` §5–9.

### 1.4 Bioacoustics: aircraft as a nuisance class

- **Buxton et al. 2019**, Front Ecol Environ, doi:10.1002/fee.2112.
  - NPS recordings from 66 US parks. Anthropogenic noise was audible in 37% of samples, and aircraft and road vehicles were the most common sources. https://esa.org/?p=33521
  - Per-park NPS reports quantify aircraft audibility, e.g. Great Basin, 25.8% of the day at one site: https://www.npshistory.com/publications/grba/sr-2026-438.html
- Aircraft are labelled and rejected, never localised.
- Takeaway: aircraft tones sit in an unremarkable fraction of passive-acoustic and bird recordings, which is what makes the **xeno-canto/Silent Cities validation idea (§2.3)** workable.
- BirdNET/AudioMoth aircraft classifiers and bat-detector literature were not searched in depth [gap].

### 1.5 ADS-B used as acoustic ground truth

- **AeroSonicDB (YPAD-0523)**, Downward et al. 2023. https://arxiv.org/abs/2311.06368
  - Aircraft audio near Adelaide airport, auto-labelled from a local dump1090 receiver.
  - Built for detection and classification; see §2.1.
- **Forum Acusticum 2026**: autonomous microphone plus automatic alignment of acoustic events to ADS-B telemetry, to cut labelling cost [authors UNV]. https://dael.euracoustics.org/confs/fa2026/data/articles/000004.pdf
- **TU Delft "Hear-and-Avoid"**: GA propeller overflights on a microphone array. https://b2find.eudat.eu/dataset/8a1a9b2a-51f8-516a-90bb-d494b00d739f
- Not found: microphone-array self-calibration from aircraft, or seismic clock/orientation correction from ADS-B. That inverse is open.

### 1.6 Analogous forensic techniques

- **ENF (mains hum):** the established "when, and coarsely where" from audio.
  - Grigoras 2005, IJSLL 12(1):63–76, https://journal.equinoxpub.com/IJSLL/article/view/9977
  - Garg et al., ICASSP 2013, https://www.mast.umd.edu/files/paper_ENF/conf_paper6_ICASSP13.pdf
  - Intra-grid feasibility, https://arxiv.org/abs/2105.00668
  - SWGDE ENF discussion paper (location needs reference data from several sites on one grid), https://www.swgde.org/documents/published-complete-listing/09-a-001-swgde-electric-network-frequency-discussion-paper/
  - Tool: RaphaelJ/libhum.
  - ENF is the closest *reporting* analogue: a match against a reference database, minimum-length caveats, "consistent with" wording.
- **OSINT audio:**
  - Forensic Architecture "audio analysis": synchronisation, weapon ID, echo profiling. https://forensis-preview.forensic-architecture.org/methodology/audio-analysis
  - Bellingcat / Washington Post, Abu Akleh case: shooter at 175–195 m from the gunshot muzzle-blast-to-crack gap. https://gijn.org/stories/how-two-forensic-teams-investigated-the-killing-of-journalist-shireen-abu-akleh/
  - Gunshot multilateration (NIJ), https://nij.ojp.gov/library/publications/audio-forensic-gunshot-analysis-and-multilateration ; urban accuracy study, https://arxiv.org/abs/2108.07377
  - These are single-event ranging or multi-sensor TDOA. None does a catalogue search.
- **OSINT flight tracking:**
  - Bellingcat Turnstone (historical flight data, 2026), https://www.bellingcat.com/resources/2026/03/05/turnstone-flight-tracking-tool/
  - ADS-B Exchange in the Bellingcat toolkit, https://bellingcat.gitbook.io/toolkit/more/all-tools/ads-b-exchange
  - Flight tracking is used to chronolocate *visible* aircraft. I found no case combining OSINT flight tracking with *audio* before colsto.

## 2. Validation data

There is no public corpus that is large, propeller-heavy, and has surveyed recorder position, absolute UTC and ADS-B
together. Pathak 2026 comes closest. A corpus is assembled in tiers below.

### 2.1 Ready-made, with ADS-B truth

| Dataset | Content | Location / time | Truth | Licence | Verdict |
|---|---|---|---|---|---|
| **Pathak 2026, Zenodo 23424278** | 40 smartphone flyovers, Livermore CA, May 2026, 136 MB | Recorder position not in the record (one site, unsurveyed?); f–t CSV in UTC | ADS-B KML per pass, geometric MSL | CC BY 4.0 | **Tier 1.** Already downloaded inside the DOPPLERGEIST release (`data-research.tar.gz`). Ask the author for recorder coordinates |
| **AeroSonicDB YPAD-0523** v1.1.2 (Zenodo 8004108; v2.0β 12775560) | 625 clips of 18–60 s (8.87 h), 301 airframes, ~150 props (114 turboprop, ~36 piston), 22.05 kHz | 3 sites near YPAD at site level, no published coordinates; absolute UTC [UNV] | ADS-B hex, altitude, ICAO type, engine/prop model | Access by request; paper CC BY 4.0 | **Tier 1–2** once the author supplies site coordinates and UTC; then pull full tracks by hex+time |
| NASA Small UAS Flyover Acoustics (data.nasa.gov) | Subscale Edge 540, multirotors; ground mics, RTK GPS | Surveyed mics, synced | Vehicle GPS/RTK | US Gov [UNV] | Likely source of DOPPLERGEIST's "Cub107/Edge0xx" MAT cases [UNV]. Geometry truth with no ADS-B: good for **forward-model** validation |
| NASA STRIVES eVTOL / Amedee MD530F | Rotor tones, mic arrays, UTC, vehicle data | Surveyed | GPS | [UNV] | Forward-model only; download not confirmed |
| TartanAviation (CMU 2024) | ADS-B + images + ATC speech, Pittsburgh | n/a | ADS-B | open | No engine audio; ADS-B format reference only |

- **Labels-only sets** have no location or time, so they serve only for training detectors and ridge extractors:
  - ESC-50 airplane/helicopter
  - FSD50K Aircraft
  - AudioSet Propeller/Fixed-wing
  - DCASE / SONYC-UST "engine" class: block-level location and hour-level time, too coarse
- **Not useful:**
  - X-59/Quesst: sonic booms, no props
  - FAA/Volpe/EASA: certification levels, not audio
  - Airport WebTrak/Envirosuite portals: per-event tracks and monitor levels, sometimes audio replay, but no bulk download and scraping is forbidden. Manual spot checks only.
- **"WSU":** DOPPLERGEIST names a 246.8 s "WSU recording with measured drone telemetry" without a URL. The institution and dataset are unidentified [UNV]. I found no WSU airport-noise audio archive.

### 2.2 ADS-B history for truth

- **adsb.lol globe_history:** daily readsb tarballs as GitHub releases, ODbL 1.0. Best free bulk source. Good feeder density in US/EU; DOPPLERGEIST uses it.
  - https://github.com/adsblol/globe_history_2025/releases
  - https://github.com/adsblol/globe_history_2024
- **OpenSky Trino** (pyopensky / traffic): ~1 s state vectors since 2013. Approved account; non-commercial research. Patchy low-altitude GA coverage.
- **ADS-B Exchange:** paid history. Free samples are the 1st of each month [UNV].
- **FlightAware AeroAPI:** paid, no redistribution. **airplanes.live:** live only [UNV].
- **Type → engine lookup:**
  - ICAO Doc 8643 description codes (`L1P`, `L2T`, ...; P/T/J/E)
  - tar1090-db (hex → registration and type)
  - OpenSky aircraft DB
- **Systematic gaps:**
  - FAA LADD and PIA (rotating ICAO address)
  - non-ADS-B GA; 978 MHz UAT in the US is often absent from 1090-only feeds
  - military
  - These are false negatives *and* break the "silence is evidence" filter (analysis.md step 7).

### 2.3 Opportunistic geotagged audio (corpus mining)

- **xeno-canto:** each recording carries lat/lon (~100 m–1 km), date and local HH:MM (no TZ; derive from position). Licences mostly CC BY-NC-SA/ND.
  - API v3 at `https://xeno-canto.org/api/3/recordings?query=…&key=…` needs a member key.
  - A remarks filter exists; the exact `rmk:` syntax is [UNV] because the docs page was bot-blocked.
  - Minute-level time is enough to shortlist ADS-B candidates, after which the Doppler fit itself pins the time.
  - Many aircraft are in the background and not mentioned in remarks. An aircraft-tone detector run over a bbox/date slice would find far more than a remarks search.
- **Silent Cities** (Challéat et al. 2024): 317 sites in 35 countries, Mar–Oct 2020, AudioMoth/SM4 at 48 kHz, 1 min every 10 min, site lat/lon plus per-file timestamps.
  - OSF https://osf.io/h285u ; paper https://pmc.ncbi.nlm.nih.gov/articles/PMC11349881
  - Licence conflicts: CC BY 4.0 on OSF vs BY-NC-ND in a repository entry.
  - Lockdown skies: sparse jets, GA continuing. **Ideal for the silence filter and false-positive tests.** But 1-minute clips truncate Doppler curves, so expect single-event mode with a known time.
- **Freesound:** geotag filter exists (`{!geofilt}`), but the date is usually the upload date. Weak.
- **Macaulay / iNaturalist:** restrictive or obscured coordinates. Weak.
- **NPS NSNSD:** mostly summary data, not raw audio [UNV].

### 2.4 Building our own ground-truth corpus (recommended backbone)

- **ADS-B side:** RTL-SDR (or Airspy) on 1090 MHz with readsb and tar1090, with full-rate history logging enabled (`--write-json-globe-history`).
  - Add a second dongle on 978 MHz UAT in the US: dump978-fa. Many GA props are UAT-only.
  - A local receiver captures low-altitude traffic the aggregators miss, and gives raw position timing.
- **Clock:** a GPS receiver feeding gpsd and chrony with PPS, on the host that both records audio and logs ADS-B. That gives one time base.
  - For separate recorders (phones, Zoom, AudioMoth with GPS firmware), emit a GPS-timed beep/chirp at start and end. That measures offset *and* drift.
- **Truth position:** a phone GPS fix is ~5 m; an RTK or averaged fix is better. Log the microphone height above ground.
- **Device matrix:** the same scene recorded at once by a reference mic (flat to 20 Hz), an iPhone, an Android phone, a doorbell/IP camera (G.711/AAC at 8–16 kHz), and a laptop in a WebRTC call. The robustness matrix (§3) then comes from real paths, not simulated codecs.
- **Sites and sessions:**
  - near a GA field or training area: dense piston traffic, harmonics, manoeuvring
  - under an approach path: jets plus turboprops
  - a quiet rural site: tests the silence filter
  - Record ≥30-min sessions with indoor and outdoor variants.
- **Labelling:** auto-align ADS-B CPA times with detected ridges (the AeroSonicDB / Forum Acusticum approach). Store a manifest per clip:
  - UTC start
  - drift in ppm
  - recorder lat/lon/h
  - device chain
  - aircraft list with CPA time and slant distance
- **Privacy:** recording in public space captures bystander speech. Record only own sites or record only <1 kHz, and publish clips low-passed or with speech removed.

## 3. Robustness and counter-forensics

The band is 50–500 Hz with sweeps of a few %. Different damage mechanisms act on it:
- a high-pass in the voice chain cuts the bottom of the band
- psychoacoustic quantisation can zero quiet tones
- stationary-noise suppressors target exactly the "steady tone" profile
- metadata loss removes the time prior

| Path | Low-frequency behaviour | Expected effect on 50–500 Hz tones |
|---|---|---|
| Opus AUDIO mode (YouTube webm itag 251 [UNV bitrate ~130–160 kbps]) | Only a 3 Hz DC reject (`dc_reject(pcm,3,…)`, libopus `src/opus_encoder.c`) | Intact unless masked |
| Opus VOIP mode (WebRTC calls; WhatsApp calls Opus at 8/16 kHz) | Variable 2nd-order HP tracking pitch, **60–100 Hz** (`VARIABLE_HP_MIN/MAX_CUTOFF_HZ`, `silk/tuning_parameters.h`) | Fundamentals below ~100 Hz attenuated (~12 dB/octave below cutoff [UNV estimate]); harmonics above survive |
| WebRTC APM high-pass | Elliptic **~90 Hz** (2019 redesign commit; the current file gives only biquad coefficients, cutoff not restated) | Sharp loss below 90 Hz; rely on harmonics |
| WebRTC NS / RNNoise / Krisp / NVIDIA / Apple Voice Isolation | Stationary-noise estimators (WebRTC NS); 22 Bark bands with ~200 Hz-wide low bands (RNNoise, arXiv 1709.08243 [UNV detail]); closed DNNs | **Most likely real-world killer.** Quasi-stationary tones are treated as noise when nobody speaks. colsto claims survival; untested by us |
| AMR-NB (3GPP TS 26.090) | Encoder pre-processing HP **80 Hz** (cl. 5.1), decoder HP 60 Hz (cl. 6.2.2), per iTecSpec mirror | Attenuates the lowest props; handset/network 300–3400 Hz shaping often worse |
| G.711 telephony, many IP cameras (8 kHz) | Nominal 300–3400 Hz, set by the analogue/handset front end, not the codec | <300 Hz strongly attenuated; harmonics at 300–500 Hz may carry the curve |
| AMR-WB / EVS | 50–7000 Hz | Mostly intact |
| MP3 (LAME), AAC-LC, HE-AAC (core) | No low-cut; SBR only rebuilds >5–8 kHz | Intact in level; quiet tones under masking may be zeroed per frame at ≤64 kbps [UNV] |
| TikTok/Instagram/X/Facebook re-encode | AAC-LC ~64–128 kbps [UNV] | As AAC; loudness normalisation is gain only |
| Phone MEMS mic + camera-app wind filter | Roll-off below ~50–100 Hz [UNV] | Fundamentals weakened; use harmonics |
| Audio clock error | ±20–100 ppm | ≤0.01% frequency scale, negligible against a few-% sweep. Fold into the f0 nuisance; matters for absolute-time locking over long gaps (100 ppm × 920 s = 92 ms) |

- **ENF literature is the quantitative analogue:** a 50/60 Hz tone survives lossy coding.
  - Zinemanas et al., CSSP 2018, "ENF-based audio authenticity robust to MP3", https://link.springer.com/article/10.1007/s00034-018-0793-9
  - Hajj-Ahmad et al., IEEE TIFS 2018, "Factors affecting ENF capture", https://par.nsf.gov/servlets/purl/10254057 (names AMR as harmful)
  - AES e-library on codec effects, https://aes2.org/publications/elibrary-page/?id=14331
  - Detection rates per bitrate need the full texts [gap].
- **colsto's claims** (analysis.md): naive HPF + MP3 + broadband boost brings the curve back; speech doesn't mask it; it survives phone codecs and noise suppression. Treat them as hypotheses for the §2.4 device matrix.
- **Countermeasures that actually work:**
  - cut segments (kills the inter-event offset)
  - strong DNN suppression
  - a steep HP above ~600 Hz (loses the voice fundamental too)
  - strip metadata
  - add a synthetic decoy Doppler curve (adversarial; also a reason to test robustness against injected fakes)

### 3.1 Recording-time priors from metadata

- **MP4/MOV:**
  - `creation_time` is specified as UTC, but some Android/camera firmware writes local time. Treat ±TZ hours as a discrete ambiguity.
  - iPhone also writes `com.apple.quicktime.creationdate` *with offset*, and `com.apple.quicktime.location.ISO6709`. GPS present → the method is moot, but the tag is useful as ground truth in our own corpus.
- **EXIF:** `DateTimeOriginal` + `OffsetTimeOriginal`.
- **Commands:** `exiftool -a -G1 -time:all -gps:all f` and `ffprobe -show_entries format_tags:stream_tags f`.
- **Platform stripping:** WhatsApp/X/Facebook/Instagram/YouTube downloads generally drop these tags [UNV per platform]. Upload time is then an upper bound only.
- **Security-camera OSD overlays:** often non-NTP, drifting seconds to minutes per week [UNV]. Treat the overlay as a prior with a learned offset, not as truth.
  - El Reno-style flash/event matching can calibrate it (thunderprint `media_chronolocation.md`).
  - The Doppler fit itself returns absolute CPA times, so a successful fix *also* chronolocates the clock.
- **File-system mtimes:** weakest prior; copying resets them.
- **Encoding:** carry a time prior as a distribution, e.g. a uniform day, a ±TZ mixture, or a Gaussian on OSD with a drift sigma. Never as a hard window, unless provenance is documented. DOPPLERGEIST refuses file times as evidence.

## 4. Statistical framing

- **The problem:** the score is maximised over ~10⁶–10⁸ (position × flight-pair × offset × f0) hypotheses.
  - A maximum is always found, and its value grows with search area, traffic density and sweep width.
  - "20.48 vs 15" is uninterpretable without the distribution of maxima under a null.
  - DOPPLERGEIST: scores "are not calibrated probabilities"; a synthetic national run ranked the truth 4th.
- **Look-elsewhere / trials factor:** Gross & Vitells 2010, EPJC 70:525, arXiv:1005.1891.
  - Global p ≈ local p + the expected number of up-crossings of the score field, estimated from a few low-threshold null runs.
  - The grid points are correlated, so the effective trials factor is far below the point count but still large.
- **Empirical null by time slides:** the LIGO background method.
  - Shift one detector's data by more than the light travel time and count chance coincidences. FAR is the background rate at ≥ the observed statistic.
  - GW150914, PRL 116:061102 (2016); PyCBC, Usman et al., CQG 33:215004 (2016).
  - **Direct analogue:** re-run the *entire* search with ADS-B shifted by Δt ∈ {±1 h … ±7 d}. Use whole-day shifts that preserve diurnal traffic structure, and avoid shifts that land within ±a few minutes of a true event.
  - Report p_global = (k+1)/(M+1), where k is the number of shifted maxima ≥ the observed maximum.
  - Same-day shifts of ±30–120 min keep the current day's routes and weather; use both kinds.
- **Max-statistic permutation** (Westfall & Young 1993) gives familywise error control under any correlation structure. Prefer it over:
  - Bonferroni: far too conservative on smooth maps
  - Benjamini–Hochberg FDR (JRSS-B 57:289, 1995): answers a different question, how many discoveries
  - Random-field theory (Worsley et al. 1996, HBM 4:58)
- **Cautionary tale:** Eklund, Nichols & Knutsson 2016, "Cluster failure", PNAS 113:7900. Parametric cluster inference gave familywise false-positive rates up to ~70%; permutation held nominal.
- **Target–decoy:** Elias & Gygi 2007, Nat Methods 4:207 (proteomics).
  - Search a decoy database alongside the real one and estimate FDR from decoy hits above threshold.
  - Our decoys are scored *in the same run* and compete for top rank:
    - time-shifted tracks
    - tracks translated/rotated by tens of km
    - tracks from another day with identity-shuffled callsigns
  - A decoy winning top-1 is a direct false-localisation event.
- **Extreme-value tail:** fit Gumbel/GEV (`scipy.stats.genextreme`) to null maxima to extrapolate p beyond M; check with a QQ plot.
- **Margin of victory:**
  - Report it relative to null quantiles, e.g. "observed max at the 99.8th percentile of shifted-time maxima".
  - The runner-up should be the best **independent mode**: a different flight pair, or more than D km away. An adjacent cell or the mirror-image side of the same track measures smoothness, not identifiability.
  - For a single event, along-track and left/right mirror ambiguities (mojomast skill) are expected bimodality; report both modes.
- **Bayesian posterior over location** (thunderprint pattern):
  - Thunderprint: `src/search/select.rs:30` computes `posterior = softmax(log_ev + ln(cell_area))`, a uniform-per-area prior. It reports HPD credible regions and 90% ellipses plus a `reliable` flag (`MIN_RELIABLE_STRIKES = 30`, `src/search/shape.rs:30`).
  - Eval: "posterior mass in cells more probable than the truth's" (`src/eval.rs:67`), a coverage/PIT statistic.
  - README admits "credible regions on real audio should be read as optimistic" and "not blind".
  - For doppler_print, raw spectrogram brightness sums are not log-likelihoods.
    - Option: convert via a calibrated mapping, i.e. a logistic regression of score → log-LR fitted on positive (true location) vs decoy candidates.
    - Or temper with a temperature chosen so that nominal 90% regions cover the truth ~90% on the corpus.
  - Priors: search region × optional built-up density (GHSL/WorldPop) × time prior (§3.1). Report results with and without the population prior, because a population prior biases towards cities.
- **Seismology reporting pattern:** NonLinLoc (Lomax et al. 2000; http://alomax.free.fr/nlloc/) reports the maximum-likelihood point, the expectation, a 68% ellipsoid, PDF scatter and a multimodality flag. Copy this set.
- **Forensic LR reporting:**
  - ENFSI Guideline for Evaluative Reporting 2015, https://enfsi.eu/wp-content/uploads/2016/09/m1_guideline.pdf. Explicit propositions Hp "recorded within region R" vs Hd "recorded elsewhere in the searched area", plus a verbal scale [UNV bands: 10–10² moderate … 10⁴–10⁶ very strong].
  - Morrison et al. 2021, Sci & Justice 61:299, consensus on validating forensic voice comparison [UNV]: log-LR calibration, **Cllr**, Tippett plots of same-source vs different-source LRs. These need the §2 corpus.
- **Abstention is a first-class output:** "inconclusive" when p_global > α, when there are fewer than N usable events, or when the time prior is too wide for the area. DOPPLERGEIST does this; thunderprint does it via `reliable`.
- **Missing aircraft** (LADD/PIA/non-ADS-B/UAT gaps) lower the true score and invalidate silence evidence. Model each silence vote with a coverage probability, and never let silence alone veto.

## 5. Ethics and legal framing

- **Precedents:**
  - **GeoSpy/Graylark.** Public access closed in Jan 2025 after 404 Media reported users seeking help stalking women; now government/enterprise only ("Raven"; LAPD, Miami-Dade).
    - https://www.malwarebytes.com/blog/news/2025/01/ai-tool-geospy-analyzes-images-and-identifies-locations-in-seconds
    - https://petapixel.com/2025/01/24/geospy-is-an-ai-tool-that-can-geolocate-photos-in-seconds/
    - https://techspot.com/news/114174-police-using-ai-locate-homes-single-interior-photo.html
    - Lesson: a hosted, frictionless service draws abuse.
  - **PIGEON** (Haas et al., CVPR 2024, arXiv:2307.05845): ethics section on stalking/surveillance; weights withheld. Staged release.
  - **DOPPLERGEIST:**
    - "runs locally; there are no plans for a hosted service"
    - data-rights notes
    - "not affiliated with" the video author
    - no dual-use section
  - **mojomast skill:**
    - "Do not use to locate private individuals without consent or lawful basis"
    - "use for forensics, journalism, research, or self-audit"
    - "Never present as proof by itself"
  - **Thunderprint:** has no ethics section (gap).
  - **Bellingcat ShadowFinder:** presents output as *possible* locations (a band); purpose-and-limits framing.
- **Berkeley Protocol on Digital Open Source Investigations** (OHCHR/UC Berkeley HRC, 2022): accountability, competence, objectivity, minimise harm, privacy by design, documented methodology, chain of custody. https://www.ohchr.org/en/publications/policy-and-methodological-publications/berkeley-protocol-digital-open-source
- **GDPR:**
  - An inferred location tied to an identifiable uploader or speaker is personal data (Art. 4(1) names location data).
  - It can *reveal* Art. 9 data (clinic, place of worship); CJEU C-184/20 read "revealing" broadly [UNV].
  - Art. 14 duties apply to third-party data, and an Art. 35 DPIA applies to systematic use.
  - The household exemption (Art. 2(2)(c)) covers only personal use.
- **Data licences:**
  - adsb.lol: ODbL 1.0, attribution plus share-alike on derived *databases*. Derived grids and scores are arguably produced works; flag this.
  - OpenSky: non-commercial research/government, cite Schäfer et al. 2014, no raw redistribution [UNV; terms page returned 403].
  - Follow thunderprint's pattern: don't commit raw ADS-B; ship fetch scripts.
- **LADD/PIA:** do not de-anonymise privacy-programme aircraft. State this, and the resulting false-negative risk, in the README.
- **Suggested README wording**, adapted from the above:
  > doppler_print shows that faint aircraft sound in a recording can reveal where and when it was made. It is published as a privacy warning and for research, verification and self-audit. Do not use it to locate people without their consent or a lawful basis; an inferred location of an identifiable person is personal data. It runs locally; there is no hosted service and no batch or reverse-lookup mode. Scores are ranking scores; reported p-values come from re-scoring against time-shifted ADS-B, and a result can be "inconclusive". A location estimate is never proof by itself: confirm it independently. Not affiliated with colsto.

  Add a "Protecting your recordings" section (§3 countermeasures) and a "Data sources & attribution" section.

## Design implications for doppler_print

1. **Positioning:** the method is public (colsto, DOPPLERGEIST, mojomast skill). doppler_print's contribution should be what nobody ships:
   - a calibrated global false-localisation rate
   - decoy-based p-values
   - credible regions with empirical coverage
   - an abstain path
   - blind real-world results
2. **Null machinery is core, not an add-on.** Make `search` accept an ADS-B time-shift and a spatial-translate transform, so the identical pipeline runs as its own null. Budget compute for M ≥ 20 null runs per real query (coarse stage only, then the top survivors) and report:
   - p_global = (k+1)/(M+1)
   - the GEV-extrapolated tail
   - the observed max as a null quantile
3. **Score → evidence.** Keep the raw brightness score for ranking. Learn score → log-LR (logistic calibration) on the corpus. Then reuse thunderprint's posterior/HPD/eval code shape (`softmax(log_ev + ln area)`, mass-above-truth metric) and add a `reliable` analogue:
   - number of usable events ≥ 2, or 1 event plus a time prior of ≤ X s
   - min ridge SNR (`ridge_snr_z` ≥ 2.7 as a starting point)
   - p_global < 0.01
4. **Runner-up = best independent mode.** That means a different flight pair, or more than 2 km apart. Always emit the along-track and mirror modes for single events.
5. **Silence filter gets a coverage model:** P(aircraft observed | it flew), using receiver density and UAT/LADD gaps. Make it soft, never a hard veto.
6. **Front-end must handle a missing low band.** Use harmonic-stack scoring (sum over k·f0) so a 60–100 Hz HP (Opus VOIP, WebRTC 90 Hz, AMR 80 Hz) or a 300 Hz phone band leaves harmonics carrying the curve. Add a metadata-prior module (ffprobe/exiftool → time distribution with a TZ-ambiguity mixture).
7. **Validation plan**, in stages; each stage gates the next. Freeze thresholds before tier C/D.
   - **A. Forward-model unit truth** (no search): synthesize curves from real ADS-B tracks; compare with measured ridges.
     - NASA Small UAS flyover data (surveyed mics + RTK) and Pathak 2026 (40 passes with KML + f–t CSV).
     - Metric: RMS Hz residual, and the speed/CPA errors that Pathak's sheet reports.
     - Also assert that the emission-time vs receive-time ablation fails, as analysis.md predicts.
   - **B. Synthetic search suite:** inject modelled curves into real noise beds (Silent Cities and own recordings) at swept SNR.
     - Pass each through the codec chain {wav, mp3 64/128, aac 96, opus audio 128, opus voip 24, AMR-NB via ffmpeg/libopencore, WebRTC APM, RNNoise}.
     - Vary area (city → state → CONUS) and time prior (exact → ±1 h → day).
     - Output: hit-rate and error CDF curves over {SNR × codec × area × time prior}, plus false-localisation rate from null runs on noise-only beds. This gives the operating envelope for the README.
   - **C. Semi-real:** Pathak 2026 recordings plus AeroSonicDB props (after obtaining site coordinates and UTC from the authors), searched over growing areas around the true site with *no* site prior. Report median error, the 90%-region coverage rate, and decoy-win rate.
   - **D. Own corpus (§2.4):** GPS-disciplined RTL-SDR 1090 + UAT, a multi-device matrix, three site types.
     - Aim for ≥50 sessions, split 50/50 dev/holdout. The holdout is sealed: scored once with frozen code and thresholds, run blind by a second person who supplies only date and region.
     - Report Tippett plots, Cllr, coverage, abstention rate, and errors by device chain.
   - **E. Wild opportunistic:** xeno-canto recordings found by running the detector over bbox × date slices (remarks search as a seed). Silent Cities sites for single-event and silence tests.
     - Truth is only ~100 m–1 km, so score "inside region" rather than metres.
   - **F. Negative controls:**
     - recordings with no aircraft (expect "inconclusive")
     - recordings scored against the wrong day (expect p_global ~ U(0,1))
     - an injected fake Doppler curve whose path matches no real track (expect inconclusive or a decoy-level score)
   - **Publish** results as a table in the README "Accuracy and limitations" (thunderprint format). Mark any non-blind row as such.
8. **Ethics deliverables:** README dual-use paragraph (§5), "Protecting your recordings" section, no hosted service, no batch mode, a LADD/PIA non-de-anonymisation statement, a data-provenance file per source, and a self-audit mode (`doppler_print audit clip.wav`: "are aircraft tones detectable here?") as the privacy-positive entry point.

## Sources

- https://www.youtube.com/watch?v=UytYrhYLOKI
- https://github.com/1337isnot1337/DOPPLERGEIST
- https://zenodo.org/records/23424278
- https://researchers.mq.edu.au/en/publications/application-of-the-short-time-fourier-transform-and-the-wigner-vi/
- https://auditory.org/asamtgs/asa95stl/4pSP/4pSP10.html
- https://upcommons.upc.edu/entities/publication/9ae18a41-e532-4d46-95ea-581283c44d9e
- https://upcommons.upc.edu/handle/2117/95089
- https://publications.drdo.gov.in/ojs/index.php/dsj/article/download/2238/1198/7489
- https://data.mendeley.com/datasets/kpftptxdj5
- https://researchrepository.ucd.ie/entities/publication/a6c4c008-8937-4b93-b045-6623d442d8cc
- https://www.gi.alaska.edu/news/seismic-data-can-identify-aircraft-type
- https://www.mdpi.com/1424-8220/21/15/5094
- https://www.earthdoc.org/content/papers/10.3997/2214-4609.202475013
- https://patents.google.com/patent/US10352763
- https://esa.org/?p=33521
- https://www.npshistory.com/publications/grba/sr-2026-438.html
- https://arxiv.org/abs/2311.06368
- https://zenodo.org/records/8004108
- https://zenodo.org/records/12775560
- https://dael.euracoustics.org/confs/fa2026/data/articles/000004.pdf
- https://b2find.eudat.eu/dataset/8a1a9b2a-51f8-516a-90bb-d494b00d739f
- https://arxiv.org/abs/2403.03372
- https://data.nasa.gov/dataset/small-uas-flyover-acoustics-data
- https://ntrs.nasa.gov/api/citations/20160007340/downloads/20160007340.pdf
- https://www.grasacoustics.com/news/nasa-publishes-acoustic-dataset-from-strives5-evtol-tests
- https://ntrs.nasa.gov/api/citations/20250004699/downloads/VFS81_Stutz_AmedeeManeuvers_05122025_Strives.pdf
- https://pmc.ncbi.nlm.nih.gov/articles/PMC11349881
- https://osf.io/h285u
- https://xeno-canto.org/explore/api
- https://github.com/adsblol/globe_history_2025/releases
- https://github.com/adsblol/globe_history_2024
- https://pypi.org/project/pyopensky/
- https://github.com/wiedehopf/tar1090-db
- https://journal.equinoxpub.com/IJSLL/article/view/9977
- https://www.mast.umd.edu/files/paper_ENF/conf_paper6_ICASSP13.pdf
- https://arxiv.org/abs/2105.00668
- https://www.swgde.org/documents/published-complete-listing/09-a-001-swgde-electric-network-frequency-discussion-paper/
- https://forensis-preview.forensic-architecture.org/methodology/audio-analysis
- https://gijn.org/stories/how-two-forensic-teams-investigated-the-killing-of-journalist-shireen-abu-akleh/
- https://nij.ojp.gov/library/publications/audio-forensic-gunshot-analysis-and-multilateration
- https://arxiv.org/abs/2108.07377
- https://www.bellingcat.com/resources/2026/03/05/turnstone-flight-tracking-tool/
- https://bellingcat.gitbook.io/toolkit/more/all-tools/ads-b-exchange
- https://raw.githubusercontent.com/xiph/opus/main/silk/tuning_parameters.h
- https://raw.githubusercontent.com/xiph/opus/main/src/opus_encoder.c
- https://webrtc.googlesource.com/src/+/a806731feaeed38dff0ef738f70efb8a9441aa38
- https://webrtc.googlesource.com/src/+/b810646c54127733fe3d6c13fcae9376fa458457
- https://www.itecspec.com/3gpp/26.090/s/5.1
- https://www.itecspec.com/3gpp/26.090/s/6.2.2
- https://arxiv.org/abs/1709.08243
- https://link.springer.com/article/10.1007/s00034-018-0793-9
- https://par.nsf.gov/servlets/purl/10254057
- https://aes2.org/publications/elibrary-page/?id=14331
- https://arxiv.org/abs/1005.1891
- https://doi.org/10.1103/PhysRevLett.116.061102
- https://doi.org/10.1088/0264-9381/33/21/215004
- https://doi.org/10.1073/pnas.1602413113
- https://doi.org/10.1038/nmeth1019
- http://alomax.free.fr/nlloc/
- https://enfsi.eu/wp-content/uploads/2016/09/m1_guideline.pdf
- https://arxiv.org/abs/2307.05845
- https://www.malwarebytes.com/blog/news/2025/01/ai-tool-geospy-analyzes-images-and-identifies-locations-in-seconds
- https://petapixel.com/2025/01/24/geospy-is-an-ai-tool-that-can-geolocate-photos-in-seconds/
- https://techspot.com/news/114174-police-using-ai-locate-homes-single-interior-photo.html
- https://www.ohchr.org/en/publications/policy-and-methodological-publications/berkeley-protocol-digital-open-source
- https://github.com/bellingcat/ShadowFinder
- https://github.com/RaphaelJ/libhum
