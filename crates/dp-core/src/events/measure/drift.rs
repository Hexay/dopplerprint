//! M2: linear f0 drift (sim `drift_per_s`, emission time from recording start) vs the free-geometry CV fit
//! of the discovered event, and vs `locate` with oracle events (geometry from the sim's ADS-B track).
//! Set MEASURE_LOCATE=1 to also run locate (slower).

use super::super::{DiscoverConfig, Event, discover_audio};
use super::*;
use crate::audio::Audio;
use crate::config::Config;
use crate::eval::case::{CaseSource, pose};
use crate::eval::run_case;
use crate::sim::scenario::{Aircraft, Device, Noise, Observer, Scenario, SimAtmosphere, Source, Straight};
use crate::sim::{Truth, render};
use crate::track::EngineClass;

pub(super) fn straight_scenario(v: f64, d: f64, beta: f64, seed: u64) -> Scenario {
    let alt = (0.5 * d).min(1000.0);
    let dur = 2.0 * (3.0 * d / v).max(90.0);
    Scenario {
        seed,
        sample_rate: 4000,
        duration_s: dur,
        start_unix: 1_725_827_400.0,
        adsb_dt_s: 1.0,
        observer: Observer { lat_deg: 38.912, lon_deg: -97.214, h_ell_m: 360.0, mic_height_m: 2.0 },
        atmosphere: SimAtmosphere { c: C, wind_enu: [0.0; 3] },
        ground: None,
        noise: Noise { snr_db: 40.0, ..Default::default() },
        device: Device::default(),
        aircraft: vec![Aircraft {
            id: "d71f00-0".into(),
            icao24: 0xD71F00,
            type_code: None,
            engine: EngineClass::Turboprop,
            amplitude: 1.0,
            source: Source { f0: 100.0, harmonics: vec![1.0, 0.5, 0.3], drift_per_s: beta, jitter_frac: 0.0 },
            straight: Some(Straight {
                heading_deg: 90.0,
                speed_mps: v,
                alt_agl_m: alt,
                cpa_offset_m: (d * d - alt * alt).sqrt(),
                cpa_time_s: 0.5 * dur,
                half_span_s: dur,
            }),
            waypoints: None,
        }],
    }
}

fn best_event(evs: &[Event], f0: f64) -> Option<&Event> {
    evs.iter()
        .filter(|e| (e.fit.best.f0 / e.ridge_harmonic.max(1) as f64 / f0 - 1.0).abs() < 0.08)
        .max_by_key(|e| e.ridge.t.len())
}

fn locate_err(sc: &Scenario, f0_cpa: f64, dir: &std::path::Path, tag: &str) -> String {
    let path = dir.join(format!("{tag}.toml"));
    std::fs::write(&path, toml::to_string(sc).expect("scenario serialises")).expect("write scenario");
    let cfg = Config::default();
    let mut posed = pose(&CaseSource::Sim(path), &cfg.eval).expect("pose sim");
    posed.events.iter_mut().for_each(|e| e.f0_range = (0.94 * f0_cpa, 1.06 * f0_cpa));
    let r = run_case(&cfg, &posed);
    // Track heads east, so north offset is the cross-track (slant-range) error; along-track trades with clock.
    let xtrack = r.best.as_ref().map(|b| (b.lat_deg - posed.truth_lat) * 111_320.0);
    format!(
        "{} | {} | {}",
        r.error_m.map_or("–".into(), |e| format!("{e:.0}")),
        xtrack.map_or("–".into(), |x| format!("{x:+.0}")),
        r.clock_err_s.map_or("–".into(), |c| format!("{c:+.2}")),
    )
}

#[test]
#[ignore]
fn drift_bias() {
    let with_locate = std::env::var("MEASURE_LOCATE").is_ok_and(|v| v == "1");
    let dir = std::env::temp_dir().join("dp_measure_drift");
    std::fs::create_dir_all(&dir).expect("tmp dir");
    println!("β in %/min (emission time). f0 err vs f0·(1+β·τ_cpa). Pred d = β·c·d/v² (research first-order).");
    println!("| v m/s | d m | β %/min | ridge s | f0 err % | v err % | d err % | pred d % | t_cpa s | v,d in bounds | locate err m | north err m | clock s |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let subset = std::env::var("MEASURE_SUBSET").is_ok_and(|v| v == "1");
    let (vs, ds, bs): (&[f64], &[f64], &[f64]) = if subset {
        (&[50.0], &[1500.0, 5000.0], &[0.0, 1.0, -1.0])
    } else {
        (&[50.0, 85.0], &[500.0, 1500.0, 3000.0, 5000.0], &[0.0, 0.25, 1.0, -1.0])
    };
    for &v in vs {
        for &d in ds {
            for &beta_pm in bs {
                let beta = beta_pm / 100.0 / 60.0;
                let sc = straight_scenario(v, d, beta, 11);
                let r = render(&sc);
                let t: &Truth = &r.truth;
                let te = &t.events[0];
                let f0_cpa = te.f0 * (1.0 + beta * te.cpa_emit_audio_s);
                let audio = Audio { sample_rate: r.sample_rate, samples: r.samples };
                let evs = discover_audio(&audio, &DiscoverConfig::default());
                let loc = if with_locate {
                    locate_err(&sc, f0_cpa, &dir, &format!("v{v}_d{d}_b{beta_pm}"))
                } else {
                    "– | – | –".into()
                };
                let pred = 100.0 * beta * C * te.slant_m / (te.speed_mps * te.speed_mps);
                let Some(e) = best_event(&evs, f0_cpa) else {
                    println!("| {v} | {d} | {beta_pm} | missed ({} events) | | | | {pred:+.1} | | | {loc} |", evs.len());
                    continue;
                };
                let b = &e.fit.best;
                let h = e.ridge_harmonic.max(1) as f64;
                let span = e.ridge.t.last().unwrap_or(&0.0) - e.ridge.t.first().unwrap_or(&0.0);
                let inb = |x: f64, r: (f64, f64)| r.0 <= x && x <= r.1;
                println!(
                    "| {v} | {d} | {beta_pm} | {span:.0} | {:+.2} | {:+.1} | {:+.1} | {pred:+.1} | {:+.2} | {}{} | {loc} |",
                    100.0 * (b.f0 / h / f0_cpa - 1.0),
                    100.0 * (b.v / te.speed_mps - 1.0),
                    100.0 * (b.d_cpa / te.slant_m - 1.0),
                    b.t_cpa_arrive - te.cpa_arrival_audio_s,
                    if inb(te.speed_mps, e.fit.v_range) { "v" } else { "-" },
                    if inb(te.slant_m, e.fit.d_range) { "d" } else { "-" },
                );
            }
        }
    }
}
