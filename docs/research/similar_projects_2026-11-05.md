# Similar projects and papers: ideas ranked for our open problems (2026-11-05)

Scope: new sources since the DOPPLERGEIST, dopplertrack (1701302), echotrace and backlog reviews. No code changed.
Tags: **[V]** checked in the source (README, commit message, code or abstract); **[D]** derived here;
**[I]** inference, untested. Problems: **P1** single-pass along-track/clock ridge, **P2** nationwide chance
matches, **P3** faint/far passes.

## What breaks the along-track ridge, and what can't [D]

Moving the recorder Δs along a straight constant-velocity track and the clock by Δs/v leaves the recorder's
geometry relative to that aircraft unchanged: R(t), elevation angle, propagation delay and Doppler curve all
stay the same. So nothing that depends on that relative geometry alone can break the ridge: level vs range,
absorption tilt, propagation delay, harmonic amplitudes, or Lloyd's-mirror fringes on flat ground under level
flight. Only four kinds of input change along the ridge:
(a) **other flights** heard or not heard; (b) **absolute time** (ENF, metadata);
(c) **non-uniform tracks** (turns, speed changes, climbs), which change the curve's shape or the aircraft's
height above ground (AGL) at CPA; (d) **terrain and DEM height** varying along the ridge. Every P1 idea below
uses one of these.

## Does dopplertrack's clock handling solve P1? No [V + I]

- **[V]** `search-single` takes candidates with |t_heard − t_cpa_unix| ≤ tol and sweeps ±8 s × ±5 % f0. It has
  no clock prior. Its own experiments say so (`docs/experiments.md`, faint single flyover): "the clock pins
  the position along the track only to the time tolerance times the speed, 3 km at ±30 s".
- **[I]** Its 3.5 s median clock error on real Livermore cases therefore comes from the objective: brightness
  over the whole fit window, with uncovered parts scored zero, on the real trajectory. That weights the
  ridge wings, where pattern traffic turns or changes speed (input (c)). It's tie-breaking by margins of
  1–7 %, not a fix. Our bench doc's next experiment (its score as a `ScoreMode`) already tests it. Idea 6
  turns it into a measured property.
- **[V] New since 1701302** (0.3.0, 2026-11-04/05): a nationwide pair-first "lens" join; `--extra`
  re-scoring on one clock (idea 1); `verdict`, a null taken from the search's own wrong pairs (idea 2); a
  broadband-burst flag on events (idea 8); a ridge-residual score (no better than brightness); and a railcam
  negative result (ideas 3 and 7).

## Ranked ideas

