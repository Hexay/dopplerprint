//! Independent synthetic flyover generator (time-domain phase integration), used as test truth.
//! Deliberately shares no code with physics::doppler_factors beyond Track::state_at.
//! Model and scenario schema: see docs/PHYSICS.md §6.

pub mod chain;
pub mod noise;
pub mod propagate;
pub mod scenario;
pub mod source;
pub mod trajectory;
pub mod truth;

use std::path::Path as FsPath;

pub use scenario::Scenario;
pub use truth::Truth;

use chain::modulation::DelayJitter;
use chain::{Chain, Channels, Stream};
use noise::Pcg;
use propagate::{Path, render_path};
use source::Tone;
use trajectory::Frame;

/// Rendered clip: samples in [-1, 1] (pre-quantisation) and the truth record.
pub struct Rendered {
    /// Mono view the pipeline consumes (the channel mean, as the decoder would produce).
    pub samples: Vec<f32>,
    /// Left/right when `[chain.device] channels` is not mono.
    pub stereo: Option<[Vec<f32>; 2]>,
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
    let default_chain = Chain::default();
    let ch = sc.chain.as_ref().unwrap_or(&default_chain);

    let mut mix = noise::background(n, fs, &sc.noise, &mut rng);
    noise::add_hum(&mut mix, fs, &sc.noise);
    let mut chain_truth = chain::add_environment(&mut mix, fs, ch, sc.noise.snr_db, sc.seed);
    let dev = &ch.device;
    let mut right = (dev.channels == Channels::Stereo).then(|| mix.clone());
    let (sa, ca) = dev.stereo_axis_deg.to_radians().sin_cos();
    let right_offset = frame.enu(sa, ca, 0.0) * dev.stereo_baseline_m;
    let (mut src_rng, mut path_rng) = (chain::rng(sc.seed, Stream::Source), chain::rng(sc.seed, Stream::Path));

    let mut events = Vec::new();
    let mut tracks = Vec::new();
    for a in &sc.aircraft {
        let local = trajectory::build_track(sc, &frame, a);
        let ev = truth::event(sc, &frame, wind, a, &local);
        let src = chain::source_mod::effective(
            &a.source,
            ch.source.as_ref(),
            ev.cpa_arrival_audio_s,
            sc.duration_s,
            &mut src_rng,
        );
        let tone = Tone::new(&src, &mut rng, &mut src_rng);
        let jitter = ch
            .path
            .as_ref()
            .filter(|p| p.ground_coherence < 1.0)
            .map(|p| DelayJitter::new(&mut path_rng, p.ground_coherence, p.turbulence_hz));
        let mut paths = vec![(frame.mic, a.amplitude, None)];
        if let Some(g) = sc.ground {
            paths.push((frame.mic_image, a.amplitude * g.reflection, jitter.as_ref()));
        }
        for (obs, gain, jitter) in paths {
            let p = Path { track: &local, tone: &tone, obs, gain, c: sc.atmosphere.c, wind, jitter };
            render_path(&p, fs, &mut mix);
            if let Some(r) = right.as_mut() {
                render_path(&Path { obs: obs + right_offset, ..p }, fs, r);
            }
        }
        chain_truth.sources.push(chain::source_mod::truth(&a.id, &src));
        events.push(ev);
        tracks.push(truth::to_unix(&local, sc.start_unix));
    }

    let mut chans = vec![mix];
    chans.extend(right);
    if chans.len() == 2 {
        let mut r = chain::rng(sc.seed, Stream::SelfNoise);
        for c in &mut chans {
            let w = noise::white(n, fs, sc.noise.snr_db, dev.self_noise_db, &mut r);
            c.iter_mut().zip(w).for_each(|(x, v)| *x += v);
        }
    }
    let (outs, output_scale) = chain::device::apply(chans, fs, &sc.device, dev);
    let (samples, stereo) = match (dev.channels, outs) {
        (Channels::Mono, mut o) => (o.swap_remove(0), None),
        (Channels::DualMono, mut o) => {
            let s = o.swap_remove(0);
            (s.clone(), Some([s.clone(), s]))
        }
        (Channels::Stereo, mut o) => {
            let (r, l) = (o.pop().expect("two channels"), o.pop().expect("two channels"));
            (l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect(), Some([l, r]))
        }
    };
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
        chain: sc.chain.is_some().then_some(chain_truth),
    };
    Rendered { samples, stereo, sample_rate: sc.sample_rate, truth }
}

pub fn render_toml(s: &str) -> anyhow::Result<Rendered> {
    Ok(render(&Scenario::from_toml_str(s)?))
}

/// Writes `audio.wav` (16-bit PCM, mono or 2-channel), `truth.json` and, with chain hum, `enf.csv`
/// (audio_s, grid_hz) into `dir` (created if missing).
pub fn write_outputs(r: &Rendered, dir: &FsPath) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let spec = hound::WavSpec {
        channels: if r.stereo.is_some() { 2 } else { 1 },
        sample_rate: r.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let q = |x: f32| (x * i16::MAX as f32).round() as i16;
    let mut w = hound::WavWriter::create(dir.join("audio.wav"), spec)?;
    match &r.stereo {
        None => r.samples.iter().try_for_each(|&x| w.write_sample(q(x)))?,
        Some([l, rr]) => l.iter().zip(rr).try_for_each(|(&a, &b)| {
            w.write_sample(q(a))?;
            w.write_sample(q(b))
        })?,
    }
    w.finalize()?;
    std::fs::write(dir.join("truth.json"), r.truth.to_json()?)?;
    if let Some(h) = r.truth.chain.as_ref().and_then(|c| c.hum.as_ref()) {
        let rows: String = h.enf_hz.iter().enumerate().map(|(i, f)| format!("{:.1},{f:.6}\n", i as f64 * h.enf_dt_s)).collect();
        std::fs::write(dir.join("enf.csv"), format!("audio_s,grid_hz\n{rows}"))?;
    }
    Ok(())
}
