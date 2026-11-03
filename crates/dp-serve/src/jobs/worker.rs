//! The one worker thread: runs queued jobs through the backend, one at a time.

use super::{Job, Queue, State, Store, now};
use crate::backend::{LocateBackend, LocateOptions, LocateOutcome, LocateRequest, Progress};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn spawn_worker(store: Arc<Store>, queue: Arc<Queue>, backend: Arc<dyn LocateBackend>) {
    std::thread::Builder::new()
        .name("dp-serve-worker".into())
        .spawn(move || {
            loop {
                let id = queue.pop_blocking();
                if let Some(job) = store.load(&id)
                    && let Err(e) = run_job(&store, backend.as_ref(), job)
                {
                    eprintln!("job {id}: {e:#}");
                }
            }
        })
        .expect("spawning worker thread");
}

fn run_job(store: &Store, backend: &dyn LocateBackend, mut job: Job) -> anyhow::Result<()> {
    let dir = store.dir(&job.id);
    job.state = State::Running;
    job.updated = now();
    store.save(&job)?;

    let req = LocateRequest {
        audio: dir.join(&job.audio),
        out_dir: dir.join("results"),
        start: job.form.start.clone(),
        tolerance_s: job.form.tolerance_s,
        near: job.form.near.clone(),
        options: LocateOptions { radius_km: job.form.radius_km },
    };
    let cell = RefCell::new(job);
    let on_progress = |p: Progress| {
        let mut j = cell.borrow_mut();
        j.events.push(p);
        j.updated = now();
        if let Err(e) = store.save(&j) {
            eprintln!("job {}: saving progress: {e:#}", j.id);
        }
    };
    let outcome = match catch_unwind(AssertUnwindSafe(|| backend.locate(req, &on_progress))) {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => LocateOutcome::error(format!("{e:#}")),
        Err(_) => LocateOutcome::error("The analysis crashed unexpectedly. Please report this recording."),
    };
    let mut job = cell.into_inner();
    job.outcome = Some(store_artifacts(&dir, outcome));
    job.state = State::Done;
    job.updated = now();
    store.save(&job)
}

/// Makes artifact paths job-relative, copying in any the backend wrote elsewhere, and moves the
/// inline result JSON to a file so job.json stays small.
fn store_artifacts(dir: &Path, mut o: LocateOutcome) -> LocateOutcome {
    let results = dir.join("results");
    if let Some(value) = o.result.take()
        && o.json.is_none()
    {
        let path = results.join("result.json");
        let written = std::fs::create_dir_all(&results)
            .and_then(|_| std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap_or_default()));
        if written.is_ok() {
            o.json = Some(path);
        }
    }
    for (slot, name) in [(&mut o.html, "result.html"), (&mut o.geojson, "result.geojson"), (&mut o.json, "result.json")] {
        *slot = slot.take().and_then(|p| relativize(dir, &results, &p, name));
    }
    o
}

fn relativize(dir: &Path, results: &Path, path: &Path, name: &str) -> Option<PathBuf> {
    let dir_c = dir.canonicalize().ok()?;
    let abs = if path.is_absolute() { path.to_path_buf() } else { dir.join(path) }.canonicalize().ok()?;
    if let Ok(rel) = abs.strip_prefix(&dir_c) {
        return Some(rel.to_path_buf());
    }
    std::fs::create_dir_all(results).ok()?;
    std::fs::copy(&abs, results.join(name)).ok()?;
    Some(Path::new("results").join(name))
}
