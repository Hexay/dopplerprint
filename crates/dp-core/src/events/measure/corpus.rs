//! Discovery on posed corpus/sim cases (eval's own posing, oracle events as truth): recall, spurious and
//! stationary-line events. MEASURE_EXPERIMENT=<toml> picks the cases (default: Oyster Bay parity).

use super::super::{DiscoverConfig, Event, discover_audio};
use crate::config::Config;
use crate::eval::case;
use crate::search::EventSpec;

const REPO: &str = "/srv/agents/work/doppler_print-fix";
/// Relative data paths in experiments resolve against the main checkout (read-only corpus).
const DATA_ROOT: &str = "/srv/agents/work/doppler_print";
const DEFAULT_EXPERIMENT: &str = "/srv/agents/work/doppler_print-fix/experiments/oysterbay_dg_parity.toml";
const T_TOL_S: f64 = 8.0;

fn fundamental(e: &Event) -> f64 {
    e.fit.best.f0 / e.ridge_harmonic.max(1) as f64
}

/// Some harmonic (or the half-order line) of the event lands in the oracle f0 range near the oracle CPA.
fn matches(e: &Event, o: &EventSpec) -> bool {
    let f = fundamental(e);
    (e.fit.best.t_cpa_arrive - o.t_cpa_audio).abs() <= T_TOL_S
        && std::iter::once(0.5).chain((1..=8).map(f64::from)).any(|m| (o.f0_range.0..=o.f0_range.1).contains(&(m * f)))
}

/// Median ridge frequency within 1 % of a 60 Hz harmonic, or a ridge that barely moves (< 0.5 % p10–p90).
fn stationary(e: &Event) -> bool {
    let mut f = e.ridge.f_hz.clone();
    f.sort_by(f64::total_cmp);
    let q = |p: f64| f[((f.len() - 1) as f64 * p) as usize];
    let med = q(0.5);
    let k = (med / 60.0).round();
    (k >= 1.0 && (med / (60.0 * k) - 1.0).abs() < 0.01) || (q(0.9) - q(0.1)) / med < 0.005
}

/// Centre frequency of each contiguous run of masked bins.
fn masked_runs(audio: &crate::audio::Audio) -> String {
    let fe = crate::audio::FrontEnd::default();
    let a = crate::audio::resample::decimate_for(audio, fe.f_max);
    let mask = fe.line_mask(&a);
    let f = |b: usize| fe.f_min * (b as f64 / fe.bins_per_octave as f64).exp2();
    let mut out = Vec::new();
    let mut b = 0;
    while b < mask.len() {
        if mask[b] {
            let s = b;
            while b + 1 < mask.len() && mask[b + 1] {
                b += 1;
            }
            out.push(format!("{:.1}", (f(s) * f(b)).sqrt()));
        }
        b += 1;
    }
    out.join(" ")
}

#[test]
#[ignore]
fn corpus_discovery() {
    let path = std::env::var("MEASURE_EXPERIMENT").unwrap_or_else(|_| DEFAULT_EXPERIMENT.into());
    let mut cfg = Config::from_toml(&std::fs::read_to_string(&path).expect("experiment")).expect("toml");
    let e = &mut cfg.eval;
    for p in [&mut e.data_dir, &mut e.store_dir] {
        *p = std::path::Path::new(DATA_ROOT).join(&*p);
    }
    e.split_file = std::path::Path::new(REPO).join(&e.split_file);
    let sim_dir = std::path::Path::new(REPO).join("experiments/sim");
    println!("experiment {path}");
    println!("| case | oracle | found | events | stationary | runtime s | events (t_cpa s, fund Hz, h, d m, v m/s; * = matched, ~ = stationary) |");
    println!("|---|---|---|---|---|---|---|");
    let (mut n_or, mut n_found, mut n_ev, mut n_stat) = (0, 0, 0, 0);
    for src in case::list(&cfg.eval, &sim_dir).expect("cases") {
        let posed = match case::pose(&src, &cfg.eval) {
            Ok(p) => p,
            Err(e) => {
                println!("| {} | skipped: {e:#} |", src.id());
                continue;
            }
        };
        let t = std::time::Instant::now();
        let evs = discover_audio(&posed.audio, &DiscoverConfig::default());
        let rt = t.elapsed().as_secs_f64();
        let found = posed.events.iter().filter(|o| evs.iter().any(|e| matches(e, o))).count();
        let stat = evs.iter().filter(|e| stationary(e)).count();
        let list: Vec<String> = evs
            .iter()
            .map(|e| {
                let tag = if posed.events.iter().any(|o| matches(e, o)) { "*" } else if stationary(e) { "~" } else { "" };
                let b = &e.fit.best;
                format!("{tag}{:.0}/{:.1}/{}/{:.0}/{:.0}", b.t_cpa_arrive, fundamental(e), e.ridge_harmonic, b.d_cpa, b.v)
            })
            .collect();
        println!("| {} | {} | {found} | {} | {stat} | {rt:.1} | {} |", posed.id, posed.events.len(), evs.len(), list.join(" "));
        println!("<!-- {} masked lines (Hz): {} -->", posed.id, masked_runs(&posed.audio));
        (n_or, n_found, n_ev, n_stat) = (n_or + posed.events.len(), n_found + found, n_ev + evs.len(), n_stat + stat);
    }
    println!("\nTOTAL recall {n_found}/{n_or}, events {n_ev}, stationary {n_stat}");
}
