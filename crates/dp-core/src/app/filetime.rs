//! Recording time from the file itself: the MP4/M4A/MOV `mvhd` creation_time, else a date tag
//! (ID3 TDRC, Vorbis DATE, iTunes ©day, RIFF ICRD) that carries a time and a UTC offset.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey, Tag};
use symphonia::core::probe::Hint;

use super::when::{InstantError, parse_instant};

/// Seconds from 1904-01-01 (QuickTime epoch) to 1970-01-01.
const MAC_EPOCH: u64 = 2_082_844_800;

#[derive(Clone, Debug, PartialEq)]
pub enum FileTime {
    /// UNIX seconds and where in the file it came from.
    Found(f64, String),
    /// A date-time without a UTC offset, verbatim.
    NoZone(String),
    Unknown,
}

pub fn recording_time(path: &Path) -> FileTime {
    if let Some(t) = mp4_creation_time(path) {
        return FileTime::Found(t, "MP4 creation_time".into());
    }
    let mut no_zone = None;
    for (key, value) in date_tags(path) {
        match parse_instant(&value) {
            Ok(t) if plausible(t) => return FileTime::Found(t, format!("{key} tag")),
            Err(InstantError::NoZone) => no_zone = no_zone.or(Some(value)),
            _ => {}
        }
    }
    no_zone.map_or(FileTime::Unknown, FileTime::NoZone)
}

fn plausible(t: f64) -> bool {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(f64::MAX, |d| d.as_secs_f64());
    t > 1_262_304_000.0 && t < now + 86_400.0
}

fn date_tags(path: &Path) -> Vec<(String, String)> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let Ok(mut probed) =
        symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
    else {
        return Vec::new();
    };
    let mut tags: Vec<Tag> = probed.metadata.get().and_then(|m| m.current().map(|r| r.tags().to_vec())).unwrap_or_default();
    tags.extend(probed.format.metadata().current().map(|r| r.tags().to_vec()).unwrap_or_default());
    tags.into_iter()
        .filter(|t| {
            matches!(t.std_key, Some(StandardTagKey::Date | StandardTagKey::OriginalDate | StandardTagKey::EncodingDate))
                || ["creation_time", "TDRC", "ICRD", "DATE", "date"].contains(&t.key.as_str())
        })
        .map(|t| (t.key.clone(), t.value.to_string()))
        .collect()
}

/// `moov/mvhd` creation time, when set and plausible.
pub fn mp4_creation_time(path: &Path) -> Option<f64> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let (moov, moov_len) = find_box(&mut f, 0, len, b"moov")?;
    let (mvhd, _) = find_box(&mut f, moov, moov + moov_len, b"mvhd")?;
    f.seek(SeekFrom::Start(mvhd)).ok()?;
    let mut v = [0u8; 12];
    f.read_exact(&mut v).ok()?;
    let ct = if v[0] == 1 { u64::from_be_bytes(v[4..12].try_into().ok()?) } else { u32::from_be_bytes(v[4..8].try_into().ok()?) as u64 };
    let t = ct.checked_sub(MAC_EPOCH)? as f64;
    plausible(t).then_some(t)
}

/// Payload offset and length of the first `kind` box among the boxes in `[start, end)`.
fn find_box(f: &mut File, start: u64, end: u64, kind: &[u8; 4]) -> Option<(u64, u64)> {
    let mut at = start;
    for _ in 0..4096 {
        if at + 8 > end {
            return None;
        }
        f.seek(SeekFrom::Start(at)).ok()?;
        let mut h = [0u8; 16];
        f.read_exact(&mut h[..8]).ok()?;
        let (mut size, mut header) = (u32::from_be_bytes(h[..4].try_into().ok()?) as u64, 8);
        if size == 1 {
            f.read_exact(&mut h[8..16]).ok()?;
            size = u64::from_be_bytes(h[8..16].try_into().ok()?);
            header = 16;
        } else if size == 0 {
            size = end - at;
        }
        if size < header || at + size > end {
            return None;
        }
        if &h[4..8] == kind {
            return Some((at + header, size - header));
        }
        at += size;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        [&((payload.len() + 8) as u32).to_be_bytes()[..], kind, payload].concat()
    }

    #[test]
    fn mvhd_creation_time() {
        let t: u64 = 1_778_778_120;
        let mut mvhd = vec![0u8; 100];
        mvhd[4..8].copy_from_slice(&((t + MAC_EPOCH) as u32).to_be_bytes());
        let file = [bx(b"ftyp", b"M4A \0\0\0\0"), bx(b"mdat", &[0; 64]), bx(b"moov", &[bx(b"trak", &[0; 8]), bx(b"mvhd", &mvhd)].concat())].concat();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("memo.m4a");
        std::fs::write(&p, &file).unwrap();
        assert_eq!(mp4_creation_time(&p), Some(t as f64));
        std::fs::write(&p, &file[..file.len() - 20]).unwrap();
        assert_eq!(mp4_creation_time(&p), None);
    }

    #[test]
    fn plain_wav_has_no_time() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        (0..800).for_each(|_| w.write_sample(0i16).unwrap());
        w.finalize().unwrap();
        assert_eq!(recording_time(&p), FileTime::Unknown);
    }
}
