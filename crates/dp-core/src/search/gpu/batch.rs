//! Struct-of-arrays launch layout for the sweep kernel, one entry per (job, event).

use rayon::prelude::*;

use crate::search::job::Job;

#[derive(Default)]
pub struct Batch {
    pub off_start: Vec<u64>,
    pub frame0: Vec<u32>,
    pub nframes: Vec<u32>,
    pub b0: Vec<i64>,
    pub nsteps: Vec<u32>,
    pub norm: Vec<f64>,
    /// Offsets over all instances (each instance's at `off_start`).
    pub total: usize,
}

impl Batch {
    /// With `host`, the jobs' host offsets are also copied there (in parallel) for upload.
    pub fn flatten(jobs: &[Job], n_k: usize, host: Option<&mut Vec<i64>>) -> Self {
        let mut b = Batch::default();
        for job in jobs {
            assert_eq!(job.n_k(), n_k, "one launch needs a uniform clock sweep");
            for pr in &job.events {
                b.off_start.push(b.total as u64);
                b.total += pr.n_offsets();
                b.frame0.push(pr.frames.start as u32);
                b.nframes.push(pr.frames.len() as u32);
                b.b0.push(pr.b0_start_fx);
                b.nsteps.push(pr.n_steps as u32);
                b.norm.push(pr.norm);
            }
        }
        if let Some(offsets) = host {
            offsets.clear();
            offsets.resize(b.total, 0);
            let mut rest = offsets.as_mut_slice();
            let mut parts = Vec::with_capacity(b.off_start.len());
            for pr in jobs.iter().flat_map(|j| &j.events) {
                let (head, tail) = rest.split_at_mut(pr.offsets.len());
                parts.push((head, &pr.offsets));
                rest = tail;
            }
            parts.into_par_iter().for_each(|(dst, src)| dst.copy_from_slice(src));
        }
        b
    }
}
