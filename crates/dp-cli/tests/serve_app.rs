//! `dopplerprint serve` with the real backend, offline, against dp-core's locate fixture.

#[path = "../../dp-core/tests/common/locate_fixture.rs"]
mod locate_fixture;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use dp_cli::AppBackend;
use dp_serve::{Handle, ServeArgs};
use locate_fixture::{Fixture, local_edt};
use serde_json::{Value, json};

/// GET without a body, POST with one.
fn request(h: &Handle, path: &str, extra: &[(&str, &str)], body: &[u8]) -> (u16, Vec<u8>) {
    let method = if body.is_empty() { "GET" } else { "POST" };
    let mut head = format!("{method} {path} HTTP/1.0\r\nHost: {}\r\nContent-Length: {}\r\n", h.addr, body.len());
    for (k, v) in extra {
        head += &format!("{k}: {v}\r\n");
    }
    let mut s = TcpStream::connect(h.addr).unwrap();
    s.write_all(format!("{head}\r\n").as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("no response head");
    let status = String::from_utf8_lossy(&raw[..split]).split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, raw[split + 4..].to_vec())
}

fn json_of(b: &[u8]) -> Value {
    serde_json::from_slice(b).unwrap()
}

#[test]
fn real_backend_finds_the_fixture_and_serves_its_files() {
    let fx = Fixture::build();
    let args = ServeArgs { host: "127.0.0.1".into(), port: 0, open: false, jobs: Some(fx.tmp.path().join("jobs")), max_upload_mb: 64 };
    let backend = AppBackend {
        allow_download: false,
        cache: Some(fx.cache.clone()),
        max_cache_gb: 20.0,
        config: Some(fx.config.clone()),
    };
    let h = dp_serve::start(&args, Box::new(backend)).unwrap();

    // The page sends ISO 8601 with an offset.
    let start = local_edt(fx.start + 20.0).replacen(' ', "T", 1).replace(" -", "-");
    let form = json!({ "start": start, "tolerance_s": 120, "near": fx.near() }).to_string();
    let enc: String = form.bytes().map(|b| format!("%{b:02X}")).collect();
    let wav = std::fs::read(&fx.audio).unwrap();
    let (code, body) = request(&h,"/api/jobs", &[("X-Filename", "rec.wav"), ("X-Locate", &enc), ("Content-Type", "application/octet-stream")], &wav);
    assert_eq!(code, 202, "{}", String::from_utf8_lossy(&body));
    let id = json_of(&body)["id"].as_str().unwrap().to_string();

    let t0 = Instant::now();
    let job = loop {
        let j = json_of(&request(&h,&format!("/api/jobs/{id}"), &[], b"").1);
        if j["state"] == "done" {
            break j;
        }
        assert!(t0.elapsed() < Duration::from_secs(300), "job never finished: {j}");
        std::thread::sleep(Duration::from_millis(100));
    };
    let o = &job["outcome"];
    assert_eq!(o["status"], "found", "{o}");
    assert_eq!(o["summary"]["aircraft"][0]["label"], "N1134F");
    assert!(o["summary"]["confidence"].is_string() && o["summary"]["best"]["lat"].is_f64(), "{o}");
    assert!(job["events"].as_array().unwrap().iter().any(|e| e["stage"] == "search"), "{job}");
    for kind in ["html", "geojson", "json"] {
        let (code, body) = request(&h,&format!("/api/jobs/{id}/files/{kind}"), &[], b"");
        assert!(code == 200 && !body.is_empty(), "{kind}: {code}");
    }
    h.stop();
}
