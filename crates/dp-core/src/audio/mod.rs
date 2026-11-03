//! Audio decode and the whitened log-frequency spectrogram every scorer reads.

pub mod spectrogram;

pub use spectrogram::{LogSpectrogram, SpecParams};

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
pub fn decode(_path: &std::path::Path) -> anyhow::Result<Audio> {
    todo!("audio agent: symphonia")
}
