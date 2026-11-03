//! Routing and handlers. Every API error is `{"error": "<human sentence>"}` with a 4xx/5xx status.
//!
//! - `GET  /api/info` → `{max_upload_bytes, version}`
//! - `GET  /api/jobs` → `{jobs: [Job]}`, newest 20
//! - `POST /api/jobs` raw audio body, `X-Filename`, `X-Locate: <percent-encoded JobForm JSON>` → 202 Job
//! - `GET  /api/jobs/{id}` → Job (job.json plus `files: {html,geojson,json → url}` and `ahead`)
//! - `POST /api/jobs/{id}/rerun` JobForm JSON → 202 Job, a new job on the same audio
//! - `GET  /api/jobs/{id}/files/{html|geojson|json}`

use crate::jobs::{Job, JobForm, Queue, State, Store, now};
use crate::assets;
use crate::upload::{UploadError, display_name, percent_decode, save_audio};
use serde_json::{Value, json};
use std::io::Read;
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response};

type Resp = Response<Box<dyn Read + Send>>;

pub struct Ctx {
    pub store: Arc<Store>,
    pub queue: Arc<Queue>,
    pub max_upload: u64,
    /// Accepted `Host` header values; anything else is refused (DNS-rebinding guard). `None` when
    /// bound to all interfaces, where the LAN address is unknowable.
    pub hosts: Option<Vec<String>>,
}

pub fn handle(ctx: &Ctx, mut req: Request) {
    let resp = route(ctx, &mut req);
    let _ = req.respond(resp);
}

fn route(ctx: &Ctx, req: &mut Request) -> Resp {
    let host = header(req, "Host").unwrap_or_default().to_ascii_lowercase();
    if ctx.hosts.as_ref().is_some_and(|hs| !hs.contains(&host)) {
        return error(421, "This app only answers on the address it was started with.");
    }
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    if method == Method::Get
        && let Some((body, ctype)) = assets::get(path.trim_start_matches('/'))
    {
        return Response::from_data(body.as_bytes())
            .with_header(h("Content-Type", ctype))
            .with_header(h("Content-Security-Policy", assets::APP_CSP))
            .with_header(h("X-Content-Type-Options", "nosniff"))
            .boxed();
    }
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (method, parts.as_slice()) {
        (Method::Get, ["api", "info"]) => ok(json!({ "max_upload_bytes": ctx.max_upload, "version": env!("CARGO_PKG_VERSION") })),
        (Method::Get, ["api", "jobs"]) => ok(json!({ "jobs": ctx.store.list(20).iter().map(|j| view(ctx, j)).collect::<Vec<_>>() })),
        (Method::Post, ["api", "jobs"]) => create(ctx, req),
        (Method::Get, ["api", "jobs", id]) => match ctx.store.load(id) {
            Some(job) => ok(view(ctx, &job)),
            None => not_found(),
        },
        (Method::Post, ["api", "jobs", id, "rerun"]) => rerun(ctx, req, id),
        (Method::Get, ["api", "jobs", id, "files", kind]) => file(ctx, id, kind),
        (Method::Get | Method::Post, _) => not_found(),
        _ => error(405, "Method not allowed."),
    }
}

/// Body is the raw audio; the form travels as percent-encoded JSON in `X-Locate`. Custom headers
/// force a CORS preflight, which we never grant, so other sites can't post here.
fn create(ctx: &Ctx, req: &mut Request) -> Resp {
    let form = match header(req, "X-Locate").map(|s| parse_form(&percent_decode(&s))).unwrap_or(Err("Missing request details.".into())) {
        Ok(f) => f,
        Err(e) => return error(400, &e),
    };
    if req.body_length().is_some_and(|n| n as u64 > ctx.max_upload) {
        return too_large(ctx);
    }
    let file_name = display_name(&header(req, "X-Filename").unwrap_or_default());
    let (id, dir) = match ctx.store.reserve() {
        Ok(r) => r,
        Err(e) => return error(500, &format!("Couldn't create a job folder: {e:#}")),
    };
    let (audio, bytes) = match save_audio(req.as_reader(), ctx.max_upload, &dir) {
        Ok(r) => r,
        Err(e) => {
            ctx.store.discard(&id);
            return match e {
                UploadError::TooLarge => too_large(ctx),
                UploadError::Empty => error(400, "The upload was empty."),
                UploadError::NotAudio => error(
                    415,
                    "That doesn't look like an audio file. Try WAV, MP3, FLAC, OGG or M4A (from a phone's voice recorder).",
                ),
                UploadError::Io(e) => error(500, &format!("Saving the upload failed: {e}")),
            };
        }
    };
    enqueue(ctx, Job { id, created: 0, updated: 0, file_name, audio, bytes, form, state: State::Queued, events: vec![], outcome: None })
}

