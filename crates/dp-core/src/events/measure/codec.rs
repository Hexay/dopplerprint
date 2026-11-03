//! M3: codec round trips (ffmpeg) of sim scenarios at 48 kHz → decode → discovery + locate, vs the 16-bit WAV.
//! Opus is decoded back to WAV by ffmpeg because symphonia has no Opus decoder (that failure is reported).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::{DiscoverConfig, Event, discover_audio};
use crate::audio::{self, Audio};
use crate::config::Config;
use crate::eval::case::{CaseSource, Posed, pose};
use crate::eval::run_case;
use crate::search::EventSpec;
use crate::sim::{Scenario, Truth, render, write_outputs};

const SCENARIOS: [&str; 3] = ["faint_single", "manoeuvring_turn", "two_props_920s"];
/// (label, [(encoder args, output ext)] chained, decode to wav via ffmpeg after)
const VARIANTS: [(&str, &[(&str, &str)], bool); 6] = [
    ("wav16 (baseline)", &[], false),
    ("aac 128k", &[("-c:a aac -b:a 128k", "m4a")], false),
    ("aac 128k, ffmpeg-decoded", &[("-c:a aac -b:a 128k", "m4a")], true),
    ("mp3 128k", &[("-c:a libmp3lame -b:a 128k", "mp3")], false),
    ("opus 135k", &[("-c:a libopus -b:a 135k", "opus")], true),
    ("aac→opus", &[("-c:a aac -b:a 128k", "m4a"), ("-c:a libopus -b:a 135k", "opus")], true),
];

fn ffmpeg(input: &Path, args: &str, out: &Path) {
    let st = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args(args.split_whitespace())
        .arg(out)
        .status()
        .expect("ffmpeg runs");
    assert!(st.success(), "ffmpeg {args} failed");
}

fn encode(wav: &Path, chain: &[(&str, &str)], to_wav: bool, tag: &str) -> PathBuf {
    let mut cur = wav.to_path_buf();
    for (i, (args, ext)) in chain.iter().enumerate() {
        let out = wav.with_file_name(format!("{tag}_{i}.{ext}"));
        ffmpeg(&cur, args, &out);
        cur = out;
    }
    if to_wav {
        if let Err(e) = audio::decode(&cur) {
            println!("<!-- symphonia on {}: {e:#} -->", cur.display());
        }
        let out = wav.with_file_name(format!("{tag}_dec.wav"));
        ffmpeg(&cur, "-c:a pcm_s16le", &out);
        cur = out;
    }
    cur
}

/// Lag (samples) of `b` relative to `a`: cross-correlation of first differences (pre-whitened, so the
/// harmonic tone's periodic peaks don't win) over a 1 s chunk at `at`.
fn delay(a: &[f32], b: &[f32], at: usize, max_lag: i64) -> i64 {
    let n = 48_000.min(a.len().saturating_sub(at + 1));
    let d = |x: &[f32], i: usize| x[i + 1] as f64 - x[i] as f64;
    let score = |lag: i64| -> f64 {
        (0..n)
            .filter_map(|i| {
                let j = (at + i) as i64 + lag;
                (j >= 0 && (j as usize) + 1 < b.len()).then(|| d(a, at + i) * d(b, j as usize))
            })
            .sum()
    };
    (-max_lag..=max_lag).max_by(|&x, &y| score(x).total_cmp(&score(y))).unwrap_or(0)
}

fn matched<'a>(evs: &'a [Event], t: &Truth) -> Vec<Option<&'a Event>> {
    t.events
        .iter()
        .filter(|e| e.in_recording)
        .map(|te| {
            evs.iter()
                .filter(|e| (e.fit.best.t_cpa_arrive - te.cpa_arrival_audio_s).abs() < 8.0)
                .filter(|e| (e.fit.best.f0 / e.ridge_harmonic.max(1) as f64 / te.f0 - 1.0).abs() < 0.05)
                .max_by_key(|e| e.ridge.t.len())
        })
        .collect()
}

fn locate_line(cfg: &Config, posed: &Posed) -> String {
    let r = run_case(cfg, posed);
    format!(
        "{} | {} | {} | {}",
        r.error_m.map_or("–".into(), |e| format!("{e:.0}")),
        r.truth_tuple_rank.map_or("–".into(), |x| x.to_string()),
        r.margin.map_or("–".into(), |m| format!("{m:.1}")),
        r.clock_err_s.map_or("–".into(), |c| format!("{c:+.3}")),
    )
}

