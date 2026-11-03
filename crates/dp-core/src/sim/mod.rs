//! Independent synthetic flyover generator (time-domain phase integration), used as test truth.
//! Deliberately shares no code with physics::doppler_factors beyond Track::state_at.
//! Model and scenario schema: see docs/PHYSICS.md §4.

pub mod noise;
pub mod propagate;
pub mod scenario;
pub mod source;
pub mod trajectory;
pub mod truth;

use std::path::Path as FsPath;

pub use scenario::Scenario;
pub use truth::Truth;

use noise::Pcg;
use propagate::{Path, render_path};
use source::Tone;
use trajectory::Frame;

/// Rendered clip: samples in [-1, 1] (pre-quantisation) and the truth record.
pub struct Rendered {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub truth: Truth,
}

pub fn render(sc: &Scenario) -> Rendered {
    let fs = sc.sample_rate as f64;
    let n = (sc.duration_s * fs).round() as usize;
    let frame = Frame::new(&sc.observer);
    let [we, wn, wu] = sc.atmosphere.wind_enu;
    let wind = frame.enu(we, wn, wu);
    let mut rng = Pcg::new(sc.seed);

    let mut mix = noise::background(n, fs, &sc.noise, &mut rng);
    noise::add_hum(&mut mix, fs, &sc.noise);

    let mut events = Vec::new();
    let mut tracks = Vec::new();
    for a in &sc.aircraft {
        let local = trajectory::build_track(sc, &frame, a);
        let tone = Tone::new(&a.source, &mut rng);
        let mut paths = vec![(frame.mic, a.amplitude)];
        if let Some(g) = sc.ground {
            paths.push((frame.mic_image, a.amplitude * g.reflection));
        }
        for (obs, gain) in paths {
            let p = Path { track: &local, tone: &tone, obs, gain, c: sc.atmosphere.c, wind };
            render_path(&p, fs, &mut mix);
        }
        events.push(truth::event(sc, &frame, wind, a, &local));
        tracks.push(truth::to_unix(&local, sc.start_unix));
    }

    let (samples, output_scale) = noise::apply_device(mix, fs, &sc.device);
    let truth = Truth {
        observer: truth::observer(sc, &frame),
        sample_rate: sc.sample_rate,
        duration_s: sc.duration_s,
        start_unix: sc.start_unix,
        seed: sc.seed,
        c: sc.atmosphere.c,
        wind_ecef: wind,
        output_scale,
        events,
        tracks,
    };
    Rendered { samples, sample_rate: sc.sample_rate, truth }
}

pub fn render_toml(s: &str) -> anyhow::Result<Rendered> {
    Ok(render(&Scenario::from_toml_str(s)?))
}

/// Writes `audio.wav` (16-bit mono PCM) and `truth.json` into `dir` (created if missing).
pub fn write_outputs(r: &Rendered, dir: &FsPath) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: r.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(dir.join("audio.wav"), spec)?;
    for &x in &r.samples {
        w.write_sample((x * i16::MAX as f32).round() as i16)?;
    }
    w.finalize()?;
    std::fs::write(dir.join("truth.json"), r.truth.to_json()?)?;
    Ok(())
}
