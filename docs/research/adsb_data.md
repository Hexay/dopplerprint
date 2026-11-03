# Historical ADS-B data for doppler_print

Scope: where to get a full day of continental-US (CONUS) trajectories in 2026, with emphasis on low-altitude
piston/turboprop GA; field semantics and pitfalls; coverage limits; national-scale storage and indexing.
Researched 2026-11-03. "Measured" means this session computed it from samples in `docs/research/samples/`
(scripts in `docs/research/samples/scripts/`). Everything else comes from the sources listed at the end.

## 0. What DOPPLERGEIST does (data side only)

- Source: adsb.lol `globe_history_2026` daily release (e.g. `v2026.09.08-planes-readsb-staging-0`), ODbL.
  `scripts/fetch_adsblol_day.py` gets release metadata from the GitHub API, then downloads the split tar parts
  (`.tar.aa`, `.tar.ab`) with HTTP Range resume and checks each part against the asset `digest` (sha256).
- `scripts/ingest_adsblol_day.py` streams the concatenated parts through `tarfile` mode `r|`. It keeps only
  `traces/xx/trace_full_<icao>.json` (gzip JSON) and drops known jets. Jets are identified from tar1090-db
  `icao_aircraft_types.js`, where a `desc` ending in `J` means jet. It then selects aircraft whose
  gap-split (30 s) trajectory envelope meets the bbox and time window, and writes each selected aircraft's
  **full** trace to zstd Parquet with selection metadata in the schema.
- Altitude (`tracks.adsblol_frame`): prefers geometric altitude (trace slot 10, or slot 3 when flag bit 8 is
  set). For barometric-only points it adds the aircraft's own interpolated geom−baro offset. A trace with no
  geometric data stays labelled `barometric`. Velocity components come from gs/track and are filled from
  position gradients when missing.
- Its index is SQLite with spatial and time indexes over gap-split segments, using Hermite/Bezier bounds.
  It explicitly does **not** claim receiver completeness. That is the right stance, and we should keep it.

## 1. Sources (status Nov 2026)

| Source | Access / cost | License | Temporal resolution | MLAT | UAT / low-alt GA |
|---|---|---|---|---|---|
| **adsb.lol globe_history_YYYY** | Free GitHub releases, one per day per pod (`prod-0`, `staging-0`, and since late 2026 `mlatonly-0`) | ODbL 1.0 (derived DBs share-alike) | readsb trace decimation: a point on change, otherwise ≤~24 s (measured p99 23 s) | yes (`mlat` source; separate mlatonly pod) | yes, via feeders' 978 receivers and ADS-R (`adsr_icao`) plus TIS-B. Unfiltered (no LADD block) |
| **ADS-B Exchange** (JETNET-owned) | Free: complete data for the **1st of each month** (samples.adsbexchange.com). Other days: paid, quote-only, bucket pull or push to S3/Azure | Samples: no license on page; terms at jetnet.com. Commercial use needs written agreement | `readsb-hist` global snapshots every 5 s (since 2020; 60 s back to 2016); `traces` (same decimation as adsb.lol); `hires-traces` 2 Hz | yes | yes (largest US feeder base). Unfiltered |
| **OpenSky Network** Trino (`trino.opensky-network.org`) | Application-reviewed. Free for university researchers, government and aviation authorities; private or commercial users need a licence | Research / non-commercial | `state_vectors_data4`: one row per aircraft-second (`time` int s) with `lastposupdate`/`lastcontact` | own MLAT (`readsb_mlat_sv`, and a position source in vectors) | 1090 only; thin US low-altitude coverage. FLARM table is mostly Europe |
| **airplanes.live** | Live REST ~1 req/s free. Historical only on a paid contributor tier (per-aircraft request quota, 8,640 req/day). No bulk dumps found | ToS, non-commercial | live | yes | yes (US feeders, UAT) |
| **adsb.fi** | Open live API `opendata.adsb.fi` ~1 req/s, ADSBx-v2 schema. No public bulk historical archive found | personal / non-commercial with attribution | live | yes | yes |
| **FAA SWIM** (STDDS/ASDE-X/TAIS, SFDPS, ADS-B SBS feeds) | Free JMS subscription via SCDS after approval. **No public historical archive** (FAA Q&A: no STDDS archive to reconstitute data); you must record it yourself from the start date | FAA data-sharing agreement | ~1 Hz surface (ASDE-X) / ~4.6–12 s radar (TAIS) | n/a (fused radar/ADS-B/MLAT) | TAIS includes Mode-C/primary targets near TRACONs. Best completeness, but only going forward |
| **traffic** (Python, v2.13) | library | MIT | whatever the source gives | — | OpenSky via pyopensky, raw-message decoding, METAR helper (`traffic.data.weather.metar`). **No adsb.lol / ADSBx loader** |
| **pyopensky** (v2.18, Oct 2026) | library | MIT | — | — | Trino `history()` (clips to window), `flightlist()`, `rawdata()`, `rebuild()`; local cache. Rust: `opensky` crate 0.2.1 (Trino client) |

