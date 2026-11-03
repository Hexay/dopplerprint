//! Doppler offsets computed on the GPU for jobs whose `Prepared::emit` is set (gpu/emission.cu).
//! Same equations as `physics::emission` in a uniform medium; offsets agree with the CPU's to rounding
//! (~1e-9 relative), not bit for bit. Given offsets, the sweep itself stays bit-identical (sweep.cu).

use std::sync::Arc;

use cudarc::driver::{CudaFunction, CudaSlice, CudaStream, LaunchConfig, PushKernelArg};

use crate::audio::LogSpectrogram;
use crate::search::job::{Emitter, Job};
use crate::track::Track;

/// Frames per thread; must match CHUNK in emission.cu.
const CHUNK: usize = 16;
const BLOCK: u32 = 128;

/// A catalogue's points on the device, struct-of-arrays, with each track's (start, len).
pub struct DevTracks {
    /// Identity of the uploaded slice (address, length): re-uploaded if a different catalogue comes.
    key: (usize, usize),
    t: CudaSlice<f64>,
    pos: CudaSlice<f64>,
    vel: CudaSlice<f64>,
    start: CudaSlice<u32>,
    len: CudaSlice<u32>,
}

impl DevTracks {
    pub fn key_of(tracks: &[Track]) -> (usize, usize) {
        (tracks.as_ptr() as usize, tracks.len())
    }

    pub fn matches(&self, tracks: &[Track]) -> bool {
        self.key == Self::key_of(tracks)
    }

    pub fn upload(s: &Arc<CudaStream>, tracks: &[Track]) -> Result<Self, String> {
        let e = |e: cudarc::driver::DriverError| format!("uploading tracks: {e}");
        let (mut t, mut pos, mut vel, mut start, mut len) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for tr in tracks {
            start.push(t.len() as u32);
            len.push(tr.points.len() as u32);
            for p in &tr.points {
                t.push(p.t);
                pos.extend([p.pos.x, p.pos.y, p.pos.z]);
                vel.extend([p.vel.x, p.vel.y, p.vel.z]);
            }
        }
        Ok(Self {
            key: Self::key_of(tracks),
            t: s.clone_htod(&t).map_err(e)?,
            pos: s.clone_htod(&pos).map_err(e)?,
            vel: s.clone_htod(&vel).map_err(e)?,
            start: s.clone_htod(&start).map_err(e)?,
            len: s.clone_htod(&len).map_err(e)?,
        })
    }
}

/// Offsets for every event instance of `jobs` (all deferred), laid out at `off_start`.
pub fn offsets(
    s: &Arc<CudaStream>,
    func: &CudaFunction,
    tracks: &DevTracks,
    spec: &LogSpectrogram,
    jobs: &[Job],
    off_start: &[u64],
    total: usize,
) -> Result<CudaSlice<i64>, String> {
    let e = |e: cudarc::driver::DriverError| format!("CUDA offsets: {e}");
    let ems: Vec<Emitter> = jobs.iter().flat_map(|j| &j.events).filter_map(|pr| pr.emit).collect();
    let mut a = Inputs::default();
    for (i, em) in ems.iter().enumerate() {
        a.push(em);
        for j0 in (0..em.n).step_by(CHUNK) {
            a.ch_inst.push(i as u32);
            a.ch_j0.push(j0 as u32);
        }
    }
    let n_chunks = a.ch_inst.len() as u32;
    let mut out = s.alloc_zeros::<i64>(total.max(1)).map_err(e)?;
    if n_chunks == 0 {
        return Ok(out);
    }
    let up = |v: &[u32]| s.clone_htod(v).map_err(e);
    let upd = |v: &[f64]| s.clone_htod(v).map_err(e);
    let (d_track, d_n, d_inst, d_j0) = (up(&a.track)?, up(&a.n)?, up(&a.ch_inst)?, up(&a.ch_j0)?);
    let (d_obs, d_clock, d_c, d_wind) = (upd(&a.obs)?, upd(&a.clock)?, upd(&a.c)?, upd(&a.wind)?);
    let d_first = s.clone_htod(&a.first).map_err(e)?;
    let d_off_start = s.clone_htod(off_start).map_err(e)?;
    let (t0, dt, bpo) = (spec.t0, spec.dt, spec.bins_per_octave as f64);
    let cfg = LaunchConfig { grid_dim: (n_chunks.div_ceil(BLOCK), 1, 1), block_dim: (BLOCK, 1, 1), shared_mem_bytes: 0 };
    let mut l = s.launch_builder(func);
    l.arg(&tracks.t).arg(&tracks.pos).arg(&tracks.vel).arg(&tracks.start).arg(&tracks.len);
    l.arg(&d_track).arg(&d_obs).arg(&d_first).arg(&d_clock).arg(&d_n).arg(&d_c).arg(&d_wind).arg(&d_off_start);
    l.arg(&d_inst).arg(&d_j0).arg(&n_chunks).arg(&t0).arg(&dt).arg(&bpo).arg(&mut out);
    // SAFETY: argument order and types match `offsets` in emission.cu; every chunk writes inside its
    // instance's [off_start, off_start + n) range of `out`, which has `total` elements.
    unsafe { l.launch(cfg) }.map_err(e)?;
    Ok(out)
}

/// Per-instance and per-chunk launch arrays.
#[derive(Default)]
struct Inputs {
    track: Vec<u32>,
    obs: Vec<f64>,
    first: Vec<i64>,
    clock: Vec<f64>,
    n: Vec<u32>,
    c: Vec<f64>,
    wind: Vec<f64>,
    ch_inst: Vec<u32>,
    ch_j0: Vec<u32>,
}

impl Inputs {
    fn push(&mut self, em: &Emitter) {
        self.track.push(em.track);
        self.obs.extend([em.obs.x, em.obs.y, em.obs.z]);
        self.first.push(em.first);
        self.clock.push(em.clock);
        self.n.push(em.n as u32);
        self.c.push(em.c);
        self.wind.extend([em.wind.x, em.wind.y, em.wind.z]);
    }
}
