//! `associate` with soft gates, per-event CPA-time uncertainty and cell slack, and the `cpa_reach_m` /
//! `clock_window` pruning bounds it relies on.

use dp_core::physics::Cpa;
use dp_core::search::hypothesis::{AssocParams, EventSpec, SoftGates, associate, clock_window, cpa_reach_m};

const T0: f64 = 1_000_000.0;

/// A discovered event whose t_cpa band half-width is `band_s` (0 = exact truth label).
fn event_band(t_cpa: f64, band_s: f64) -> EventSpec {
    EventSpec {
        window: (t_cpa - 30.0, t_cpa + 30.0),
        t_cpa_audio: t_cpa,
        d_range: (500.0, 2_000.0),
        v_range: (40.0, 60.0),
        f0_range: (80.0, 90.0),
        t_cpa_halfwidth_s: band_s,
    }
}

fn event(t_cpa: f64) -> EventSpec {
    event_band(t_cpa, 3.0)
}

fn cpa(t_arrive: f64, slant_m: f64, ground_speed: f64) -> Option<Cpa> {
    Some(Cpa { t_emit: t_arrive - slant_m / 340.0, t_arrive, slant_m, ground_speed })
}

fn params(soft: SoftGates) -> AssocParams {
    AssocParams { t_start_prior: (T0 - 2.0, T0 + 2.0), soft, ..Default::default() }
}

/// Gate cost of event `e` explained by a single CPA, or None if not admitted.
fn cost_of(e: &EventSpec, p: &AssocParams, c: Option<Cpa>, half_diag: f64) -> Option<f64> {
    associate(std::slice::from_ref(e), &[c], 600.0, p, half_diag).first().map(|h| h.gate_cost)
}

fn cost(p: &AssocParams, c: Option<Cpa>, half_diag: f64) -> Option<f64> {
    cost_of(&event(100.0), p, c, half_diag)
}

fn rho(s: &SoftGates, u: f64, sigma: f64) -> f64 {
    0.5 * (s.nu + 1.0) * (1.0 + (u / sigma).powi(2) / s.nu).ln()
}

/// Deterministic CPAs spread in slant and around the two events' implied clocks.
fn spread_cpas() -> Vec<Option<Cpa>> {
    (0..80)
        .map(|i| {
            let slant = 300.0 * 1.12f64.powi(i % 60);
            let t = T0 + if i % 2 == 0 { 100.0 } else { 300.0 } + 1.7 * ((i % 13) as f64 - 6.0);
            cpa(t, slant, 38.0 + (i % 9) as f64 * 3.0)
        })
        .collect()
}

#[test]
fn hard_gates_are_inclusive_ranges() {
    let p = params(SoftGates::hard());
    let t = T0 + 100.0;
    assert_eq!(cost(&p, cpa(t, 2_000.0, 60.0), 0.0), Some(0.0));
    assert_eq!(cost(&p, cpa(t + 2.0, 500.0, 40.0), 0.0), Some(0.0));
    assert_eq!(cost(&p, cpa(t, 2_001.0, 50.0), 0.0), None);
    assert_eq!(cost(&p, cpa(t, 1_000.0, 39.9), 0.0), None);
    assert_eq!(cost(&p, cpa(t + 2.01, 1_000.0, 50.0), 0.0), None);
    assert_eq!(cost(&p, cpa(t + 2.01, 1_000.0, 50.0), 500.0), None, "hard() has no cell slack");
}

