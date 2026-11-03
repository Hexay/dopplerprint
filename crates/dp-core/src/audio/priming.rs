//! Leading encoder delay that symphonia leaves in the decoded stream (AAC priming, Opus pre-skip), in
//! samples at the codec's output rate. MP3/Vorbis/FLAC gapless trims are applied by symphonia itself.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::codecs::{CODEC_TYPE_AAC, CODEC_TYPE_OPUS, CodecParameters};
use symphonia::core::formats::FormatReader;

/// AAC-LC encoder delay when the container says nothing (ffmpeg, Apple and FDK all use 1024).
const AAC_DEFAULT_PRIMING: u64 = 1024;

pub(super) fn leading_trim(path: &Path, reader: &mut dyn FormatReader, params: &CodecParameters, sr: u32) -> u64 {
    if params.codec == CODEC_TYPE_OPUS {
        return opus_pre_skip(params.extra_data.as_deref()).or(params.delay.map(u64::from)).unwrap_or(0);
    }
    if params.codec == CODEC_TYPE_AAC {
        return mp4_edit_start(path, sr)
            .or_else(|| itunsmpb_priming(reader))
            .or(params.delay.map(u64::from))
            .unwrap_or(AAC_DEFAULT_PRIMING);
    }
    0
}

/// Pre-skip from an RFC 7845 OpusHead (Ogg and Matroska CodecPrivate both carry it).
fn opus_pre_skip(head: Option<&[u8]>) -> Option<u64> {
    let h = head?;
    (h.len() >= 12 && &h[..8] == b"OpusHead").then(|| u16::from_le_bytes([h[10], h[11]]) as u64)
}

/// iTunes gapless tag: " 00000000 PPPPPPPP EEEEEEEE LLLLLLLLLLLLLLLL ..." with hex priming P.
fn itunsmpb_priming(reader: &mut dyn FormatReader) -> Option<u64> {
    let meta = reader.metadata();
    let rev = meta.current()?;
    let tag = rev.tags().iter().find(|t| t.key.ends_with("iTunSMPB"))?;
    let v = tag.value.to_string();
    u64::from_str_radix(v.split_whitespace().nth(1)?, 16).ok()
}

fn read_box(f: &mut File, end: u64) -> Option<([u8; 4], u64, u64)> {
    let pos = f.stream_position().ok()?;
    if pos + 8 > end {
        return None;
    }
    let mut h = [0u8; 8];
    f.read_exact(&mut h).ok()?;
    let mut size = u32::from_be_bytes(h[..4].try_into().ok()?) as u64;
    let mut body = pos + 8;
    if size == 1 {
        let mut l = [0u8; 8];
        f.read_exact(&mut l).ok()?;
        size = u64::from_be_bytes(l);
        body += 8;
    } else if size == 0 {
        size = end - pos;
    }
    (size >= body - pos).then(|| (h[4..8].try_into().expect("4 bytes"), body, pos + size))
}

/// Child boxes of `[start, end)` as (type, body start, end).
fn children(f: &mut File, start: u64, end: u64) -> Vec<([u8; 4], u64, u64)> {
    let mut out = Vec::new();
    let mut at = start;
    while f.seek(SeekFrom::Start(at)).is_ok() {
        let Some(b) = read_box(f, end) else { break };
        at = b.2;
        out.push(b);
    }
    out
}

fn body(f: &mut File, start: u64, end: u64) -> Option<Vec<u8>> {
    let mut v = vec![0u8; usize::try_from(end.checked_sub(start)?).ok()?.min(1 << 20)];
    f.seek(SeekFrom::Start(start)).ok()?;
    f.read_exact(&mut v).ok()?;
    Some(v)
}

fn find(bs: &[([u8; 4], u64, u64)], t: &[u8; 4]) -> Option<(u64, u64)> {
    bs.iter().find(|b| &b.0 == t).map(|b| (b.1, b.2))
}

/// media_time of the first non-empty edit of the sound track (moov/trak/edts/elst), rescaled from the
/// mdhd timescale to `sr`. This is where ffmpeg and Apple record the priming.
fn mp4_edit_start(path: &Path, sr: u32) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let (ms, me) = find(&children(&mut f, 0, len), b"moov")?;
    for (t, ts, te) in children(&mut f, ms, me) {
        if &t != b"trak" {
            continue;
        }
        let trak = children(&mut f, ts, te);
        let (ds, de) = find(&trak, b"mdia")?;
        let mdia = children(&mut f, ds, de);
        let hdlr = find(&mdia, b"hdlr").and_then(|(s, e)| body(&mut f, s, e))?;
        if hdlr.get(8..12) != Some(b"soun") {
            continue;
        }
        let mdhd = find(&mdia, b"mdhd").and_then(|(s, e)| body(&mut f, s, e))?;
        let ts_off = if mdhd[0] == 1 { 20 } else { 12 };
        let scale = u32::from_be_bytes(mdhd.get(ts_off..ts_off + 4)?.try_into().ok()?) as u64;
        let (es, ee) = find(&trak, b"edts")?;
        let elst = find(&children(&mut f, es, ee), b"elst").and_then(|(s, e)| body(&mut f, s, e))?;
        let (v1, n) = (elst[0] == 1, u32::from_be_bytes(elst.get(4..8)?.try_into().ok()?) as usize);
        let entry = if v1 { 20 } else { 12 };
        for i in 0..n {
            let o = 8 + i * entry;
            let mt = if v1 {
                i64::from_be_bytes(elst.get(o + 8..o + 16)?.try_into().ok()?)
            } else {
                i32::from_be_bytes(elst.get(o + 4..o + 8)?.try_into().ok()?) as i64
            };
            if mt >= 0 && scale > 0 {
                return Some((mt as u64 * sr as u64 + scale / 2) / scale);
            }
        }
        return None;
    }
    None
}