Notes:
- adsb.lol release sizes (GitHub API, measured): 2026-09-08 prod = 2.00 GB + 1.87 GB = **3.87 GB**.
  2026-10-31…11-02 prod 3.1–3.5 GB, staging 2.5–3.0 GB, mlatonly 0.7–1.0 GB (single `.tar`). The repo README
  says to use whichever of prod or staging is not "significantly smaller". The 2026 repo totals 1976 GiB.
  Tar layout: `acas/`, `traces/<last-2-hex>/trace_full_<icao|~id>.json` (gzip despite `.json`), plus
  `heatmap/` (30 s replay snapshots) later in the stream.
- OpenSky Trino limits: 2 concurrent + 2 queued queries per user, 30 min max per query, a 100 GB scan
  limit, and a mandatory `hour` partition predicate (rejected without it since Trino 479). Runs longer
  than 5 min must be split. A full CONUS day = 24 hourly bbox queries. Feasible, but OpenSky's US
  low-altitude coverage is the weakest of these sources. The video's "Python scraping package run for
  hours" is most likely pyopensky/traffic against this DB.
- ADSBx `readsb-hist` measured: one snapshot (2026-10-01 18:00:00Z) is 9.1 MB of JSON with 18,459 aircraft
  globally, 10,237 in CONUS, and 2,479 CONUS aircraft below 4,000 ft baro. At 17,280 snapshots/day that is
  ~150 GB raw JSON per day, compressing to roughly 15–30 GB gzip (estimate). Snapshots are the only
  *uniformly sampled*, *complete-population* view (fixed 5 s), and they are the most likely origin of
  ~50 GB-scale Parquet like the video author's.
- **Free full-day choice:** adsb.lol (any day) or ADSBx (1st of the month only). If you control the date,
  test both on a 1st-of-month day to measure each one's low-altitude recall against the other.

## 2. Fields and pitfalls

### 2.1 readsb trace point (adsb.lol / ADSBx traces)
`[dt_s, lat, lon, alt_baro_ft|"ground"|null, gs_kt, track_deg (true heading when on ground), flags,
baro_rate_fpm, aircraft_obj|null, source_type, alt_geom_ft, geom_rate, ias, roll]`, with an absolute
file-level `timestamp` (midnight UTC of the day).
Flags: 1 = stale (no position for 20 s before this point), 2 = new leg, 4 = vertical rate is geometric,
8 = slot-3 altitude is geometric. `aircraft_obj` arrives every few points and carries `category` (A1 = light),
`nac_p`, `nic`, `gva`, `version`, `squawk`, `flight`. File-level metadata includes `r` (registration),
`t` (type designator), `desc`, `ownOp`, `year` (from the tar1090/Mictronics DB at write time).

Measured on 786 traces / 1.10 M points (first 30 MB of 2026-09-08 prod):
- Sources (all points): adsb_icao 93.0 %, adsr_icao 2.6 %, adsb_icao_nt 2.1 %, mlat 1.6 %, tisb_* 0.7 %.
  CONUS below 4,000 ft: adsb_icao 92.7 %, **adsr_icao 5.4 %**, tisb 1.9 %, mlat 0.1 %.