#[test]
fn soft_cost_is_zero_inside_grows_outside_and_stops_at_outer_bound() {
    let s = SoftGates::default();
    let p = params(s.clone());
    let t = T0 + 100.0;
    let sigma = s.clock_sigma_s(&event(100.0));
    assert_eq!(sigma, s.sigma_clock_ref_s, "a floor-width band gets the reference σ_t");
    assert_eq!(cost(&p, cpa(t, 1_000.0, 50.0), 0.0), Some(0.0));
    let d: Vec<f64> = [2_100.0, 2_400.0, 2_900.0].iter().map(|&d| cost(&p, cpa(t, d, 50.0), 0.0).unwrap()).collect();
    assert!(d[0] > 0.0 && d[0] < d[1] && d[1] < d[2], "{d:?}");
    let w = 2.0f64.ln();
    let want = s.z_per_nat * (rho(&s, w + 1.2f64.ln(), s.sigma_log_d) - rho(&s, w, s.sigma_log_d));
    assert!((d[1] - want).abs() < 1e-9, "cost is z_per_nat × Student-t NLL relative to the range edge");
    let beyond = 2_000.0 * (s.outer_log_d.exp() * 1.001);
    assert_eq!(cost(&p, cpa(t, beyond, 50.0), 0.0), None);
    let c = [1.0, 3.0].map(|dt| cost(&p, cpa(t + 2.0 + dt, 1_000.0, 50.0), 0.0).unwrap());
    assert!(0.0 < c[0] && c[0] < c[1]);
    assert!((c[1] - s.z_per_nat * rho(&s, 3.0, sigma)).abs() < 1e-9);
    let outer = s.outer_clock_sigmas * sigma;
    assert!(cost(&p, cpa(t + 2.0 + outer - 0.01, 1_000.0, 50.0), 0.0).is_some());
    assert_eq!(cost(&p, cpa(t + 2.0 + outer + 0.01, 1_000.0, 50.0), 0.0), None);
    assert!(cost(&p, cpa(t, 1_000.0, 65.0), 0.0).unwrap() > 0.0);
    assert_eq!(cost(&p, cpa(t, 1_000.0, 60.0 * (s.outer_log_v.exp() * 1.001)), 0.0), None);
}

#[test]
fn cell_slack_widens_pruning_but_cost_stays_at_the_centre() {
    let s = SoftGates::default();
    let p = params(s.clone());
    let t = T0 + 100.0;
    let sigma = s.clock_sigma_s(&event(100.0));
    // 141 m half-diagonal at 50 m/s moves CPA arrival by up to ~2.8 s + r/c.
    let x = s.outer_clock_sigmas * sigma + 2.5;
    let late = cpa(t + 2.0 + x, 1_000.0, 50.0);
    assert_eq!(cost(&p, late, 0.0), None);
    let c = cost(&p, late, 141.0).expect("some point in the cell is inside the outer bound");
    assert!((c - s.z_per_nat * rho(&s, x, sigma)).abs() < 1e-9);
    let far = cpa(t, 2_000.0 * s.outer_log_d.exp() + 100.0, 50.0);
    assert_eq!(cost(&p, far, 0.0), None);
    assert!(cost(&p, far, 141.0).is_some());
    assert_eq!(cost(&p, cpa(t, 1_000.0, 50.0), 141.0), Some(0.0));
    let hard_d = params(SoftGates { outer_log_d: 0.0, ..s });
    assert_eq!(cost(&hard_d, cpa(t, 2_100.0, 50.0), 141.0), None, "slack never relaxes a hard dimension");
}

#[test]
fn wider_cpa_time_band_means_wider_clock_tail() {
    let s = SoftGates::default();
    let p = params(s.clone());
    let (narrow, wide) = (event_band(100.0, 3.0), event_band(100.0, 30.0));
    let (sn, sw) = (s.clock_sigma_s(&narrow), s.clock_sigma_s(&wide));
    assert!((sw - s.sigma_clock_ref_s * 10f64.powf(s.clock_band_exp)).abs() < 1e-12 && sw > sn);
    assert_eq!(s.clock_sigma_s(&event_band(100.0, 1.0)), sn, "bands under the reference width get the reference σ_t");
    assert_eq!(s.clock_sigma_s(&event_band(100.0, 600.0)), s.clock_sigma_s(&event_band(100.0, s.clock_band_max_s)));
    let late = cpa(T0 + 100.0 + 2.0 + 5.0, 1_000.0, 50.0);
    let (cn, cw) = (cost_of(&narrow, &p, late, 0.0).unwrap(), cost_of(&wide, &p, late, 0.0).unwrap());
    assert!(cw < cn, "the same 5 s residual costs less for the wider band: {cw} vs {cn}");
    let past_narrow = cpa(T0 + 100.0 + 2.0 + s.outer_clock_sigmas * sn + 1.0, 1_000.0, 50.0);
    assert_eq!(cost_of(&narrow, &p, past_narrow, 0.0), None);
    assert!(cost_of(&wide, &p, past_narrow, 0.0).is_some(), "the outer bound scales with σ_t");
    let pad = |evs: &[EventSpec]| clock_window(evs, &p, 0.0).1 - p.t_start_prior.1;
    assert!((pad(std::slice::from_ref(&narrow)) - s.outer_clock_sigmas * sn).abs() < 1e-9);
    assert!((pad(&[narrow, wide]) - s.outer_clock_sigmas * sw).abs() < 1e-9, "the window covers the widest event");
}

