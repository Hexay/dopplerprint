# ADS-B track store

Built by `doppler-print ingest <archive> --out <store>` (`dp_core::adsb::ingest_day`). Read with
`load_tracks` / `doppler-print tracks` / `doppler_print._core.load_tracks`. Source rationale:
`research/adsb_data.md`.

## Input

- adsb.lol `globe_history_YYYY` day: pass `*.tar.aa` (all `*.tar.a?` siblings are chained), a release dir holding
  the parts, a single `.tar`, or a dir of extracted `trace_full_*.json` (gzip or plain, detected by magic).
- A truncated tar is ingested up to the break; `manifest.truncated_archive` is set.
- Type DB: `--typedb DIR` or `$DP_TYPEDB`, else `./data/typedb`, with tar1090-db `icao_aircraft_types.js[on]` and
  `aircraft.csv[.gz]`. Without one every engine is `Unknown`.

## Cleaning (per aircraft, `adsb::clean`)

- Ground points are dropped and end the current track. Points with no altitude are dropped.
- A new track starts at a gap over 60 s (`max_gap_s`) or at a readsb new-leg flag (bit 2). The stale flag
  (bit 1, a 20 s gap) does not split.
- Jumps: a point is rejected if, relative to the last accepted point, ground distance > 350 m/s · dt + 150 m or
  |Δalt| > 60 m/s · dt + 60 m. After 3 consecutive rejections the anchor is assumed bad: the track is closed and
  the rejected points start a new one.
- Altitude basis `alt_source`: 0 `Geometric` = WGS84 ellipsoidal (slot 10, or slot 3 when flag bit 8 is set);
  1 `BaroOffset` = baro + the aircraft's own interpolated geom−baro offset (nearest sample ≤ 600 s away);
  2 `Baro` = pressure altitude only. Never treat 2 as a height.
- Velocity: ENU from gs/track and geom_rate (else baro_rate). Missing components come from position finite
  differences within the track. ECEF position/velocity is computed at load time.
- Tracks with fewer than 2 points are dropped. Jets are kept and flagged (`engine`).

## Layout

```
<store>/date=YYYY-MM-DD/
  manifest.json                     counts, drops, params, type-DB files, licence
  coverage.parquet
  points/part-BBB.parquet           BBB = icao >> 17; ascending files = ascending icao
  segments/hour=HH/part-0.parquet   HH = floor((t0 − day0)/3600); may be −1 or ≥ 24 near midnight
```

Every file is zstd Parquet with page statistics and KV metadata `doppler_print.schema = doppler_print.adsb.v1`,
`doppler_print.date` and `doppler_print.licence`. One store can hold many days. A flight crossing midnight is
two tracks, one per day.

### points: cleaned samples, sorted (icao, t), row groups of 128k rows

| column | type | meaning |
|---|---|---|
| icao | u32 | 24-bit address; bit 24 set = non-ICAO `~` id (TIS-B, anonymous) |
| track_no | u32 | gap-split track index within the aircraft's day, from 0 |
| t | f64 | UNIX s UTC |
| lat, lon | f64 | degrees |
| alt_m | f32 | metres in the `alt_source` basis |
| alt_source | u8 | 0 Geometric, 1 BaroOffset, 2 Baro |
| ve, vn, vu | f32 | ENU velocity, m/s |
| src | u8 | index into `adsb::model::SOURCES` (`adsb_icao`, `adsr_icao`, `tisb_*`, `mlat`, …); 12 = unknown |
| flags | u8 | raw readsb trace flags |

### segments: query table, one row per piece of ≤ 60 s and ≤ 5 km, sorted (hilbert, t0), row groups of 64k

Consecutive pieces share their boundary point, so every point-to-point leg lies inside one piece's bbox.

| column | type | meaning |
|---|---|---|
| seg_id | u64 | icao << 39 \| track_no << 24 \| piece index |
| icao, track_no | u32 | join key into `points` |
| hilbert | u64 | Hilbert index of the bbox centre on a 2³²×2³² lat/lon grid |
| t0, t1 | f64 | piece time span |
| lat_min, lat_max, lon_min, lon_max | f64 | bbox |
| alt_min_m, alt_max_m | f32 | |
| n_points | u32 | |
| engine | u8 | 0 Piston, 1 Turboprop, 2 Jet, 3 Helicopter, 4 Unknown |
| engine_source | u8 | 0 none, 1 trace `t`, 2 registry `aircraft.csv`, 3 ADS-B category (A7 only) |
| type_code | utf8? | ICAO designator |
| alt_source | u8 | worst basis in the piece |
| src | u8 | source of the first point |

### coverage: detection-probability proxy

One row per (hour, 0.1° cell, 1000 ft band). Band 20 holds everything at or above 20,000 ft.

| column | type | meaning |
|---|---|---|
| hour | i16 | as for segments |
| lat_cell, lon_cell | i16 | floor(lat / 0.1), floor(lon / 0.1) |
| alt_band | u8 | floor(alt_ft / 1000), clamped to 0..=20 |
| n_aircraft | u32 | distinct aircraft seen in the bin |
| track_s | f32 | track-seconds (each leg's dt is credited to its start point's bin) |

## Queries

- `load_tracks(store, TrackQuery)` opens only the hour partitions that overlap `[t_start − 60 s, t_end]`. It prunes
  row groups on bbox, altitude and `t1` statistics, then filters rows on time overlap, bbox, `alt_min_m ≤ max_alt_m`
  and the engine exclusions.
- Matching tracks' points are read from `points` with icao/t row-group pruning. They are clipped to the window
  plus one point either side, but not to the bbox.
- Track ids are `YYYY-MM-DD/<icao>-<track_no>`.
- `SegmentIndex::from_tracks(&tracks)` builds an rstar tree of ECEF leg chords. `.within(p, r, t0, t1)` and
  `.within_geodetic(..)` return legs within `r` m that are active during `[t0, t1]`, nearest first.

## Licence

Stores built from adsb.lol are ODbL 1.0 derivative databases (data © adsb.lol contributors). Keep the
attribution, share any redistributed store under ODbL, and don't mix it with ADSBx or OpenSky data in a
redistributed artifact.
