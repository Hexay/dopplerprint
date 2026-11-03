//! Unexplained events (`AssocParams::unexplained_cost`), the abstain rule and the likelihood silence term.

use dp_core::geo::albers::Cell;
use dp_core::physics::Cpa;
use dp_core::search::hypothesis::{AssocParams, EventSpec, UNEXPLAINED_ID, associate};
use dp_core::search::pipeline::Candidate;
use dp_core::search::score::EventScore;
use dp_core::search::silence::{SilenceMode, SilenceParams};
use dp_core::search::SearchResult;

const T0: f64 = 1_000_000.0;

fn event(t_cpa: f64) -> EventSpec {
    EventSpec {
        window: (t_cpa - 30.0, t_cpa + 30.0),
        t_cpa_audio: t_cpa,
        d_range: (500.0, 2_000.0),
        v_range: (40.0, 60.0),
        f0_range: (80.0, 90.0),
        t_cpa_halfwidth_s: 3.0,
        kappa_range: None,
    }
}

fn cpa(t_arrive: f64, slant_m: f64, at_track_end: bool) -> Option<Cpa> {
    Some(Cpa { t_emit: t_arrive - slant_m / 340.0, t_arrive, slant_m, ground_speed: 50.0, at_track_end })
}

fn params(unexplained_cost: Option<f64>) -> AssocParams {
    AssocParams { t_start_prior: (T0 - 2.0, T0 + 2.0), unexplained_cost, ..Default::default() }
}

#[test]
fn an_event_may_stay_unexplained_but_one_must_be_explained() {
    // Event 0 pairs with CPA 0 only; event 1 (at 300 s) has no admissible CPA.
    let events = [event(100.0), event(300.0)];
    let cpas = [cpa(T0 + 100.5, 1_000.0, false)];
    assert!(associate(&events, &cpas, 600.0, &params(None), 0.0).is_empty(), "off: every event needs a flight");
    let h = associate(&events, &cpas, 600.0, &params(Some(2.0)), 0.0);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].tracks, vec![Some(0), None]);
    assert_eq!(h[0].clock, T0 + 0.5, "clock from the explained event only");
    assert_eq!(h[0].n_unexplained_events(), 1);
    assert_eq!(h[0].penalty(&params(Some(2.0)), 1.58), h[0].gate_cost + 2.0);
    assert!(associate(&events[1..], &cpas, 600.0, &params(Some(2.0)), 0.0).is_empty(), "all-unexplained is the null, not a hypothesis");
}

#[test]
fn full_explanations_are_kept_and_per_event_outputs_mark_the_unexplained() {
    let events = [event(100.0), event(300.0)];
    let cpas = [cpa(T0 + 100.0, 1_000.0, false), cpa(T0 + 301.0, 1_200.0, false)];
    let p = params(Some(-4.0));
    let h = associate(&events, &cpas, 600.0, &p, 0.0);
    let tuples: Vec<_> = h.iter().map(|h| h.tracks.clone()).collect();
    let off: Vec<_> = associate(&events, &cpas, 600.0, &params(None), 0.0).into_iter().map(|h| h.tracks).collect();
    assert_eq!(tuples[..off.len()], off[..], "full hypotheses are exactly those without λ_u, and come first");
    assert!(tuples.contains(&vec![None, Some(1)]) && tuples.contains(&vec![Some(0), None]));
    let partial = h.iter().find(|h| h.tracks == [None, Some(1)]).unwrap();
    assert_eq!(partial.clock, T0 + 1.0);
    assert_eq!(partial.unexplained, 1, "flight 0 is audible and unassigned");
    assert_eq!(partial.track_ids(|i| format!("ac{i}")), [UNEXPLAINED_ID, "ac1"]);
    let s = partial.per_event_scores(vec![EventScore { z: 7.0, f0: 85.0, coverage: 1.0 }]);
    assert_eq!((s[0].z, s[1].z, s[1].f0), (0.0, 7.0, 85.0));
}

#[test]
fn the_search_abstains_when_no_cell_beats_the_all_unexplained_null() {
    let p = params(Some(-5.0));
    assert_eq!(p.null_score(2), Some(10.0));
    assert_eq!(params(None).null_score(2), None);
    let cand = |score| Candidate {
        cell: Cell { ix: 0, iy: 0, step_m: 200 },
        lat_deg: 0.0,
        lon_deg: 0.0,
        score,
        raw: score,
        clock: 0.0,
        track_ids: vec!["a".into(), "b".into()],
        events: Vec::new(),
        unexplained: 0,
    };
    let res = |best: f64, null_score| SearchResult { ranked: vec![cand(best)], null_score, ..Default::default() };
    assert!(res(10.0, p.null_score(2)).abstains());
    assert!(!res(10.5, p.null_score(2)).abstains());
    assert!(!res(-100.0, None).abstains(), "off: never abstains");
    assert!(SearchResult { null_score: Some(0.0), ..Default::default() }.abstains());
    let json = serde_json::to_string(&res(1.0, None)).unwrap();
    assert!(!json.contains("null_score"), "unchanged output when off");
}

#[test]
fn likelihood_silence_falls_with_range_and_skips_track_ends() {
    let events = [event(100.0)];
    let mut p = params(None);
    p.silence = SilenceParams { mode: SilenceMode::Likelihood, ..Default::default() };
    let cost = |p: &AssocParams, extra: Option<Cpa>| {
        let cpas = [cpa(T0 + 100.0, 1_000.0, false), extra];
        associate(&events, &cpas, 600.0, p, 0.0).into_iter().find(|h| h.tracks == [Some(0)]).unwrap()
    };
    let near = cost(&p, cpa(T0 + 200.0, 800.0, false));
    let (far, end) = (cost(&p, cpa(T0 + 200.0, 2_900.0, false)), cost(&p, cpa(T0 + 200.0, 800.0, true)));
    assert!(near.silence_nats > far.silence_nats && far.silence_nats > 0.0);
    assert!(near.silence_nats <= -(1.0 - p.silence.cap).ln());
    assert_eq!((end.unexplained, end.silence_nats), (0, 0.0), "source-end loss is never silence");
    assert_eq!(near.penalty(&p, 1.58), near.gate_cost + 1.58 * near.silence_nats);
    p.silence.mode = SilenceMode::Flat;
    assert_eq!(cost(&p, cpa(T0 + 200.0, 800.0, true)).unexplained, 1, "flat mode is unchanged");
}
