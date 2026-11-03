//! Where the f0 sweep runs. `Auto` (default) uses CUDA when a usable NVIDIA GPU is present, else the CPU.
//! Both give bit-identical scores (see search/gpu/mod.rs), so the choice only affects speed.

use super::job::Job;
use super::score::{JointScore, ScoreMode, ScoreParams, ScoringPlane};
use super::sweep;

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

    /// One `JointScore` per job. A failing GPU launch is reported once per batch and rescored on the CPU.
    pub fn score(&self, jobs: &[Job], p: &ScoreParams) -> Vec<Option<JointScore>> {
        if self.plane.mode == ScoreMode::PowerLlr {
            return super::power_llr::score_jobs(&self.plane.spec, jobs, p);
        }
        #[cfg(feature = "cuda")]
        if let Some(g) = &self.gpu {
            match g.sweep(&self.plane.spec, jobs, p) {
                Ok(table) => return assemble_table(self.plane, jobs, p, &table),
                Err(e) => eprintln!("warning: {e}; scoring this batch on the CPU"),
            }
        }
        sweep::score_jobs(&self.plane.spec, jobs, p)
    }
}

#[cfg(feature = "cuda")]
fn assemble_table(
    plane: &ScoringPlane,
    jobs: &[Job],
    p: &ScoreParams,
    table: &[super::job::StepBest],
) -> Vec<Option<JointScore>> {
    use rayon::prelude::*;
    let starts: Vec<usize> = jobs
        .iter()
        .scan(0, |base, job| {
            let start = *base;
            *base += job.events.len() * job.n_k();
            Some(start)
        })
        .collect();
    jobs.par_iter()
        .zip(starts)
        .map(|(job, base)| {
            let n_k = job.n_k();
            super::job::assemble(&plane.spec, job, p, |e, k| table[base + e * n_k + k])
        })
        .collect()
}
