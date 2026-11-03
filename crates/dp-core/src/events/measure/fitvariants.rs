//! CV-fit variants re-run on dumped event ridges (`passdiag::pass_dump`), scored against the ADS-B pass:
//! d/v/t error and whether the true flight passes associate's d, v and clock gates.
//! MEASURE_DUMP=<jsonl>[,<jsonl>...] [MEASURE_TIME_PRIOR=2].

use super::super::discover::confident;
use super::super::{Event, FitParams, fit_cv_with};

struct Truth {
    slant: f64,
    speed: f64,
    t_cpa: f64,
}

fn load() -> Vec<(String, Event, Truth)> {
    let paths = std::env::var("MEASURE_DUMP").expect("MEASURE_DUMP");
    let test_ids: Vec<String> = std::env::var("MEASURE_SPLIT_FILE")
        .map(|p| std::fs::read_to_string(p).expect("split").lines().filter_map(|l| l.split_once(" = ").map(|x| x.0.to_string())).collect())
        .unwrap_or_default();
    let want_test = std::env::var("MEASURE_SPLIT").is_ok_and(|s| s == "test");
    let mut out = Vec::new();
    for path in paths.split(',') {
        for line in std::fs::read_to_string(path).expect("dump").lines() {
            let v: serde_json::Value = serde_json::from_str(line).expect("json");
            let (Some(tp), true) = (v["truth_pass"].as_object(), v["label"]["source"].is_string()) else { continue };
            let case = v["case"].as_str().unwrap_or("");
            if !test_ids.is_empty() && test_ids.iter().any(|t| t == case) != want_test {
                continue;
            }
            let ev: Event = serde_json::from_value(v["event"].clone()).expect("event");
            let g = |k: &str| tp[k].as_f64().unwrap_or(f64::NAN);
            out.push((format!("{}#{}", v["case"].as_str().unwrap_or(""), v["i"]), ev, Truth { slant: g("slant"), speed: g("speed"), t_cpa: g("t_cpa") }));
        }
    }
    out
}

fn median(mut x: Vec<f64>) -> f64 {
    x.sort_by(f64::total_cmp);
    if x.is_empty() { f64::NAN } else { x[x.len() / 2] }
}

fn variants() -> Vec<(&'static str, FitParams)> {
    let b = FitParams::default();
    vec![
        ("huber (main)", FitParams { window_k: 0.0, ..b }),
        ("window 1", FitParams { window_k: 1.0, ..b }),
        ("window 1.5", FitParams { window_k: 1.5, ..b }),
        ("window 2", FitParams { window_k: 2.0, ..b }),
        ("window 3", FitParams { window_k: 3.0, ..b }),
    ]
}

#[test]
#[ignore]
fn fit_variants() {
    let rows = load();
    let prior: f64 = std::env::var("MEASURE_TIME_PRIOR").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let verbose = std::env::var("MEASURE_VERBOSE").is_ok();
    println!("n = {} real events with an ADS-B pass; clock gate ±{prior} s", rows.len());
    println!("| variant | d err median | d \\|err\\| median | v \\|err\\| median | \\|dt\\| median | d gate | v gate | clock | all 3 | median d range ×, v range ± |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for (name, fp) in variants() {
        let (mut de, mut ve, mut te, mut dw, mut vw) = (vec![], vec![], vec![], vec![], vec![]);
        let (mut gd, mut gv, mut gc, mut g3) = (0, 0, 0, 0);
        for (id, ev, tr) in &rows {
            let f = fit_cv_with(&confident(&ev.ridge), ev.fit.best.c, &fp);
            let b = &f.best;
            let (d, v, t) = (b.d_cpa / tr.slant - 1.0, b.v / tr.speed - 1.0, b.t_cpa_arrive - tr.t_cpa);
            let okd = (f.d_range.0..=f.d_range.1).contains(&tr.slant);
            let okv = (f.v_range.0..=f.v_range.1).contains(&tr.speed);
            let okc = t.abs() <= prior;
            (gd, gv, gc, g3) = (gd + okd as usize, gv + okv as usize, gc + okc as usize, g3 + (okd && okv && okc) as usize);
            de.push(d);
            ve.push(v.abs());
            te.push(t.abs());
            dw.push((f.d_range.1 / f.d_range.0).sqrt());
            vw.push(0.5 * (f.v_range.1 - f.v_range.0) / b.v);
            if verbose {
                let ht = 0.5 * (f.t_cpa_range.1 - f.t_cpa_range.0);
                println!("<!-- {name} {id} d {:+.0}% v {:+.0}% t {t:+.1} (±{ht:.1}) gates {}{}{} -->", 100.0 * d, 100.0 * v, okd as u8, okv as u8, okc as u8);
            }
        }
        let n = rows.len();
        println!(
            "| {name} | {:+.1}% | {:.1}% | {:.1}% | {:.2} s | {gd}/{n} | {gv}/{n} | {gc}/{n} | **{g3}/{n}** | ×{:.2}, ±{:.0}% |",
            100.0 * median(de.clone()),
            100.0 * median(de.iter().map(|x| x.abs()).collect()),
            100.0 * median(ve),
            median(te),
            median(dw),
            100.0 * median(vw)
        );
    }
}