- 384 CONUS aircraft with ≥5 points below 4,000 ft, by dominant source: adsb_icao 314 (82 %), adsr_icao 24
  (6 %), tisb_other 23 + tisb_trackfile 21 (11 %, all **without** a type code), mlat 2.
- Geometric altitude present on 85 % of points. In the ADSBx snapshot it is on 96 % of aircraft below
  4,000 ft (mostly ADS-B v2).
- Baro altitude: 59 % of values are on a 25 ft step only and 41 % on 100 ft steps. Nothing finer.
- Inter-point interval: all points p10/50/90/99 = 0.7/4.1/19.7/23.2 s. CONUS below 4,000 ft =
  0.5/2.1/8.8/22.2 s. 4.5k gaps > 60 s in 1.1 M points.
- geom − baro below 5,000 ft: median +150 ft, p10 −50, p90 +350 ft. This is not a constant. Calibrate per
  aircraft and time.

### 2.2 Altitude: barometric vs geometric
- **alt_baro** is *pressure altitude* referenced to 1013.25 hPa (29.92 inHg), not MSL. Encoding: 25 ft steps
  when the Q-bit is set (most ADS-B), 100 ft Gillham steps otherwise (older and Mode-C-derived encoders; also
  above 50,175 ft). Static-source and altimetry errors on GA add tens of feet.
- **alt_geom** is GNSS height referenced to the **WGS84 ellipsoid (HAE)** per readsb. On 1090ES it is usually
  sent as a 25 ft-quantised *difference from baro* in the velocity message, so it inherits baro update
  timing. Quality comes from `gva` (GVA 2 ⇒ ≤45 m, 95 %). Some installations report MSL-like values. Sanity-check
  against DEM plus the ground/airborne transition at airports.
- HAE → MSL: `h_msl = h_hae − N`, with N from EGM2008. In CONUS N is roughly −8 m (Florida) to −37 m (Great
  Lakes / Rockies flank), so ~100 ft errors come from skipping this. DEMs (Copernicus GLO-30/90) are
  EGM2008 MSL. Compare like with like: either DEM+N vs HAE, or DEM vs HAE−N.
- **baro → MSL** when no geometric altitude exists:
  1. `p = 1013.25·(1 − PA/145366.45)^(1/0.190284)` hPa (static pressure from pressure altitude, ft).
  2. `h_ind = 145366.45·(1 − (p/QNH)^0.190284)` ft. The rule of thumb is ≈ PA + 27–30 ft per hPa of
     (QNH − 1013.25).
  3. Temperature: `h_true − h_stn ≈ (h_ind − h_stn)·T_actual/T_ISA` (≈ 4 % per 10 °C ISA deviation; small at
     ≤3,000 ft AGL).
  - QNH comes from nearby METAR `altim` (IEM ASOS archive: `mesonet.agron.iastate.edu/cgi-bin/request/asos.py`,
    fields `alti`, `tmpf`), interpolated in space and time.
  - Bonus: ADS-B v2 `nav_qnh` (the altimeter setting the pilot dialled) was present on 5,814 of 8,305 CONUS
    airborne aircraft in the ADSBx snapshot. It is a dense, crowd-sourced QNH field, available in snapshots
    and in the trace `aircraft_obj`.
- Recommended: per aircraft, use alt_geom where available. Elsewhere use baro plus that aircraft's own
  interpolated (geom − baro) offset, as DOPPLERGEIST does. Only when the aircraft never sends geom, use
  METAR/nav_qnh QNH conversion plus EGM2008. Label every point with its altitude basis.

### 2.3 Position, velocity, timing
- **Position:** CPR airborne resolution is ~5 m. Accuracy is reported by NACp (GA WAAS units typically NACp
  9–10, ≤30 m / ≤10 m). MLAT positions are noisy (tens to hundreds of m), and readsb ignores MLAT track
  for decimation.
