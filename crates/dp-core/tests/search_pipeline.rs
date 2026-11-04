//! End-to-end search on a painted spectrogram: two prop events 920 s apart plus decoy flights.

use dp_core::audio::LogSpectrogram;
use dp_core::geo::{Geodetic, Vec3, albers, east_north, geodetic_to_ecef, up_vector};
use dp_core::physics::{Atmosphere, cpa, cv::StraightLine, doppler_factors};
use dp_core::search::score::{ScoreParams, ScoringPlane};
use dp_core::search::pipeline::score_cell;
use dp_core::search::{Catalog, EventSpec, MemCatalog, Region, Scorer, SearchInput, SearchParams, search};
use dp_core::track::Track;

const T_REC: f64 = 1.7e9 + 500.0;
const LAT: f64 = 40.0;
const LON: f64 = -96.0;
const H: f64 = 300.0;

fn line(obs: Vec3, dir: Vec3, offset: Vec3, speed: f64, t_cpa: f64, id: &str) -> Track {
    StraightLine { obs, dir, offset, speed, t_cpa_emit: t_cpa, half_span_s: 500.0, dt: 2.0 }.track(id)
}

fn paint(tracks: &[(&Track, f64)], obs: Vec3, n_t: usize) -> LogSpectrogram {
    let (dt, bpo) = (0.25, 96u32);
    let n_f = (96.0 * (1000.0f64 / 20.0).log2()) as usize;
    let mut s = LogSpectrogram { t0: 0.0, dt, f_min: 20.0, bins_per_octave: bpo, n_t, n_f, data: vec![0.0; n_t * n_f] };
    let mut x: u64 = 0xD1B5_4A32_D192_ED03;
    for v in s.data.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *v = ((x >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 2.0;
    }
    let times: Vec<f64> = (0..n_t).map(|i| T_REC + i as f64 * dt).collect();
    for (tr, f0) in tracks {
        for (i, f) in doppler_factors(tr, obs, &times, &Atmosphere::default()).iter().enumerate() {
            for h in (1..=4).filter(|_| f.is_finite()) {
                let b = s.bin_of(f0 * h as f64 * f).round() as usize;
                if b < n_f {
                    s.data[i * n_f + b] = 4.0;
                }
            }
        }
    }
    s
}

fn spec_for(tr: &Track, obs: Vec3, f0: f64) -> EventSpec {
    let c = cpa(tr, obs, &Atmosphere::default()).unwrap();
    let t = c.t_arrive - T_REC + 1.5;
    EventSpec {
        window: (t - 60.0, t + 60.0),
        t_cpa_audio: t,
        d_range: (c.slant_m * 0.7, c.slant_m * 1.4),
        v_range: (c.ground_speed * 0.8, c.ground_speed * 1.25),
        f0_range: (f0 * 0.95, f0 * 1.05),
        t_cpa_halfwidth_s: 0.0,
        kappa_range: None,
    }
}

#[test]
fn hypothesis_cap_is_flagged() {
    let obs = geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: H });
    let (e, n) = east_north(LAT, LON);
    let u = up_vector(LAT, LON);
    let a1 = line(obs, n, u * 1200.0 + e * 700.0, 62.0, 1.7e9 + 600.0, "ac1");
    let a2 = line(obs, (e + n * 0.3).normalized(), u * 900.0 - n * 1100.0, 75.0, 1.7e9 + 1520.0, "ac2");
    let spec = paint(&[(&a1, 92.0), (&a2, 76.0)], obs, 4 * 1200);
    let plane = ScoringPlane::new(&spec, &ScoreParams::default());
    let events = [spec_for(&a1, obs, 92.0), spec_for(&a2, obs, 76.0)];
    let catalog = MemCatalog::new(vec![a1, a2]);
    let scorer = Scorer::new(&plane, Default::default()).unwrap();
    let region = Region { lat_deg: LAT, lon_deg: LON, radius_m: 100.0 };
    let inp = SearchInput { scorer: &scorer, events: &events, catalog: &catalog, region, duration_s: 1200.0, dem: None };
    let cell = albers::Cell::of(albers::forward(LAT, LON), 25);
    let capped = |max| {
        let mut p = SearchParams { flat_h_ell_m: H - 1.5, ..Default::default() };
        p.assoc.max_hypotheses = max;
        score_cell(&inp, &p, cell).0.expect("truth cell scores").capped
    };
    assert!(capped(1), "one full hypothesis fills a cap of 1");
    assert!(!capped(64));
}

