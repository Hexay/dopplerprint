# Archived weather: wind aloft and sound speed in the search, 2026-11-03

Built: `dp_core::weather` (Open-Meteo archive → `physics::Column`), per-emission layer-mean c and wind
vector (docs/PHYSICS.md §4a), `dopplerprint locate` default on (`--no-weather`, `--offline`), eval
`[eval] weather = "archive" | "truth"`, `nuisance_dc`. Numbers: paired Δ = variant − reference, median
[95% bootstrap CI]; better/worse = cases moving > 1 m. Devbox heavily loaded, so wall times are not
comparable between runs.

## Why: the ray-trace finding

Exact moving-medium ray trace, 460 simulated level passes (log-law, mixed-layer and low-level-jet
profiles), straight-ray uniform model given different winds. CPA-range bias, median / worst:

| class | still air | z-mean along-track only | **z-mean full vector** |
|---|---|---|---|
| jets | 113–133 / 434 m | crosswind leaves 85–139 m along-track | **5–6 / 38 m** |
| light aircraft | 15–20 / 144 m | crosswind leaves 36 m | **1–7 / 72 m** |

Weight: uniform in height over [receiver ground, aircraft], harmonic mean for c. Residual grows ~3× per
wind doubling (curved rays); the 10 m wind alone removes ~½ of a log-law bias and ~0 of an LLJ bias.
Shadow-zone fragments and nocturnal turning rays are not fixed by any wind model.

## Data source

| | Open-Meteo Historical Forecast API |
|---|---|
| Endpoint | `historical-forecast-api.open-meteo.com/v1/forecast`, `models=best_match` (NOAA HRRR 3 km over the US; GFS/ECMWF elsewhere) |
| Fields | hourly T, RH, wind speed/direction, geopotential height at 1000–300 hPa (12 levels); T + wind at 80/120/180 m; 2 m T/RH, 10 m wind, surface pressure |
| Archive | HRRR from 2018; most models from 2021–22; recent days fill in within ~1–2 days |
| Licence | CC BY 4.0, credit "Weather data by Open-Meteo.com" with link (README, report `Air:` line) |
| Terms | free API non-commercial only; 600/min, 5,000/h, 10,000/day, 300,000/month per IP; no key |
| Privacy | sent: lat/lon rounded to 0.01°, UTC date, variable list; server logs kept ≤ 90 days |
| Our use | 1 request per site × UTC day, cached in `<cache>/weather/` once the day is ≥ 2 days old |

Not chosen: Open-Meteo's ERA5 archive API (0.25°, ~5-day lag; HRRR is 3 km and assimilates more US
data); NOAA RAP/HRRR GRIB2 on AWS (needs a GRIB2 decoder plus byte-range index reads per field); ECMWF CDS
(account and API key).

Thermodynamics: c = 20.05·√(T·(1 + 0.51 q)) (sonic virtual temperature, q from RH and pressure). Height
levels missing RH take q interpolated in height. Pressure levels at or below the 10 m node are dropped.
Profiles are linear in time between hours. Levels are MSL (geopotential height); the column adds geoid N.

## Sim: does supplying the atmosphere help? (`experiments/sim_wind`, n = 24)

Rendered in a uniform medium, c 337 m/s and 12 m/s from 240°; 16 single + 8 two-aircraft cases, half with
low jets (1.2–3 km AGL, 110–180 m/s). Uniform sim cannot test layer averaging, only the value of the
right c and wind. Reference `simwind_still` = the old default (c 343, no wind).

| variant vs still air | stratum | n | median error m | Δ error m | better/worse | Δ log score (mean) |
|---|---|---|---|---|---|---|
| c right, no wind (`simwind_c_true`) | pairs | 8 | 65 → 25 | −44 [−121, −11] | 7/1 | +0.59 [+0.03, +1.41] |
| c + wind right (`simwind_truth`) | pairs | 8 | 65 → **10** | −64 [−152, −34] | **7/0** | +0.72 [+0.20, +1.50] |
| | jets (single + pair) | 12 | 354 → 253 | −79 [−350, +151] | 7/5 | +0.85 [+0.24, +1.57] |
| | GA (single + pair) | 12 | 170 → 87 | −17 [−74, +56] | 6/5 | +0.10 [−0.02, +0.22] |
| | singles | 16 | 348 → 337 | +56 [−329, +155] | 6/10 | +0.36 [+0.00, +0.83] |
| wind 3 m/s wrong, 90° off (`simwind_truth_err3`) | pairs | 8 | 65 → 1 | −64 [−121, −34] | 7/1 | +0.69 [+0.15, +1.48] |
| | singles | 16 | 348 → 502 | +92 [+6, +154] | 4/12 | +0.37 [+0.01, +0.83] |

