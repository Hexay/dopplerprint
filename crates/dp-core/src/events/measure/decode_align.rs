//! M3b: decoder alignment against ffmpeg. A 48 kHz tone-plus-noise WAV is encoded by ffmpeg (AAC m4a, Opus
//! in Ogg and WebM); each file is decoded by `audio::decode` and by ffmpeg, and the lag and residual compared.

use std::path::Path;
use std::process::Command;

use super::*;
use crate::audio;

const SR: u32 = 48_000;
const VARIANTS: [(&str, &str); 4] = [
    ("aac 128k m4a", "-c:a aac -b:a 128k|m4a"),
    ("opus 135k ogg", "-c:a libopus -b:a 135k|opus"),
    ("opus 135k webm (itag 251)", "-c:a libopus -b:a 135k|webm"),
    ("mp3 128k", "-c:a libmp3lame -b:a 128k|mp3"),
];

fn ffmpeg(input: &Path, args: &str, out: &Path) -> bool {
    Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args(args.split_whitespace())
        .arg(out)
        .status()
        .is_ok_and(|s| s.success())
}

fn write_wav(path: &Path, x: &[f32]) {
    let mut b = Vec::with_capacity(44 + 2 * x.len());
    let data_len = (2 * x.len()) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | (1 << 16), SR, 2 * SR, 2 | (16 << 16)] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for &s in x {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).expect("write wav");
}

/// (lag of `b` vs `a` in samples, residual dB of a vs lag-aligned b) over the middle second.
fn align(a: &[f32], b: &[f32]) -> (i64, f64) {
    let at = a.len() / 2;
    let n = SR as usize / 2;
    let err = |lag: i64| -> (f64, f64) {
        (0..n).fold((0.0, 0.0), |(e, p), i| {
            let j = ((at + i) as i64 + lag) as usize;
            let (x, y) = (a[at + i] as f64, b.get(j).copied().unwrap_or(0.0) as f64);
            (e + (x - y).powi(2), p + x * x)
        })
    };
    let lag = (-3000..=3000).min_by(|&x, &y| err(x).0.total_cmp(&err(y).0)).unwrap_or(0);
    let (e, p) = err(lag);
    (lag, 10.0 * (e / p.max(1e-300)).log10())
}

#[test]
#[ignore]
fn decode_alignment() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/decode_align_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let f = |t: f64| 440.0 + 30.0 * (0.7 * t).sin();
    let x = synth(SR, 6.0, &f, &[0.3, 0.15, 0.1], 1.0, 0.01, 7);
    let wav = dir.join("ref.wav");
    write_wav(&wav, &x);
    let refx = audio::decode(&wav).expect("wav").samples;
    println!("| variant | ours: lag smp vs source | ours: residual dB | ffmpeg: lag smp | ours − ffmpeg lag | ours len − ffmpeg len |");
    println!("|---|---|---|---|---|---|");
    for (label, spec) in VARIANTS {
        let (args, ext) = spec.split_once('|').expect("spec");
        let enc = dir.join(format!("enc.{ext}"));
        let ffdec = dir.join(format!("ff_{ext}.wav"));
        if !ffmpeg(&wav, args, &enc) || !ffmpeg(&enc, "-c:a pcm_f32le", &ffdec) {
            println!("| {label} | ffmpeg encode/decode unavailable |");
            continue;
        }
        let ff = audio::decode(&ffdec).expect("ffmpeg wav").samples;
        let (lf, _) = align(&refx, &ff);
        match audio::decode(&enc) {
            Ok(a) => {
                let (lo, ro) = align(&refx, &a.samples);
                let dl = a.samples.len() as i64 - ff.len() as i64;
                println!("| {label} | {lo} | {ro:.1} | {lf} | {} | {dl} |", lo - lf);
            }
            Err(e) => println!("| {label} | error: {e:#} | | {lf} | | |"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