#[test]
#[ignore]
fn codec_robustness() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../experiments/sim");
    let work = std::env::temp_dir().join("dp_measure_codec");
    let cfg = Config::default();
    println!("Per matched event vs the wav16 baseline fit: Δf0 ppm, Δt_cpa ms, Δv %, Δd %. Locate = oracle events | discovered events.");
    println!("| scenario | variant | delay smp | events | recall | Δf0 ppm | Δt_cpa ms | Δv % | Δd % | oracle err m | rank | margin | clock s | disc err m | rank | margin | clock s |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for name in SCENARIOS {
        let mut sc = Scenario::from_toml_str(&std::fs::read_to_string(root.join(format!("{name}.toml"))).unwrap()).unwrap();
        sc.sample_rate = 48_000;
        let dir = work.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join(format!("{name}.toml"));
        std::fs::write(&toml_path, toml::to_string(&sc).unwrap()).unwrap();
        let r = render(&sc);
        write_outputs(&r, &dir).unwrap();
        let wav = dir.join("audio.wav");
        let base = audio::decode(&wav).unwrap();
        let cpa = r.truth.events.iter().find(|e| e.in_recording).map_or(10.0, |e| e.cpa_arrival_audio_s);
        let at = ((cpa - 0.5).max(0.0) * 48_000.0) as usize;
        let mut posed = pose(&CaseSource::Sim(toml_path), &cfg.eval).unwrap();
        let mut base_fits: Vec<Option<crate::physics::cv::CvParams>> = Vec::new();
        for (label, chain, to_wav) in VARIANTS {
            let path = if chain.is_empty() { wav.clone() } else { encode(&wav, chain, to_wav, &label.replace(' ', "_")) };
            let a: Audio = audio::decode(&path).unwrap();
            let lag = delay(&base.samples, &a.samples, at, 4000);
            let evs = discover_audio(&a, &DiscoverConfig::default());
            let m = matched(&evs, &r.truth);
            if base_fits.is_empty() {
                base_fits = m.iter().map(|e| e.map(|e| e.fit.best)).collect();
            }
            let mut deltas = Vec::new();
            for (e, b) in m.iter().zip(&base_fits) {
                if let (Some(e), Some(b)) = (e, b) {
                    let f = &e.fit.best;
                    deltas.push(format!(
                        "{:+.0}/{:+.0}/{:+.2}/{:+.2}",
                        1e6 * (f.f0 / b.f0 - 1.0),
                        1e3 * (f.t_cpa_arrive - b.t_cpa_arrive),
                        100.0 * (f.v / b.v - 1.0),
                        100.0 * (f.d_cpa / b.d_cpa - 1.0)
                    ));
                }
            }
            let parts: Vec<Vec<&str>> = deltas.iter().map(|d| d.split('/').collect()).collect();
            let col = |i: usize| parts.iter().map(|p| p[i]).collect::<Vec<_>>().join(", ");
            let recall = m.iter().filter(|e| e.is_some()).count();
            posed.audio = a.clone();
            let oracle = locate_line(&cfg, &posed);
            let ids: Vec<Option<String>> = evs
                .iter()
                .map(|e| {
                    let hit = m.iter().position(|x| x.is_some_and(|x| std::ptr::eq(x, e)))?;
                    r.truth.events.iter().filter(|t| t.in_recording).nth(hit).map(|t| t.track_id.clone())
                })
                .collect();
            let saved_ev = std::mem::replace(&mut posed.events, evs.iter().map(EventSpec::from_event).collect());
            let saved_id = std::mem::replace(&mut posed.truth_flights, ids);
            let disc = if posed.events.is_empty() { "– | – | – | –".into() } else { locate_line(&cfg, &posed) };
            (posed.events, posed.truth_flights) = (saved_ev, saved_id);
            println!(
                "| {name} | {label} | {lag} | {} | {recall}/{} | {} | {} | {} | {} | {oracle} | {disc} |",
                evs.len(),
                m.len(),
                col(0),
                col(1),
                col(2),
                col(3)
            );
        }
    }
}
