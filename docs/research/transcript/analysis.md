# Transcript analysis: colsto, "This background sound leaks your exact location"

Source: https://www.youtube.com/watch?v=UytYrhYLOKI (transcript in `transcript_raw.txt`, via pastebin 7rAw1Mcv).
DOPPLERGEIST is an independent reconstruction of this video's method.

## Problem setup
- Input: a static security-camera clip. Known: date (Sept 8) and region (continental US). Nothing else.
- Signal: faint propeller-aircraft Doppler curves visible only in a zoomed, high-resolution spectrogram (inaudible).
- Two events ~920 s apart, plus a third faint event (used later on its own). The louder event shows harmonics.
- Author's scale: >5k LOC, ~50 GB Parquet ADS-B, 3D physics, nationwide DEMs. Result: within ~100 m (vs ~5 km for thunder).

## Pipeline (in order)
1. **ADS-B acquisition**: full-day CONUS flights (paid API or a Python scraping package, run for hours). Full flight paths, including segments outside the US.
2. **Propeller filter**: map ICAO type codes to engine type through a lookup table. The table has gaps, so the method *excludes known jets* rather than keeping known props.
3. **CPA grid (coarse)**: around each flight lay a 200 m grid within a 10 km radius. For each point:
   - CPA = time of minimum slant distance between aircraft and observer.
   - Slant distance uses the aircraft altitude (MSL) minus the DEM ground elevation at the point.
   - **Delayed CPA** = CPA time + slant_dist / c (when the CPA sound arrives).
   - Each grid point stores a list of (flight_id, delayed_CPA_time).
4. **Time-offset filter**: keep points that hear one flight and then another 920 s ± ~5 s later. Small test area (<0.05% of CONUS) leaves ~35k points.
5. **Constant-velocity Doppler fit per event**: the frequency drop sets speed and the drop steepness sets CPA slant distance. Also fits f0 and the CPA time inside the audio. Use generous ranges (e.g. ~100 m/s, clearly not 70). Event 1's left side does not fit the ideal curve because of manoeuvring.
6. **Speed filter + distance-constrained candidates**: keep flight segments whose speed is in range, and lay candidates only at the fitted slant-distance range from them, not across the whole 0–10 km disc. About 73k points remain, clustered at airports.
7. **Silence filter**: disqualify points that would have heard any other flight between the two events (conservative thresholds). 70k → 15k. Silence is evidence.
8. **Full-geometry Doppler modelling** (the key step):
   - For observer time t, solve for the emission time τ with t = τ + |p(τ) − obs| / c (retarded time).
   - Compute the radial velocity v_r at τ from the real trajectory, then f(t) = f0 · c / (c − v_r) (v_r positive when approaching).
   - Using the radial velocity at time t instead of at τ gives an entirely different curve, not a time-shifted one, and makes the search fail.
9. **Scoring**: overlay the modelled curve on the spectrogram and sum the brightness under it.
   - Event 1: sweep a few seconds of time offset and a range of f0, then take the max (absorbs measurement error).
   - Event 2: sweep f0 only. Its time is **locked** to event 1 through the flight geometry. This is the "combined locked scoring" step.
   - 200 m is too coarse (the true point can be up to 141 m off and score badly), so survivors are refined to a 100 m grid, roughly 30–40k points.
10. **Ranking**: plot event-1 score against event-2 score, group by flight pair, keep the max per pair. The winner scored 20.48 against a runner-up of ~15, a clear outlier. Then the winning flights get a finer re-grid and per-event and combined heatmaps in QGIS.
11. **Single-event mode with known time** (8:37 pm UTC, 1-minute clip):
    - Search all of CONUS with one flight under a tight time window.
    - The ridge was too faint for brightness scoring, so it was hand-traced and candidates were scored by distance-to-polyline.
    - This found the same location as the two-event search.
12. **Verification**: Street View tree-line match, which found the camera on a pole.

## Generalisable trade-offs
- At least one constraint must be strong: several events (relative timing) or a tight absolute time, or a small spatial prior (a city is ~0.01% of CONUS). A smaller area allows a wider time window.
- Busy areas give many events. Quiet areas let silence prune candidates.

## Robustness claims (counter-forensics)
- Fundamental frequencies overlap the human voice band, so a high-pass filter would remove wanted content.
- A naive high-pass followed by MP3 export, then a broadband boost, brings the curve back.
- Speech and other loud foreground sound do not mask the curve.
- The curve survives phone-call codecs and noise suppression.
- Cutting the audio removes the inter-event offset, but known-time single-event search still works.

## Implications for our build
- Hot loops:
  - CPA/delayed-CPA over (flight segments × grid points)
  - retarded-time solve and curve synthesis per candidate
  - spectrogram line-integral scoring over (offset × f0) sweeps
  - These go in Rust with rayon.
- Needs:
  - ADS-B Parquet ingest with a spatio-temporal index
  - aircraft type → engine lookup
  - DEM sampling (MSL heights; ADS-B baro vs geometric altitude matters)
  - ridge measurement/fitting tool (constant-velocity model)
  - silence model (audibility threshold vs distance/aircraft type)
  - QGIS outputs (GPKG heatmaps, per-event layers)