- **Trace decimation contract** (readsb `traceAddInternal`, `globe_index.c`):
  - A point is written when any of these hold:
    - Δgs ≥ 5 kt (10 kt above 10,000 ft)
    - baro-rate change ≥ 200 fpm
    - Δalt ≥ 75 ft below 7,000 ft
    - in a turn, `elapsed_s × Δtrack_deg × 5 > 100` (≈ every 2.6 s in a standard-rate turn; chord sagitta
      ≈ 2 m at 50 m/s)
    - stale reception
    - air/ground transition (then 1 Hz for a while)
  - Otherwise, at most every `json-trace-interval` (30 s default; adsb.lol ≈ 20–24 s in practice), with
    MLAT max(interval/2, 5 s).
  - ⇒ **Linear interpolation between stored points is accurate by construction** within ~5 kt / 75 ft /
    a few metres lateral. Do not assume steady speed beyond a 5 kt band: for f0 = 100 Hz, 2.5 m/s of radial
    speed error is ~0.75 Hz. Fit the Doppler curve with speed as a free parameter, constrained by the band.
- **Ground speed vs TAS:** `gs` is over-ground speed and `track` is the true course over ground. For the
  geometry (relative velocity to a ground observer) gs/track is exactly what you need, so wind does not
  matter. Heading (`true_heading`, mag→true via WMM) and TAS/IAS matter only for crab-angle and propeller
  source modelling, not for Doppler kinematics. Velocity resolution is 1 kt and track ~0.1°. Vertical rate
  is in 64 fpm steps.
- **Timestamps:** 1090ES carries no time-of-applicability. Trace times are aggregator receive times (feeder
  clock; MLAT uses GPS-disciplined clocks). Expect ~0.1–1 s latency/jitter, plus ≤2 s allowed total ADS-B
  position latency (≤0.6 s uncompensated, 14 CFR 91.227). At 50 m/s, 1 s is 50 m along-track, i.e. a
  CPA-time bias. This is negligible for the 920 s inter-event offset, but keep ±1–2 s in the
  delayed-CPA tolerance.
- **Gaps:** split segments on the stale flag or dt > ~60 s. Never interpolate across a gap that ends at
  a landing or takeoff. A gap that starts at low altitude near an airport usually means a reception floor,
  not that the aircraft vanished.

### 2.4 Non-ADS-B and the UAT question
- **UAT (978 MHz)** is legal below FL180 in the US and is popular in GA. Fleet share is roughly a third of
  GA installs (2014 FAA ≈ 1/3; Q1 2019 new GA installs 19.5 % UAT). Treat that as an order of magnitude,
  since the FAA equipage page could not be fetched.
- UAT aircraft enter 1090 datasets in two ways:
  1. A feeder runs a 978 receiver. dump978 → readsb `uat_in` → `uat2esnt` re-encodes it as DF18
     **CF=6 (ADS-R)**, so it is labelled `adsr_icao` (verified in readsb `uat2esnt.c`).
  2. FAA ground stations rebroadcast ADS-R on 1090, but **only when an ADS-B-In client aircraft is nearby**
     (client-based service).
  - Either way, UAT-only GA is under-sampled wherever no local 978 feeder exists.
- **UAT anonymous mode** (VFR, squawk 1200) uses a self-assigned rotating address → `adsr_other` / `~` IDs,
  with no registration or type. **PIA** (1090 Privacy ICAO Address) gives a temporary address that is not in
  the registry → typecode missing.
- **TIS-B** (`tisb_icao`, `tisb_other`, `tisb_trackfile`): FAA-radar targets (Mode-C/primary) rebroadcast,
  again only near ADS-B-In clients. The positions come from radar (coarse, ~5–12 s), IDs are non-ICAO and
  change, and these targets can duplicate real ADS-B tracks. In the sample they were 11 % of low-altitude
  CONUS aircraft, all untyped.
- **Mode-C-only / non-transponder** aircraft (gliders, ultralights, some antiques, outside rule airspace):
  invisible except via MLAT (needs ≥4 receivers, rare at low altitude: 0.1 % of low points), TIS-B, or
  FAA radar (SWIM TAIS, not public historically). uAvionix estimates 20–30k unequipped low-flying GA and
  helicopters.

## 3. Coverage: what fraction of low GA is captured?

- No published figure exists for "% of GA flights below 3,000 ft AGL captured by crowd networks in the US".
  Studies give reception-probability models (TU Delft/OpenSky), and FAA work documents low-altitude
  surveillance gaps at many non-towered airports.
