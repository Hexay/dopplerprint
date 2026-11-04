use super::*;
use crate::search::job::Prepared;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The pre-layout sweep: one strided pass per (frame, harmonic) through `fixed::lerp` and `fixed::term`.
fn reference(spec: &LogSpectrogram, pr: &Prepared, k: usize, shifts: &[(i64, f32)], p: &ScoreParams) -> StepBest {
    let mut acc = vec![0i64; pr.n_steps];
    let (step, top) = (fixed::pos(p.f0_step_bins), fixed::top(spec.n_f));
    let mut covered = 0;
    for (i, frame) in pr.frames.clone().enumerate() {
        let off = pr.offsets[i + k];
        if off == fixed::NONE {
            continue;
        }
        covered += 1;
        for &(hs, w) in shifts {
            let base = pr.b0_start_fx + off + hs;
            let run = fixed::in_row(base, step, top, pr.n_steps);
            let mut pos = base + run.start as i64 * step;
            for a in &mut acc[run] {
                *a += fixed::term(w * fixed::lerp(spec.row(frame), pos));
                pos += step;
            }
        }
    }
    let mut best = StepBest { z: f64::NEG_INFINITY, step: 0, covered };
    for (s, &a) in acc.iter().enumerate() {
        let z = fixed::total(a) / pr.norm;
        if z > best.z {
            best = StepBest { z, step: s, covered };
        }
    }
    best
}

#[test]
fn layout_sweep_matches_strided_reference() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let (n_t, n_f) = (40, 160);
    let mut data: Vec<f32> = (0..n_t * n_f).map(|_| (rng.unit() * 20.0 - 8.0) as f32).collect();
    data[7 * n_f + 30] = f32::NAN;
    data[9 * n_f + 31] = 1e12;
    let spec = LogSpectrogram { t0: 0.0, dt: 0.1, f_min: 20.0, bins_per_octave: 24, n_t, n_f, data };
    for (step_bins, n_steps, b0) in [(0.25, 89, 10.3), (0.25, 7, 140.6), (0.25, 120, -60.2), (0.125, 33, 90.0), (0.3, 50, 20.7), (1.0, 40, 3.1)] {
        let p = ScoreParams { f0_step_bins: step_bins, ..ScoreParams::default() };
        let shifts = harmonic_shifts(&spec, &p);
        let k_max = 3;
        let frames = 2..n_t - 2 * k_max;
        let offsets = (0..frames.len() + 2 * k_max)
            .map(|_| if rng.unit() < 0.1 { fixed::NONE } else { fixed::pos(rng.unit() * 30.0 - 15.0) })
            .collect();
        let pr = Prepared { frames, offsets, emit: None, drift: Vec::new(), b0_start: b0, b0_start_fx: fixed::pos(b0), n_steps, norm: 7.5 };
        let mut acc = Vec::new();
        for k in 0..=2 * k_max {
            let got = sweep_one(&spec, &pr, None, k, &shifts, &mut acc, &p);
            assert_eq!(got, reference(&spec, &pr, k, &shifts, &p), "step {step_bins} n {n_steps} b0 {b0} k {k}");
        }
    }
}
