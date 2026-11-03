//! Split-conformal levels from HPD-split scores (Izbicki, Shimizu & Stern 2022) and the small-n
//! intervals that go with them: Clopper–Pearson for observed coverage, and the Beta law of the coverage a
//! given calibration set delivers (Angelopoulos & Bates 2021, §3.2).

/// q̂ = the ⌈(n+1)·level⌉-th smallest score; `None` when n is too small for that level.
pub fn q_hat(scores: &[f64], level: f64) -> Option<f64> {
    let n = scores.len();
    let k = ((n + 1) as f64 * level - 1e-9).ceil() as usize;
    if k == 0 || k > n {
        return None;
    }
    let mut s = scores.to_vec();
    s.sort_by(f64::total_cmp);
    Some(s[k - 1])
}

/// P(Bin(n, p) ≥ k).
fn binom_tail(k: usize, n: usize, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let (lp, lq) = (p.ln(), (1.0 - p).ln());
    let mut ln_choose = 0.0;
    let mut sum = 0.0;
    for j in 0..=n {
        if j > 0 {
            ln_choose += ((n - j + 1) as f64).ln() - (j as f64).ln();
        }
        if j >= k {
            sum += (ln_choose + j as f64 * lp + (n - j) as f64 * lq).exp();
        }
    }
    sum.min(1.0)
}

/// Smallest p with f(p) ≥ target for f increasing on [0, 1].
fn invert(f: impl Fn(f64) -> f64, target: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if f(mid) >= target { hi = mid } else { lo = mid }
    }
    hi
}

/// 95% Clopper–Pearson interval for k successes in n.
pub fn clopper_pearson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let lo = if k == 0 { 0.0 } else { invert(|p| binom_tail(k, n, p), 0.025) };
    let hi = if k == n { 1.0 } else { invert(|p| binom_tail(k + 1, n, p), 0.975) };
    (lo, hi)
}

/// 5% quantile of the coverage delivered by q̂ from n calibration scores at `level`: coverage ~ Beta(k, n+1−k).
pub fn coverage_floor(n: usize, level: f64) -> Option<f64> {
    let k = ((n + 1) as f64 * level - 1e-9).ceil() as usize;
    if k == 0 || k > n {
        return None;
    }
    // Beta(a, b) CDF at x = P(Bin(a + b − 1, x) ≥ a).
    Some(invert(|x| binom_tail(k, n, x), 0.05))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_hat_rank_and_too_small() {
        let s: Vec<f64> = (1..=9).map(|i| i as f64 / 10.0).collect();
        assert_eq!(q_hat(&s, 0.5), Some(0.5));
        assert_eq!(q_hat(&s, 0.9), Some(0.9));
        assert_eq!(q_hat(&s[..5], 0.9), None);
    }

    #[test]
    fn clopper_pearson_known_values() {
        let (lo, hi) = clopper_pearson(5, 10);
        assert!((lo - 0.1871).abs() < 1e-3 && (hi - 0.8129).abs() < 1e-3);
        let (lo, hi) = clopper_pearson(10, 10);
        assert!((lo - 0.6915).abs() < 1e-3 && hi == 1.0);
    }

    #[test]
    fn coverage_floor_grows_with_n() {
        let a = coverage_floor(20, 0.9).unwrap();
        let b = coverage_floor(200, 0.9).unwrap();
        assert!(a < b && b < 0.9 && a > 0.7);
    }
}
