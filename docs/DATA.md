# ADS-B track store (`doppler_print.adsb.v2`)

Built by `doppler-print ingest <archive> --out <store>` (`dp_core::adsb::ingest_day`). Read with
`load_tracks` / `doppler-print tracks` / `dopplerprint._core.load_tracks`. Source rationale:
`docs/research/adsb_data.md`. v1 stores are not readable by v2 code; re-ingest (`tools/ingest_corpus_days.sh`).

A full CONUS adsb.lol day ingests in ~30–40 s at ~2.4 GB peak RSS and takes ~1.5 GB on disk.

## Input

- adsb.lol `globe_history_YYYY` day: pass `*.tar.aa` (all `*.tar.a?` siblings are chained), a release dir holding
  the parts, a single `.tar`, or a dir of extracted `trace_full_*.json` (gzip or plain, detected by magic).
- A truncated tar is ingested up to the break; `manifest.truncated_archive` is set.
- Type DB: `--typedb DIR` or `$DP_TYPEDB`, else `./data/typedb`, with tar1090-db `icao_aircraft_types.js[on]` and
  `aircraft.csv[.gz]`. Without one every engine is `Unknown`.

## Pipeline (`adsb::ingest`, `adsb::finalize`)

Tar reader thread → rayon per-trace parse/clean → single writer spilling rows into buckets → finalize sorts and
writes each bucket on `finalize_threads` workers. Peak memory ≈ threads × largest bucket.

1. **Non-aircraft filter.** Traces whose type is a tower, ground vehicle (tar1090 class `T0`/`V0`) or `SHIP` are
   dropped whole (`manifest.non_aircraft_traces`); they are never acoustic sources.
2. **Cleaning (per aircraft, `adsb::clean`).**
   - Ground points are dropped and end the current track. Points with no altitude are dropped.
   - A new track starts at a gap over 60 s (`max_gap_s`) or at a readsb new-leg flag (bit 2). The stale flag
     (bit 1, a 20 s gap) does not split.
   - Jumps: a point is rejected if, relative to the last accepted point, ground distance > 350 m/s · dt + 150 m or
     |Δalt| > 60 m/s · dt + 60 m. After 3 consecutive rejections the anchor is assumed bad: the track is closed
     and the rejected points start a new one.
   - Velocity: ENU from gs/track and geom_rate (else baro_rate); missing components from position finite
     differences within the track. ECEF position/velocity is computed at load time.
   - Tracks with fewer than 2 points are dropped. Jets are kept and flagged (`engine`).
3. **TIS-B dedup (`adsb::dedup`).** A non-ICAO (`~`) track is dropped when ≥ 80% of its points lie within
   1500 m of a concurrent ICAO track (an FAA radar rebroadcast of an aircraft already on ADS-B). Unmatched `~`
   tracks stay: they are real, audible traffic.
4. **Altitude basis `alt_source`** (`adsb::altitude`, `adsb::neighbour`):
   - 0 `Geometric`: WGS84 ellipsoidal (slot 10, or slot 3 when flag bit 8 is set).
   - 1 `BaroOffset`: baro + the aircraft's own interpolated geom−baro offset (nearest sample ≤ 600 s away).
   - 3 `BaroNeighbour`: baro + the mean geom−baro offset of other aircraft in the same (hour, 1° cell, 2000 ft
     baro band), needing ≥ 2 contributing aircraft. Applied at finalize to rows that would otherwise be `Baro`
     (mostly TIS-B / MLAT aircraft that never report geometric altitude).
   - 2 `Baro`: pressure altitude only. Never treat it as a height.

## Layout

```
<store>/date=YYYY-MM-DD/
  manifest.json                     counts, drops, params, type-DB files, licence
  coverage.parquet
  points/part-BBB.parquet           BBB = icao >> 16; ascending files = ascending icao
  segments/hour=HH/part-0.parquet   HH = floor((t0 − day0)/3600); may be −1 or ≥ 24 near midnight
```

Every file is zstd (level 6) Parquet with page statistics and KV metadata
`doppler_print.schema = doppler_print.adsb.v2`, `doppler_print.date` and `doppler_print.licence`.

