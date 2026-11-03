//! The contract between the web app and whatever actually locates a recording.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What the user asked for. `audio` and `out_dir` are server-owned paths inside the job directory.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LocateRequest {
    pub audio: PathBuf,
    /// Artifacts (result.html, result.geojson, result.json) must be written here.
    pub out_dir: PathBuf,
    /// Recording start as entered: ISO 8601 with an explicit UTC offset, e.g. `2026-05-01T14:03:00-07:00`.
    pub start: Option<String>,
    /// Half-width of the start-time window, seconds.
    pub tolerance_s: Option<f64>,
    /// Place name or `lat, lon`.
    pub near: Option<String>,
    #[serde(default)]
    pub options: LocateOptions,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LocateOptions {
    pub radius_km: Option<f64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Found,
    NeedsInput,
    Inconclusive,
    Error,
}

impl Status {
    /// Mirrors `dopplerprint locate`'s exit codes.
    pub fn exit_code(self) -> i32 {
        match self {
            Status::Found => 0,
            Status::Error => 1,
            Status::NeedsInput => 2,
            Status::Inconclusive => 3,
        }
    }

    pub fn from_exit_code(code: i32) -> Self {
        match code {
            0 => Status::Found,
            2 => Status::NeedsInput,
            3 => Status::Inconclusive,
            _ => Status::Error,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Progress {
    /// Short machine-ish stage name, e.g. `audio`, `tracks`, `search`.
    pub stage: String,
    /// Overall completion 0..=1 when known.
    pub fraction: Option<f64>,
    /// One human sentence shown under the progress bar.
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    /// Radius of the 90% region, metres.
    pub radius_m: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Aircraft {
    /// Callsign, registration or hex — whatever identifies it best.
    pub label: String,
    pub type_code: Option<String>,
    /// Closest approach, seconds into the recording.
    pub t_audio_s: Option<f64>,
    pub jet: bool,
}

/// Display-ready digest of a result; the raw result JSON is kept alongside for download.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub best: Option<Location>,
    /// Human label such as "high", "medium", "low".
    pub confidence: Option<String>,
    /// Mirror / along-track ambiguity explained in one sentence; `None` when unambiguous.
    pub ambiguity: Option<String>,
    /// Second-best mode (typically the mirror image across the flight path).
    pub alternative: Option<Location>,
    pub aircraft: Vec<Aircraft>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LocateOutcome {
    pub status: Status,
    /// Shown to the user verbatim; must be plain language.
    pub message: String,
    /// Form fields the user should fill for a `NeedsInput` retry: `start`, `near`.
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub summary: Summary,
    /// The CLI's result JSON, passed through unchanged.
    pub result: Option<serde_json::Value>,
    pub html: Option<PathBuf>,
    pub geojson: Option<PathBuf>,
    pub json: Option<PathBuf>,
}

impl LocateOutcome {
    pub fn error(message: impl Into<String>) -> Self {
        Self::bare(Status::Error, message)
    }

    pub fn bare(status: Status, message: impl Into<String>) -> Self {
        LocateOutcome {
            status,
            message: message.into(),
            needs: Vec::new(),
            summary: Summary::default(),
            result: None,
            html: None,
            geojson: None,
            json: None,
        }
    }
}

/// Runs one locate. Called from a single worker thread, one job at a time.
/// An `Err` is reported to the user as an error outcome with the error's message.
pub trait LocateBackend: Send + Sync + 'static {
    fn locate(&self, req: LocateRequest, progress: &dyn Fn(Progress)) -> anyhow::Result<LocateOutcome>;
}