Wind on top of the right c (`simwind_truth` vs `simwind_c_true`): pairs −20 m [−47, 0], 5/1; log score
+0.13 [+0.03, +0.25]; score gap −0.27. All 24 cases: score gap smaller in 24/24 vs still air.

Shared nuisance grid around a 3 m/s-wrong centre (`simwind_err3_grid`: c ±3 × wind ±3 E/N, pairs): error
0 [−27, +35] vs the fixed wrong centre, log score −0.11 [−0.16, −0.05]; picks c = 337 in 4/8.
**Negative: at ±3 m/s steps the grid does not recover the truth result and costs log score.** Keep the
weather value fixed; revisit a finer, search-internal nuisance (roadmap #4) later.

Single-pass point error is dominated by mirror/along-track flips (±0.3–2 km either way), so it moves
randomly with any atmosphere change, as in `roadmap_experiments_2026-11-03.md`. The continuous scores
improve in every stratum.

## Real: archived weather vs still air

| run | n | median error m | Δ error m | better/worse | ≤ 300 m | flights | Δ log score (mean) |
|---|---|---|---|---|---|---|---|
| Livermore dev (`livermore_weather` vs `livermore_baseline`) | 28 | 334 → 250 | 0 [−13, 0] | 12/8 | 14 → 15 | 23 → 23 | −0.01 [−0.05, +0.04] |
| Livermore **test, run once** (`livermore_test_weather` vs `_still`) | 12 | 186 → 180 | −10 [−33, +101] | 7/3 | 9 → 8 | 11 → 11 | +0.21 [+0.05, +0.44] |
| Oyster Bay (`oysterbay_weather` vs `oysterbay_dg_parity`) | 1 | 79 → 99 | +20 | 0/1 | 1 → 1 | 1 → 1 | −1.43 (truth cell rank 1 → 4) |

Livermore weather: HRRR 3–5 m/s, mostly westerly, c 339–344 m/s (afternoon, 15–25 °C). Oyster Bay: 2 m/s
from 317°, c 344.4. The Livermore passages span GA to jets (height above the recorder 0.26–5.7 km, median
1.25 km; 33–215 m/s, median 81 m/s), but on light-wind afternoons (3–5 m/s, c within ±4 m/s of 343) the
expected shift is tens of metres, below the corpus noise (±0.18 on hit@300 at n = 28) and below the
along-track/mirror flips that dominate single-pass error. Oyster Bay's parity settings were tuned at c 343 on that one case, so its −1.4 nats is weak evidence.

## Verdict

- **On by default.** The physics case is strong (vector z-mean removes > 90% of the wind bias), sim pairs
  improve 65 → 10 m with 7/0, continuous scores improve in every sim stratum, and real data is neutral to
  slightly positive (test log score +0.21). No real case has the regime where it matters most (strong
  wind aloft) yet.
- Fixed weather value, not a nuisance grid (grid negative above).
- Cost: one HTTP request per recording; the column lookup per Newton step was not visible above the
  load noise (Livermore weather runs had equal or lower wall time than still air).

## Not done / next

- Sim cannot render a layered atmosphere (`sim` is uniform; the ray-trace study proposes a stratified
  moving-medium eigenray port into `sim`). Until it exists, layer averaging is validated only by `tests/physics_column.rs`.
- Curved-ray weighting (2–5× smaller residual under LLJs and for GA beyond ~2 km), censoring at shadow
  edges, night-time σ_w inflation.
- Real recordings on windy days (or at night under a low-level jet), to measure the effect on real data.
