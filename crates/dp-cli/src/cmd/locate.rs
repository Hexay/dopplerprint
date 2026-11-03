use std::path::PathBuf;

use dp_core::app::{self, ConsoleUi, LocateRequest};

#[derive(clap::Args)]
#[command(after_help = "Example:\n  dopplerprint locate memo.m4a --start \"2026-05-14 10:02 -07:00\" --near \"Livermore, CA\"\n\n\
Writes result.html, .geojson, .gpkg and .json. Exit codes: 0 found, 1 error, 2 needs input, 3 inconclusive.\n\
More: docs/USAGE.md")]
pub struct Args {
    /// Recording (mp3, m4a/aac, wav, flac, ogg).
    pub audio: PathBuf,
    /// When it started, with UTC offset: "2026-05-14 10:02 -07:00" or "2026-05-14T17:02Z"; add "±10m" if unsure
    /// (default ±2m). Taken from the file's timestamp when omitted.
    #[arg(long, value_name = "WHEN")]
    pub start: Option<String>,
    /// Roughly where: a place ("Livermore, CA"), lat,lon, lat,lon,radius_km or west,south,east,north.
    #[arg(long, value_name = "WHERE")]
    pub near: Option<String>,
    /// Search radius around --near (default 10 km, max 50).
    #[arg(long, value_name = "KM")]
    pub radius: Option<f64>,
    /// Start-time uncertainty, e.g. 90s or 10m (overrides "±..." in --start).
    #[arg(long, value_name = "DUR")]
    pub start_tolerance: Option<String>,
    /// Result directory [default: ./dopplerprint-results/<name>].
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
    /// Only print the report.
    #[arg(short, long)]
    pub quiet: bool,
    /// Download ADS-B without asking.
    #[arg(short, long, help_heading = "Data")]
    pub yes: bool,
    /// Never download; use only the cache.
    #[arg(long, help_heading = "Data")]
    pub offline: bool,
    /// Cache directory [default: $DOPPLERPRINT_CACHE or ~/.cache/dopplerprint].
    #[arg(long, value_name = "DIR", help_heading = "Data")]
    pub cache: Option<PathBuf>,
    /// Refuse downloads that would grow the cache past this.
    #[arg(long, value_name = "GB", default_value_t = app::DEFAULT_MAX_CACHE_GB, help_heading = "Data")]
    pub max_cache_gb: f64,
    /// Keep the raw adsb.lol archives after processing.
    #[arg(long, help_heading = "Data")]
    pub keep_raw: bool,
    /// Search in still air instead of archived winds aloft and temperature (Open-Meteo).
    #[arg(long, help_heading = "Data")]
    pub no_weather: bool,
    /// Ground model (dp-dem-u16 .u16 file); default: one in <cache>/dem, else flat ground.
    #[arg(long, value_name = "FILE", help_heading = "Advanced")]
    pub dem: Option<PathBuf>,
    /// Notch mains hum at this frequency (50 or 60) and its harmonics.
    #[arg(long, value_name = "HZ", help_heading = "Advanced")]
    pub mains: Option<f64>,
    /// Experiment TOML overriding search defaults (see experiments/).
    #[arg(long, value_name = "FILE", help_heading = "Advanced")]
    pub config: Option<PathBuf>,
    /// Use at most this many of the strongest passes.
    #[arg(long, value_name = "N", default_value_t = 4, help_heading = "Advanced")]
    pub max_events: usize,
}

pub fn run(a: Args) -> anyhow::Result<u8> {
    let req = LocateRequest {
        audio: a.audio,
        start: a.start,
        start_tolerance: a.start_tolerance,
        near: a.near,
        radius_km: a.radius,
        cache: a.cache,
        out: a.out,
        dem: a.dem,
        mains_hz: a.mains,
        config: a.config,
        offline: a.offline,
        no_weather: a.no_weather,
        yes: a.yes,
        keep_raw: a.keep_raw,
        max_cache_gb: a.max_cache_gb,
        max_events: a.max_events,
    };
    let ui = ConsoleUi::new(true, a.quiet || a.json);
    let report = app::locate(&req, &ui)?;
    if a.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", app::text::render(&report));
    }
    Ok(report.status.exit_code())
}