/// Same audio, new answers (typically after a needs-input result). Requires a JSON content type,
/// which likewise forces a preflight.
fn rerun(ctx: &Ctx, req: &mut Request, id: &str) -> Resp {
    if !header(req, "Content-Type").is_some_and(|c| c.starts_with("application/json")) {
        return error(415, "Expected JSON.");
    }
    let Some(prev) = ctx.store.load(id) else { return not_found() };
    let mut body = String::new();
    if req.as_reader().take(64 * 1024).read_to_string(&mut body).is_err() {
        return error(400, "Couldn't read the request.");
    }
    let form = match parse_form(&body) {
        Ok(f) => f,
        Err(e) => return error(400, &e),
    };
    let (new_id, dir) = match ctx.store.reserve() {
        Ok(r) => r,
        Err(e) => return error(500, &format!("Couldn't create a job folder: {e:#}")),
    };
    let src = ctx.store.dir(&prev.id).join(&prev.audio);
    let dst = dir.join(&prev.audio);
    if std::fs::hard_link(&src, &dst).or_else(|_| std::fs::copy(&src, &dst).map(|_| ())).is_err() {
        ctx.store.discard(&new_id);
        return error(410, "The original recording is no longer available. Please upload it again.");
    }
    enqueue(ctx, Job { id: new_id, state: State::Queued, events: vec![], outcome: None, form, ..prev })
}

fn enqueue(ctx: &Ctx, mut job: Job) -> Resp {
    job.created = now();
    job.updated = job.created;
    if let Err(e) = ctx.store.save(&job) {
        ctx.store.discard(&job.id);
        return error(500, &format!("Couldn't save the job: {e:#}"));
    }
    ctx.queue.push(job.id.clone());
    respond_json(202, &view(ctx, &job))
}

fn file(ctx: &Ctx, id: &str, kind: &str) -> Resp {
    let Some(job) = ctx.store.load(id) else { return not_found() };
    let Some(path) = ctx.store.artifact(&job, kind) else { return not_found() };
    let Ok(f) = std::fs::File::open(&path) else { return not_found() };
    let ctype = match kind {
        "html" => "text/html; charset=utf-8",
        "geojson" => "application/geo+json",
        _ => "application/json",
    };
    let resp = Response::from_file(f).with_header(h("Content-Type", ctype)).boxed();
    // The viewer may load map tiles and scripts, but must never reach this API with our origin.
    if kind == "html" { resp.with_header(h("Content-Security-Policy", "sandbox allow-scripts allow-popups")) } else { resp }
}

fn parse_form(raw: &str) -> Result<JobForm, String> {
    let mut f: JobForm = serde_json::from_str(raw).map_err(|_| "The request details weren't valid JSON.".to_string())?;
    let clean = |s: Option<String>, max: usize| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).map(|s| s.chars().take(max).collect());
    f.start = clean(f.start, 64);
    f.near = clean(f.near, 200);
    if f.tolerance_s.is_some_and(|t| !(0.0..=7.0 * 86400.0).contains(&t)) {
        return Err("The time window must be between 0 seconds and 7 days.".into());
    }
    if f.radius_km.is_some_and(|r| !(0.1..=500.0).contains(&r)) {
        return Err("The search radius must be between 0.1 and 500 km.".into());
    }
    Ok(f)
}

fn view(ctx: &Ctx, job: &Job) -> Value {
    let mut v = serde_json::to_value(job).unwrap_or(Value::Null);
    let files: serde_json::Map<String, Value> = ["html", "geojson", "json"]
        .into_iter()
        .filter(|k| ctx.store.artifact(job, k).is_some())
        .map(|k| (k.to_string(), json!(format!("/api/jobs/{}/files/{k}", job.id))))
        .collect();
    v["files"] = Value::Object(files);
    v["ahead"] = json!(ctx.queue.ahead_of(&job.id));
    if let Some(o) = v.get_mut("outcome").and_then(Value::as_object_mut) {
        for k in ["html", "geojson", "json", "result"] {
            o.remove(k);
        }
    }
    v
}

fn header(req: &Request, name: &str) -> Option<String> {
    req.headers().iter().find(|x| x.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|x| x.value.as_str().to_string())
}

fn h(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header")
}

fn respond_json(code: u16, v: &Value) -> Resp {
    Response::from_data(v.to_string().into_bytes())
        .with_status_code(code)
        .with_header(h("Content-Type", "application/json"))
        .with_header(h("Cache-Control", "no-store"))
        .boxed()
}

fn ok(v: Value) -> Resp {
    respond_json(200, &v)
}

fn error(code: u16, msg: &str) -> Resp {
    respond_json(code, &json!({ "error": msg }))
}

fn not_found() -> Resp {
    error(404, "Not found.")
}

fn too_large(ctx: &Ctx) -> Resp {
    let mb = ctx.max_upload / (1024 * 1024);
    error(413, &format!("That file is too large. The limit is {mb} MB; trim the recording to the part with aircraft."))
}