**Encoding.** Continuous values are stored as scaled integers so `DELTA_BINARY_PACKED` + zstd exploit the
(icao, t) sort (`adsb::schema`): time in ms (`*_ms`, i64), degrees × 1e6 (`*_e6`, i32), altitude in dm
(`alt_dm`), velocity in cm/s (`*_cms`). Integer columns are delta-encoded; only u8/string columns use
dictionaries. Readers convert back to `f64` seconds/degrees/metres; callers never see the integers.

One store can hold many days. A flight crossing midnight is two tracks, one per day.

### points: cleaned samples, sorted (icao, t), row groups of 128k rows

| column | type | meaning |
|---|---|---|
| icao | u32 | 24-bit address; bit 24 set = non-ICAO `~` id (TIS-B, anonymous) |
| track_no | u32 | gap-split track index within the aircraft's day, from 0 |
| t_ms | i64 | UNIX ms UTC |
| lat_e6, lon_e6 | i32 | degrees × 1e6 |
| alt_dm | i32 | decimetres in the `alt_source` basis |
| alt_source | u8 | 0 Geometric, 1 BaroOffset, 2 Baro, 3 BaroNeighbour |
| ve_cms, vn_cms, vu_cms | i32 | ENU velocity, cm/s |
| src | u8 | index into `adsb::model::SOURCES` (`adsb_icao`, `adsr_icao`, `tisb_*`, `mlat`, …) |
| flags | u8 | raw readsb trace flags |

### segments: query table, one row per piece of ≤ 60 s and ≤ 5 km, sorted (hilbert, t0), row groups of 64k

Consecutive pieces share their boundary point, so every point-to-point leg lies inside one piece's bbox.
Bboxes are rounded outward (floor/ceil) when scaled, so they never shrink.

| column | type | meaning |
|---|---|---|
| icao, track_no | u32 | join key into `points` |
| hilbert | u64 | Hilbert index of the bbox centre on a 2³²×2³² lat/lon grid |
| t0_ms, t1_ms | i64 | piece time span, UNIX ms |
| lat_min_e6, lat_max_e6, lon_min_e6, lon_max_e6 | i32 | bbox, degrees × 1e6 |
| alt_min_m, alt_max_m | i32 | whole metres (floor / ceil) |
| n_points | u32 | |
| engine | u8 | 0 Piston, 1 Turboprop, 2 Jet, 3 Helicopter, 4 Unknown |
| engine_source | u8 | 0 none, 1 trace `t`, 2 registry `aircraft.csv`, 3 ADS-B category (A7 only) |
| type_code | utf8? | ICAO designator |
| alt_source | u8 | worst basis in the piece |
| src | u8 | source of the first point |

### coverage: detection-probability proxy for the silence stage

One row per (hour, 0.1° cell, 1000 ft band). Band 20 holds everything at or above 20,000 ft.

| column | type | meaning |
|---|---|---|
| hour | i16 | as for segments |
| lat_cell, lon_cell | i16 | floor(lat / 0.1), floor(lon / 0.1) |
| alt_band | u8 | floor(alt_ft / 1000), clamped to 0..=20 |
| n_aircraft | u32 | distinct aircraft seen in the bin |
| track_s | f32 | track-seconds (each leg's dt is credited to its start point's bin) |
| n_appear | u32 | tracks that start airborne in the bin |
| n_vanish | u32 | tracks that end airborne in the bin; many at low bands mark the local reception floor |

## Queries

- `load_tracks(store, TrackQuery)` opens only the hour partitions that overlap `[t_start − 60 s, t_end]`. It prunes
  row groups on bbox, altitude and `t1` statistics, then filters rows on time overlap, bbox, `alt_min_m ≤ max_alt_m`
  and the engine exclusions.
- Matching tracks' points are read from `points` with icao/t row-group pruning. They are clipped to the window
  plus one point either side, but not to the bbox.
- Track ids are `YYYY-MM-DD/<icao>-<track_no>`. The Python dicts also carry per-point `lat`, `lon`, `h_ell`.
- `SegmentIndex::from_tracks(&tracks)` builds an rstar tree of ECEF leg chords. `.within(p, r, t0, t1)` and
  `.within_geodetic(..)` return legs within `r` m that are active during `[t0, t1]`, nearest first.

## Licence

Stores built from adsb.lol are ODbL 1.0 derivative databases (data © adsb.lol contributors). Keep the
attribution, share any redistributed store under ODbL, and don't mix it with ADSBx or OpenSky data in a
redistributed artifact.
