//! CUDA f0 sweep, bit-identical to search::sweep (shared arithmetic: search/fixed.rs). The PTX is
//! committed (tools/dev.sh ptx) so builds need no CUDA toolkit; at run time only the NVIDIA driver is
//! needed, and a driver older than the PTX ISA fails to load → CPU fallback.

use std::sync::{Arc, OnceLock};

use cudarc::driver::{CudaContext, CudaFunction, CudaSlice, CudaStream, LaunchConfig, PushKernelArg};
use cudarc::nvrtc::Ptx;

use super::fixed;
use super::job::{Job, StepBest, harmonic_shifts};
use super::score::ScoreParams;
use crate::audio::LogSpectrogram;

const PTX: &str = include_str!("sweep.ptx");
/// Must match WARPS in sweep.cu: one warp per (event, k) pair.
const WARPS: u64 = 4;

struct Device {
    stream: Arc<CudaStream>,
    func: CudaFunction,
    name: String,
}

static DEVICE: OnceLock<Result<Device, String>> = OnceLock::new();

fn device() -> Result<&'static Device, String> {
    DEVICE.get_or_init(open).as_ref().map_err(Clone::clone)
}

fn open() -> Result<Device, String> {
    // cudarc's dynamic loader panics on first use when libcuda is absent, so probe first.
    if !unsafe { cudarc::driver::sys::is_culib_present() } {
        return Err("no NVIDIA driver (libcuda) found".into());
    }
    let ctx = CudaContext::new(0).map_err(|e| format!("CUDA device 0: {e}"))?;
    let name = ctx.name().map_err(|e| format!("CUDA device name: {e}"))?;
    let module = ctx
        .load_module(Ptx::from_src(PTX))
        .map_err(|e| format!("loading kernel on {name}: {e} (driver older than the PTX ISA?)"))?;
    let func = module.load_function("best_f0").map_err(|e| format!("kernel best_f0: {e}"))?;
    Ok(Device { stream: ctx.default_stream(), func, name })
}

/// A recording's scoring plane resident on the GPU.
pub struct GpuPlane {
    dev: &'static Device,
    plane: CudaSlice<f32>,
    n_f: u32,
}

impl GpuPlane {
    pub fn new(spec: &LogSpectrogram) -> Result<Self, String> {
        let dev = device()?;
        let plane = dev.stream.clone_htod(&spec.data).map_err(|e| format!("uploading plane: {e}"))?;
        Ok(Self { dev, plane, n_f: spec.n_f as u32 })
    }

    pub fn device_name(&self) -> &str {
        &self.dev.name
    }

    /// Best f0 step per (event instance, k), event instances in job-then-event order, k fastest.
    pub fn sweep(&self, spec: &LogSpectrogram, jobs: &[Job], p: &ScoreParams) -> Result<Vec<StepBest>, String> {
        let Some(n_k) = jobs.first().map(Job::n_k) else { return Ok(Vec::new()) };
        let b = Batch::flatten(jobs, n_k);
        if b.frame0.is_empty() {
            return Ok(Vec::new());
        }
        let (hs, hw): (Vec<i64>, Vec<f32>) = harmonic_shifts(spec, p).into_iter().unzip();
        let s = &self.dev.stream;
        let e = |e: cudarc::driver::DriverError| format!("CUDA sweep: {e}");
        let d_off = s.clone_htod(&b.offsets).map_err(e)?;
        let d_off_start = s.clone_htod(&b.off_start).map_err(e)?;
        let d_frame0 = s.clone_htod(&b.frame0).map_err(e)?;
        let d_nframes = s.clone_htod(&b.nframes).map_err(e)?;
        let d_b0 = s.clone_htod(&b.b0).map_err(e)?;
        let d_nsteps = s.clone_htod(&b.nsteps).map_err(e)?;
        let d_norm = s.clone_htod(&b.norm).map_err(e)?;
        let d_hs = s.clone_htod(&hs).map_err(e)?;
        let d_hw = s.clone_htod(&hw).map_err(e)?;
        let n_pairs = (b.frame0.len() * n_k) as u64;
        let mut d_z = s.alloc_zeros::<f64>(n_pairs as usize).map_err(e)?;
        let mut d_step = s.alloc_zeros::<u32>(n_pairs as usize).map_err(e)?;
        let (n_h, step, n_k32) = (hs.len() as u32, fixed::pos(p.f0_step_bins), n_k as u32);
        let blocks = u32::try_from(n_pairs.div_ceil(WARPS)).map_err(|_| "CUDA sweep: batch too large".to_string())?;
        let cfg = LaunchConfig { grid_dim: (blocks, 1, 1), block_dim: (32 * WARPS as u32, 1, 1), shared_mem_bytes: 0 };
        let mut l = s.launch_builder(&self.dev.func);
        l.arg(&self.plane).arg(&self.n_f).arg(&d_off).arg(&d_off_start).arg(&d_frame0).arg(&d_nframes);
        l.arg(&d_b0).arg(&d_nsteps).arg(&d_norm).arg(&d_hs).arg(&d_hw).arg(&n_h).arg(&step).arg(&n_k32);
        l.arg(&n_pairs).arg(&mut d_z).arg(&mut d_step);
        // SAFETY: argument order and types match best_f0 in sweep.cu; the kernel bounds-checks pair < n_pairs.
        unsafe { l.launch(cfg) }.map_err(e)?;
        let z = s.clone_dtoh(&d_z).map_err(e)?;
        let st = s.clone_dtoh(&d_step).map_err(e)?;
        Ok(z.into_iter().zip(st).map(|(z, step)| StepBest { z, step: step as usize }).collect())
    }
}

/// Struct-of-arrays upload layout, one entry per (job, event).
#[derive(Default)]
struct Batch {
    offsets: Vec<i64>,
    off_start: Vec<u64>,
    frame0: Vec<u32>,
    nframes: Vec<u32>,
    b0: Vec<i64>,
    nsteps: Vec<u32>,
    norm: Vec<f64>,
}

impl Batch {
    fn flatten(jobs: &[Job], n_k: usize) -> Self {
        use rayon::prelude::*;
        let mut b = Batch::default();
        let mut total = 0usize;
        for job in jobs {
            assert_eq!(job.n_k(), n_k, "one launch needs a uniform clock sweep");
            for pr in &job.events {
                b.off_start.push(total as u64);
                total += pr.offsets.len();
                b.frame0.push(pr.frames.start as u32);
                b.nframes.push(pr.frames.len() as u32);
                b.b0.push(pr.b0_start_fx);
                b.nsteps.push(pr.n_steps as u32);
                b.norm.push(pr.norm);
            }
        }
        b.offsets = vec![0; total];
        let mut rest = b.offsets.as_mut_slice();
        let mut parts = Vec::with_capacity(b.off_start.len());
        for pr in jobs.iter().flat_map(|j| &j.events) {
            let (head, tail) = rest.split_at_mut(pr.offsets.len());
            parts.push((head, &pr.offsets));
            rest = tail;
        }
        parts.into_par_iter().for_each(|(dst, src)| dst.copy_from_slice(src));
        b
    }
}
