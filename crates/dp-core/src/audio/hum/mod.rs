//! Mains-hum (ENF) detector: presence and grid, ENF trace, sample-rate check, dropouts/splices and hum
//! bands for masking. Needs no external data. See docs/AUDIO.md, "Mains hum".

mod grid;
mod rate;
mod report;
mod spectrum;
mod splice;
mod trace;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use super::Audio;
use super::resample::decimate_for;

pub use report::{
    Discontinuity, DiscontinuityKind, HarmonicLine, HumBand, HumFrame, HumReport, RateCheck, RateStatus,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HumParams {
    /// Highest harmonic frequency analysed (Hz); audio is decimated to about 2.5× this first.
    pub f_max: f64,
    pub max_harmonics: usize,
    /// ENF trace frame length and hop (s).
    pub frame_s: f64,
    pub hop_s: f64,
    /// Welch segment of the long-term spectrum used for detection (s).
    pub welch_s: f64,
    /// Long-term line-to-floor ratio (dB) for a harmonic to count.
    pub min_line_snr_db: f64,
    /// Per-frame line SNR (dB) for a trace frame to be valid.
    pub frame_snr_db: f64,
    /// Fraction of valid frames required for `present`.
    pub min_detect_frac: f64,
    /// Largest trace std (Hz at the fundamental) still mains-like.
    pub max_wander_hz: f64,
    /// Per-frame search half-width per harmonic order (Hz at the fundamental).
    pub search_hz: f64,
    /// Assumed 1σ of the grid's mean frequency over one recording (Hz at nominal).
    pub grid_sigma_hz: f64,
    /// Relative rate deviation tolerated before flagging (device clock + grid).
    pub rate_tol: f64,
    /// Trend over the file (Hz at the fundamental) beyond which drift is flagged.
    pub drift_tol_hz: f64,
    /// Phase-jump threshold in robust σ of the jump statistic, and its floor in fundamental cycles.
    pub jump_sigma: f64,
    pub min_jump_cycles: f64,
    /// Container rate when `audio` was decimated before the call; defaults to `audio.sample_rate`.
    pub labelled_sr: Option<u32>,
}

impl Default for HumParams {
    fn default() -> Self {
        Self {
            f_max: 1000.0,
            max_harmonics: 12,
            frame_s: 2.0,
            hop_s: 1.0,
            welch_s: 4.0,
            min_line_snr_db: 6.0,
            frame_snr_db: 10.0,
            min_detect_frac: 0.3,
            max_wander_hz: 0.1,
            search_hz: 0.12,
            grid_sigma_hz: 0.02,
            rate_tol: 0.002,
            drift_tol_hz: 0.1,
            jump_sigma: 8.0,
            min_jump_cycles: 0.05,
            labelled_sr: None,
        }
    }
}

impl HumParams {
    /// Flat JSON object; missing fields take defaults.
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

pub fn analyse_hum(audio: &Audio, p: &HumParams) -> HumReport {
    let labelled_sr = p.labelled_sr.unwrap_or(audio.sample_rate);
    let mut rep = HumReport { rate: RateCheck { labelled_sr, ..Default::default() }, ..Default::default() };
    let a = decimate_for(audio, p.f_max);
    let sr = a.sample_rate as f64;
    let f_hi = p.f_max.min(0.45 * sr);
    let Some(lt) = spectrum::welch(&a.samples, sr, p.welch_s) else {
        rep.notes.push("shorter than one Welch segment".into());
        return rep;
    };
    let found = grid::search(&lt, p, f_hi);
    let Some(h) = found.best else {
        rep.notes.push("no line near any 50/60 Hz harmonic (any common sample-rate ratio)".into());
        return rep;
    };
    let ks: Vec<u32> = h.lines.iter().map(|l| l.k).collect();
    let mut tr = trace::track(&a.samples, sr, h.f_hz, &ks, p);
    let mut fp = p.clone();
    if valid_frac(&tr.frames) < p.min_detect_frac {
        // Weak hum: 4× longer frames buy 6 dB of per-frame SNR at the cost of time resolution.
        let long = HumParams { frame_s: LONG_FRAME * p.frame_s, hop_s: LONG_FRAME * p.hop_s, ..p.clone() };
        let t2 = trace::track(&a.samples, sr, h.f_hz, &ks, &long);
        if valid_frac(&t2.frames) > valid_frac(&tr.frames) {
            (tr, fp) = (t2, long);
        }
    }
    let p = &fp;
    rep.frame_s = p.frame_s;
    let n_fr = tr.frames.len().max(1) as f64;
    rep.ambiguous = found.ambiguous;
    rep.harmonics = h
        .lines
        .iter()
        .zip(&tr.k_valid)
        .map(|(l, &c)| HarmonicLine { k: l.k, f_hz: l.f_hz, snr_db: l.snr_db, detect_frac: c as f64 / n_fr })
        .collect();
    rep.snr_db = h.lines.iter().map(|l| l.snr_db).fold(f64::NEG_INFINITY, f64::max);
    rep.detect_frac = valid_frac(&tr.frames);
    let stats = rate::trace_stats(&tr.frames, p);
    rep.fundamental_hz = stats.as_ref().map_or(h.f_hz, |s| s.mean_hz);
    rep.wander_hz = stats.as_ref().map_or(0.0, |s| s.wander_hz);
    rep.trace = tr.frames;
    rep.present = decide(&mut rep.notes, &h, rep.detect_frac, rep.wander_hz, stats.is_some(), p);
    if !rep.present {
        return rep;
    }
    let st = stats.expect("present implies stats");
    rep.nominal_hz = Some(h.nominal);
    rep.rate = rate::check(&st, h.nominal, labelled_sr, p);
    let k_strong = h.lines.iter().max_by(|a, b| a.snr_db.total_cmp(&b.snr_db)).map_or(1, |l| l.k);
    rep.discontinuities = splice::find(&a.samples, sr, rep.fundamental_hz, k_strong, &rep.trace, p);
    rep.bands = report::bands(rep.fundamental_hz, rep.wander_hz, p.f_max);
    if h.ratio != 1.0 && h.lines.len() == 1 {
        rep.ambiguous = true;
        rep.notes.push("off-nominal rate rests on a single line".into());
    } else if rep.ambiguous {
        rep.notes.push("another grid/rate hypothesis fits almost as well; treat nominal as uncertain".into());
    }
    rep
}

const LONG_FRAME: f64 = 4.0;

fn valid_frac(frames: &[HumFrame]) -> f64 {
    frames.iter().filter(|f| f.valid).count() as f64 / frames.len().max(1) as f64
}

fn decide(notes: &mut Vec<String>, h: &grid::Hypothesis, frac: f64, wander: f64, stats: bool, p: &HumParams) -> bool {
    let mut ok = true;
    let mut fail = |c: bool, why: String| {
        if c {
            notes.push(why);
            ok = false;
        }
    };
    fail(!stats, "fewer than 3 valid trace frames".into());
    fail(frac < p.min_detect_frac, format!("line detectable in {:.0}% of frames", 100.0 * frac));
    fail(wander > p.max_wander_hz, format!("trace wanders {wander:.3} Hz: not grid-stable"));
    // A single stationary tone lands in some (nominal, ratio) window by chance. One line may carry a rate
    // claim only if it is the fundamental and grid-like throughout; the caller then marks it ambiguous.
    fail(
        h.ratio != 1.0 && h.lines.len() < 2 && !single_fundamental_ok(h, frac, wander),
        format!("one line at {:.2} Hz fits only an off-nominal rate", h.lines[0].f_hz),
    );
    ok
}

fn single_fundamental_ok(h: &grid::Hypothesis, frac: f64, wander: f64) -> bool {
    h.lines.len() == 1 && h.lines[0].k == 1 && frac >= 0.8 && wander <= 0.05
}
