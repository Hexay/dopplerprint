//! Comparator only (not product code): Candan-style three-sample bias-corrected estimator on the complex
//! Hann DFT, δ̂ = c·Re{(R[k+1] − R[k−1]) / (2R[k] − R[k−1] − R[k+1])}, iterated about the previous estimate.
//! Phase is referenced to the window centre so the Hann main lobe is real; `c` is fitted on clean tones
//! (Candan 2015 fits c_N per window the same way).

use std::f64::consts::TAU;

pub(super) struct Candan {
    sr: f64,
    win: Vec<f64>,
    c: f64,
    iters: usize,
}

type Cx = (f64, f64);

impl Candan {
    pub fn new(sr: u32, win_s: f64, iters: usize) -> Self {
        let n = (win_s * sr as f64).round() as usize;
        let win = (0..n).map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()).collect();
        let mut s = Self { sr: sr as f64, win, c: 1.0, iters };
        s.c = s.calibrate();
        s
    }

    fn n(&self) -> usize {
        self.win.len()
    }

    fn df(&self) -> f64 {
        self.sr / self.n() as f64
    }

    /// Windowed DTFT at `f` of the frame centred on sample `centre`, phase referenced to the centre.
    fn dtft(&self, x: &[f32], centre: i64, f: f64) -> Cx {
        let n = self.n();
        let start = centre - (n / 2) as i64;
        let mid = (n as f64 - 1.0) / 2.0;
        let (step_c, step_s) = ((TAU * f / self.sr).cos(), -(TAU * f / self.sr).sin());
        let a0 = -TAU * f * (-mid) / self.sr;
        let (mut zc, mut zs) = (a0.cos(), a0.sin());
        let (mut re, mut im) = (0.0, 0.0);
        for (i, w) in self.win.iter().enumerate() {
            let j = start + i as i64;
            if j >= 0 && (j as usize) < x.len() {
                let v = w * x[j as usize] as f64;
                re += v * zc;
                im += v * zs;
            }
            (zc, zs) = (zc * step_c - zs * step_s, zc * step_s + zs * step_c);
        }
        (re, im)
    }

    fn raw(&self, x: &[f32], centre: i64, fc: f64) -> f64 {
        let df = self.df();
        let (m, z, p) = (self.dtft(x, centre, fc - df), self.dtft(x, centre, fc), self.dtft(x, centre, fc + df));
        let num = (p.0 - m.0, p.1 - m.1);
        let den = (2.0 * z.0 - m.0 - p.0, 2.0 * z.1 - m.1 - p.1);
        (num.0 * den.0 + num.1 * den.1) / (den.0 * den.0 + den.1 * den.1).max(1e-300)
    }

    fn calibrate(&self) -> f64 {
        let n = self.n();
        let k = (100.0 / self.df()).round();
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for i in 0..=18 {
            let delta = -0.45 + 0.05 * i as f64;
            let f = (k + delta) * self.df();
            let x: Vec<f32> = (0..n).map(|j| (TAU * f * j as f64 / self.sr).sin() as f32).collect();
            let r = self.raw(&x, (n / 2) as i64, k * self.df());
            sxy += delta * r;
            sxx += r * r;
        }
        sxy / sxx
    }

    pub fn c(&self) -> f64 {
        self.c
    }

    /// Frequency near `f_seed` in the frame centred at time `t` (s).
    pub fn estimate(&self, x: &[f32], t: f64, f_seed: f64) -> f64 {
        let centre = (t * self.sr).round() as i64;
        let df = self.df();
        let k0 = (f_seed / df).round();
        let mag = |k: f64| {
            let z = self.dtft(x, centre, k * df);
            z.0 * z.0 + z.1 * z.1
        };
        let k = (-3..=3).map(|i| k0 + i as f64).max_by(|a, b| mag(*a).total_cmp(&mag(*b))).unwrap_or(k0);
        let mut fc = k * df;
        for _ in 0..self.iters {
            fc += (self.c * self.raw(x, centre, fc)).clamp(-0.6, 0.6) * df;
        }
        fc
    }
}

#[test]
#[ignore]
fn candan_clean_tone_residual() {
    for iters in [1, 2, 3] {
        let e = Candan::new(2500, 1.0, iters);
        let n = e.n();
        let worst = (0..=20)
            .map(|i| {
                let f = (150.0 + -0.5 + 0.05 * i as f64) * e.df();
                let x: Vec<f32> = (0..n).map(|j| (TAU * f * j as f64 / 2500.0).sin() as f32).collect();
                ((e.estimate(&x, (n / 2) as f64 / 2500.0, f + 0.3) - f) / e.df()).abs()
            })
            .fold(0.0, f64::max);
        println!("iters {iters}: c = {:.4}, worst clean-tone |δ error| = {worst:.2e} bins", e.c());
    }
}