#[test]
fn exact_events_get_the_legacy_hard_gates_under_default_params() {
    let events = [event_band(100.0, 0.0), event_band(300.0, 0.0)];
    let cpas = spread_cpas();
    let (soft, hard) = (params(SoftGates::default()), params(SoftGates::hard()));
    for half_diag in [0.0, 141.0, 1_000.0] {
        let a = associate(&events, &cpas, 600.0, &soft, half_diag);
        let b = associate(&events, &cpas, 600.0, &hard, half_diag);
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!((&x.tracks, x.unexplained, x.gate_cost), (&y.tracks, y.unexplained, 0.0));
            assert_eq!(x.clock.to_bits(), y.clock.to_bits());
        }
        assert_eq!(cpa_reach_m(&events, &soft, half_diag), cpa_reach_m(&events, &hard, half_diag));
        assert_eq!(clock_window(&events, &soft, half_diag), soft.t_start_prior);
    }
}

#[test]
fn reach_and_clock_window_bound_every_admission_and_pruning_beyond_them_is_exact() {
    let mixes = [
        vec![event(100.0), event(300.0)],
        vec![event_band(100.0, 0.0), event_band(300.0, 40.0)],
        vec![event_band(100.0, 12.0), event_band(300.0, 3.0)],
    ];
    let cpas = spread_cpas();
    for events in &mixes {
        for soft in [SoftGates::default(), SoftGates::hard()] {
            let p = params(soft);
            for half_diag in [0.0, 141.0, 1_000.0] {
                let reach = cpa_reach_m(events, &p, half_diag);
                let (w0, w1) = clock_window(events, &p, half_diag);
                let inside = |c: &Cpa| events.iter().any(|e| (w0..=w1).contains(&(c.t_arrive - e.t_cpa_audio)));
                let pruned: Vec<Option<Cpa>> = cpas.iter().map(|c| c.filter(|c| c.slant_m <= reach)).collect();
                let full = associate(events, &cpas, 600.0, &p, half_diag);
                let cut = associate(events, &pruned, 600.0, &p, half_diag);
                assert!(!full.is_empty(), "{events:?}");
                assert_eq!(full.len(), cut.len());
                for (a, b) in full.iter().zip(&cut) {
                    assert_eq!((&a.tracks, a.unexplained, a.gate_cost), (&b.tracks, b.unexplained, b.gate_cost));
                    for (e, &i) in events.iter().zip(&a.tracks) {
                        let c = cpas[i].unwrap();
                        let clock = c.t_arrive - e.t_cpa_audio;
                        assert!(c.slant_m <= reach && clock >= w0 && clock <= w1);
                    }
                }
                // Drop every CPA no event could pair with in time: hypotheses are unchanged.
                let timed: Vec<Option<Cpa>> = cpas.iter().map(|c| c.filter(|c| inside(c) || c.slant_m > reach)).collect();
                let kept = associate(events, &timed, 600.0, &p, half_diag);
                assert_eq!(full.iter().map(|h| &h.tracks).collect::<Vec<_>>(), kept.iter().map(|h| &h.tracks).collect::<Vec<_>>());
            }
        }
    }
}

#[test]
fn soft_reach_exceeds_hard_reach_by_the_outer_bound() {
    let events = [event(100.0)];
    let hard = cpa_reach_m(&events, &params(SoftGates::hard()), 141.0);
    let s = SoftGates::default();
    let soft = cpa_reach_m(&events, &params(s.clone()), 141.0);
    assert!((hard - 3_000.0).abs() < 1e-3, "hard: max(d_hi, silence range)");
    assert!((soft - (2_000.0 + 141.0) * s.outer_log_d.exp()).abs() < 1e-3);
    let exact = cpa_reach_m(&[event_band(100.0, 0.0)], &params(s), 141.0);
    assert!((exact - 3_000.0).abs() < 1e-3, "an exact event gets no slack or tail");
}

#[test]
fn cap_keeps_the_cheapest_hypotheses() {
    let p = AssocParams { max_hypotheses: 3, ..params(SoftGates::default()) };
    let t = T0 + 100.0;
    let cpas: Vec<Option<Cpa>> = (0..40).rev().map(|i| cpa(t, 2_000.0 + 50.0 * i as f64, 50.0)).collect();
    let h = associate(&[event(100.0)], &cpas, 600.0, &p, 0.0);
    assert_eq!(h.iter().map(|h| h.tracks[0]).collect::<Vec<_>>(), vec![39, 38, 37]);
    assert_eq!(h[0].gate_cost, 0.0);
    assert!(h[1].gate_cost < h[2].gate_cost);
}
