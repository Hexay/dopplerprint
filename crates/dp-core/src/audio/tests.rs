use std::path::{Path, PathBuf};

use super::resample::{decimate_for, decimation_factor};
use super::*;

fn tone(sr: u32, secs: f64, f: f64, amp: f32) -> Vec<f32> {
    (0..(sr as f64 * secs) as usize)
        .map(|i| amp * (2.0 * std::f64::consts::PI * f * i as f64 / sr as f64).sin() as f32)
        .collect()
}

fn noise(n: usize, sigma: f32, mut seed: u64) -> Vec<f32> {
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| sigma * ((-2.0 * next().max(1e-300).ln()).sqrt() * (2.0 * std::f64::consts::PI * next()).cos()) as f32)
        .collect()
}

fn tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dp_audio_{}_{name}", std::process::id()))
}

fn write_wav16(path: &Path, sr: u32, channels: u16, interleaved: &[f32]) {
    let data: Vec<u8> = interleaved
        .iter()
        .flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&sr.to_le_bytes());
    b.extend_from_slice(&(sr * channels as u32 * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    std::fs::write(path, b).unwrap();
}

fn peak_freq(spec: &LogSpectrogram) -> f64 {
    let mid = spec.n_t / 2;
    let row = spec.row(mid);
    let b = (0..spec.n_f).max_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap();
    spec.freq_of(b as f64)
}

#[test]
fn decode_wav_stereo_downmix_and_rate() {
    let sr = 22050;
    let left = tone(sr, 2.0, 300.0, 0.5);
    let inter: Vec<f32> = left.iter().flat_map(|&x| [x, 0.6 * x]).collect();
    let left: Vec<f32> = left.iter().map(|x| 0.8 * x).collect();
    let p = tmp_path("stereo.wav");
    write_wav16(&p, sr, 2, &inter);
    let a = decode(&p).unwrap();
    let info = probe(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(a.sample_rate, 22050);
    assert_eq!(info.sample_rate, 22050);
    assert_eq!(info.channels, 2);
    assert_eq!(a.samples.len(), left.len());
    let err = a
        .samples
        .iter()
        .zip(&left)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max);
    assert!(err < 1e-3, "downmix error {err}");
}

#[test]
fn decode_mp3_fixture() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/audio/testdata/tone250.mp3");
    let a = decode(&p).unwrap();
    assert_eq!(a.sample_rate, 16000);
    assert!((a.duration_s() - 4.0).abs() < 0.1, "duration {}", a.duration_s());
    let (band, info) = decode_band(&p, 1000.0).unwrap();
    assert_eq!(info.sample_rate, 16000);
    assert_eq!(band.sample_rate, 16000 / decimation_factor(16000, 1000.0));
    let spec = log_power_spectrogram(
        &band,
        &SpecParams {
            win_s: 0.5,
            hop_s: 0.125,
            ..Default::default()
        },
    );
    assert!((peak_freq(&spec) / 250.0 - 1.0).abs() < 0.006);
}

#[test]
fn decimation_is_exact_rate_and_rejects_aliases() {
    assert_eq!(decimation_factor(48000, 1000.0), 16);
    assert_eq!(decimation_factor(44100, 1000.0), 15);
    assert_eq!(decimation_factor(8000, 1000.0), 2);
    let sr = 48000;
    let pass = Audio {
        sample_rate: sr,
        samples: tone(sr, 4.0, 300.0, 0.5),
    };
    let alias = Audio {
        sample_rate: sr,
        samples: tone(sr, 4.0, 2500.0, 0.5),
    };
    let rms = |a: &Audio| {
        let d = decimate_for(a, 1000.0);
        assert_eq!(d.sample_rate, 3000);
        let s = &d.samples[3000..9000];
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    };
    let (rp, ra) = (rms(&pass), rms(&alias));
    assert!((rp / (0.5 / 2f32.sqrt()) - 1.0).abs() < 0.01, "passband rms {rp}");
    assert!(ra / rp < 1e-3, "alias leak {}", ra / rp);
}

