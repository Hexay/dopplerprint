use std::fs::File;
use std::path::Path;

use anyhow::{Context, bail};
use symphonia::core::codecs::{CODEC_TYPE_NULL, CODEC_TYPE_OPUS, CodecParameters};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::Audio;
use super::packet::{Decoded, PacketDecoder};
use super::priming::leading_trim;
use super::resample::{Decimator, decimation_factor};

/// Container facts as declared, before any resampling. A 44.1 kHz file labelled 48 kHz is still reported as 48 kHz.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: usize,
    pub n_frames: Option<u64>,
    pub codec: String,
    /// Packets the decoder rejected as corrupt and skipped.
    pub skipped_packets: usize,
    /// Encoder delay removed from the start (AAC priming, Opus pre-skip), in samples at `sample_rate`.
    #[serde(default)]
    pub trimmed_samples: u64,
}

impl AudioInfo {
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

fn open(path: &Path) -> anyhow::Result<(Box<dyn FormatReader>, u32, CodecParameters)> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let fmt_opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &MetadataOptions::default())
        .with_context(|| format!("unrecognised container: {}", path.display()))?;
    let reader = probed.format;
    let track = reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .context("no audio track")?;
    let (id, params) = (track.id, track.codec_params.clone());
    Ok((reader, id, params))
}

fn info_of(params: &CodecParameters) -> anyhow::Result<AudioInfo> {
    let sample_rate = params.sample_rate.context("container declares no sample rate")?;
    let codec = if params.codec == CODEC_TYPE_OPUS {
        "opus".to_string()
    } else {
        symphonia::default::get_codecs()
            .get_codec(params.codec)
            .map_or_else(|| "unknown".to_string(), |d| d.short_name.to_string())
    };
    Ok(AudioInfo {
        sample_rate,
        channels: params.channels.map_or(0, |c| c.count()),
        n_frames: params.n_frames,
        codec,
        skipped_packets: 0,
        trimmed_samples: 0,
    })
}

/// Read the container header only.
pub fn probe(path: &Path) -> anyhow::Result<AudioInfo> {
    let (_, _, params) = open(path)?;
    info_of(&params)
}

/// Decode and decimate on the fly so that `[0, f_max]` stays alias-free. Peak RAM is the decimated signal.
/// `Audio::sample_rate` is the decimated rate; `AudioInfo::sample_rate` is the container's.
pub fn decode_band(path: &Path, f_max: f64) -> anyhow::Result<(Audio, AudioInfo)> {
    decode_inner(path, Some(f_max))
}

pub(super) fn decode_all(path: &Path, f_max: Option<f64>) -> anyhow::Result<Audio> {
    decode_inner(path, f_max).map(|(a, _)| a)
}

fn decode_inner(path: &Path, f_max: Option<f64>) -> anyhow::Result<(Audio, AudioInfo)> {
    let (mut reader, tid, params) = open(path)?;
    let mut info = info_of(&params)?;
    let sr = info.sample_rate;
    let mut decoder = PacketDecoder::new(&params, sr)?;
    info.trimmed_samples = leading_trim(path, reader.as_mut(), &params, sr);
    let mut skip = info.trimmed_samples as usize;
    let q = f_max.map_or(1, |f| decimation_factor(sr, f));
    let mut dec = (q > 1).then(|| Decimator::new(sr, q, f_max.unwrap_or(0.0)));
    let cap = params.n_frames.map_or(0, |n| (n / q as u64).min(1 << 28) as usize);
    let mut out = Vec::with_capacity(cap);
    let mut mono = Vec::new();
    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != tid {
            continue;
        }
        mono.clear();
        if let Decoded::Skipped = decoder.decode_mono(&packet, sr, &mut mono)? {
            info.skipped_packets += 1;
            continue;
        }
        let drop = skip.min(mono.len());
        skip -= drop;
        match dec.as_mut() {
            Some(d) => d.push(&mono[drop..], &mut out),
            None => out.extend_from_slice(&mono[drop..]),
        }
    }
    if let Some(d) = dec {
        d.finish(&mut out);
    }
    if out.is_empty() {
        bail!("no audio decoded from {}", path.display());
    }
    Ok((
        Audio {
            sample_rate: sr / q,
            samples: out,
        },
        info,
    ))
}