- Physics: radio line-of-sight `d ≈ 4.12(√h_rx + √h_ac)` km (heights in m). For an aircraft at 300 m AGL and
  a receiver at 10 m, the geometric horizon is ~84 km. In practice it is 30–60 km for GA's 125–250 W
  transponders, less in terrain, and the bottom of the coverage floor is set by the nearest feeder. Metro
  areas are dense, so traffic is tracked to the runway. Rural West, Appalachians and the Plains have
  holes at 1,000–3,000 ft AGL.
- Measured snapshot: 2,479 CONUS aircraft below 4,000 ft at 18Z on a weekday (ADSBx), ~24 % of all CONUS
  airborne aircraft. adsb.lol has fewer US feeders than ADSBx, so expect lower recall. Quantify the
  difference on a 1st-of-month day.
- **How holes break the silence filter:**
  1. *Unseen audible aircraft* (holes, UAT without 978 feeder or ADS-R, Mode-C only). The true location looks
     "silent" between events when it was not. That alone is harmless to pruning but corrupts scoring.
     Worse, if a recorded event *came from* an unseen aircraft, there is no trajectory to match and the
     true location cannot be found.
  2. *Systematic bias:* the silence filter keeps points that "should have heard nothing". Poorly covered
     regions appear quieter, so they **survive preferentially**. Candidate density then concentrates in
     coverage holes (rural, mountains), exactly where truth is least checkable.
  3. *Within-track dropouts:* aircraft lose coverage when they are lowest, i.e. loudest and nearest
     (pattern work, approaches). A gap there makes nearby points look silent.
  4. *Seen-but-inaudible aircraft* (high, quiet, masked) wrongly *disqualify* the true point. This is an
     audibility-model problem, but it pairs with (1).
- **Mitigation:** build a per-day coverage map, e.g. per H3 res-6 cell and 1 kft altitude band:
  - observed low-altitude track-seconds
  - median **reception floor** = last-seen height AGL of arrivals at each airport, first-seen of departures
  - fraction of segments ending in stale gaps

  Weight silence evidence by P(detect | cell, altitude) and never let silence count in cells below a
  coverage threshold. Report the coverage class of every finalist.

## 4. Storage and indexing at national scale

### 4.1 Sizes (one day)
| Item | Size |
|---|---|
| adsb.lol prod tar (global, gz JSON traces + heatmap + acas) | 3.1–3.9 GB |
| Trace points/day global (extrapolated from 3 of 256 hex dirs) | ~95–100 M; CONUS ~50 M; CONUS non-jet ~24 M; CONUS non-jet <6,000 ft ~18 M |
| Bytes/point in source | 27.6 B (gz JSON) |
| Compacted Parquet (measured on 584k CONUS rows, int-scaled columns, zstd-6, 1 M-row groups) | **16.4 B/row** sorted (icao, t); 18.8 B/row sorted by t |
| ⇒ CONUS day, all aircraft / non-jet only | ~0.8 GB / ~0.4 GB |
| ADSBx readsb-hist (5 s snapshots) | ~150 GB raw JSON/day; 10–25 GB as Parquet (estimate) |
| Derived segments (one per consecutive point pair) + CPA grids | segments ≈ rows; the CPA grid output, not ADS-B, dominates RAM and disk |

Measured column scaling: `icao u32` (bit 24 = non-ICAO `~`), `t_ms i64`, `lat_e6/lon_e6 i32`,
`alt_baro_ft/alt_geom_ft i32`, `gs_dkt u16`, `trk_cdeg u16`, `vrate i16`, `flags u8`, `src u8` (dictionary).
DELTA_BINARY_PACKED on t/lat/lon within the (icao, t) sort should beat 16 B/row further.

### 4.2 Parquet layout
- **Two tables:**
  - `points` (immutable, provenance): sorted (icao, t), with row groups of ~1 M rows aligned to
    whole aircraft.
  - `segments` (query table): one row per gap-split segment piece. Columns: `seg_id, icao, t0, t1`, start
    and end lat/lon/alt_msl/alt_hae, `vx, vy, vz` (or a Hermite tangent), bbox, `cell` (spatial key),
    `engine_class`, `alt_basis`, `src`, `quality`.
    Cut long legs into pieces of ≤60 s or ≤5 km so bboxes stay tight.
