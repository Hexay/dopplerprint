//! Test-only flyover synthesiser, independent of the closed form: retarded time is solved per sample for the
//! direct path and the ground-image path (Lloyd's mirror), and the tone phase is the emitted phase at τ.

pub struct Flyover {
    pub f0: f64,
    pub v: f64,
    /// Slant range at CPA (m).
    pub d: f64,
    /// CPA arrival time on the direct path (s).
    pub ta: f64,
    pub alt_frac: f64,
    pub mic_h: f64,
    pub reflect: f64,
    pub harmonics: Vec<(u32, f64)>,
    pub c: f64,
}

pub struct Rng(pub u64);

impl Rng {
    pub fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn gauss(&mut self) -> f64 {
        (-2.0 * self.uniform().max(1e-300).ln()).sqrt() * (2.0 * std::f64::consts::PI * self.uniform()).cos()
    }
}

impl Flyover {
    /// Emission time τ for reception at `t` from a source at height `z` (CPA emission at τ = tc).
    fn emission(&self, t: f64, tc: f64, y0: f64, dz: f64, guess: f64) -> (f64, f64) {
        let mut tau = guess;
        for _ in 0..20 {
            let x = self.v * (tau - tc);
            let r = (x * x + y0 * y0 + dz * dz).sqrt();
            let g = tau + r / self.c - t;
            let dg = 1.0 + self.v * x / (r * self.c);
            let step = g / dg;
            tau -= step;
            if step.abs() < 1e-10 {
                break;
            }
        }
        let x = self.v * (tau - tc);
        (tau, (x * x + y0 * y0 + dz * dz).sqrt())
    }

    pub fn render(&self, sr: u32, dur: f64, noise: f64, rng: &mut Rng) -> Vec<f32> {
        let alt = self.alt_frac * self.d + self.mic_h;
        let dz = alt - self.mic_h;
        let y0 = (self.d * self.d - dz * dz).max(0.0).sqrt();
        let tc = self.ta - self.d / self.c;
        let n = (dur * sr as f64) as usize;
        let (mut gd, mut gi) = (-self.d / self.c, -self.d / self.c);
        let tau_pi = 2.0 * std::f64::consts::PI * self.f0;
        (0..n)
            .map(|i| {
                let t = i as f64 / sr as f64;
                let (td, rd) = self.emission(t, tc, y0, dz, gd + t);
                let (ti, ri) = self.emission(t, tc, y0, alt + self.mic_h, gi + t);
                (gd, gi) = (td - t, ti - t);
                let mut s = 0.0;
                for &(h, a) in &self.harmonics {
                    let k = tau_pi * h as f64;
                    s += a * ((k * td).sin() * self.d / rd + self.reflect * (k * ti).sin() * self.d / ri);
                }
                (s + noise * rng.gauss()) as f32
            })
            .collect()
    }
}
