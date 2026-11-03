//! Dropouts, phase jumps and frequency steps in the hum: complex demodulation of one harmonic along the
//! ENF trace, then level and phase-continuity tests on the baseband blocks.

use std::f64::consts::{PI, TAU};

use super::spectrum::{median, robust_sigma};
use super::{Discontinuity, DiscontinuityKind, HumFrame, HumParams};

const BLOCK_S: f64 = 0.05;
/// Blocks per side for the coherent phase before/after a boundary.
const FIT_BLOCKS: usize = 10;
/// Block RMS below this fraction of the median is digital silence.
const SILENT_REL: f64 = 0.01;
const HUM_DROP_REL: f64 = 0.5;
/// A hum-level dip longer than this is a fade (source moved, AGC), not a dropout.
const MAX_HUM_GAP_S: f64 = 5.0;
const MIN_FREQ_JUMP_HZ: f64 = 0.02;
const MIN_COHERENCE: f64 = 0.6;

struct Blocks {
    t: Vec<f64>,
    z: Vec<(f64, f64)>,
    rms: Vec<f64>,
    ok: Vec<bool>,
    dt: f64,
}

pub(super) fn find(x: &[f32], sr: f64, f0: f64, k: u32, frames: &[HumFrame], p: &HumParams) -> Vec<Discontinuity> {
    let b = demodulate(x, sr, f0, k, frames);
    let mut out = Vec::new();
    if b.t.len() >= 3 * FIT_BLOCKS {
        let dropped = dropped_blocks(&b);
        out.extend(gaps(&b, &dropped, k));
        out.extend(phase_jumps(&b, &dropped, k, p));
    }
    let fj = freq_jumps(frames, p, &out);
    out.extend(fj);
    out.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    out
}

/// Valid-frame frequency, linearly interpolated; `f0` when there are none.
fn freq_at(valid: &[(f64, f64)], t: f64, f0: f64) -> f64 {
    let i = valid.partition_point(|v| v.0 < t);
    match (i.checked_sub(1).map(|j| valid[j]), valid.get(i)) {
        (Some(a), Some(b)) if b.0 > a.0 => a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0),
        (Some(a), _) => a.1,
        (_, Some(b)) => b.1,
        _ => f0,
    }
}

fn demodulate(x: &[f32], sr: f64, f0: f64, k: u32, frames: &[HumFrame]) -> Blocks {
    let valid: Vec<(f64, f64)> = frames.iter().filter(|f| f.valid).map(|f| (f.t_s, f.f_hz)).collect();
    // A whole number of fundamental periods per block nulls every other harmonic and the 2kf image.
    let len = (BLOCK_S * f0).round().max(1.0) / f0 * sr;
    let n_blocks = (x.len() as f64 / len).floor() as usize;
    let mut b = Blocks { t: Vec::new(), z: Vec::new(), rms: Vec::new(), ok: Vec::new(), dt: len / sr };
    let mut psi = 0.0f64;
    for j in 0..n_blocks {
        let (s, e) = ((j as f64 * len).round() as usize, ((j + 1) as f64 * len).round() as usize);
        let tc = 0.5 * (s + e) as f64 / sr;
        let w = TAU * k as f64 * freq_at(&valid, tc, f0) / sr;
        let (mut re, mut im, mut ss) = (0.0, 0.0, 0.0);
        for &v in &x[s..e] {
            let (sn, cs) = psi.sin_cos();
            re += v as f64 * cs;
            im -= v as f64 * sn;
            ss += (v as f64).powi(2);
            psi += w;
        }
        psi = psi.rem_euclid(TAU);
        let n = (e - s).max(1) as f64;
        b.t.push(tc);
        b.z.push((re / n, im / n));
        b.rms.push((ss / n).sqrt());
        let i = frames.partition_point(|f| f.t_s < tc);
        let near = [i.checked_sub(1), Some(i)]
            .into_iter()
            .flatten()
            .filter_map(|i| frames.get(i))
            .min_by(|a, c| (a.t_s - tc).abs().total_cmp(&(c.t_s - tc).abs()));
        b.ok.push(near.is_some_and(|f| f.valid));
    }
    b
}

fn dropped_blocks(b: &Blocks) -> Vec<bool> {
    let n = b.t.len();
    let med_rms = median(&mut b.rms.clone());
    let a: Vec<f64> = b.z.iter().map(|z| z.0.hypot(z.1)).collect();
    let a_s: Vec<f64> = (0..n)
        .map(|j| {
            let w = &a[j.saturating_sub(1)..(j + 2).min(n)];
            w.iter().sum::<f64>() / w.len() as f64
        })
        .collect();
    let med_a = median(&mut a.clone());
    let sig = robust_sigma(&a_s);
    (0..n)
        .map(|j| {
            b.rms[j] < SILENT_REL * med_rms || (a_s[j] < HUM_DROP_REL * med_a && med_a - a_s[j] > 6.0 * sig)
        })
        .collect()
}

fn wrap(x: f64) -> f64 {
    (x + PI).rem_euclid(TAU) - PI
}

