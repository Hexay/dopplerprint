//! `eval.oracle_events = false` end to end on one phone-chain sim flyover: both modes run, the front end
//! recalls the pass, and the outputs are written.

use std::path::Path;

use dp_core::config::Config;

#[test]
fn discovered_mode_pairs_oracle_and_discovered_on_a_chain_sim_case() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = tempfile::tempdir().unwrap();
    let sim = tmp.path().join("sim");
    std::fs::create_dir_all(&sim).unwrap();
    std::fs::copy(root.join("experiments/sim_chain_phone_livermore_like/atm_single_00.toml"), sim.join("one.toml")).unwrap();
    let cfg = Config::from_toml(
        r#"
        [eval]
        cases = ["sim_one"]
        data_dir = "/nonexistent"
        time_prior_s = 5.0
        oracle_events = false
        weather = "truth"
        [search]
        keep_top = 100
        [search.score]
        clock_sweep_s = 2.0
        "#,
    )
    .unwrap();
    let out = tmp.path().join("run");
    let disc = dp_core::eval::run(&cfg, &sim, &out).unwrap();
    assert_eq!(disc.len(), 1);
    let r = &disc[0];
    assert_eq!(r.status, "ok");
    let names: Vec<&str> = r.metrics.stages.iter().map(|s| s.stage.as_str()).collect();
    assert_eq!(&names[..2], ["discover", "select"]);
    assert_eq!(r.flights_correct, Some(true), "lost at {:?}", r.metrics.lost_at);
    let oracle: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("oracle/sim_one.json")).unwrap()).unwrap();
    let (o, d) = (oracle["error_m"].as_f64().unwrap(), r.error_m.unwrap());
    assert!(d < o + 300.0, "discovered {d:.0} m vs oracle {o:.0} m");

    let front: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join("discovery/sim_one.json")).unwrap()).unwrap();
    assert_eq!(front["recalled"], 1);
    for f in ["summary.md", "cases.csv", "oracle/summary.md", "oracle/sim_one.json", "sim_one.json"] {
        assert!(out.join(f).is_file(), "{f}");
    }
    let md = std::fs::read_to_string(out.join("summary.md")).unwrap();
    assert!(md.starts_with("# Discovered vs oracle events"));
}
