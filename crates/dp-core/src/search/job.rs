//! One hypothesis prepared for scoring, independent of where the f0 sweep runs (CPU or CUDA).
//! Both backends produce a per-(event, clock step) best f0 step; `assemble` turns that into a `JointScore`.

use super::fixed;
use super::score::{EventScore, EventTarget, JointScore, ScoreParams};
use crate::audio::LogSpectrogram;
use crate::geo::Vec3;
use crate::physics::{self, Atmosphere};
use crate::track::Track;

pub struct Job {
    pub clock_nominal: f64,
    /// Clock sweep is k ∈ [−k_max, k_max] frames; index `k + k_max` everywhere below.
    pub k_max: usize,
    pub events: Vec<Prepared>,
}

pub struct Prepared {
    pub frames: std::ops::Range<usize>,
    /// bpo·log2(F) for frame (frames.start + i) at sweep index k is `offsets[i + k]`, as `fixed::pos`
    /// (`fixed::NONE` where the track does not cover the frame). Empty while `emit` is set.
    pub offsets: Vec<i64>,
    /// Offsets still to be computed, on the GPU (`gpu::emit`) or by `materialise`.
    pub emit: Option<Emitter>,
    /// First f0 step in bins (f64, for reporting f0) and as `fixed::pos` (for scoring).
    pub b0_start: f64,
    pub b0_start_fx: i64,
    pub n_steps: usize,
    /// Matched-filter normaliser: sqrt(frames · Σw²).
    pub norm: f64,
}

/// Best f0 step for one (event, clock index): z = Σ / norm; `covered` frames had an offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepBest {
    pub z: f64,
    pub step: usize,
    pub covered: u32,
}

impl Job {
    pub fn n_k(&self) -> usize {
        2 * self.k_max + 1
    }
}

/// Inputs of one offsets array: observation times `spec.t0 + (first + j)·dt + clock` for j in 0..n, seen
/// from `obs` through a uniform medium (c, wind), for catalogue track `track`.
#[derive(Clone, Copy, Debug)]
pub struct Emitter {
    pub track: u32,
    pub obs: Vec3,
    pub first: i64,
    pub clock: f64,
    pub n: usize,
    pub c: f64,
    pub wind: Vec3,
}

impl Prepared {
    pub fn b0(&self, step: usize, p: &ScoreParams) -> f64 {
        self.b0_start + step as f64 * p.f0_step_bins
    }

    pub fn n_offsets(&self) -> usize {
        self.emit.map_or(self.offsets.len(), |e| e.n)
    }
}

/// Compute any deferred offsets on the CPU (`tracks` = the catalogue the emitters index).
pub fn materialise(spec: &LogSpectrogram, jobs: &mut [Job], tracks: &[Track]) {
    for pr in jobs.iter_mut().flat_map(|j| &mut j.events) {
        if let Some(e) = pr.emit.take() {
            pr.offsets = host_offsets(spec, &tracks[e.track as usize], e.obs, &Atmosphere::uniform(e.c, e.wind), e.first, e.clock, e.n);
        }
    }
}

fn host_offsets(spec: &LogSpectrogram, track: &Track, obs: Vec3, atm: &Atmosphere, first: i64, clock: f64, n: usize) -> Vec<i64> {
    let times: Vec<f64> = (0..n as i64).map(|j| spec.t0 + (first + j) as f64 * spec.dt + clock).collect();
    let bpo = spec.bins_per_octave as f64;
    physics::doppler_factors(track, obs, &times, atm).into_iter().map(|f| fixed::pos(bpo * f.log2())).collect()
}

pub fn k_max(spec: &LogSpectrogram, p: &ScoreParams) -> usize {
    (p.clock_sweep_s / spec.dt).round() as usize
}

/// (bpo·log2 h as `fixed::pos`, weight) per harmonic: on a log axis harmonic h is a fixed vertical shift.
pub fn harmonic_shifts(spec: &LogSpectrogram, p: &ScoreParams) -> Vec<(i64, f32)> {
    let bpo = spec.bins_per_octave as f64;
    p.harmonics.iter().map(|&(h, w)| (fixed::pos(bpo * (h as f64).log2()), w)).collect()
}

