//! Small robust statistics and the CV-fit seeding heuristic.

pub(super) fn median_abs(r: &[f64]) -> f64 {
    let mut a: Vec<f64> = r.iter().map(|x| x.abs()).collect();
    if a.is_empty() {
        return 0.0;
    }
    let mid = a.len() / 2;
    *a.select_nth_unstable_by(mid, f64::total_cmp).1
}

pub(super) fn quantile(x: &[f64], q: f64) -> f64 {
    let mut s = x.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * q).round() as usize]
}

pub(super) fn lag1_autocorr(r: &[f64]) -> f64 {
    let n = r.len();
    if n < 3 {
        return 0.0;
    }
    let m = r.iter().sum::<f64>() / n as f64;
    let var: f64 = r.iter().map(|x| (x - m) * (x - m)).sum();
    let cov: f64 = r.windows(2).map(|w| (w[0] - m) * (w[1] - m)).sum();
    if var > 0.0 { cov / var } else { 0.0 }
}

fn linear_slope(t: &[f64], f: &[f64]) -> f64 {
    let n = t.len() as f64;
    let (mt, mf) = (t.iter().sum::<f64>() / n, f.iter().sum::<f64>() / n);
    let num: f64 = t.iter().zip(f).map(|(a, b)| (a - mt) * (b - mf)).sum();
    let den: f64 = t.iter().map(|a| (a - mt) * (a - mt)).sum();
    if den > 0.0 { num / den } else { 0.0 }
}

/// (f0, v, d, t_arr) seeds: v and f0 from the asymptotes (f_hi/f_lo = (c+v)/(c−v)), d from the steepest local
/// slope (|ḟ|max ≈ f0·v²/(c·d), reached d/c before the CPA sound arrives), on a 3×3 grid around them.
pub(super) fn seeds(t: &[f64], f: &[f64], c: f64) -> Vec<[f64; 4]> {
    let (hi, lo) = (quantile(f, 0.95), quantile(f, 0.05));
    let ratio = (hi / lo).max(1.0 + 1e-4);
    let v0 = (c * (ratio - 1.0) / (ratio + 1.0)).clamp(2.0, 0.8 * c);
    let f00 = 2.0 * hi * lo / (hi + lo);
    let half = ((t[t.len() - 1] - t[0]) / 20.0).max(1.0);
    let (mut best_slope, mut t_s) = (0.0, 0.5 * (t[0] + t[t.len() - 1]));
    let (mut j0, mut j1) = (0, 0);
    for i in 0..t.len() {
        while t[j0] < t[i] - half {
            j0 += 1;
        }
        while j1 + 1 < t.len() && t[j1 + 1] <= t[i] + half {
            j1 += 1;
        }
        if j1 - j0 >= 3 {
            let slope = linear_slope(&t[j0..=j1], &f[j0..=j1]);
            if slope < best_slope {
                (best_slope, t_s) = (slope, t[i]);
            }
        }
    }
    let d0 = if best_slope < 0.0 {
        (f00 * v0 * v0 / (c * -best_slope)).clamp(20.0, 5e4)
    } else {
        1000.0
    };
    let mut out = Vec::with_capacity(9);
    for vm in [0.6, 1.0, 1.6] {
        for dm in [0.4, 1.0, 2.5] {
            let (v, d) = ((v0 * vm).min(0.9 * c), d0 * dm);
            out.push([f00, v, d, t_s + d / c]);
        }
    }
    out
}
