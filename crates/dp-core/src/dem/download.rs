//! HTTP for DEM tiles: size probe and streamed download to `.part` then rename.
//! TODO: merge with app's download helper (same ureq agent setup).

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};

/// What the server says about a tile URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remote {
    /// Exists; `Content-Length` when the server sent one.
    Available(Option<u64>),
    /// 403/404: S3 answers these for tiles that do not exist (open ocean, outside coverage).
    Missing,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_body(Some(Duration::from_secs(120)))
        .user_agent(concat!("dopplerprint/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn status(url: &str, code: u16) -> Result<Option<Remote>> {
    match code {
        200 => Ok(None),
        403 | 404 => Ok(Some(Remote::Missing)),
        _ => bail!("{url}: HTTP {code}"),
    }
}

pub fn probe(url: &str) -> Result<Remote> {
    let resp = agent().head(url).call().with_context(|| format!("HEAD {url}"))?;
    if let Some(r) = status(url, resp.status().as_u16())? {
        return Ok(r);
    }
    let len = resp.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse().ok());
    Ok(Remote::Available(len))
}

/// Streams `url` into `dst` (via `dst.part`), refusing bodies over `max_bytes` and anything that is not a
/// TIFF. `progress(done, total)` is called as bytes arrive.
pub fn download(url: &str, dst: &Path, max_bytes: u64, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<Remote> {
    let mut resp = agent().get(url).call().with_context(|| format!("GET {url}"))?;
    if let Some(r) = status(url, resp.status().as_u16())? {
        return Ok(r);
    }
    let total = resp.body().content_length();
    if let Some(t) = total {
        ensure!(t <= max_bytes, "{url}: {t} bytes exceeds the {max_bytes}-byte limit");
    }
    let part = dst.with_extension("part");
    let mut out = std::fs::File::create(&part).with_context(|| format!("creating {}", part.display()))?;
    let mut reader = resp.body_mut().with_config().limit(max_bytes).reader();
    let (mut buf, mut done) = (vec![0u8; 1 << 20], 0u64);
    let result: Result<()> = (|| {
        loop {
            let n = reader.read(&mut buf).with_context(|| format!("reading {url}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, total);
        }
        ensure!(total.is_none_or(|t| t == done), "{url}: got {done} of {total:?} bytes");
        out.sync_all()?;
        let mut head = [0u8; 4];
        std::fs::File::open(&part)?.read_exact(&mut head).ok();
        ensure!(is_tiff_magic(&head), "{url}: response is not a TIFF");
        Ok(())
    })();
    if let Err(e) = result {
        std::fs::remove_file(&part).ok();
        return Err(e);
    }
    std::fs::rename(&part, dst).with_context(|| format!("renaming {}", part.display()))?;
    Ok(Remote::Available(Some(done)))
}

/// Classic or BigTIFF header, either byte order.
pub fn is_tiff_magic(b: &[u8]) -> bool {
    matches!(b.get(..4), Some(b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn tiff_magic() {
        assert!(super::is_tiff_magic(b"II*\0\x08\0\0\0"));
        assert!(super::is_tiff_magic(b"MM\0*"));
        assert!(!super::is_tiff_magic(b"<?xml"));
        assert!(!super::is_tiff_magic(b"II"));
    }
}
