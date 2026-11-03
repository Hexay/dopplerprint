//! Packet → mono samples. symphonia for every codec it has; the pure-Rust `opus-decoder` for Opus, which
//! symphonia 0.5 lacks (YouTube itag 251 audio is Opus in WebM).

use anyhow::Context;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_OPUS, CodecParameters, Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::Packet;

pub(super) enum PacketDecoder {
    Symphonia { dec: Box<dyn Decoder>, buf: Option<SampleBuffer<f32>> },
    Opus { dec: Box<opus_decoder::OpusDecoder>, channels: usize, pcm: Vec<f32> },
}

/// Outcome of one packet: samples were appended, or the packet was corrupt and skipped.
pub(super) enum Decoded {
    Ok,
    Skipped,
}

impl PacketDecoder {
    pub fn new(params: &CodecParameters, sr: u32) -> anyhow::Result<Self> {
        if params.codec == CODEC_TYPE_OPUS {
            let channels = params
                .extra_data
                .as_deref()
                .filter(|h| h.len() >= 10 && &h[..8] == b"OpusHead")
                .map(|h| h[9] as usize)
                .or(params.channels.map(|c| c.count()))
                .or(params.channel_layout.map(|l| l.into_channels().count()))
                .unwrap_or(1);
            let dec = opus_decoder::OpusDecoder::new(sr, channels)
                .map_err(|e| anyhow::anyhow!("opus decoder: {e}"))
                .context("Opus streams with more than 2 channels or non-Opus rates are not supported")?;
            let pcm = vec![0.0; opus_decoder::OpusDecoder::MAX_FRAME_SIZE_48K * channels];
            return Ok(Self::Opus { dec: Box::new(dec), channels, pcm });
        }
        let dec = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
        Ok(Self::Symphonia { dec, buf: None })
    }

    /// Decode one packet and append its channel-mean downmix to `mono`.
    pub fn decode_mono(&mut self, packet: &Packet, sr: u32, mono: &mut Vec<f32>) -> anyhow::Result<Decoded> {
        match self {
            Self::Opus { dec, channels, pcm } => {
                let Ok(n) = dec.decode_float(&packet.data, pcm, false) else { return Ok(Decoded::Skipped) };
                downmix(&pcm[..n * *channels], *channels, mono);
            }
            Self::Symphonia { dec, buf } => {
                let b = match dec.decode(packet) {
                    Ok(b) => b,
                    Err(Error::DecodeError(_) | Error::IoError(_)) => return Ok(Decoded::Skipped),
                    Err(e) => return Err(e.into()),
                };
                let spec = *b.spec();
                anyhow::ensure!(spec.rate == sr, "sample rate changes mid-stream ({sr} -> {})", spec.rate);
                let n_ch = spec.channels.count().max(1);
                if buf.as_ref().is_none_or(|s| s.capacity() < b.capacity() * n_ch) {
                    *buf = Some(SampleBuffer::new(b.capacity() as u64, spec));
                }
                let sb = buf.as_mut().expect("allocated above");
                sb.copy_interleaved_ref(b);
                downmix(sb.samples(), n_ch, mono);
            }
        }
        Ok(Decoded::Ok)
    }
}

fn downmix(interleaved: &[f32], n_ch: usize, mono: &mut Vec<f32>) {
    let scale = 1.0 / n_ch as f32;
    mono.extend(interleaved.chunks_exact(n_ch).map(|fr| fr.iter().sum::<f32>() * scale));
}
