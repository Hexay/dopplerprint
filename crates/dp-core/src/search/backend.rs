//! Where the f0 sweep runs. `Auto` (default) uses CUDA when a usable NVIDIA GPU is present, else the CPU.
//! Given the same Doppler offsets both sweeps are bit-identical; CUDA computes offsets on the device,
//! equal to the CPU's to rounding (see search/gpu/mod.rs), so scores agree to ~1e-9 relative.

use super::job::{Job, materialise};
use super::score::{JointScore, ScoreMode, ScoreParams, ScoringPlane};
use super::sweep;
use crate::track::Track;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Auto,
    Cpu,
    Cuda,
}

pub struct Scorer<'a> {
    plane: &'a ScoringPlane,
    #[cfg(feature = "cuda")]
    gpu: Option<super::gpu::GpuPlane>,
}

impl<'a> Scorer<'a> {
    /// `Cuda` fails without a usable GPU; `Auto` falls back to the CPU.
    pub fn new(plane: &'a ScoringPlane, backend: Backend) -> anyhow::Result<Self> {
        if plane.mode == ScoreMode::PowerLlr {
            anyhow::ensure!(backend != Backend::Cuda, "mode = \"power_llr\" runs on the CPU only");
            return Ok(Self::cpu(plane));
        }
        match backend {
            Backend::Cpu => Ok(Self::cpu(plane)),
            Backend::Auto => Ok(Self::with_gpu(plane).unwrap_or_else(|_| Self::cpu(plane))),
            Backend::Cuda => Self::with_gpu(plane).map_err(|e| anyhow::anyhow!("backend = \"cuda\": {e}")),
        }
    }

    pub fn cpu(plane: &'a ScoringPlane) -> Self {
        Self {
            plane,
            #[cfg(feature = "cuda")]
            gpu: None,
        }
    }

    #[cfg(feature = "cuda")]
    fn with_gpu(plane: &'a ScoringPlane) -> Result<Self, String> {
        Ok(Self { plane, gpu: Some(super::gpu::GpuPlane::new(&plane.spec)?) })
    }

    #[cfg(not(feature = "cuda"))]
    fn with_gpu(_: &'a ScoringPlane) -> Result<Self, String> {
        Err("built without the `cuda` feature".into())
    }

    pub fn plane(&self) -> &ScoringPlane {
        self.plane
    }

    /// "cpu" or "cuda (<device name>)".
    pub fn device(&self) -> String {
        #[cfg(feature = "cuda")]
        if let Some(g) = &self.gpu {
            return format!("cuda ({})", g.device_name());
        }
        "cpu".into()
    }

    /// Whether jobs should leave their Doppler offsets to the scorer (`job::prepare_job_with`).
    /// `DP_CUDA_HOST_OFFSETS=1` keeps them on the CPU: tools/gpu_parity.sh's sweep bit-identity check.
    pub fn defers_offsets(&self) -> bool {
        #[cfg(feature = "cuda")]
        if self.gpu.is_some() {
            return std::env::var_os("DP_CUDA_HOST_OFFSETS").is_none_or(|v| v != "1");
        }
        false
    }

    /// One `JointScore` per job; deferred offsets index `tracks`. A failing GPU launch is reported once
    /// per batch and the batch is rescored on the CPU, offsets included.
    pub fn score(&self, jobs: &mut [Job], p: &ScoreParams, tracks: &[Track]) -> Vec<Option<JointScore>> {
        if self.plane.mode == ScoreMode::PowerLlr {
            materialise(&self.plane.spec, jobs, tracks, &[]);
            return super::power_llr::score_jobs(&self.plane.spec, jobs, p);
        }
        // TODO: CUDA drift sweep (gpu/sweep.cu); until then a drift grid scores on the CPU.
        #[cfg(feature = "cuda")]
        if let Some(g) = self.gpu.as_ref().filter(|_| p.drift_grid.is_empty()) {
            match g.sweep(&self.plane.spec, jobs, p, tracks) {
                Ok((picks, evs)) => return joint_scores(self.plane, jobs, p, &picks, &evs),
                Err(e) => eprintln!("warning: {e}; scoring this batch on the CPU"),
            }
        }
        materialise(&self.plane.spec, jobs, tracks, &p.drift_grid);
        sweep::score_jobs(&self.plane.spec, jobs, p)
    }
}

/// `JointScore`s from the GPU's picks; `evs` holds each job's events at its pick, in job order.
#[cfg(feature = "cuda")]
fn joint_scores(
    plane: &ScoringPlane,
    jobs: &[Job],
    p: &ScoreParams,
    picks: &[super::job::Pick],
    evs: &[super::job::StepBest],
) -> Vec<Option<JointScore>> {
    let mut rest = evs;
    jobs.iter()
        .zip(picks)
        .map(|(job, &pick)| {
            let (win, tail) = rest.split_at(job.events.len());
            rest = tail;
            Some(super::job::joint(&plane.spec, job, p, pick, win))
        })
        .collect()
}
