//! CUDA backend. The f0 sweep is bit-identical to search::sweep given the same offsets (shared
//! arithmetic: search/fixed.rs). Deferred offsets (`Prepared::emit`) are computed on the device (emit.rs),
//! matching the CPU's to rounding. The PTX is committed (tools/dev.sh ptx) so builds need no CUDA toolkit;
//! at run time only the NVIDIA driver is needed, and a driver older than the PTX ISA fails to load → CPU.

mod batch;
mod emit;

use std::sync::{Arc, Mutex, OnceLock};

use cudarc::driver::{CudaContext, CudaFunction, CudaSlice, CudaStream, LaunchConfig, PushKernelArg};
use cudarc::nvrtc::Ptx;

use super::fixed;
use super::job::{Job, StepBest, harmonic_shifts};
use super::score::ScoreParams;
use crate::audio::LogSpectrogram;
use crate::track::Track;
use batch::Batch;
use emit::DevTracks;

const SWEEP_PTX: &str = include_str!("sweep.ptx");
const EMISSION_PTX: &str = include_str!("emission.ptx");
/// Must match WARPS in sweep.cu: one warp per (event, k) pair.
const WARPS: u64 = 4;

struct Device {
    stream: Arc<CudaStream>,
    sweep: CudaFunction,
    offsets: CudaFunction,
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
    let load = |ptx: &str, f: &str| {
        let m = ctx
            .load_module(Ptx::from_src(ptx))
            .map_err(|e| format!("loading kernel on {name}: {e} (driver older than the PTX ISA?)"))?;
        m.load_function(f).map_err(|e| format!("kernel {f}: {e}"))
    };
    let (sweep, offsets) = (load(SWEEP_PTX, "best_f0")?, load(EMISSION_PTX, "offsets")?);
    Ok(Device { stream: ctx.default_stream(), sweep, offsets, name })
}

/// A recording's scoring plane resident on the GPU.
pub struct GpuPlane {
    dev: &'static Device,
    plane: CudaSlice<f32>,
    n_f: u32,
    /// Offsets staging buffer reused across batches: a fresh ~100 MB allocation per batch page-faults.
    staging: Mutex<Vec<i64>>,
    /// The catalogue deferred offsets index, uploaded on first use.
    tracks: Mutex<Option<DevTracks>>,
}

impl GpuPlane {
    pub fn new(spec: &LogSpectrogram) -> Result<Self, String> {
        let dev = device()?;
        let plane = dev.stream.clone_htod(&spec.data).map_err(|e| format!("uploading plane: {e}"))?;
        Ok(Self { dev, plane, n_f: spec.n_f as u32, staging: Mutex::default(), tracks: Mutex::default() })
    }

    pub fn device_name(&self) -> &str {
        &self.dev.name
    }

    /// Best f0 step per (event instance, k), event instances in job-then-event order, k fastest. Offsets
    /// come from the jobs (uploaded) or, when every instance is deferred, from the device (`tracks` is
    /// the catalogue they index). A mix is an error, so the caller rescores on the CPU.
    pub fn sweep(&self, spec: &LogSpectrogram, jobs: &[Job], p: &ScoreParams, tracks: &[Track]) -> Result<Vec<StepBest>, String> {
        let Some(n_k) = jobs.first().map(Job::n_k) else { return Ok(Vec::new()) };
        let (n_inst, deferred) = jobs
            .iter()
            .flat_map(|j| &j.events)
            .fold((0, 0), |(n, d), pr| (n + 1, d + pr.emit.is_some() as usize));
        if deferred != 0 && deferred != n_inst {
            return Err("CUDA sweep: batch mixes deferred and host offsets".into());
        }
        let s = &self.dev.stream;
        let e = |e: cudarc::driver::DriverError| format!("CUDA sweep: {e}");
        let (b, d_off) = if deferred == 0 {
            let mut staging = self.staging.lock().unwrap_or_else(|p| p.into_inner());
            let b = Batch::flatten(jobs, n_k, Some(&mut staging));
            let d_off = s.clone_htod(staging.as_slice()).map_err(e)?;
            (b, d_off)
        } else {
            let b = Batch::flatten(jobs, n_k, None);
            let mut dt = self.tracks.lock().unwrap_or_else(|p| p.into_inner());
            if !dt.as_ref().is_some_and(|d| d.matches(tracks)) {
                *dt = Some(DevTracks::upload(s, tracks)?);
            }
            let d_off = emit::offsets(s, &self.dev.offsets, dt.as_ref().unwrap(), spec, jobs, &b.off_start, b.total)?;
            (b, d_off)
        };
        if b.frame0.is_empty() {
            return Ok(Vec::new());
        }
        let (hs, hw): (Vec<i64>, Vec<f32>) = harmonic_shifts(spec, p).into_iter().unzip();
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
        let mut d_cov = s.alloc_zeros::<u32>(n_pairs as usize).map_err(e)?;
        let (n_h, step, n_k32) = (hs.len() as u32, fixed::pos(p.f0_step_bins), n_k as u32);
        let blocks = u32::try_from(n_pairs.div_ceil(WARPS)).map_err(|_| "CUDA sweep: batch too large".to_string())?;
        let cfg = LaunchConfig { grid_dim: (blocks, 1, 1), block_dim: (32 * WARPS as u32, 1, 1), shared_mem_bytes: 0 };
        let mut l = s.launch_builder(&self.dev.sweep);
        l.arg(&self.plane).arg(&self.n_f).arg(&d_off).arg(&d_off_start).arg(&d_frame0).arg(&d_nframes);
        l.arg(&d_b0).arg(&d_nsteps).arg(&d_norm).arg(&d_hs).arg(&d_hw).arg(&n_h).arg(&step).arg(&n_k32);
        l.arg(&n_pairs).arg(&mut d_z).arg(&mut d_step).arg(&mut d_cov);
        // SAFETY: argument order and types match best_f0 in sweep.cu; the kernel bounds-checks pair < n_pairs.
        unsafe { l.launch(cfg) }.map_err(e)?;
        let z = s.clone_dtoh(&d_z).map_err(e)?;
        let st = s.clone_dtoh(&d_step).map_err(e)?;
        let cov = s.clone_dtoh(&d_cov).map_err(e)?;
        Ok(z.into_iter().zip(st).zip(cov).map(|((z, step), covered)| StepBest { z, step: step as usize, covered }).collect())
    }
}
