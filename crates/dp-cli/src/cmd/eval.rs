use std::path::PathBuf;

use dp_core::config::Config;

#[derive(clap::Args)]
pub struct Args {
    /// Experiment TOML overriding config defaults.
    #[arg(long)]
    pub experiment: Option<PathBuf>,
    /// Only cases whose id contains this (repeatable).
    #[arg(long = "case")]
    pub cases: Vec<String>,
    #[arg(long, default_value = "experiments/sim")]
    pub sim_dir: PathBuf,
    /// Output dir (default runs/<unix-seconds>[-<experiment>]).
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let mut cfg = match &a.experiment {
        Some(p) => Config::from_toml(&std::fs::read_to_string(p)?)?,
        None => Config::default(),
    };
    if !a.cases.is_empty() {
        cfg.eval.cases = a.cases;
    }
    let out = a.out.unwrap_or_else(|| {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let tag = a.experiment.as_ref().and_then(|p| p.file_stem()).map(|s| format!("-{}", s.to_string_lossy()));
        PathBuf::from(format!("runs/{stamp}{}", tag.unwrap_or_default()))
    });
    dp_core::eval::run(&cfg, &a.sim_dir, &out)?;
    println!("{}", std::fs::read_to_string(out.join("summary.md"))?);
    Ok(())
}
