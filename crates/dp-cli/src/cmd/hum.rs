use std::path::PathBuf;

use dp_core::audio::{self, HumParams, analyse_hum};

#[derive(clap::Args)]
pub struct Args {
    pub audio: PathBuf,
    /// Print the full report (incl. the ENF trace) as JSON on stdout.
    #[arg(long)]
    pub json: bool,
    /// JSON object overriding any HumParams field.
    #[arg(long)]
    pub params: Option<PathBuf>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let mut p: HumParams = match &a.params {
        Some(f) => HumParams::from_json(&std::fs::read_to_string(f)?)?,
        None => HumParams::default(),
    };
    let (audio, info) = audio::decode_band(&a.audio, p.f_max)?;
    p.labelled_sr = Some(info.sample_rate);
    let r = analyse_hum(&audio, &p);
    if a.json {
        println!("{}", r.to_json()?);
        return Ok(());
    }
    println!("{}: {:.0} s @ {} Hz ({})", a.audio.display(), audio.duration_s(), info.sample_rate, info.codec);
    if !r.present {
        println!("  hum: absent ({})", r.notes.join("; "));
        return Ok(());
    }
    let ks: Vec<String> = r.harmonics.iter().map(|h| format!("{}×{:.0}dB", h.k, h.snr_db)).collect();
    println!(
        "  hum: {:.0} Hz grid, f = {:.4} Hz, harmonics [{}], detectable {:.0}%, wander {:.3} Hz",
        r.nominal_hz.unwrap_or(0.0),
        r.fundamental_hz,
        ks.join(" "),
        100.0 * r.detect_frac,
        r.wander_hz
    );
    let q = &r.rate;
    println!(
        "  rate: {:?}: implied {:.0} ± {:.0} Hz vs labelled {} ({:+.0} ppm){}; drift {:+.3} Hz{}",
        q.status,
        q.implied_sr,
        q.implied_sr_sigma,
        q.labelled_sr,
        q.deviation_ppm,
        q.likely_sr.map(|s| format!(", likely {s}")).unwrap_or_default(),
        q.drift_hz,
        if q.drift_flag { " (FLAG)" } else { "" }
    );
    for d in &r.discontinuities {
        println!(
            "  {:?} at {:.2}-{:.2} s{}",
            d.kind,
            d.t_s,
            d.t_end_s,
            d.jump_cycles.map(|c| format!(" (phase {c:+.3} cyc)")).unwrap_or_default()
        );
    }
    for n in &r.notes {
        println!("  note: {n}");
    }
    Ok(())
}