#[test]
#[ignore = "full end-to-end search: 30 s+ on fast machines, minutes on CI; run with --run-ignored all"]
fn two_event_search_recovers_observer() {
    let obs = geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: H });
    let (e, n) = east_north(LAT, LON);
    let u = up_vector(LAT, LON);
    let a1 = line(obs, n, u * 1200.0 + e * 700.0, 62.0, 1.7e9 + 600.0, "ac1");
    let a2 = line(obs, (e + n * 0.3).normalized(), u * 900.0 - n * 1100.0, 75.0, 1.7e9 + 1520.0, "ac2");
    let decoy1 = line(obs, n, u * 1000.0 + e * 4000.0, 64.0, 1.7e9 + 610.0, "decoy1");
    let decoy2 = line(obs, e, u * 1500.0 + n * 3500.0, 70.0, 1.7e9 + 1530.0, "decoy2");
    let (f1, f2) = (92.0, 76.0);

    let spec = paint(&[(&a1, f1), (&a2, f2)], obs, 4 * 1200);
    let plane = ScoringPlane::new(&spec, &ScoreParams::default());
    let events = [spec_for(&a1, obs, f1), spec_for(&a2, obs, f2)];
    let catalog = MemCatalog::new(vec![a1, a2, decoy1, decoy2]);
    let p = SearchParams { flat_h_ell_m: H - 1.5, ..Default::default() };
    // DP_BACKEND=cpu|cuda forces a backend, for CPU↔GPU dump comparisons.
    let backend = std::env::var("DP_BACKEND").map_or(p.backend, |b| serde_json::from_value(b.into()).unwrap());
    let scorer = Scorer::new(&plane, backend).unwrap();
    eprintln!("scoring on {}", scorer.device());
    let inp = SearchInput {
        scorer: &scorer,
        events: &events,
        catalog: &catalog,
        region: Region { lat_deg: LAT + 0.006, lon_deg: LON - 0.008, radius_m: 2_500.0 },
        duration_s: 1200.0,
        dem: None,
    };
    let r = search(&inp, &p);
    // Full ranking for output-equivalence checks across optimisations.
    if let Some(path) = std::env::var_os("DP_SEARCH_DUMP") {
        std::fs::write(path, serde_json::to_vec(&r).unwrap()).unwrap();
    }
    let best = &r.ranked[0];
    let tracks = catalog.all();
    let targets: Vec<_> = events
        .iter()
        .zip([&tracks[0], &tracks[1]])
        .map(|(e, t)| dp_core::search::score::EventTarget { track: t, window: e.window, f0_range: e.f0_range })
        .collect();
    let at_truth = dp_core::search::score::score_joint(&plane, &targets, obs, &p.atmosphere, T_REC, &p.score).unwrap();
    eprintln!("score at truth {:.2} (clock {:+.2}) vs best {:.2}", at_truth.total, at_truth.clock_offset - T_REC, best.raw);
    let got = geodetic_to_ecef(Geodetic { lat_deg: best.lat_deg, lon_deg: best.lon_deg, h_ell_m: H });
    let err = (got - obs).norm();
    eprintln!("cells {} hyps {} best {:?} err {err:.1} m", r.cells_scored, r.hypotheses_scored, best.track_ids);
    assert_eq!(best.track_ids, ["ac1", "ac2"]);
    assert!((best.clock - T_REC).abs() <= 0.5, "clock {}", best.clock - T_REC);
    assert!(err < 60.0, "error {err:.1} m");
}
