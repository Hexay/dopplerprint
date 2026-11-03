//! On-disk job directory: `<jobs>/<id>/{job.json, audio.<ext>, results/}`. Disk is the source of truth.

use crate::backend::{LocateOutcome, Progress};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct JobForm {
    pub start: Option<String>,
    pub tolerance_s: Option<f64>,
    pub near: Option<String>,
    pub radius_km: Option<f64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    Running,
    Done,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub created: u64,
    pub updated: u64,
    /// Sanitised original name, display only.
    pub file_name: String,
    /// File name of the stored audio inside the job dir.
    pub audio: String,
    pub bytes: u64,
    pub form: JobForm,
    pub state: State,
    #[serde(default)]
    pub events: Vec<Progress>,
    /// Artifact paths are relative to the job dir.
    pub outcome: Option<LocateOutcome>,
}

pub struct Store {
    root: PathBuf,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Ids are `<12 hex unix ms>-<6 hex random>`: sortable by creation and free of path syntax.
pub fn valid_id(id: &str) -> bool {
    id.len() == 19
        && id.bytes().enumerate().all(|(i, b)| if i == 12 { b == b'-' } else { b.is_ascii_digit() || (b'a'..=b'f').contains(&b) })
}

fn new_id() -> String {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(t.as_nanos());
    format!("{:012x}-{:06x}", t.as_millis() as u64 & 0xffff_ffff_ffff, h.finish() & 0xff_ffff)
}

impl Store {
    pub fn open(root: &Path) -> Result<Store> {
        std::fs::create_dir_all(root).with_context(|| format!("creating jobs dir {}", root.display()))?;
        Ok(Store { root: root.to_path_buf() })
    }

    /// Callers must have checked `valid_id`.
    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Reserves a fresh, empty job directory.
    pub fn reserve(&self) -> Result<(String, PathBuf)> {
        loop {
            let id = new_id();
            let dir = self.dir(&id);
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).context("creating job dir"),
            }
        }
    }

    pub fn discard(&self, id: &str) {
        if valid_id(id) {
            let _ = std::fs::remove_dir_all(self.dir(id));
        }
    }

    pub fn save(&self, job: &Job) -> Result<()> {
        let dir = self.dir(&job.id);
        let tmp = dir.join("job.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(job)?)?;
        std::fs::rename(&tmp, dir.join("job.json"))?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Option<Job> {
        if !valid_id(id) {
            return None;
        }
        let bytes = std::fs::read(self.dir(id).join("job.json")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Newest first.
    pub fn list(&self, limit: usize) -> Vec<Job> {
        let mut ids: Vec<String> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_id(n))
            .collect();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        ids.iter().filter_map(|id| self.load(id)).take(limit).collect()
    }

    /// After a restart: returns queued ids oldest first, and fails jobs that were mid-run.
    pub fn recover(&self) -> Result<Vec<String>> {
        let mut queued = Vec::new();
        for mut job in self.list(usize::MAX).into_iter().rev() {
            match job.state {
                State::Queued => queued.push(job.id),
                State::Running => {
                    job.state = State::Done;
                    job.updated = now();
                    job.outcome = Some(LocateOutcome::error(
                        "The app was closed while this recording was being analysed. Use \"Run again\" to restart it.",
                    ));
                    self.save(&job)?;
                }
                State::Done => {}
            }
        }
        Ok(queued)
    }

    /// Absolute path of a named artifact (`html`, `geojson`, `json`) of a finished job, if it stays
    /// inside the job dir.
    pub fn artifact(&self, job: &Job, kind: &str) -> Option<PathBuf> {
        let o = job.outcome.as_ref()?;
        let rel = match kind {
            "html" => o.html.as_ref(),
            "geojson" => o.geojson.as_ref(),
            "json" => o.json.as_ref(),
            _ => None,
        }?;
        let dir = self.dir(&job.id).canonicalize().ok()?;
        let path = dir.join(rel).canonicalize().ok()?;
        path.starts_with(&dir).then_some(path)
    }
}