/// Offsets computed here on the CPU.
pub fn prepare_job(
    spec: &LogSpectrogram,
    targets: &[EventTarget<'_>],
    obs: Vec3,
    atm: &Atmosphere,
    clock_nominal: f64,
    p: &ScoreParams,
) -> Job {
    prepare_job_with(spec, targets, None, obs, atm, clock_nominal, p)
}

/// With `track_index` (catalogue index per target) and a uniform `atm`, offsets are left to an `Emitter`.
pub fn prepare_job_with(
    spec: &LogSpectrogram,
    targets: &[EventTarget<'_>],
    track_index: Option<&[u32]>,
    obs: Vec3,
    atm: &Atmosphere,
    clock_nominal: f64,
    p: &ScoreParams,
) -> Job {
    let k_max = k_max(spec, p);
    let deferred = track_index.filter(|_| atm.column.is_none());
    let events = targets
        .iter()
        .enumerate()
        .map(|(i, t)| prepare(spec, t, deferred.map(|ix| ix[i]), obs, atm, clock_nominal, k_max, p))
        .collect();
    Job { clock_nominal, k_max, events }
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    spec: &LogSpectrogram,
    t: &EventTarget<'_>,
    defer_track: Option<u32>,
    obs: Vec3,
    atm: &Atmosphere,
    clock: f64,
    k_max: usize,
    p: &ScoreParams,
) -> Prepared {
    let k_max = k_max as i64;
    let f_a = spec.frame_of(t.window.0).ceil().max(0.0) as usize;
    let f_b = (spec.frame_of(t.window.1).floor().max(0.0) as usize + 1).min(spec.n_t);
    let first = f_a as i64 - k_max;
    let n = ((f_b.saturating_sub(f_a)) as i64 + 2 * k_max).max(0) as usize;
    let (offsets, emit) = match defer_track {
        Some(track) => (Vec::new(), Some(Emitter { track, obs, first, clock, n, c: atm.c, wind: atm.wind })),
        None => (host_offsets(spec, t.track, obs, atm, first, clock, n), None),
    };
    let (b0_lo, b0_hi) = (spec.bin_of(t.f0_range.0), spec.bin_of(t.f0_range.1));
    let frames = f_a..f_b.max(f_a);
    let w2 = p.harmonics.iter().map(|(_, w)| (*w as f64).powi(2)).sum::<f64>();
    Prepared {
        norm: ((frames.len().max(1)) as f64 * w2).sqrt(),
        frames,
        offsets,
        emit,
        b0_start: b0_lo,
        b0_start_fx: fixed::pos(b0_lo),
        n_steps: ((b0_hi - b0_lo) / p.f0_step_bins).ceil().max(0.0) as usize + 1,
    }
}

/// Best clock step: `best(e, k)` is called once per (event, k), k outer, events inner; first maximum wins.
/// Totals sum z in event order, so only the winning k needs its `EventScore`s built.
pub fn assemble(
    spec: &LogSpectrogram,
    job: &Job,
    p: &ScoreParams,
    mut best: impl FnMut(usize, usize) -> StepBest,
) -> Option<JointScore> {
    let n_e = job.events.len();
    let (mut cur, mut win) = (Vec::with_capacity(n_e), Vec::with_capacity(n_e));
    let mut top: Option<(f64, usize)> = None;
    for k in 0..job.n_k() {
        cur.clear();
        cur.extend((0..n_e).map(|e| best(e, k)));
        let total = cur.iter().map(|sb| sb.z).sum::<f64>();
        if top.is_none_or(|(t, _)| total > t) {
            top = Some((total, k));
            std::mem::swap(&mut cur, &mut win);
        }
    }
    let (total, k) = top?;
    let events = job
        .events
        .iter()
        .zip(&win)
        .map(|(pr, sb)| EventScore {
            z: sb.z,
            f0: spec.freq_of(pr.b0(sb.step, p)),
            coverage: sb.covered as f64 / pr.frames.len().max(1) as f64,
        })
        .collect();
    let clock_offset = job.clock_nominal + (k as i64 - job.k_max as i64) as f64 * spec.dt;
    Some(JointScore { total, clock_offset, events })
}
