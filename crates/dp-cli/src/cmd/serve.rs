use std::path::PathBuf;

use dp_core::app;

use super::serve_backend::AppBackend;

#[derive(clap::Args)]
#[command(after_help = "Example:\n  dopplerprint serve --open\n\nMore: docs/USAGE.md#browser-app")]
pub struct Args {
    #[command(flatten)]
    pub serve: dp_serve::ServeArgs,
    /// Download missing ADS-B days without asking (the page has no yes/no prompt); `false` to never download.
    #[arg(long, value_name = "BOOL", default_value_t = true, action = clap::ArgAction::Set, help_heading = "Data")]
    pub allow_download: bool,
    /// Cache directory [default: $DOPPLERPRINT_CACHE or ~/.cache/dopplerprint].
    #[arg(long, value_name = "DIR", help_heading = "Data")]
    pub cache: Option<PathBuf>,
    /// Refuse downloads that would grow the cache past this.
    #[arg(long, value_name = "GB", default_value_t = app::DEFAULT_MAX_CACHE_GB, help_heading = "Data")]
    pub max_cache_gb: f64,
    /// Experiment TOML overriding search defaults (see experiments/).
    #[arg(long, value_name = "FILE", help_heading = "Advanced")]
    pub config: Option<PathBuf>,
    /// Serve canned results instead of running the pipeline, to try the page without any data.
    #[arg(long, help_heading = "Advanced")]
    pub demo: bool,
}

pub fn run(mut a: Args) -> anyhow::Result<u8> {
    if a.serve.jobs.is_none() {
        a.serve.jobs = Some(app::cache::Cache::resolve(a.cache.as_deref())?.root.join("jobs"));
    }
    let backend: Box<dyn dp_serve::LocateBackend> = if a.demo {
        Box::new(dp_serve::FakeBackend::default())
    } else {
        Box::new(AppBackend { allow_download: a.allow_download, cache: a.cache, max_cache_gb: a.max_cache_gb, config: a.config })
    };
    dp_serve::run(a.serve, backend)?;
    Ok(0)
}