- Sort `segments` by (time bucket = 10 min, Hilbert/H3 key). Row groups of 64–256k rows give each row
  group min/max statistics on `t0/t1` and `cell` that prune well. Add a page index.
  - Partition files by UTC hour: `segments/date=2026-09-08/hour=20/part-*.parquet`. That matches the
    OpenSky partitioning and lets a single-event search open 1–2 files.
- Keep a Parquet KV metadata block: source release tag, asset sha256, selection bbox and window, gap
  threshold, type-DB version, schema version. DOPPLERGEIST's `dopplergeist.selection.v2` key is the
  pattern to copy.

### 4.3 Spatial-temporal query: "all segments within 10 km of P during [t0, t1]"
- **H3 (h3o):**
  - res 6 cells have a ~3.2 km edge and 36 km² area; res 5 ~8.5 km and 253 km².
  - Index each segment piece under every res-6 cell its bbox buffer touches, as a `(cell, seg_id)` side
    table. Query with `grid_disk(P_cell, k)`, where k = ceil(10 km / ~5.6 km cell spacing) + 1 ≈ 3
    (37 cells), then filter on time and exact segment-to-point distance.
  - Pros: uniform and easy to join with coverage maps. Cons: duplication for long segments.
- **Z-order/Hilbert on EPSG:5070 (CONUS Albers, metres) grid:** one u64 key per segment midpoint, plus
  per-row-group bbox stats. Simplest for Parquet pruning; range queries come out as a few key intervals.
- **R-tree (rstar 0.14):** bulk-load `AABB<[f64;3]>` over (x, y, t·v_scale) or 2D bbox per time bucket,
  in memory at query time. 24 M segments × ~48 B ≈ 1.2 GB, so one tree per hour (~1–2 M segments) is
  ~100 MB and builds in under a second with `bulk_load`. Best for the hot loop where every grid point
  needs nearby segments.
- **S2** (crate `s2` 0.2) works, but its Rust ecosystem is weaker than h3o. Not recommended.
- **Recommended:** Hilbert key (5070) for on-disk ordering and coarse pruning, then an in-memory
  per-time-window rstar tree for the CPA kernel. Treat H3 res 6 as the *reporting and coverage* grid.
- **Rust crates (crates.io, Nov 2026):**
  - `arrow` / `parquet` 60.1: native async reader, row-group/page-index pruning, `ProjectionMask`
  - `polars` 0.55: convenient, but heavy compile, and its API churns
  - `datafusion` 55 (SQL over Parquet with pruning; optional)
  - `h3o` 0.11
  - `rstar` 0.14
  - `geo` 0.33
  - `geoarrow` 0.9
  - `rayon` 1.12
  - `zstd` 0.14

  Prefer arrow-rs/parquet directly in the core crate. Use polars or pyarrow on the Python side only.

## 5. Aircraft type metadata (icao24 → type → engine)

- **tar1090-db** (Mictronics-derived; `csv` branch `aircraft.csv.gz`, 8.2 MB, 623,847 rows
  `ICAO;reg;type;dbFlags;desc;year;owner;`; `db/icao_aircraft_types.js`, 2,788 designators →
  `{desc: "L1P", wtc}`). `desc` = ICAO Doc 8643 class: L/S/A/H/G/T + engine count + engine type
  (P piston, T turboprop/turboshaft, J jet, E electric, R rocket, `-` none).
- Measured US range (A00000–ADF7C7): 393,511 registrations, of which **320,351 (81 %) have a type code**.
  Engine class among those: P 236,728 / T 26,450 / J 35,826 / none 19,985 / other 1.2k.
  19 % untyped, plus TIS-B and anonymous tracks: that is why the method *excludes known jets* rather than
  keeping known props. Unknown designators seen include KODI, JS3E, VENT, DISC (gliders/new types).