| # | Idea | Source | P | Payoff | Cost | Fit (module) |
|---|---|---|---|---|---|---|
| 1 | **Targeted evidence for every audible ADS-B flight on the shared clock**: a positive power-LLR along each unassigned flight's predicted curve, plus *measured* silence (spectrogram), not just modelled P_d | dopplertrack `--extra` [V] ([74f4c63](https://github.com/mjshiggins/dopplertrack/commit/74f4c63)); our `silence.rs` is model-only [V] | P1, P2, P3 | **High** [I] | M | new `search/scene.rs` on `power_llr`, called at refine on top-mode cells only; replaces `silence.rs` term there |
| 2 | **Null from the search's own wrong candidates**: exponential upper tail on the scores of candidates that share no flight with the winner; p = exp(−gap/m) | dopplertrack `verdict` [V] ([65c2929](https://github.com/mjshiggins/dopplertrack/commit/65c2929), `docs/experiments.md` §Confidence) | P2 | Med–High | **S** | `search/global` result table → app verdict next to `decoy`; decoys stay as a second check |
| 3 | **χ² consistency veto** (Allen): split each predicted curve into p segments of equal expected LLR (time chunks × harmonics); down-weight scores whose power sits in a few segments | [Allen 2005, gr-qc/0405045](https://arxiv.org/abs/gr-qc/0405045) [V abstract]; railcam "a wrong pair would have to fit the comb" [V] | P2, wrong flights | Med–High [I] | M | post-hoc on the top-K candidates in `power_llr`/`score.rs` (non-sweep, so no CUDA change) |
| 4 | **ENF clock inside the ±120 s prior, with a reference rebuilt from public seismometers** | jdak907/[provenance](https://github.com/jdak907/provenance) `enf-rebuild` [V] | P1 | High where hum exists; coverage low | M | Python prep fetches the reference (FDSN); `audio::hum` → ENF trace; correlation as a clock prior in `app` |
| 5 | **Ground-dip / Lloyd's-mirror null track** as a measured elevation angle; breaks the ridge only via (c)/(d) | Lo, Perry & Ferguson, [IEEE TAES 38(1) 2002](https://doi.org/10.1109/7.993235); Lo et al. [TAES 2003](https://doi.org/10.1109/TAES.2003.1188908); Jung et al. [2015](https://www.koreascience.or.kr/article/JAKO201534165173876.page) [V titles/approach] | P1 (climb/terrain cases) | Low–Med [D] | M–L | `events/` null-track extractor; per-mode term in `search/`; `sim` already renders the mirror |
| 6 | **Along-track identifiability per mode**: predicted score drop along the ridge from the ADS-B track alone (noise-free self-match); report it and stratify the calibration by it | [I], motivated by dopplertrack's tie-breaks; extends backlog #6 | P1 (honesty) | Med | **S** | `search/modes.rs`, `calibration.rs`; uses existing Doppler offsets |
| 7 | **Smooth tracks before radial velocity** (or use reported GS/track): a position glitch puts a kink in the modelled curve and the truth loses | dopplertrack railcam [V] ([2d899bb](https://github.com/mjshiggins/dopplertrack/commit/2d899bb)) | P2, P1 | Med | M | = backlog #12 (`adsb/`, `track/`); new evidence that it matters |
| 8 | **Broadband-burst flag** on events (vehicles: whole band loud plus a falling engine tone) | dopplertrack [V] ([7d9ae3e](https://github.com/mjshiggins/dopplertrack/commit/7d9ae3e)) | P2 | Low–Med | S | `events/passlike.rs` feature |
| 9 | **Gates from modelled curves, not the CV fit, on faint passes**: on a non-straight faint pass the CV fit said 41 m/s at 750 m vs the truth of 70 m/s at 8.2 km, and the gates dropped the truth | dopplertrack faint single flyover [V] | P3 | Med | S (subsumed by 1) | association gates (DISC); idea 1 needs no gate |
| 10 | **Second-order continuity DP** (penalise slope *change*) over overlapping frequency bands, so strong high-band clutter can't take every path slot | codyps/[echolocate](https://github.com/codyps/echolocate) `docs/ALGORITHM_IMPROVEMENT.md` [V] | P3 | Low–Med | S–M | `events/track.rs` (now first-order Smith–Waterman) |
| 11 | **Same-site multi-clip fusion**, one clock per clip | echolocate `fuse-site` [V] (5 clips: 2.47 → 1.43 km, still rank 4) | P1 (users with several clips) | Low–Med | S–M | `app/` |
| 12 | **Line-aware statistic**: p_sig/(p_line + p_noise), so stationary tones can't feed curve scores | SOAP, [Bayley et al. 2019](https://arxiv.org/abs/1903.12614) [V summary] | P2, P3 | Low | S | `power_llr` per-bin prior from the `audio::hum`/stationary-line mask |

New test data [V]: dopplertrack eagle-cam and railcam streams (the railcam supplies searches that contain no
right answer, so it measures false "located"); echolocate's DLR measured-microphone recordings
(`corpus/ground-truth`); AeroSonicDB ([arXiv 2311.06368](https://arxiv.org/abs/2311.06368); coordinates
deliberately shifted, so recall/false-event tests only).
**Gotcha [V]:** echolocate rebuilt one common recorder position for the Pathak/Livermore clips (Zenodo
23424278), residuals < 3 m. The Livermore corpus is one site. Never fuse or share priors across Livermore cases
in eval.

## Notes on the top 5

**1. Targeted all-flight evidence.** For each unassigned flight j whose predicted curve falls in the recording
from cell x at clock δ, score a mixture:
`ℓ_j(x,δ) = log[(1 − P_d,j) + P_d,j · exp(LLR_j(x,δ))]`, where `LLR_j = max over f0 ∈ class band of the
power-LLR along j's predicted harmonic comb`. P_d,j is the existing `silence.rs` range model (cap 0.9).
- A clean band where j should be loud gives LLR_j ≪ 0, so the term goes to log(1 − P_d): measured silence.
- A faint ridge on j's curve gives LLR_j > 0, so the term earns positive credit.
- Capping at log(1 − P_d) keeps a single noisy flight from vetoing anything.

**[D]** Along the ridge, (x + Δs, δ + Δs/v) leaves the matched flight's curve unchanged. For flight j with
heading ψ_j ≠ ψ, both its CPA time and its curve shape move, so every audible flight adds a constraint across
the ridge. That is input (a).
**[V]** The same mechanism in dopplertrack (detected extras, not targeted ones): Oyster Bay margin 6.2 → 24 %;
the eagle cam's true pair moved from rank 15 to rank 1; its own Oyster Bay hour has three faint flyovers, each
matched by the curve its ADS-B track predicts.
**Cost:** an f0 sweep per extra flight, run only on refine cells (hundreds × ~10 flights). Jets are kept,
flagged and given a class prior.
**Risk [I]:** a flight with a bad ADS-B track gets charged as silent. Gate on NIC and coverage, as `silence.rs`
already does for track ends.

**2. Null from wrong candidates [V].** dopplertrack, `dopplertrack.verdict`:
1. Drop pairs that share a flight with the winner.
2. Take the top k = 5 % of the rest (15 ≤ k ≤ 100) and fit the mean excess m above the k-th score.
3. The spacing between the two largest values of an exponential tail is itself exponential with mean m, so
   p = exp(−gap/m), where gap = winner − best other.
4. Verdict: located if p < 0.1, ambiguous if p < 0.5, otherwise no match.

Results: Oyster Bay nationwide, two events, p = 0.23 (right answer, still ambiguous); with the faint extra,
p = 0.0001; the box moved 150 km gives p = 0.88. Railcam: 2 of 20 no-answer searches were called located.
- **For us [I]:** the nationwide stage already holds thousands of region maxima, so this costs no new
  search. It complements decoys, which cost 50–60 s each and reject a chance winner only about ⅔ of the time.
- **Caveat [I]:** our scores are summed over events and are not i.i.d. across neighbouring cells, so take one
  maximum per flight-set, as they do per pair.

**3. χ² veto.** Allen's χ² (PRD 71 062001). Split a template into p bins of equal expected signal power. For a
true signal in Gaussian noise each bin carries 1/p of the total, so
`χ² = p · Σ_i (s_i − S/p)²` (normalised by the noise variance) ~ χ² with 2p − 2 degrees of freedom; glitches
and partial overlaps inflate it.
- Formula: **[I]**, from memory of the paper; only the abstract was re-read. LIGO's re-weighted statistic,
  ρ̂ = ρ/[(1 + (χ²_r)³)/2]^{1/6} for χ²_r > 1, is likewise from memory and needs checking.
- **For us [I]:** bins = (time thirds or quarters) × (harmonics with nonzero prior), weighted by the
  per-pixel SNR prior that `power_llr` already uses. Chance matches nationwide (z ≈ 100 vs truth 97.5,
  nationwide doc) and duplicate-passage hires (bench Cause 1) should take most of their score from a short
  overlap with an unrelated ridge.
- dopplertrack's "uncovered parts count zero" and echolocate's contiguous-support alternatives [V] are crude
  versions of the same check.

**4. ENF clock in a narrow window.**
- **[V] provenance:** the US-West reference rebuilt from ~16 FDSN seismometers near power lines matches the
  WECC 1 s log at r = 0.990–0.996, rmse 1.7–2.2 mHz, timing ~1 s. Stations are kept per 600 s window only if
  they agree with the median of the others, then combined by weighted median. East and Texas are
  experimental. On ENF-WHU, 65 % of real clips land within 10 s, but with only the first 2 minutes just
  35/130 are right.
- **[I]** That short-clip failure is a false peak over a 24 h search. Inside our ±120 s prior there are
  ~240 lags, so a 1–3 min clip should hold up much better.
- **[D]** A 1 s clock gives ~60 m along track at 60 m/s. That fixes P1 outright where hum is present.
- **[V] Our hum doc:** hum in only 2 of 41 real outdoor files (Oyster Bay 360 Hz, East; one Livermore clip),
  so corpus gains are small. Phone and video clips recorded near buildings are the target. This resolves
  backlog #14's "reference not held" for WECC.

**5. Ground-dip null track.**
- **[V]** Lo/Perry/Ferguson model the direct + ground-reflected interference in the time-frequency output of
  a microphone near the ground during a transit. They give a CRLB and estimate flight parameters by NLS and
  by a generalized Hough transform, on real jet data. A 2015 follow-up found the cepstrogram more accurate
  than the spectrogram.
- **[D]** Rigid ground, far field: path difference Δr ≈ 2·h_r·h_AGL/R(t), and nulls fall at
  `f_n(t) = (2n+1)·c·R(t) / (4·h_r·h_AGL)`. R(t) is known from the Doppler fit, so the null track measures one
  number, h_r·h_AGL. Example: h_r = 1.5 m, h_AGL = 500 m, R = 1 km puts the first null at 114 Hz, sweeping
  through the blade-pass harmonics.
- **[I]** With an h_r prior of 1.0–1.7 m, h_AGL is known to ~±25 %. That pins the clock only for climbing or
  descending traffic: ~3.5 m/s gives σ_t ≈ 30 s at 450 m AGL, against ±120 s now. Grass impedance shifts the
  nulls. Low payoff unless pattern traffic dominates. Today `power_llr` treats fringes as a nuisance (its
  mixture); this would turn them into a measurement.

## Not new / already covered (no action)

- ENF as a time anchor, sirens, trains, thunder, dual-mic phones (localization_deeper_factors).
- Search-theory negative evidence; Koch, ["negative sensor evidence"](https://doi.org/10.1016/j.inffus.2005.09.002) (2005).
- Viterbi ridge tracking (`events/track.rs`), drift β (`search/drift.rs`), fragment stitching.
- DOPPLERGEIST since 994d1ee: "Improve Doppler detection" (#410/#411) is now the default; no description, unread.
- echotrace since review: UI and airport coverage data only.
- New repos, skimmed: dyneresearch/DopplerGPS (README is a DG-style two-pass/silence pipeline, synthetic
  demo only); imperator-clawdius (stub); NORSAR DAS helicopter Doppler vs ADS-B
  ([EGU GC14 abstract](https://meetingorganizer.copernicus.org/GC14-FibreOptic/GC14-FibreOptic-42.html)),
  an abstract with nothing to take.
- Ferguson 1992 JASA ([10.1121/1.403934](https://doi.org/10.1121/1.403934)): constant-altitude CV model,
  the ancestor of `events/fit`. Full text is closed.

## Suggested order

2 (S, P2, no new search) → 6 (S, tells you which cases 1/4/5 can help) → 1 (M, the main P1 lever) →
3 (M) → 4 when a hum-bearing user clip appears. Measure each with an off-centre start prior
(clock_along_track §Not a fix), on discovered events.
