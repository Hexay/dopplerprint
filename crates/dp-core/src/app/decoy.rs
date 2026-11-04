//! Look-elsewhere (decoy) check: the same recording, area and start window searched against ADS-B from times
//! the recording was not made. A real answer must out-score every decoy. Validation:
//! docs/research/decoy_check_2026-11-04.md.

use anyhow::Result;

use super::cache::Cache;
use super::fetch::dates_between;
use super::when::StartPrior;

/// Minimum decoy shift (s); the shift also clears the whole start window plus the recording.
const MIN_SHIFT_S: f64 = 900.0;
const EXTRA_SHIFT_S: f64 = 600.0;

#[derive(Clone, Debug, serde::Serialize)]
pub struct DecoyCheck {
    /// Start shift of each decoy searched (s) and its best score (`None`: no flight matched).
    pub shifts_s: Vec<f64>,
    pub scores: Vec<Option<f64>>,
    /// Best real score minus the best decoy score; `None` when no decoy matched anything.
    pub margin: Option<f64>,
}

impl DecoyCheck {
    /// False when some decoy, which cannot be the recording, scores at least as well as the answer.
    pub fn passed(&self) -> bool {
        self.margin.is_none_or(|m| m > 0.0)
    }
}

/// Shifts (−s, +s) whose ADS-B days are already cached; decoys never trigger a download.
pub fn shifts(prior: &StartPrior, duration_s: f64, window: (f64, f64), cache: &Cache) -> Vec<f64> {
    let s = ((prior.hi - prior.lo) + duration_s + EXTRA_SHIFT_S).max(MIN_SHIFT_S);
    [-s, s].into_iter().filter(|&d| dates_between(window.0 + d, window.1 + d).iter().all(|day| cache.has_day(day))).collect()
}

/// Runs `search(shift)` (best score of the shifted search, if any) for every shift and compares with `best`.
pub fn check(best: f64, shifts: &[f64], search: &mut dyn FnMut(f64) -> Result<Option<f64>>) -> Result<Option<DecoyCheck>> {
    if shifts.is_empty() {
        return Ok(None);
    }
    let scores = shifts.iter().map(|&d| search(d)).collect::<Result<Vec<_>>>()?;
    let top = scores.iter().flatten().copied().fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))));
    Ok(Some(DecoyCheck { shifts_s: shifts.to_vec(), scores, margin: top.map(|t| best - t) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_is_best_minus_top_decoy() {
        let mut f = |d: f64| Ok(if d < 0.0 { Some(40.0) } else { None });
        let c = check(50.0, &[-900.0, 900.0], &mut f).unwrap().unwrap();
        assert_eq!(c.margin, Some(10.0));
        assert!(c.passed());
    }

    #[test]
    fn a_decoy_that_ties_fails() {
        let mut f = |_: f64| Ok(Some(50.0));
        assert!(!check(50.0, &[-900.0], &mut f).unwrap().unwrap().passed());
    }

    #[test]
    fn no_decoy_match_passes() {
        let mut f = |_: f64| Ok(None);
        let c = check(50.0, &[-900.0, 900.0], &mut f).unwrap().unwrap();
        assert_eq!(c.margin, None);
        assert!(c.passed());
    }
}
