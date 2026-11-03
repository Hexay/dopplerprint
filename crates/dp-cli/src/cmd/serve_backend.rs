//! `dopplerprint serve`'s real backend: one `dp_core::app::locate` call per job, its report mapped to the page.

use std::cell::Cell;
use std::path::PathBuf;

use dp_core::app::{self, Report, Ui};
use dp_serve::{Aircraft, LocateBackend, LocateOutcome, LocateRequest, Location, Progress, Status, Summary};

pub struct AppBackend {
    /// Stands in for the CLI's y/N download question, which the page can't ask.
    pub allow_download: bool,
    pub cache: Option<PathBuf>,
    pub max_cache_gb: f64,
    pub config: Option<PathBuf>,
}

impl LocateBackend for AppBackend {
    fn locate(&self, req: LocateRequest, progress: &dyn Fn(Progress)) -> anyhow::Result<LocateOutcome> {
        let app_req = app::LocateRequest {
            audio: req.audio.clone(),
            start: req.start.clone(),
            start_tolerance: req.tolerance_s.map(|t| format!("{t:.0}s")),
            near: req.near.clone(),
            radius_km: req.options.radius_km,
            cache: self.cache.clone(),
            out: Some(req.out_dir.clone()),
            config: self.config.clone(),
            max_cache_gb: self.max_cache_gb,
            ..app::LocateRequest::default()
        };
        let ui = PageUi { progress, allow_download: self.allow_download, refused: Cell::new(false), fraction: Cell::new(None), pct: Cell::new(u64::MAX) };
        let rep = app::locate(&app_req, &ui)?;
        let needs = app::missing_fields(&app_req, &rep);
        let mut o = LocateOutcome::bare(status(&rep), message(&rep, &req, &needs, ui.refused.get()));
        o.needs = needs.iter().map(|s| s.to_string()).collect();
        o.summary = summary(&rep);
        if !rep.files.is_empty() {
            let file = |name: &str| Some(req.out_dir.join(name)).filter(|p| p.is_file());
            (o.html, o.geojson, o.json) = (file("result.html"), file("result.geojson"), file("result.json"));
        }
        if o.json.is_none() {
            o.result = Some(serde_json::to_value(&rep)?);
        }
        Ok(o)
    }
}

fn status(rep: &Report) -> Status {
    match rep.status {
        app::Status::Found => Status::Found,
        app::Status::NeedsInput => Status::NeedsInput,
        app::Status::Inconclusive => Status::Inconclusive,
    }
}

fn message(rep: &Report, req: &LocateRequest, needs: &[&str], download_refused: bool) -> String {
    if download_refused {
        return "The flight data for this time isn't downloaded yet, and this app was started with downloads turned off. \
                Restart it without --allow-download false to fetch it."
            .into();
    }
    let blank = |field: &Option<String>| field.as_deref().is_none_or(|s| s.trim().is_empty());
    let asks: Vec<&str> = needs
        .iter()
        .filter_map(|&n| match n {
            "start" if blank(&req.start) => Some("when the recording started (a rough time is fine; widen \"give or take\" to match)"),
            "near" if blank(&req.near) => Some("roughly where it was made (a town, an address or coordinates)"),
            _ => None,
        })
        .collect();
    if !asks.is_empty() {
        return format!("Please add {}.", asks.join(" and "));
    }
    match (&rep.message, rep.status) {
        (Some(m), _) => plain(m),
        (None, app::Status::Found) => {
            let n = rep.aircraft.len();
            format!("Found where this was probably recorded, from {n} aircraft pass{}.", if n == 1 { "" } else { "es" })
        }
        (None, _) => "No location could be settled on.".into(),
    }
}

/// App messages are written for the terminal; name the page's fields instead of the CLI flags.
fn plain(msg: &str) -> String {
    let s = msg.replace("--start", "the start time").replace("--near", "the place");
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

fn summary(rep: &Report) -> Summary {
    let alt = rep.alternatives.first();
    Summary {
        best: rep.location.as_ref().map(|l| Location { lat: l.lat, lon: l.lon, radius_m: None }),
        confidence: rep.confidence.as_ref().map(|c| c.level.clone()),
        ambiguity: alt.map(|a| {
            let at = format!("{:.1} km {}", a.distance_km, a.direction);
            if a.same_aircraft {
                format!("The same aircraft fit a spot {at} of here almost as well: a single pass can't tell which side of its path you were on.")
            } else {
                format!("A spot {at} of here, matched to different aircraft, fits almost as well.")
            }
        }),
        alternative: alt.map(|a| Location { lat: a.lat, lon: a.lon, radius_m: None }),
        aircraft: rep
            .aircraft
            .iter()
            .map(|m| Aircraft {
                label: m.registration.clone().unwrap_or_else(|| m.icao24.clone()),
                type_code: m.type_code.clone(),
                t_audio_s: Some(m.t_audio_s),
                jet: m.jet,
            })
            .collect(),
    }
}

struct PageUi<'a> {
    progress: &'a dyn Fn(Progress),
    allow_download: bool,
    refused: Cell<bool>,
    fraction: Cell<Option<f64>>,
    pct: Cell<u64>,
}

impl PageUi<'_> {
    fn send(&self, stage: &str, message: &str) {
        let message = message.trim().trim_end_matches('.').to_string();
        (self.progress)(Progress { stage: stage.into(), fraction: self.fraction.get(), message });
    }
}

impl Ui for PageUi<'_> {
    fn info(&self, msg: &str) {
        self.send("note", msg);
    }

    fn step(&self, stage: &str, fraction: f64, msg: &str) {
        self.fraction.set(Some(fraction));
        self.send(stage, msg);
    }

    fn confirm(&self, _: &str) -> bool {
        self.refused.set(!self.allow_download);
        self.allow_download
    }

    // Each call is saved to job.json, so only whole-percent changes are forwarded.
    fn progress(&self, label: &str, done: u64, total: u64) {
        if let Some(pct) = (done * 100).checked_div(total)
            && pct != self.pct.replace(pct)
        {
            self.send("download", &format!("Downloading flight data, {label}: {pct}% of {:.2} GB", total as f64 / 1e9));
        }
    }
}
