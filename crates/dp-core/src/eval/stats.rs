//! Small-sample statistics for eval summaries: percentile bootstrap CIs over cases.

use crate::sim::noise::Pcg;

const RESAMPLES: usize = 2000;

/// Point estimate with a 95% percentile-bootstrap interval.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Ci {
    pub est: f64,
    pub lo: f64,
    pub hi: f64,
    pub n: usize,
}

impl Ci {
    pub fn fmt(&self, prec: usize) -> String {
        if self.n == 0 {
            return "–".into();
        }
        format!("{:.prec$} [{:.prec$}, {:.prec$}]", self.est, self.lo, self.hi)
    }
}

pub fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    match n {
        0 => f64::NAN,
        _ if n % 2 == 1 => s[n / 2],
        _ => 0.5 * (s[n / 2 - 1] + s[n / 2]),
    }
}

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Bootstrap `stat` over cases; seeded, so a summary is reproducible.
pub fn bootstrap(v: &[f64], stat: fn(&[f64]) -> f64) -> Ci {
    let n = v.len();
    if n == 0 {
        return Ci { est: f64::NAN, lo: f64::NAN, hi: f64::NAN, n };
    }
    let mut rng = Pcg::new(0x0b00_7517);
    let mut buf = vec![0.0; n];
    let mut stats: Vec<f64> = (0..RESAMPLES)
        .map(|_| {
            for b in buf.iter_mut() {
                *b = v[(rng.next_u64() % n as u64) as usize];
            }
            stat(&buf)
        })
        .collect();
    stats.sort_by(f64::total_cmp);
    let q = |p: f64| stats[((p * (RESAMPLES - 1) as f64).round() as usize).min(RESAMPLES - 1)];
    Ci { est: stat(v), lo: q(0.025), hi: q(0.975), n }
}

/// Hit rate (0/1 values) with its bootstrap interval.
pub fn rate(hits: impl Iterator<Item = bool>) -> Ci {
    let v: Vec<f64> = hits.map(|b| b as u8 as f64).collect();
    bootstrap(&v, mean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_brackets_the_estimate_and_is_degenerate_for_constants() {
        let v: Vec<f64> = (0..30).map(|i| i as f64).collect();
        let ci = bootstrap(&v, mean);
        assert!(ci.lo < ci.est && ci.est < ci.hi && (ci.est - 14.5).abs() < 1e-12);
        let c = bootstrap(&[3.0; 5], median);
        assert_eq!((c.lo, c.est, c.hi), (3.0, 3.0, 3.0));
        assert_eq!(median(&[1.0, 4.0, 2.0, 3.0]), 2.5);
    }
}
