use dp_serve::{FakeBackend, Handle, ServeArgs};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\0\0\0\0";
const START: &str = "2026-05-01T14:03:00-07:00";

fn serve(jobs: &Path) -> Handle {
    let args = ServeArgs { host: "127.0.0.1".into(), port: 0, open: false, jobs: Some(jobs.into()), max_upload_mb: 1 };
    dp_serve::start(&args, Box::new(FakeBackend { step_delay: Duration::ZERO })).unwrap()
}

struct Resp {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

fn request(h: &Handle, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Resp {
    let host = h.addr.to_string();
    let mut s = TcpStream::connect(h.addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut head = format!("{method} {path} HTTP/1.0\r\nContent-Length: {}\r\n", body.len());
    if !headers.iter().any(|(k, _)| *k == "Host") {
        head += &format!("Host: {host}\r\n");
    }
    for (k, v) in headers {
        head += &format!("{k}: {v}\r\n");
    }
    head += "\r\n";
    // The server may answer and close before reading the whole body; that's what we test for.
    let _ = s.write_all(head.as_bytes()).and_then(|_| s.write_all(body));
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("no response head");
    let headers = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    Resp { status, headers, body: raw[split + 4..].to_vec() }
}

fn upload(h: &Handle, name: &str, form: Value, body: &[u8]) -> Resp {
    let form = form.to_string();
    let enc: String = form.bytes().map(|b| format!("%{b:02X}")).collect();
    request(h, "POST", "/api/jobs", &[("X-Filename", name), ("X-Locate", &enc), ("Content-Type", "application/octet-stream")], body)
}

fn wait_done(h: &Handle, id: &str) -> Value {
    let t0 = Instant::now();
    loop {
        let j = request(h, "GET", &format!("/api/jobs/{id}"), &[], b"").json();
        if j["state"] == "done" {
            return j;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "job {id} never finished: {j}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn job_dirs(jobs: &Path) -> usize {
    std::fs::read_dir(jobs).unwrap().count()
}

#[test]
fn upload_runs_to_a_found_result_with_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let h = serve(tmp.path());
    assert!(request(&h, "GET", "/", &[], b"").body.starts_with(b"<!doctype html>"));

    let r = upload(&h, "walk.wav", json!({ "start": START, "tolerance_s": 60, "near": "Livermore, CA" }), WAV);
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    let id = r.json()["id"].as_str().unwrap().to_string();
    let job = wait_done(&h, &id);
    assert_eq!(job["outcome"]["status"], "found");
    assert_eq!(job["file_name"], "walk.wav");
    assert_eq!(job["form"]["start"], START);
    assert!(job["outcome"]["summary"]["best"]["lat"].is_f64());
    assert_eq!(job["events"].as_array().unwrap().len(), 4);

    let html = request(&h, "GET", job["files"]["html"].as_str().unwrap(), &[], b"");
    assert_eq!(html.status, 200);
    assert!(html.headers.contains("sandbox"), "result viewer must be sandboxed");
    let geo = request(&h, "GET", job["files"]["geojson"].as_str().unwrap(), &[], b"").json();
    assert_eq!(geo["type"], "FeatureCollection");

    let list = request(&h, "GET", "/api/jobs", &[], b"").json();
    assert_eq!(list["jobs"][0]["id"], id.as_str());
    h.stop();
}

#[test]
fn needs_input_then_rerun_reuses_the_upload() {
    let tmp = tempfile::tempdir().unwrap();
    let h = serve(tmp.path());
    let id = upload(&h, "walk.wav", json!({ "near": "Livermore" }), WAV).json()["id"].as_str().unwrap().to_string();
    let job = wait_done(&h, &id);
    assert_eq!(job["outcome"]["status"], "needs_input");
    assert_eq!(job["outcome"]["needs"], json!(["start"]));
    assert!(job["outcome"]["message"].as_str().unwrap().contains("when the recording started"));

    let body = json!({ "start": START, "tolerance_s": 300, "near": "Livermore" }).to_string();
    let no_type = request(&h, "POST", &format!("/api/jobs/{id}/rerun"), &[], body.as_bytes());
    assert_eq!(no_type.status, 415);
    let r = request(&h, "POST", &format!("/api/jobs/{id}/rerun"), &[("Content-Type", "application/json")], body.as_bytes());
    assert_eq!(r.status, 202);
    let id2 = r.json()["id"].as_str().unwrap().to_string();
    assert_ne!(id, id2);
    let job2 = wait_done(&h, &id2);
    assert_eq!(job2["outcome"]["status"], "found");
    assert_eq!(job2["file_name"], "walk.wav");
    assert!(tmp.path().join(&id2).join("audio.wav").is_file());
    h.stop();
}

#[test]
fn backend_errors_and_inconclusive_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let h = serve(tmp.path());
    for (near, status) in [("explode", "error"), ("nowhere", "inconclusive")] {
        let id = upload(&h, "a.wav", json!({ "start": START, "near": near }), WAV).json()["id"].as_str().unwrap().to_string();
        let job = wait_done(&h, &id);
        assert_eq!(job["outcome"]["status"], status);
        assert!(!job["outcome"]["message"].as_str().unwrap().is_empty());
        assert_eq!(job["files"], json!({}));
    }
    h.stop();
}

#[test]
fn bad_uploads_are_rejected_and_leave_nothing_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let h = serve(tmp.path());
    let form = json!({ "start": START });
    assert_eq!(upload(&h, "x.wav", form.clone(), b"<html><script>alert(1)</script>").status, 415);
    assert_eq!(upload(&h, "x.wav", form.clone(), b"").status, 400);
    let mut big = WAV.to_vec();
    big.resize(1024 * 1024 + 1, 0);
    assert_eq!(upload(&h, "x.wav", form.clone(), &big).status, 413);
    assert_eq!(upload(&h, "x.wav", json!({ "start": START, "tolerance_s": -5 }), WAV).status, 400);
    assert_eq!(request(&h, "POST", "/api/jobs", &[("X-Locate", "not json")], WAV).status, 400);
    assert_eq!(job_dirs(tmp.path()), 0);
    h.stop();
}

#[test]
fn path_traversal_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let jobs = tmp.path().join("jobs");
    let h = serve(&jobs);
    let r = upload(&h, "..%2F..%2F..%2Fevil.wav", json!({ "start": START }), WAV);
    assert_eq!(r.status, 202);
    let j = r.json();
    assert_eq!(j["file_name"], "evil.wav");
    assert_eq!(j["audio"], "audio.wav");
    assert!(!tmp.path().join("evil.wav").exists());
    let id = j["id"].as_str().unwrap();
    wait_done(&h, id);

    for path in [
        "/api/jobs/..%2F..%2Fetc",
        "/api/jobs/../jobs",
        &format!("/api/jobs/{id}/files/..%2Fjob.json"),
        &format!("/api/jobs/{id}/files/..%2F..%2F..%2Fetc%2Fpasswd"),
        &format!("/api/jobs/{id}/files/audio"),
        "/api/jobs/..%2F..%2Fetc/files/html",
        "/../../etc/passwd",
    ] {
        assert_eq!(request(&h, "GET", path, &[], b"").status, 404, "{path}");
    }
    let r = request(&h, "POST", "/api/jobs/..%2F..%2Fx/rerun", &[("Content-Type", "application/json")], b"{}");
    assert_eq!(r.status, 404);
    h.stop();
}

#[test]
fn foreign_host_header_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let h = serve(tmp.path());
    assert_eq!(request(&h, "GET", "/api/jobs", &[("Host", "evil.example:80")], b"").status, 421);
    let port = h.addr.port();
    assert_eq!(request(&h, "GET", "/api/jobs", &[("Host", &format!("localhost:{port}"))], b"").status, 200);
    h.stop();
}

#[test]
fn jobs_survive_a_restart_and_interrupted_runs_fail_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let h = serve(tmp.path());
    let id = upload(&h, "a.wav", json!({ "start": START }), WAV).json()["id"].as_str().unwrap().to_string();
    wait_done(&h, &id);
    let id2 = upload(&h, "b.wav", json!({ "start": START }), WAV).json()["id"].as_str().unwrap().to_string();
    wait_done(&h, &id2);
    h.stop();

    let path = tmp.path().join(&id2).join("job.json");
    let mut j: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    j["state"] = json!("running");
    j["outcome"] = Value::Null;
    std::fs::write(&path, j.to_string()).unwrap();

    let h = serve(tmp.path());
    assert_eq!(request(&h, "GET", &format!("/api/jobs/{id}"), &[], b"").json()["outcome"]["status"], "found");
    let j2 = request(&h, "GET", &format!("/api/jobs/{id2}"), &[], b"").json();
    assert_eq!(j2["state"], "done");
    assert_eq!(j2["outcome"]["status"], "error");
    assert_eq!(request(&h, "GET", "/api/jobs", &[], b"").json()["jobs"].as_array().unwrap().len(), 2);
    h.stop();
}
