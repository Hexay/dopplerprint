# Public aircraft recordings with mains hum, for an ENF clock (2026-11-07)

Goal: clips that have all six of: (1) recorder location ≤ ~100 m, (2) start time to ~1 min,
(3) date inside ADS-B archives, (4) audible prop passes, (5) mains hum, (6) a grid-frequency reference.
Detector: `dopplerprint hum --json`, thresholds in `hum_detector_2026-11-03.md`. [V] = checked by me this
session; [I] = inferred. Scratch samples (not in repo):
`/tmp/claude-1001/-srv-agents-work-doppler-print/83679991-c801-44ae-bacc-c65574465eb6/scratchpad/humsrc/`.

## Verdict

Nothing found meets all six criteria today. The two closest:

- **AeroSonicDB YPAD-0523, location 2 (Adelaide)** has a real grid-tracking 100 Hz line, criteria 1–5.
  The gap is criterion 6: there is no public NEM 1 s frequency for 2023.
- **xeno-canto UK (via GBIF)** meets 1–4 and 6 (NESO 1 s). Criterion 5, hum, is what fails: 1 of 37 screened
  clips has grid hum, and that clip has no aircraft.
- **ENF clock proven on public data** [V]: XC1146616 (hum) matched against NESO 1 s puts the start at
  +68 s from the listed minute, with rms 6 mHz against a 75 mHz median over other lags.

## Grid-frequency references

