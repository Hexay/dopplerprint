//! Model-adequacy spread of a CV fit: refit on the leading and trailing parts of the ridge from the full-fit
//! solution. A true straight constant-speed pass gives the same (v, d, t) on any sub-window; accelerating,
//! climbing or turning traffic does not, and that disagreement is the honest size of the fit's model error.

use super::cvlm::{Problem, Th};

/// Fraction of the ridge span each sub-window drops (from one end).
const DROP: f64 = 0.3;

/// Largest change over the sub-window refits: relative v, |ln d|, |ln κ| (κ = v²/d) and absolute t_arrive.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Spread {
    pub v: f64,
    pub ln_d: f64,
    pub ln_kappa: f64,
    pub t: f64,
}

pub(super) fn window_spread(prob: &Problem, best: &Th) -> Spread {
    let (t, f) = (prob.t, prob.f);
    let (t0, t1) = (t[0], t[t.len() - 1]);
    let cut = DROP * (t1 - t0);
    let parts = [(t0 + cut, f64::INFINITY), (f64::NEG_INFINITY, t1 - cut)];
    let mut out = Spread::default();
    for (lo, hi) in parts {
        let (a, b) = (t.partition_point(|&x| x < lo), t.partition_point(|&x| x <= hi));
        if b < a + 8 {
            continue;
        }
        let sub = Problem { t: &t[a..b], f: &f[a..b], ..*prob };
        let th = sub.lm(*best);
        let (rv, rd) = (th[1] / best[1], th[2] / best[2]);
        out.v = out.v.max((rv - 1.0).abs());
        out.ln_d = out.ln_d.max(rd.ln().abs());
        out.ln_kappa = out.ln_kappa.max((rv * rv / rd).ln().abs());
        out.t = out.t.max((th[3] - best[3]).abs());
    }
    out
}