- **FAA Releasable Aircraft DB** (`registry.faa.gov/database/ReleasableAircraft.zip`, ~60 MB, updated each
  federal workday):
  - `MASTER.txt` has `MODE S CODE HEX` and `TYPE ENGINE`: 0 None, 1 Reciprocating, 2 Turbo-prop,
    3 Turbo-shaft, 4 Turbo-jet, 5 Turbo-fan, 6 Ramjet, 7 2-cycle, 8 4-cycle, 9 Unknown, 10 Electric,
    11 Rotary.
  - `ACFTREF.txt` (via MFR MDL CODE) gives TYPE-ACFT, TYPE-ENG, NO-ENG, weight class and cruise speed.
  - This fills the tar1090 gaps for N-registered aircraft (with the PIA caveat) and adds a cruise-speed
    prior for the speed filter.
- Also available: OpenSky aircraft DB (CSV, mixed quality), and `category` emitted in ADS-B (A1 light
  <15.5k lb, A7 rotorcraft, B1 glider, A2 small) as a fallback for untyped tracks.
- Join order:
  1. trace `t`
  2. FAA MASTER by hex → TYPE ENGINE
  3. tar1090 `aircraft.csv` → `desc`
  4. ADS-B `category`
  5. `unknown`

  Store `engine_class ∈ {piston, turboprop, turboshaft, jet, electric, none, unknown}` and
  `engine_source`.

## Design implications for doppler_print

1. **Primary source:** adsb.lol daily release (free, ODbL, any date, includes adsr/tisb/mlat). Pick prod vs
   staging per day by size from the GitHub API. Optionally merge the `mlatonly` pod. **Secondary:**
   ADSBx 1st-of-month samples for validation/recall studies; OpenSky Trino only if we get research access,
   as a cross-check. SWIM is future-only. Expose the source as a pluggable reader trait; never hard-wire
   the readsb format past the ingest stage.
2. **Ingest pipeline (Rust, streaming, one pass, bounded RAM):**
   1. Fetch: GitHub release JSON → range-resumable download → verify the `digest` sha256 → record a receipt.
   2. Stream `cat parts | tar` (crate `tar` over a chained reader) → per `trace_full_*` → `flate2` gunzip →
      `serde_json` (or `simd-json`) into a typed point struct. Use rayon with a bounded channel, with a
      single Parquet writer thread.
   3. Normalise: absolute `t_ms`; drop `"ground"` points except as leg delimiters; split on the stale flag
      or dt > 60 s. Altitude: geom → per-aircraft offset → QNH (METAR/nav_qnh) + EGM2008 fallback, keeping
      the `alt_basis` label. MSL and HAE both stored.
   4. Classify: type/engine join (§5). Keep everything, and mark jets instead of dropping them, so the
      silence model can use the full population and the jet ablation stays possible.
   5. Write `points` (sorted icao, t) and `segments` (≤60 s / ≤5 km pieces, sorted by hour / Hilbert-5070
      key), zstd, with provenance KV metadata.
   6. Write a `coverage` table (H3 res 6 × altitude band × hour: track-seconds, reception-floor estimates,
      stale-gap rate).
3. **On-disk format:** `adsb/<date>/{points,segments}/hour=HH/*.parquet` + `coverage.parquet` +
   `manifest.json` (source tags, sha256, versions). Expect ~1 GB/day all aircraft, ~0.5 GB/day non-jet.
   No SQLite needed; the query layer loads one hour's segments into an `rstar` tree (~100 MB).
4. **Physics hygiene:** compute in ECEF/ENU with HAE throughout. Convert the DEM (EGM2008 MSL) to HAE
   once. Speed is a free parameter inside the 5 kt decimation band. Add ±2 s to delayed-CPA tolerances
   for timestamp latency. Interpolate linearly within segments, never across gaps.
5. **Silence is conditional on coverage:** gate or weight the silence filter by the coverage table,
   report the coverage class per candidate, and surface the "survives because unobserved" bias in
   confidence output. Treat TIS-B/anonymous `~` tracks as audible traffic but unidentifiable (unknown
   engine), and deduplicate them against ADS-B tracks by spatial/temporal proximity.
6. **Licensing:** derived Parquet from adsb.lol is an ODbL derivative database. Keep attribution, publish
   any shared derived DB under ODbL, and do not commingle with ADSBx or OpenSky data in a redistributed
   artifact.

