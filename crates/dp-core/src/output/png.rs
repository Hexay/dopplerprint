//! Minimal 8-bit greyscale PNG encoder and the spectrogram image embedded in the viewer.

use std::io::Write;

use super::viewer::SpectrogramImage;
use crate::audio::LogSpectrogram;

/// Whitened z range mapped to black..white.
const Z_LO: f32 = -1.0;
const Z_HI: f32 = 6.0;

/// Render a whitened spectrogram, high frequencies at the top, max-pooled to at most `max_w` columns.
pub fn spectrogram_image(s: &LogSpectrogram, max_w: usize) -> SpectrogramImage {
    let pool = s.n_t.div_ceil(max_w.max(1)).max(1);
    let w = s.n_t.div_ceil(pool).max(1);
    let h = s.n_f.max(1);
    let mut px = vec![0u8; w * h];
    for x in 0..w {
        let frames = x * pool..((x + 1) * pool).min(s.n_t);
        for b in 0..s.n_f {
            let v = frames.clone().map(|t| s.data[t * s.n_f + b]).fold(f32::MIN, f32::max);
            let g = ((v - Z_LO) / (Z_HI - Z_LO)).clamp(0.0, 1.0);
            px[(h - 1 - b) * w + x] = (255.0 * g) as u8;
        }
    }
    SpectrogramImage {
        png: encode_grey(&px, w, h),
        t0_s: s.t0,
        t1_s: s.time_of(s.n_t.saturating_sub(1)),
        f_lo_hz: s.f_min,
        f_hi_hz: s.freq_of(s.n_f as f64),
        log_freq: true,
    }
}

/// `px` is row-major, `w × h`, one byte per pixel.
pub fn encode_grey(px: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w + 1) * h);
    for row in px.chunks_exact(w) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).expect("in-memory write");
    let idat = z.finish().expect("in-memory write");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(b"IHDR", &ihdr[..]), (b"IDAT", &idat[..]), (b"IEND", &[][..])] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc.sum().to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_structure_and_inflate_roundtrip() {
        let px: Vec<u8> = (0..6).map(|i| i * 40).collect();
        let png = encode_grey(&px, 3, 2);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        // IEND chunk with its well-known CRC.
        assert_eq!(&png[png.len() - 12..], b"\0\0\0\0IEND\xae\x42\x60\x82");
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&png[41..41 + idat_len]), &mut raw).unwrap();
        assert_eq!(raw, [0, 0, 40, 80, 0, 120, 160, 200]);
    }

    #[test]
    fn spectrogram_pooling_and_flip() {
        let mut s = LogSpectrogram { t0: 0.0, dt: 0.25, f_min: 20.0, bins_per_octave: 96, n_t: 10, n_f: 4, data: vec![-5.0; 40] };
        s.data[3 * 4] = 6.0;
        let img = spectrogram_image(&s, 5);
        assert_eq!(u32::from_be_bytes(img.png[16..20].try_into().unwrap()), 5);
        assert_eq!(u32::from_be_bytes(img.png[20..24].try_into().unwrap()), 4);
        assert!(img.log_freq && img.f_hi_hz > 20.0);
    }
}
