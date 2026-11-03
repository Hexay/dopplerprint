//! Model-adequacy spread of a CV fit: refit on the leading and trailing parts of the ridge from the full-fit
//! solution. A true straight constant-speed pass gives the same (v, d, t) on any sub-window; accelerating,
//! climbing or turning traffic does not, and that disagreement is the honest size of the fit's model error.

use super::cvlm::{Problem, Th};

/// Fraction of the ridge span each sub-window drops (from one end).
const DROP: f64 = 0.3;

/// Largest relative change of v and d, and absolute change of t_arrive, over the sub-window refits.
pub(super) fn window_spread(prob: &Problem, best: &Th) -> (f64, f64, f64) {
    let (t, f) = (prob.t, prob.f);
    let (t0, t1) = (t[0], t[t.len() - 1]);
    let cut = DROP * (t1 - t0);
    let parts = [(t0 + cut, f64::INFINITY), (f64::NEG_INFINITY, t1 - cut)];
    let mut out = (0.0f64, 0.0f64, 0.0f64);
    for (lo, hi) in parts {
        let (a, b) = (t.partition_point(|&x| x < lo), t.partition_point(|&x| x <= hi));
        if b < a + 8 {
            continue;
        }
        let sub = Problem { t: &t[a..b], f: &f[a..b], ..*prob };
        let th = sub.lm(*best);
        out.0 = out.0.max((th[1] / best[1] - 1.0).abs());
        out.1 = out.1.max((th[2] / best[2]).ln().abs());
        out.2 = out.2.max((th[3] - best[3]).abs());
    }
    out
}
