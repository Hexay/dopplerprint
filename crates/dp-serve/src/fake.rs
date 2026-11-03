//! Canned backend for tests and UI demos. No computation; outcomes are picked from the request.

use crate::backend::*;
use std::time::Duration;

/// - no `start` → `NeedsInput`
/// - `near` = "nowhere" → `Inconclusive`
/// - `near` = "explode" → `Err`
/// - otherwise `Found` near Livermore, CA with the three artifacts written to `out_dir`.
pub struct FakeBackend {
    pub step_delay: Duration,
}

impl Default for FakeBackend {
    fn default() -> Self {
        FakeBackend { step_delay: Duration::from_millis(700) }
    }
}

const STAGES: [(&str, &str); 4] = [
    ("audio", "Listening for aircraft in the recording"),
    ("tracks", "Loading flight tracks for that time and area"),
    ("search", "Searching the ground for the best match"),
    ("confidence", "Checking how sure we can be"),
];

impl LocateBackend for FakeBackend {
    fn locate(&self, req: LocateRequest, progress: &dyn Fn(Progress)) -> anyhow::Result<LocateOutcome> {
        if req.start.is_none() {
            let mut o = LocateOutcome::bare(
                Status::NeedsInput,
                "We need to know when the recording started to look up the flights overhead. \
                 Add the date and time, even if it is only roughly right, and widen the ± window.",
            );
            o.needs = vec!["start".into()];
            return Ok(o);
        }
        let near = req.near.as_deref().unwrap_or("");
        for (i, (stage, message)) in STAGES.iter().enumerate() {
            progress(Progress {
                stage: (*stage).into(),
                fraction: Some(i as f64 / STAGES.len() as f64),
                message: (*message).into(),
            });
            std::thread::sleep(self.step_delay);
        }
        match near {
            "explode" => anyhow::bail!("The fake backend was asked to fail."),
            "nowhere" => {
                return Ok(LocateOutcome::bare(
                    Status::Inconclusive,
                    "Two aircraft were heard, but several places fit them about equally well.",
                ));
            }
            _ => {}
        }
        std::fs::create_dir_all(&req.out_dir)?;
        let (lat, lon) = (37.6929, -121.8150);
        let result = serde_json::json!({ "best": { "lat": lat, "lon": lon, "radius_m": 420.0 }, "fake": true });
        let json = req.out_dir.join("result.json");
        let geojson = req.out_dir.join("result.geojson");
        let html = req.out_dir.join("result.html");
        std::fs::write(&json, serde_json::to_vec_pretty(&result)?)?;
        std::fs::write(&geojson, geojson_point(lat, lon))?;
        std::fs::write(&html, FAKE_VIEWER)?;
        Ok(LocateOutcome {
            status: Status::Found,
            message: "Found a likely recording location from 2 aircraft.".into(),
            needs: Vec::new(),
            summary: Summary {
                best: Some(Location { lat, lon, radius_m: Some(420.0) }),
                confidence: Some("medium".into()),
                ambiguity: Some(
                    "One aircraft can't tell which side of its path you were on; the second one mostly settles it."
                        .into(),
                ),
                alternative: Some(Location { lat: 37.7012, lon: -121.8233, radius_m: Some(600.0) }),
                aircraft: vec![
                    Aircraft { label: "N9658K".into(), type_code: Some("P28A".into()), t_audio_s: Some(41.0), jet: false },
                    Aircraft { label: "SKW5521".into(), type_code: Some("CRJ9".into()), t_audio_s: Some(97.5), jet: true },
                ],
            },
            result: Some(result),
            html: Some(html),
            geojson: Some(geojson),
            json: Some(json),
        })
    }
}

fn geojson_point(lat: f64, lon: f64) -> String {
    serde_json::json!({
        "type": "FeatureCollection",
        "features": [{ "type": "Feature", "properties": { "kind": "best" },
                       "geometry": { "type": "Point", "coordinates": [lon, lat] } }]
    })
    .to_string()
}

const FAKE_VIEWER: &str = "<!doctype html><meta charset=utf-8><title>Result</title>\
<body style=\"margin:0;font:16px system-ui;display:grid;place-items:center;height:100vh;background:#eef3f1\">\
<p>Fake result viewer. The real backend writes its map here.</p>";
