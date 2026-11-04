# Decoy (look-elsewhere) check, 2026-11-04

Code: `app/decoy.rs`, called from `app/run.rs`. User docs: docs/USAGE.md#decoy.

## Problem

With realistic inputs (start typed to the minute ±2 min, `--near` 10 km), `locate` reported confident wrong
answers. On the end-to-end benchmark (docs/research/bench_vs_dopplergeist_2026-11-03.md), 5 of 11 real cases
named the wrong flight, all with status `found`. Some were rated confidence "high" (N60RS: 9.8 km off, no
distinct alternative, so `report::alternatives` defaulted to "high").

Diagnosis (eval harness, the same 10 Livermore cases, discovered events):

| start prior, radius | right flight | median error |
|---|---|---|
| ±5 s, 4 km | 8/10 | 0.74 km |
| ±120 s, 4 km | 6/10 | 1.9 km |
| ±120 s, 10 km | 5/10 | 5.6 km |

Most real recordings have one extra event: another aircraft, or a ridge that is not a pass. In a narrow window
and a small area no logged flight fits it, so `search_dropping_weakest` drops it. In 314 km² over ±2 min, some
pair of unrelated flights somewhere fits every event better than the truth fits the real ones. The evidence
event chooser (feat/evidence-event-set) fixes this at 4 km (6 → 8/10) and does nothing at 10 km. Configs:
tools/bench_vs_dg/diag/.

The calibrated 90% region cannot flag these cases. Its q̂ saturates (hpd_mass ≈ 1), so the region is the
whole search circle for the 5–10 m synthetic answers too.

## Check

The same recording, events, area, atmosphere and window width are searched against ADS-B shifted by ±s, where
s = max(15 min, window + duration + 10 min). No flight in a decoy can be the recorded one. The best score
a decoy reaches is the score chance reaches for this exact recording and search volume. If
`best − max(decoy) ≤ 0`, the status becomes inconclusive (the location is still written, as a guess).
Decoys use only cached ADS-B days and never download.

## Validation (benchmark protocol, 16 cases; runs/bench-vs-dg/var-decoy vs var-base)

| | before | after |
|---|---|---|
| `found` with the wrong flight | 5 | **0** |
| `found`, real cases (errors) | 11 (median 5.6 km) | 5: 218 m, 923 m, 1.6 km, 5.6 km, 7.4 km, all the right flight |
| synthetic cases `found` | 5/5 (5–10 m) | 5/5 (no decoy matched anything) |
| total wall time | 373 s | 975 s (2.6×) |

Margins: wrong-flight cases −0.1 to −36; correct real cases +2.8 to +13.7; N86LP (right flight, 5.3 km
off) −1.8, so inconclusive. N456CS (right flight, 5.6 km off) passes at +0.2. A threshold above 0 would drop it
and also N413AK (+2.8, 218 m), so the threshold stays at 0.

The base build changed between runs (c1777da → main with likelihood silence by default), which moved
N788BM from 454 m to 923 m. The decoy check does not change any location; it only changes status.

## Limits

- 11 real cases and 2 decoys each. With more decoys the maximum rises and the check gets stricter.
- Single-pass along-track ambiguity (the right flight, km off; N248LF, N456CS) is physics, not chance, and
  passes the check. Its region needs to be honest instead (calibration q̂ saturation is open).
- Cost is 2–3× search time. Shorter windows and smaller radii make both the search and the decoys cheaper.