## Samples in `docs/research/samples/` (all < 50 MB total)
- `adsblol_2026-09-08_head/prod-0.tar.aa.head30M`: first 30 MB of the prod tar (786 complete traces; the
  last member is truncated). Extract with `tar -xf … ` and ignore the EOF error.
- `adsbx_2026-10-01/readsb-hist_180000Z.json.gz`: one 5 s global snapshot (plain JSON despite the name).
- `tar1090_db/{aircraft.csv.gz, icao_aircraft_types.js}` (the latter is gzip despite `.js`).
- `scripts/{trace_stats,per_ac,share,parquet_size}.py`: regenerate every "measured" number above.

## Sources
- https://github.com/adsblol/globe_history_2026 (README, releases, RELEASES.md)
- https://api.github.com/repos/adsblol/globe_history_2026/releases
- https://github.com/wiedehopf/readsb/blob/dev/README-json.md (trace format, source types, alt_geom = WGS84)
- https://github.com/wiedehopf/readsb/blob/dev/globe_index.c (`traceAddInternal` decimation rules)
- https://github.com/wiedehopf/readsb/blob/dev/uat2esnt/uat2esnt.c (UAT → DF18 CF=6)
- https://github.com/sdr-enthusiasts/docker-adsb-ultrafeeder (uat_in ingest of dump978)
- https://www.adsbexchange.com/data-products/sample-data/ and https://www.adsbexchange.com/api
- https://samples.adsbexchange.com/readsb-hist/2026/10/01/
- https://openskynetwork.github.io/opensky-api/trino.html (eligibility, limits, state_vectors_data4 schema)
- https://pypi.org/project/pyopensky/ ; https://docs.rs/opensky
- https://github.com/xoolive/traffic ; https://traffic-viz.github.io/
- https://flyitalyadsb.com/en/dati/confronto/ (airplanes.live / adsb.fi API terms comparison)
- https://www.faa.gov/air_traffic/technology/swim/stdds ;
  https://www.faa.gov/sites/faa.gov/files/2022-12/2015-April-SWIM_Users_Forum-Q%26A.docx
- https://www.faa.gov/air_traffic/technology/equipadsb/installation/current_equipage_levels (403 to fetch)
- https://www.kitplanes.com/what-is-happening-on-the-ads-b-scene/ (UAT share of new GA installs)
- https://discussions.flightaware.com/t/any-idea-how-many-1090-verses-uat-out-there-are/70835
- https://uavionix.com/harnessing-the-shifting-landscape-of-ads-b-equipage-opportunities-for-electronic-conspicuity-and-uas/
- https://www.faa.gov/air_traffic/technology/equipadsb/privacy (PIA)
- https://teleavo.substack.com/p/how-ads-b-r-magic-happens (ADS-R client-based rebroadcast)
- https://journals.open.tudelft.nl/joas/article/download/8459/6435 (ADS-B reception probability, OpenSky)
- https://dspace.mit.edu/handle/1721.1/67188 (low-altitude surveillance gaps, CONUS)
- https://github.com/wiedehopf/tar1090-db ; https://github.com/wiedehopf/tar1090-db/raw/refs/heads/csv/aircraft.csv.gz
- https://registry.faa.gov/database/ardata.pdf ;
  https://www.faa.gov/licenses_certificates/aircraft_certification/aircraft_registry/releasable_aircraft_download
- https://mesonet.agron.iastate.edu/request/download.phtml (IEM ASOS/METAR archive)
- https://cdn.proj.org/ (EGM2008 geoid grids) ; https://copernicus-dem-30m.s3.amazonaws.com/readme.html
- https://crates.io/crates/{arrow,parquet,polars,h3o,rstar,s2,geo,geoarrow,datafusion,rayon,zstd}
- https://h3geo.org/docs/core-library/restable/ (H3 cell sizes)
- Local: reference/DOPPLERGEIST/docs/{DATA_SOURCES,DATA_AND_SCALE,HISTORICAL_INGESTION}.md,
  scripts/{fetch,ingest}_adsblol_day.py, src/dopplergeist/tracks.py
