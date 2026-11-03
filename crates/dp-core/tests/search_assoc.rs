//! `associate` with soft gates and cell slack, and the `cpa_reach_m` pruning bound it relies on.

use dp_core::physics::Cpa;
use dp_core::search::hypothesis::{AssocParams, EventSpec, SoftGates, associate, clock_window, cpa_reach_m};

const T0: f64 = 1_000_000.0;

fn event(t_cpa: f64) -> EventSpec {
    EventSpec { window: (t_cpa - 30.0, t_cpa + 30.0), t_cpa_audio: t_cpa, d_range: (500.0, 2_000.0), v_range: (40.0, 60.0), f0_range: (80.0, 90.0) }
}

fn cpa(t_arrive: f64, slant_m: f64, ground_speed: f64) -> Option<Cpa> {
    Some(Cpa { t_emit: t_arrive - slant_m / 340.0, t_arrive, slant_m, ground_speed })
}

fn params(soft: SoftGates) -> AssocParams {
    AssocParams { t_start_prior: (T0 - 2.0, T0 + 2.0), soft, ..Default::default() }
}

/// Gate cost of a single event explained by a single CPA, or None if not admitted.
fn cost(p: &AssocParams, c: Option<Cpa>, half_diag: f64) -> Option<f64> {
    associate(&[event(100.0)], &[c], 600.0, p, half_diag).first().map(|h| h.gate_cost)
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
    assert_eq!(cost(&p, cpa(t, 1_000.0, 50.0), 0.0), Some(0.0));
    let d: Vec<f64> = [2_100.0, 2_400.0, 2_900.0].iter().map(|&d| cost(&p, cpa(t, d, 50.0), 0.0).unwrap()).collect();
    assert!(d[0] > 0.0 && d[0] < d[1] && d[1] < d[2], "{d:?}");
    let rho = |u: f64| 0.5 * (s.nu + 1.0) * (1.0 + (u / s.sigma_log_d).powi(2) / s.nu).ln();
    let w = 2.0f64.ln();
    let want = s.z_per_nat * (rho(w + 1.2f64.ln()) - rho(w));
    assert!((d[1] - want).abs() < 1e-9, "cost is z_per_nat × Student-t NLL relative to the range edge");
    let beyond = 2_000.0 * (s.outer_log_d.exp() * 1.001);
    assert_eq!(cost(&p, cpa(t, beyond, 50.0), 0.0), None);
    let c = [3.0, 6.0].map(|dt| cost(&p, cpa(t + 2.0 + dt, 1_000.0, 50.0), 0.0).unwrap());
    assert!(0.0 < c[0] && c[0] < c[1]);
    assert_eq!(cost(&p, cpa(t + 2.0 + s.outer_clock_s + 0.1, 1_000.0, 50.0), 0.0), None);
    assert!(cost(&p, cpa(t, 1_000.0, 65.0), 0.0).unwrap() > 0.0);
    assert_eq!(cost(&p, cpa(t, 1_000.0, 60.0 * (s.outer_log_v.exp() * 1.001)), 0.0), None);
}

#[test]
fn cell_slack_widens_pruning_but_cost_stays_at_the_centre() {
    let s = SoftGates::default();
    let p = params(s.clone());
    let t = T0 + 100.0;
    // 141 m half-diagonal at 50 m/s moves CPA arrival by up to ~2.8 s + r/c.
    let late = cpa(t + 2.0 + s.outer_clock_s + 2.5, 1_000.0, 50.0);
    assert_eq!(cost(&p, late, 0.0), None);
    let c = cost(&p, late, 141.0).expect("some point in the cell is inside the outer bound");
    let x = (s.outer_clock_s + 2.5) / s.sigma_clock_s;
    assert!((c - s.z_per_nat * 0.5 * (s.nu + 1.0) * (1.0 + x * x / s.nu).ln()).abs() < 1e-9);
    let far = cpa(t, 2_000.0 * s.outer_log_d.exp() + 100.0, 50.0);
    assert_eq!(cost(&p, far, 0.0), None);
    assert!(cost(&p, far, 141.0).is_some());
    assert_eq!(cost(&p, cpa(t, 1_000.0, 50.0), 141.0), Some(0.0));
    let hard_d = params(SoftGates { outer_log_d: 0.0, ..s });
    assert_eq!(cost(&hard_d, cpa(t, 2_100.0, 50.0), 141.0), None, "slack never relaxes a hard dimension");
}

#[test]
fn reach_bounds_every_admitted_slant_and_pruning_beyond_it_is_exact() {
    let events = [event(100.0), event(300.0)];
    for soft in [SoftGates::default(), SoftGates::hard()] {
        let p = params(soft);
        for half_diag in [0.0, 141.0, 1_000.0] {
            let reach = cpa_reach_m(&events, &p, half_diag);
            let (w0, w1) = clock_window(&p, half_diag);
            let cpas: Vec<Option<Cpa>> = (0..60)
                .map(|i| {
                    let slant = 300.0 * 1.12f64.powi(i);
                    let t = T0 + if i % 2 == 0 { 100.0 } else { 300.0 } + (i % 7) as f64 - 3.0;
                    cpa(t, slant, 45.0 + (i % 5) as f64)
                })
                .collect();
            let pruned: Vec<Option<Cpa>> = cpas.iter().map(|c| c.filter(|c| c.slant_m <= reach)).collect();
            let full = associate(&events, &cpas, 600.0, &p, half_diag);
            let cut = associate(&events, &pruned, 600.0, &p, half_diag);
            assert!(!full.is_empty());
            assert_eq!(full.len(), cut.len());
            for (a, b) in full.iter().zip(&cut) {
                assert_eq!((&a.tracks, a.unexplained), (&b.tracks, b.unexplained));
                assert_eq!(a.gate_cost, b.gate_cost);
                for (e, &i) in events.iter().zip(&a.tracks) {
                    let c = cpas[i].unwrap();
                    let clock = c.t_arrive - e.t_cpa_audio;
                    assert!(c.slant_m <= reach && clock >= w0 && clock <= w1);
                }
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