#[test]
fn log_axis_peak_and_low_frequency_interpolation() {
    let sr = 4000;
    for f in [23.0, 61.0, 140.0, 777.0] {
        let mut s = tone(sr, 20.0, f, 0.3);
        let n = noise(s.len(), 0.05, 7);
        s.iter_mut().zip(n).for_each(|(x, n)| *x += n);
        let spec = log_power_spectrogram(
            &Audio {
                sample_rate: sr,
                samples: s,
            },
            &SpecParams::default(),
        );
        let pf = peak_freq(&spec);
        assert!((pf / f - 1.0).abs() < 0.008, "{f} Hz peaked at {pf}");
    }
}

#[test]
fn whitening_is_gain_invariant() {
    let sr = 4000;
    let mut s = tone(sr, 60.0, 120.0, 0.1);
    let n = noise(s.len(), 0.2, 3);
    s.iter_mut().zip(n).for_each(|(x, n)| *x += n);
    let loud: Vec<f32> = s.iter().map(|x| x * 1000.0).collect();
    let p = SpecParams::default();
    let w = WhitenParams {
        mains_hz: Some(60.0),
        ..Default::default()
    };
    let a = spectrogram::log_spectrogram_with(
        &Audio {
            sample_rate: sr,
            samples: s,
        },
        &p,
        &w,
    );
    let b = spectrogram::log_spectrogram_with(
        &Audio {
            sample_rate: sr,
            samples: loud,
        },
        &p,
        &w,
    );
    let diff = a
        .data
        .iter()
        .zip(&b.data)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max);
    assert!(diff < 1e-3, "max |Δz| = {diff}");
    let notch = a.bin_of(120.0).round() as usize;
    assert_eq!(a.row(a.n_t / 2)[notch], 0.0, "120 Hz mains harmonic notched");
}

#[test]
fn whitened_noise_is_standardised() {
    let sr = 4000;
    let s = noise(sr as usize * 120, 0.1, 11);
    let spec = log_spectrogram(
        &Audio {
            sample_rate: sr,
            samples: s,
        },
        &SpecParams::default(),
    );
    let n = spec.data.len() as f64;
    let mean = spec.data.iter().map(|&x| x as f64).sum::<f64>() / n;
    let sd = (spec.data.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / n).sqrt();
    assert!(mean.abs() < 0.2 && (sd - 1.0).abs() < 0.25, "mean {mean} sd {sd}");
}

#[test]
fn multi_window_shares_grid() {
    let sr = 4000;
    let mut s: Vec<f32> = (0..sr as usize * 30)
        .map(|i| {
            let t = i as f64 / sr as f64;
            (0.01 * (2.0 * std::f64::consts::PI * (150.0 * t + 50.0 * t * t / 30.0)).sin()) as f32
        })
        .collect();
    let n = noise(s.len(), 0.05, 5);
    s.iter_mut().zip(n).for_each(|(x, n)| *x += n);
    let a = Audio {
        sample_rate: sr,
        samples: s,
    };
    let specs = log_spectrogram_multi(&a, &SpecParams::default(), &WhitenParams::default(), &[0.5, 1.0, 2.0]);
    assert!(specs.iter().all(|s| s.n_t == specs[0].n_t && s.n_f == specs[0].n_f));
    let c = combine_max(&specs).unwrap();
    assert!((peak_freq(&c) / 200.0 - 1.0).abs() < 0.008);
}

#[test]
fn parallel_decimation_equals_streaming() {
    use super::resample::Decimator;
    for (n, q) in [(1, 3), (50, 4), (99_999, 6), (300_001, 12)] {
        let x = noise(n, 0.3, n as u64);
        let mut d = Decimator::new(48_000, q, 1_500.0);
        let want = d.decimate_all(&x);
        let mut got = Vec::new();
        for chunk in x.chunks(7_919) {
            d.push(chunk, &mut got);
        }
        d.finish(&mut got);
        assert_eq!(got, want, "n {n} q {q}");
    }
}