/// Coherent phase of a run of blocks. The demodulator already follows the ENF trace, so residual
/// frequency is negligible; unwrap-then-fit (or re-estimating it) slips at low block SNR and fakes ½-cycle jumps.
fn phase_of(z: &[(f64, f64)]) -> f64 {
    let (re, im) = z.iter().fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    im.atan2(re)
}

fn gaps(b: &Blocks, dropped: &[bool], k: u32) -> Vec<Discontinuity> {
    let n = dropped.len();
    let silent = SILENT_REL * median(&mut b.rms.clone());
    let mut out = Vec::new();
    let mut j = 0;
    while j < n {
        if !dropped[j] {
            j += 1;
            continue;
        }
        let s = j;
        while j < n && dropped[j] {
            j += 1;
        }
        let all_silent = (s..j).all(|i| b.rms[i] < silent);
        let (t0, t1) = (b.t[s] - 0.5 * b.dt, b.t[j - 1] + 0.5 * b.dt);
        // Leading/trailing silence is padding, not a gap.
        if (t1 - t0 > MAX_HUM_GAP_S && !all_silent) || s == 0 || j == n {
            continue;
        }
        let ok_side = |r: std::ops::Range<usize>| r.end <= n && !dropped[r.clone()].iter().any(|&d| d);
        let (before, after) = (s.saturating_sub(FIT_BLOCKS)..s, j..j + FIT_BLOCKS);
        let jump_cycles = (s >= FIT_BLOCKS && ok_side(before.clone()) && ok_side(after.clone()))
            .then(|| wrap(phase_of(&b.z[after]) - phase_of(&b.z[before])) / (TAU * k as f64));
        out.push(Discontinuity { t_s: t0, t_end_s: t1, kind: DiscontinuityKind::Dropout, jump_cycles, jump_hz: None });
    }
    out
}

fn phase_jumps(b: &Blocks, dropped: &[bool], k: u32, p: &HumParams) -> Vec<Discontinuity> {
    let (n, w) = (b.t.len(), FIT_BLOCKS);
    let med_amp = median(&mut b.z.iter().map(|z| z.0.hypot(z.1)).collect::<Vec<_>>());
    // Noise-dominated windows sum to ~median/√w with near-uniform phase: their difference fakes ½-cycle jumps.
    let coherent = |r: &[(f64, f64)]| {
        let (re, im) = r.iter().fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
        re.hypot(im) / r.len() as f64 >= MIN_COHERENCE * med_amp
    };
    let jumps: Vec<Option<f64>> = (0..n)
        .map(|j| {
            if j < w || j + w > n || (j - w..j + w).any(|i| dropped[i] || !b.ok[i]) {
                return None;
            }
            if !coherent(&b.z[j - w..j]) || !coherent(&b.z[j..j + w]) {
                return None;
            }
            Some(wrap(phase_of(&b.z[j..j + w]) - phase_of(&b.z[j - w..j])))
        })
        .collect();
    let mut mags: Vec<f64> = jumps.iter().flatten().map(|x| x.abs()).collect();
    if mags.is_empty() {
        return Vec::new();
    }
    let sig = 1.4826 * median(&mut mags);
    let thr = (p.jump_sigma * sig).max(TAU * k as f64 * p.min_jump_cycles);
    (0..n)
        .filter_map(|j| {
            let jv = jumps[j]?;
            let m = jv.abs();
            let local_max = (j.saturating_sub(w)..(j + w).min(n)).all(|i| jumps[i].is_none_or(|x| x.abs() <= m));
            (m >= thr && local_max).then(|| {
                let tb = 0.5 * (b.t[j - 1] + b.t[j]);
                Discontinuity {
                    t_s: tb,
                    t_end_s: tb,
                    kind: DiscontinuityKind::PhaseJump,
                    jump_cycles: Some(jv / (TAU * k as f64)),
                    jump_hz: None,
                }
            })
        })
        .collect()
}

/// Steps between frames j−1 and j+1 (non-overlapping windows) beyond what grid wander allows.
fn freq_jumps(frames: &[HumFrame], p: &HumParams, known: &[Discontinuity]) -> Vec<Discontinuity> {
    let d: Vec<Option<f64>> = (0..frames.len())
        .map(|j| {
            let (a, c) = (frames.get(j.wrapping_sub(1))?, frames.get(j + 1)?);
            (a.valid && c.valid && frames[j].valid).then_some(c.f_hz - a.f_hz)
        })
        .collect();
    let all: Vec<f64> = d.iter().flatten().copied().collect();
    if all.len() < 10 {
        return Vec::new();
    }
    let thr = (8.0 * robust_sigma(&all)).max(MIN_FREQ_JUMP_HZ);
    let near_known = |t: f64| known.iter().any(|k| t >= k.t_s - p.frame_s && t <= k.t_end_s + p.frame_s);
    (0..frames.len())
        .filter_map(|j| {
            let m = d[j]?.abs();
            let local_max = (j.saturating_sub(2)..(j + 3).min(d.len())).all(|i| d[i].is_none_or(|x| x.abs() <= m));
            (m >= thr && local_max && !near_known(frames[j].t_s)).then(|| Discontinuity {
                t_s: frames[j].t_s,
                t_end_s: frames[j].t_s,
                kind: DiscontinuityKind::FreqJump,
                jump_cycles: None,
                jump_hz: d[j],
            })
        })
        .collect()
}
