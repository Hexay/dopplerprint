//! Audio decode and the whitened log-frequency spectrogram every scorer reads. See docs/AUDIO.md.

mod decode;
mod frontend;
mod lines;
mod refine;
pub mod resample;
pub mod spectrogram;
mod stft;
pub mod whiten;

#[cfg(test)]
mod tests;

pub use decode::{AudioInfo, decode_band, probe};
pub use frontend::FrontEnd;
pub use refine::IfRefiner;
pub use spectrogram::{
    LogSpectrogram, SpecParams, combine_max, log_power_spectrogram, log_spectrogram, log_spectrogram_multi,
    log_spectrogram_with,
};
pub use whiten::WhitenParams;

#[derive(Clone, Debug)]
pub struct Audio {
    pub sample_rate: u32,
    /// Mono, normalised to roughly [-1, 1].
    pub samples: Vec<f32>,
}

impl Audio {
    pub fn duration_s(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

/// Decode any container symphonia supports (wav/mp3/aac/m4a/flac/ogg) and downmix to mono.
/// `sample_rate` is the container's declared rate, unmodified.
pub fn decode(path: &std::path::Path) -> anyhow::Result<Audio> {
    decode::decode_all(path, None)
}
