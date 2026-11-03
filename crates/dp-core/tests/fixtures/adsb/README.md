# ADS-B test fixtures

- `traces/xx/trace_full_*.json`: eight unmodified readsb traces (gzip despite `.json`) from the adsb.lol
  `globe_history_2026` release `v2026.09.08-planes-readsb-prod-0`.
  Data © adsb.lol contributors, licensed under the Open Database License (ODbL) 1.0,
  https://opendatacommons.org/licenses/odbl/1-0/ — source https://github.com/adsblol/globe_history_2026.
  Any database derived from these fixtures must keep this attribution and stay under ODbL.
- `typedb/`: a few rows from tar1090-db (https://github.com/wiedehopf/tar1090-db): `icao_aircraft_types.json`
  (ICAO Doc 8643 descriptions) and `aircraft.csv` (registry; the `ABCDEF` row is invented for tests).
