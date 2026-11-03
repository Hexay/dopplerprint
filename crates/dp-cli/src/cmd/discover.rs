use std::path::PathBuf;
use std::time::Instant;

use dp_core::audio;
use dp_core::events::{DiscoverConfig, discover_with, events_to_json};

#[derive(clap::Args)]
pub struct Args {
    pub audio: PathBuf,
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// JSON object overriding any FrontEnd / DiscoverParams field (flat keys).
    #[arg(long)]
    pub params: Option<PathBuf>,
    /// STFT window lengths (s), max-combined on one grid.
    #[arg(long, value_delimiter = ',')]
    pub wins: Option<Vec<f64>>,
    #[arg(long)]
    pub f_max: Option<f64>,
    /// Notch this mains frequency (50 or 60) and its harmonics.
    #[arg(long)]
    pub mains: Option<f64>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let t0 = Instant::now();
    let mut cfg = match &a.params {
        Some(p) => DiscoverConfig::from_json(&std::fs::read_to_string(p)?)?,
        None => DiscoverConfig::default(),
    };
    if let Some(w) = a.wins {
        cfg.front.wins_s = w;
    }
    cfg.front.f_max = a.f_max.unwrap_or(cfg.front.f_max);
    cfg.front.mains_hz = a.mains.or(cfg.front.mains_hz);

    let (audio, info) = audio::decode_band(&a.audio, cfg.front.f_max)?;
    let t_dec = t0.elapsed().as_secs_f64();
    let spec = cfg.front.spectrogram(&audio);
    drop(audio);
    let t_spec = t0.elapsed().as_secs_f64();
    let events = discover_with(&spec, &cfg.params);
    eprintln!(
        "{}: {} Hz {}ch {} ({:.0} s): {} events; decode {:.1} s, spectrogram {:.1} s, discover {:.1} s",
        a.audio.display(),
        info.sample_rate,
        info.channels,
        info.codec,
        spec.n_t as f64 * spec.dt,
        events.len(),
        t_dec,
        t_spec - t_dec,
        t0.elapsed().as_secs_f64() - t_spec
    );
    for e in &events {
        let b = &e.fit.best;
        let (ts, te) = (
            e.ridge.t.first().copied().unwrap_or(0.0),
            e.ridge.t.last().copied().unwrap_or(0.0),
        );
        eprintln!(
            "  #{:<2} t_cpa {:7.1} s  f0 {:6.1} Hz (h{} of {:?})  v {:5.1} m/s  d {:6.0} m  snr {:5.1}  ridge {:.0}-{:.0} s",
            e.id, b.t_cpa_arrive, b.f0, e.ridge_harmonic, e.harmonics, b.v, b.d_cpa, e.snr, ts, te
        );
    }
    let json = events_to_json(&events)?;
    match a.out {
        Some(p) => std::fs::write(p, json)?,
        None => println!("{json}"),
    }
    Ok(())
}
