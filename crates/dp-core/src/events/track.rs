//! Local (Smith–Waterman style) Viterbi ridge tracker on the log-f comb image.
//! V[t][u] = (C[t][u] − bias) + max(0, max_Δ V[t−1][u+Δ] − pen(Δ)); a path restarts wherever its score would go
//! negative, so it bridges short fades but ends where the tone does. Steps allow falling up to `dmax` bins per
//! frame (Doppler ridges only fall) and rising one bin at a cost (jitter, fringes).

const START: i8 = i8::MIN;

#[derive(Clone, Copy, Debug)]
pub(super) struct TrackerParams {
    pub dmax: usize,
    pub lam_down: f32,
    pub lam_steep: f32,
    pub lam_up: f32,
    pub bias: f32,
}

impl TrackerParams {
    /// One-bin falls are cheap (asymptotes drift slowly); steeper falls cost progressively more, which keeps
    /// noise paths from diving across octaves while a real CPA (a few steep frames) still pays off.
    fn step_cost(&self, d: usize) -> f32 {
        match d {
            0 => 0.0,
            1 => self.lam_down,
            _ => self.lam_down + self.lam_steep * (d - 1) as f32,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Path {
    pub start: usize,
    pub u: Vec<f32>,
}

impl Path {
    /// Cut the path where it cannot be one Doppler ridge. A high-scoring path happily pays a bridge to the next
    /// event or a hop onto a crossing line (it only restarts when its score would go negative), so cut where
    /// the comb stays weak for `gap_frames`, where the path climbs more than `max_rise` bins within
    /// `gap_frames`, or where it falls much faster than its own slope on either side (a hop, not a CPA:
    /// a real ridge steepens gradually).
    pub fn segments(
        &self,
        c: &[f32],
        n_f: usize,
        bias: f32,
        gap_frames: usize,
        max_rise: f32,
    ) -> Vec<(usize, Vec<f32>)> {
        let n = self.u.len();
        let w = gap_frames.max(2);
        let em: Vec<f32> = (0..n)
            .map(|i| c[(self.start + i) * n_f + self.u[i] as usize] - bias)
            .collect();
        let mut cut = vec![false; n];
        let mut acc: f32 = em[..w.min(n)].iter().sum();
        for i in 0..n.saturating_sub(w) {
            if acc < -0.5 * w as f32 || self.u[i + w] - self.u[i] > max_rise {
                cut[i..i + w].iter_mut().for_each(|x| *x = true);
            }
            acc += em[i + w] - em[i];
        }
        const SIDE: usize = 6;
        const HOP: usize = 3;
        let step = |i: usize| self.u[i - 1] - self.u[i];
        let mut scratch = Vec::new();
        for i in SIDE + 1..n.saturating_sub(HOP + SIDE) {
            let before: Vec<f32> = (i - SIDE..i).map(step).collect();
            let after: Vec<f32> = (i + HOP + 1..=i + HOP + SIDE).map(step).collect();
            let side = crate::audio::whiten::median(&before, &mut scratch)
                .max(crate::audio::whiten::median(&after, &mut scratch));
            if self.u[i - 1] - self.u[i + HOP - 1] > (3.0 * HOP as f32 * side).max(HOP as f32 + 2.0) {
                cut[i - 1..i + HOP].iter_mut().for_each(|x| *x = true);
            }
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < n {
            if cut[i] {
                i += 1;
                continue;
            }
            let s = i;
            while i < n && !cut[i] {
                i += 1;
            }
            out.push((self.start + s, self.u[s..i].to_vec()));
        }
        out
    }
}

pub(super) struct Tracker {
    n_t: usize,
    n_f: usize,
    p: TrackerParams,
    v: Vec<f32>,
    back: Vec<i8>,
    row_best: Vec<(f32, usize)>,
}

impl Tracker {
    pub fn new(n_t: usize, n_f: usize, p: TrackerParams) -> Self {
        assert!(p.dmax < i8::MAX as usize);
        Self {
            n_t,
            n_f,
            p,
            v: vec![0.0; n_t * n_f],
            back: vec![START; n_t * n_f],
            row_best: vec![(f32::MIN, 0); n_t],
        }
    }

    /// Recompute rows from `t0`. Rows after `dirty_end` depend only on their predecessor, so the sweep stops
    /// as soon as one of them comes out bit-identical.
    pub fn run_from(&mut self, c: &[f32], t0: usize, dirty_end: usize) {
        let (n_f, p) = (self.n_f, self.p);
        let mut row = vec![0f32; n_f];
        let mut brow = vec![START; n_f];
        for t in t0..self.n_t {
            let crow = &c[t * n_f..(t + 1) * n_f];
            for u in 0..n_f {
                let (mut best, mut bp) = (0.0f32, START);
                if t > 0 {
                    let prev = &self.v[(t - 1) * n_f..t * n_f];
                    if u > 0 && prev[u - 1] - p.lam_up > best {
                        (best, bp) = (prev[u - 1] - p.lam_up, -1);
                    }
                    for d in 0..=p.dmax.min(n_f - 1 - u) {
                        let cand = prev[u + d] - p.step_cost(d);
                        if cand > best {
                            (best, bp) = (cand, d as i8);
                        }
                    }
                }
                row[u] = crow[u] - p.bias + best;
                brow[u] = bp;
            }
            let dst = &mut self.v[t * n_f..(t + 1) * n_f];
            let same = dst == row.as_slice();
            dst.copy_from_slice(&row);
            self.back[t * n_f..(t + 1) * n_f].copy_from_slice(&brow);
            self.row_best[t] = row
                .iter()
                .enumerate()
                .fold((f32::MIN, 0), |a, (u, &x)| if x > a.0 { (x, u) } else { a });
            if same && t > dirty_end {
                break;
            }
        }
    }

    pub fn best(&self) -> Option<(usize, usize, f32)> {
        self.row_best
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.0.total_cmp(&b.1.0))
            .map(|(t, &(s, u))| (t, u, s))
    }

    pub fn backtrack(&self, mut t: usize, mut u: usize) -> Path {
        let mut us = vec![u as f32];
        loop {
            let b = self.back[t * self.n_f + u];
            if b == START || t == 0 {
                break;
            }
            u = (u as i64 + b as i64) as usize;
            t -= 1;
            us.push(u as f32);
        }
        us.reverse();
        Path { start: t, u: us }
    }
}
