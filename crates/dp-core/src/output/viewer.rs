//! Single-file Leaflet viewer (assets/viewer.html) with the result GeoJSON and an optional spectrogram embedded.

use serde_json::json;

use super::features::Results;
use super::geojson::feature_collection;

const TEMPLATE: &str = include_str!("../../../../assets/viewer.html");
const SLOT: &str = "/*DATA*/null";

/// A pre-rendered spectrogram PNG and the axis extents it covers (audio-relative seconds, Hz).
#[derive(Clone, Debug)]
pub struct SpectrogramImage {
    pub png: Vec<u8>,
    pub t0_s: f64,
    pub t1_s: f64,
    pub f_lo_hz: f64,
    pub f_hi_hz: f64,
    pub log_freq: bool,
}

pub fn render(r: &Results, spec: Option<&SpectrogramImage>) -> String {
    let spec = spec.map(|s| {
        json!({ "png_b64": base64(&s.png), "t0_s": s.t0_s, "t1_s": s.t1_s, "f_lo_hz": s.f_lo_hz,
                "f_hi_hz": s.f_hi_hz, "log_freq": s.log_freq })
    });
    let data = json!({ "fc": feature_collection(r), "spectrogram": spec }).to_string();
    // A literal "</script>" inside the embedded JSON would close the script element early.
    TEMPLATE.replacen(SLOT, &data.replace("</", "<\\/"), 1)
}

pub fn write(path: &std::path::Path, r: &Results, spec: Option<&SpectrogramImage>) -> anyhow::Result<()> {
    std::fs::write(path, render(r, spec))?;
    Ok(())
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::sample::demo_results;

    #[test]
    fn base64_rfc4648_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn template_slot_is_filled_and_escaped() {
        assert_eq!(TEMPLATE.matches(SLOT).count(), 1);
        let mut r = demo_results();
        r.run.note = "</script><b>".into();
        let html = render(&r, None);
        assert!(!html.contains(SLOT));
        assert_eq!(html.matches("</script>").count(), TEMPLATE.matches("</script>").count());
    }
}
