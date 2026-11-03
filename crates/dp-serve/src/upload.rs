//! Untrusted upload intake: magic-byte sniffing, size-limited streaming to disk, display-name sanitising.

use std::io::{self, Read, Write};
use std::path::Path;

#[derive(Debug)]
pub enum UploadError {
    TooLarge,
    NotAudio,
    Empty,
    Io(io::Error),
}

impl From<io::Error> for UploadError {
    fn from(e: io::Error) -> Self {
        UploadError::Io(e)
    }
}

/// Container type from the first 12 bytes, as the extension the decoder probes by.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    let at = |i: usize, m: &[u8]| head.len() >= i + m.len() && &head[i..i + m.len()] == m;
    if at(0, b"RIFF") && at(8, b"WAVE") {
        Some("wav")
    } else if at(0, b"fLaC") {
        Some("flac")
    } else if at(0, b"OggS") {
        Some("ogg")
    } else if at(0, b"ID3") {
        Some("mp3")
    } else if at(4, b"ftyp") {
        Some("m4a")
    } else if head.len() >= 2 && head[0] == 0xFF && head[1] & 0xF6 == 0xF0 {
        Some("aac")
    } else if head.len() >= 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0 {
        Some("mp3")
    } else {
        None
    }
}

/// Streams `body` to `dir/audio.<ext>`. Returns the stored file name and byte count.
pub fn save_audio(body: &mut dyn Read, limit: u64, dir: &Path) -> Result<(String, u64), UploadError> {
    let mut head = [0u8; 12];
    let mut n = 0;
    while n < head.len() {
        match body.read(&mut head[n..])? {
            0 => break,
            k => n += k,
        }
    }
    if n == 0 {
        return Err(UploadError::Empty);
    }
    let ext = sniff(&head[..n]).ok_or(UploadError::NotAudio)?;
    let name = format!("audio.{ext}");
    let mut out = io::BufWriter::new(std::fs::File::create(dir.join(&name))?);
    out.write_all(&head[..n])?;
    let rest = io::copy(&mut body.take(limit.saturating_sub(n as u64) + 1), &mut out)?;
    out.flush()?;
    let total = n as u64 + rest;
    if total > limit {
        return Err(UploadError::TooLarge);
    }
    Ok((name, total))
}

/// Percent-decodes and reduces a client-supplied file name to a safe display string.
pub fn display_name(raw: &str) -> String {
    let decoded = percent_decode(raw);
    let base = decoded.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() || " .-_()".contains(c) { c } else { '_' })
        .take(120)
        .collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    if clean.is_empty() { "recording".into() } else { clean }
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_common_containers() {
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), Some("wav"));
        assert_eq!(sniff(b"ID3\x04\0\0\0\0\0\0\0\0"), Some("mp3"));
        assert_eq!(sniff(&[0xFF, 0xFB, 0x90, 0x64]), Some("mp3"));
        assert_eq!(sniff(&[0xFF, 0xF1, 0x50, 0x80]), Some("aac"));
        assert_eq!(sniff(b"\0\0\0\x20ftypM4A "), Some("m4a"));
        assert_eq!(sniff(b"<html><body>"), None);
        assert_eq!(sniff(b"PK\x03\x04"), None);
    }

    #[test]
    fn display_names_are_flattened() {
        assert_eq!(display_name("..%2F..%2Fetc%2Fpasswd"), "passwd");
        assert_eq!(display_name("C:\\Users\\me\\walk.mp3"), "walk.mp3");
        assert_eq!(display_name("<script>.wav"), "_script_.wav");
        assert_eq!(display_name("..."), "recording");
        assert_eq!(display_name("Spaziergang%20%C3%BC.m4a"), "Spaziergang ü.m4a");
    }
}