| Grid | Source | Resolution / span | Access | Status |
|---|---|---|---|---|
| GB | NESO "System Frequency" ([portal](https://www.neso.energy/data-portal/system-frequency-data)); CKAN id `cb1cc925-ecd8-4406-b021-3a3f368196e1` | 1 s, monthly CSV `fnew-YYYY-M.csv`, Apr 2014 → Sep 2026 (153 resources) | open, NESO Open Data Licence, no key | [V] via CKAN `package_show` |
| Nordic (FI/SE/NO/DK-E) | Fingrid dataset 339 ([page](https://data.fingrid.fi/en/datasets/339)) | 10 Hz, monthly 7z of daily CSVs, times UTC+2/+3 | CC BY 4.0; page download is open, API needs a key | [V] 2026 months listed; earlier years [I] |
| US-West (WECC) | provenance `enf-rebuild` from FDSN seismometers (`similar_projects_2026-11-05.md` idea 4) | ~1 s, r 0.99 vs WECC log | rebuild ourselves | [V] in that doc; East/ERCOT experimental |
| AU NEM | AEMO FPP 4 s `FPP_REGION_FREQ_MEASURE` ([NEMWEB FPPDAILY](https://nemweb.com.au/Reports/Archive/FPPDAILY/)) | 4 s; archive starts 2025-09 | open | [V] listing. Causer-Pays 4 s history is gone for 2017–May 2025 ([nemosis list](https://groups.google.com/g/nemosis-discuss/c/fChlV1vkEN0)) [I] |
| AU NEM 2023 alt. | seismometer rebuild: AusPass FDSN near Adelaide is 100 sps only (4B.SNKG*), so 50 Hz sits at Nyquist; ≥ 200 sps: OZ.GOOGA 250 sps (Canberra), YW.RDK12 200 sps (NSW), same synchronous grid | — | FDSN, open? | [V] station list; hum content and access [I] |
| Continental EU | no TSO publishes 1 s history; Jumar et al. DB ([arXiv 2006.01771](https://arxiv.org/abs/2006.01771)) covers 2017–2020 only | — | — | [V] searches negative; seismometer rebuild possible [I] |

## Sources ranked

"Usable" counts clips that pass criteria 1–5 on my screen, or plausibly would.

| # | Source | Loc | Time | ADS-B | Props | Hum | Ref | Licence | Usable |
|---|---|---|---|---|---|---|---|---|---|
| 1 | **AeroSonicDB YPAD-0523 v1.1.1** ([Zenodo 8371595](https://zenodo.org/records/8371595), [arXiv 2311.06368](https://arxiv.org/abs/2311.06368)) | 3 fixed sites; coords are "a road or public space nearby" (privacy), so ≤ ~200 m [I] | filename to the second, ADS-B-triggered; local time (hours run 06–23) [I] | **adsb.lol 2023 has no feeder near Adelaide [V 2026-11-08: 0 aircraft in a ±25 km box on 04-06/14/15/24, 05-09; nearest ~390 km]**: needs OpenSky. Hex id in metadata [V] | 155 prop clips of 625 (114 turboprop, 37 piston) [V] | loc 2: 100 Hz line in 21/34 silences and 4/16 aircraft clips [V]; locs 0/1 none [V] | none public for 2023 (above) | CC BY-NC 4.0 | **3–4** loc-2 props with hum (DA42, SF34, R44 heli; plus a B38M jet) |
| 2 | **xeno-canto via GBIF** ([bird dataset](https://www.gbif.org/dataset/b1047888-ae52-4179-9dd5-5448ea342a24)) | lat/lon, 4 dp [V] | date + HH:MM, local; some recordists round to :00/:30 [V] | 2023+ selected [V] | 114 GB/Nordic/US-West records mention aircraft in `behavior`/`fieldNotes` [V]; prop vs jet unknown | 1 real grid hum in 37 screened; 0 of the aircraft-tagged GB clips with planes [V] | GB: NESO 1 s ✔; Nordic: Fingrid ✔; AZ: WECC rebuild | mostly CC BY-NC-SA | **0 now**; ENF itself verified on one clip |
| 3 | Hear-and-Avoid, TU Delft ([DOI 10.34894/ARULTD](https://doi.org/10.34894/ARULTD)) | array, site not on landing page | overflight labels, no wall-clock time listed | 2020, pre-adsb.lol | GA props ✔ | not screened | CE: none | CC0 | 0 as listed [V metadata] |
| 4 | AeroSonicDB 3K v2.0 ([Zenodo 12775560](https://zenodo.org/records/12775560)) | same Adelaide sites | ADS-B-triggered, trimmed to the event | likely 2023–24 [I] | >3000 events | unknown; 16 kHz | NEM 4 s only from 2025-09 | CC BY-NC-SA, **access by request** | unknown |
| 5 | Webcam / railcam / eagle-cam live streams (dopplertrack's test data) | fixed cams | live → absolute time | ✔ | some | mains-powered cams: hum likely [I] | US East: none public | YouTube; blocked from devbox | unknown |
| 6 | Freesound | geotags | upload date only; recording time rarely in text | — | many | — | — | API needs a token [V] | ~0 without manual curation [I] |
| 7 | Macaulay Library | lat/lon | date | — | — | — | — | site behind Anubis [V]; GBIF copy has no `eventTime` and no aircraft remarks [V] | 0 |
| 8 | SONYC-UST, ESC-50, DCASE, TAU, Urban Soundscapes of the World | blurred or none | hour or none | — | — | — | — | — | 0 (fail 1 or 2) [I] |

Access notes:

- **xeno-canto.** Both the HTML site and `/api/*` sit behind Anubis bot protection. curl timed out, and
  WebFetch got "Access Denied" [V]. The API v3 key question is unresolved because I couldn't reach the docs.
  But GBIF mirrors the metadata (eventTime, coords, `behavior`, `fieldNotes`), and the media URLs
  `xeno-canto.org/sounds/uploaded/...` download fine with plain curl [V]. So no key is needed.
- **Zenodo.** Single WAVs can be pulled from the 1.2 GB `audio.zip` with HTTP Range requests through the
  central directory [V], so the whole archive never has to be downloaded.

## Screening: AeroSonicDB (22.05 kHz mono WAV) [V]

| Set | Files | Hum present | Lines | Notes |
|---|---|---|---|---|
| loc 0 (suburb 15.5 km NE), aircraft + silence | 6 | 0 | — | stray lines detectable in 0–21% of frames |
| loc 1 (500 m SE of rwy), aircraft | 9 | 0 | — | |
| loc 1 silences (10 s) | 13 | 3, all `mislabelled` | k=3–12, inconsistent | spurious: 10 s is too short (< 3 valid frames typical) |
| loc 2 (950 m SE, wastewater plant to the N), silences 2023-04-24 | 17 | 7 | k=2 (100 Hz), 13–18 dB | consistent across the session |
| loc 2 silences 2023-05-09 | 17 | 14 | k=2 100 Hz, plus k=1/4/5/10 in a 12:20–12:29 burst | burst flagged `mislabelled`/ambiguous (another source) |
| loc 2 aircraft, 2023-04-24 + 05-09 | 16 | 4 | k=2, 7–15 dB | masked by loud low passes (25–450 ft) in the rest |

Loc-2 hits (60 s each, frame 2 s, rate `ok`):

| Clip | Type | Line SNR | Detectable | Mean f₀ | Wander |
|---|---|---|---|---|---|
| 7C79B7_2023-05-09_12-39-52 | DA42 (piston twin), alt 800 ft | 14.9 dB | 80% | 49.991 Hz | 0.033 Hz |
| 7C75E6_2023-05-09_12-25-09 | R44 heli, 25 ft | 14.5 dB | 93% | 49.975 Hz | 0.032 Hz |
| 7C495B_2023-04-24_11-40-02 | SF34 turboprop, 400 ft | 7.7 dB | 61% | 50.002 Hz | 0.033 Hz |
| C88092_2023-04-24_12-01-23 | B38M (jet) | 7.4 dB | 54% | — | — |

- The 100 Hz line wanders like a grid (σ ~0.03 Hz, mean within 0.03 Hz of 50) [V]. Its source is plausibly
  transformer or motor magnetostriction at the wastewater plant [I], so it should track ENF.
- Gotcha: some filenames encode a different location id from `sample_meta.csv`. For example
  `000000_2023-05-09_12-52-20_1_1.wav` is location 2 in the CSV [V]. Trust the CSV.
- A recurring 37.5 Hz line (0.75 × 50) on 2023-05-09 is rejected as off-nominal [V].
- Sessions are short: loc 2 has two (~1 h each) and loc 1 one. Even with a reference, this gives 3–4
  prop/heli cases at one site [V].

## Screening: xeno-canto (GB / Nordic / US-West, 2023+) [V]

Selection: the 114 GBIF records (GB, Nordic, US lon < −104, 2023+, with coords) whose `behavior` or
`fieldNotes` mention aircraft [V]. I screened 30 of them, spread across 20 recordists, plus 4 garden or
unattended GB clips and 3 more from the one site with a hit. 37 files, ~0.55 GB.

| Result | n | Clips |
|---|---|---|
| no line | 31 | e.g. MVBSLTSBUW St Mellons garden ×11 (31 aircraft-tagged clips in GBIF), QKELZYKUGX Essex ×2, KDNLNVNAOM Sandwich Bay ×2, TVUQXQGQNF Knepp ×2, Liverpool Speke (next to the airport), Wyre Forest (helicopter + light aircraft), West Heslerton 38 min unattended (light aircraft take-off), SE, FI, DE Frankfurt ×3 |
| stray line, rejected | 3 | XC1173298 92 Hz; XC1064850 784 Hz; XC999865 high k |
| present but **not grid** | 2 | XC901040 (St Mellons): 225 + 450 Hz machine tone, flagged `mislabelled`/ambiguous. XC977084 (Barking, London, construction site): 60.0007 Hz k=1 at 80%. GB is a 50 Hz grid, so this is a generator or plant [I] |
| present, plausible grid | 1 | XC1069922 Tucson AZ (WECC), 2025-12-29 09:15: 60.008 Hz k=1, 6.2 dB, 37%. Notes say "HP filtered … a lot of road and aircraft noise" |
| **present, grid ENF confirmed** | 1 | **XC1146616** Netheravon Airfield, Wilts, 2026-06-10 00:20 local: 100.25 Hz (k=2), f₀ 50.123 Hz, wander 0.008 Hz |

**ENF check on XC1146616 against NESO 1 s** [V] (scratch script `enf_match.py`; the trace is 6 frames of 8 s):

- **Grid level matches.** NESO at 23:20 UTC (= 00:20 BST) averaged 50.122 Hz, against the clip's 50.123.
- **Best lag.** Over a ±1 h lag sweep, rms(trace − grid) bottoms out at **6.2 mHz at +68 s**. The median
  over all lags is 74.8 mHz. A second minimum sits at −3112 s (6.1 mHz), so a 60 s clip is ambiguous over
  ±1 h but unique inside ±120 s.
- **What this establishes.** xeno-canto times are local clock time, and the full pipeline (xeno-canto →
  `hum` → NESO) works with no new code. The clip is a night recording at an airfield with no aircraft, and
  the site's other 3 clips have no hum [V]. So it is not a corpus case.

Why xeno-canto rarely has hum [V on notes and band levels; cause I]:

- **Low band removed.** 18/114 notes mention filtering, e.g. "filtering to reduce aircraft noise",
  "dampen the lower frequencies of plane", "HPF 400 Hz", "Izotope RX de-hum … below 500 Hz". These
  recordists remove exactly the band we need (both hum and prop BPF).
- **Low band suppressed in the rest.** On 5 St Mellons WAVs, the band below 150 Hz sits 9–24 dB under the
  band above 1 kHz.

Detector gotchas found [V]:

- **Grid deviation read as clock offset.** XC1146616 is reported `rate: clock_offset, +2466 ppm`, but the
  offset is the grid itself (+0.123 Hz, above `rate_tol` 0.2%). When a reference exists, the rate check
  should use the reference mean, not the nominal.
- **No grid-vs-country check.** `present` + `nominal 60` in GB (XC977084) means equipment, not ENF. Ingest
  must check the nominal against the country's grid.

## Recommendation

Be clear about the result: no public source gives a prop pass + hum + reference today. Ranked for ingest:

1. **AeroSonicDB YPAD-0523 location 2** gives 3–4 cases now (DA42 7C79B7, SF34 7C495B, R44 7C75E6; the
   B38M jet is flagged).
   - Even without ENF these are good Doppler cases. Start times come from the ADS-B trigger and the ICAO
     hex is given, so they also measure the along-track error we're trying to fix.
   - ENF reference, which is experimental: rebuild NEM frequency from AusPass ≥ 200 sps stations
     (OZ.GOOGA, YW.RDK12), as provenance's `enf-rebuild` does. First check that those channels show 50 Hz.
   - Tooling: `tools/corpus/aerosonicdb.py`. It reads `sample_meta.csv` + `locations.json` and pulls the
     single WAVs by HTTP Range from `audio.zip` (the scratch `rzip.py` approach, stdlib only).
   - Local → UTC conversion: Adelaide is ACDT +10:30 until 2023-04-02, then ACST +9:30 [I]. Verify against
     adsb.lol.
   - Uncertainty: the site coords are a nearby road, so set the location prior accordingly.
   - Licence is CC BY-NC.
2. **xeno-canto via GBIF + NESO** is the ENF-proven path, but needs a wider search to find aircraft clips.
   - Tooling: `tools/corpus/xenocanto.py`. Query the GBIF occurrence API with no key (datasets `b1047888…`
     birds, `ff571aeb…` soundscapes; `q`, `country`, `year`, `geoDistance`) and download from
     `xeno-canto.org/sounds/uploaded/…`.
   - Pre-filter: keep notes that say "no modification" and reject notes that say filter/HPF/de-hum.
     Run `hum` on the rest.
   - Then search adsb.lol for prop passes at each hum-positive site. Try GB sites within ~5 km of GA
     airfields first, because the `q=aircraft` hit rate for hum was 1/37.
   - Add `tools/corpus/neso.py`, which takes the monthly `fnew-YYYY-M.csv` via CKAN `package_show` and
     follows the 302 to R2 (`curl -L`).
   - Expect single-digit yields per few hundred screened [I].
3. **Make our own, or use mains-powered streams.** dopplertrack's eagle-cam and railcam live streams,
   and airfield webcams, are mains-powered and have absolute time. Capture has to happen off the devbox,
   where YouTube is reachable.
   - US-East has no public reference, so prefer GB or WECC cams.
   - Cheapest of all is a phone recording near a building under a GA circuit in GB, which gives NESO 1 s
     + adsb.lol for free.

Not worth pursuing: Freesound (no recording times), Macaulay (behind Anubis, no times on GBIF),
SONYC/DCASE/ESC-50/TAU (locations blurred, times missing or hour-level), and Hear-and-Avoid (2020, no
wall-clock times).

## Airfield screen: xeno-canto GB near GA airfields (2026-11-08) [V]

Tooling: `tools/corpus/neso.py` (NESO 1 s, cached in `data/enf/gb/`, `match()` = ±1 h lag sweep),
`tools/corpus/xenocanto.py` (GBIF + OurAirports), `tools/hum_scout.py select download screen report`.
Results: `humsrc/xc_gb_meta/results.{csv,md}` under the scratch dir above.

- **Selection.** 21 222 GB recordings (2023+, coords). 17 053 lie ≤ 8 km from an OurAirports
  small/medium airfield, because GB has ~1000 farm strips, so distance alone barely filters. 393 were rejected
  on notes. 1360 kept (20–600 s, ≤ 40 per recordist), ordered "unprocessed" first (62), then nearest.
- **Budget.** The 3 GB cap bought only **242 clips** (4.6 h), because most are 48 kHz WAV (~12 MB each).
  Putting MP3s first would roughly triple the count.
- **Hum.** 55/242 have hum. **40 are GB grid** (f₀ within 0.5 Hz of 50). 15 are equipment: nine at 60 Hz
  and six 37.5 Hz lines (West Heslerton ×3, Pound Green ×2, Smithy Fen). The detector labels the 37.5 Hz
  lines `nominal 50` through the 0.75 ratio, so ingest must check f₀, not only `nominal`.
- **ENF.** 17/40 match NESO (rms < 15 mHz and < 0.3 × median). Only **1 is unique** over ±1 h (next
  minimum ≥ 120 s away is ≥ 1.5× worse): **XC824881**, Knott End-on-Sea, 2023-08-24 01:06 BST, lag
  **+16 s**, 2.5 mHz vs 57 median. In 4/17 the best lag falls within ±300 s of the listed time. Clips of
  20–60 s with 3–10 valid frames are ambiguous over ±1 h, so restrict the sweep to ±5 min when the listed
  time is trusted.
- **Aircraft.** 24/242 have a pass-like event (`pass_p` ≥ 0.5). **4 have GB-grid hum and a pass-like
  event**, but none is a prize:

| XC | Site (airfield, km) | Local | ENF | Verdict |
|---|---|---|---|---|
| XC1137317 | West Heslerton (Moor Farm strip, 0.7) | 2026-05-25 06:29 | +197 s, 6.5 mHz, not unique (7.4 at +717) | spectrogram: steady 100–320 Hz broadband, no visible Doppler |
| XC1154485 | Panfield, Essex (Rayne Hall strip, 1.3) | 2026-07-07 05:30 | no match (29 mHz) | flat ~100/280 Hz bands at 0–23 s, no sweep; possibly a vehicle |
| XC1040318 | West Heslerton (0.7) | 2025-06-11 05:22 | no match (36 mHz) | broadband low band, no visible pass |
| XC1105256 | Great Massingham (1.4) | 2026-05-03 00:00 | no match | notes say "unsure of exact time"; low bird calls |

So hum alone is common near these sites (40/242 is about 6× the 1/37 rate from `q=aircraft`). Still no
clip has hum, a unique ENF lock and a real prop pass together. Next steps, cheapest first:

1. Screen the MP3s.
2. Weight selection toward active GA airfields: ICAO-coded (`EG..`) idents rather than all
   `GB-nnnn` farm strips.
3. Check adsb.lol for GA traffic over West Heslerton and Panfield on the listed mornings before spending
   more on audio.
