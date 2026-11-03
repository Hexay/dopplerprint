//! Blocking HTTPS (ureq + rustls) behind the `net` feature. Downloads stream to `<name>.part`, resume with a
//! Range request, are checked against the expected size and only then renamed into place.

use std::path::Path;

use anyhow::Result;

/// Bytes done and total for one file.
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

pub fn user_agent() -> String {
    format!("dopplerprint/{} (+https://github.com/Hexay/dopplerprint)", env!("CARGO_PKG_VERSION"))
}

#[cfg(feature = "net")]
mod imp {
    use std::io::{Read, Write};
    use std::path::Path;
    use std::time::Duration;

    use anyhow::{Context, Result, bail, ensure};

    use super::Progress;

    pub enum Fetched {
        Json(serde_json::Value),
        NotFound,
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .user_agent(super::user_agent())
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_per_read(Some(Duration::from_secs(60)))
            .build()
            .into()
    }

    pub fn get_json(url: &str, query: &[(&str, &str)]) -> Result<Fetched> {
        let req = agent().get(url).query_pairs(query.iter().copied());
        match req.call() {
            Ok(mut r) => Ok(Fetched::Json(serde_json::from_str(&r.body_mut().read_to_string()?)?)),
            Err(ureq::Error::StatusCode(404)) => Ok(Fetched::NotFound),
            Err(e) => Err(e).with_context(|| format!("GET {url}")),
        }
    }

    pub fn download(url: &str, dest: &Path, size: Option<u64>, progress: Progress<'_>) -> Result<()> {
        if dest.metadata().is_ok_and(|m| size.is_none_or(|s| m.len() == s)) {
            return Ok(());
        }
        let expected = size.unwrap_or(super::MAX_UNSIZED);
        let part = dest.with_extension(format!("{}.part", dest.extension().and_then(|e| e.to_str()).unwrap_or("")));
        let mut have = if size.is_some() { part.metadata().map_or(0, |m| m.len()) } else { 0 };
        if have > expected {
            std::fs::remove_file(&part)?;
            have = 0;
        }
        let mut req = agent().get(url);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        let mut resp = req.call().with_context(|| format!("GET {url}"))?;
        if resp.status().as_u16() != 206 {
            have = 0;
        }
        let mut file = std::fs::OpenOptions::new().create(true).append(have > 0).write(true).truncate(have == 0).open(&part)?;
        let mut reader = resp.body_mut().as_reader();
        let mut buf = vec![0u8; 1 << 20];
        let mut done = have;
        loop {
            let n = reader.read(&mut buf).with_context(|| format!("reading {url}"))?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > expected {
                bail!("{url} is larger than the expected {expected} bytes; refusing it");
            }
            file.write_all(&buf[..n])?;
            progress(done, size.unwrap_or(0));
        }
        file.sync_all()?;
        ensure!(size.is_none_or(|s| done == s), "{url}: got {done} of {expected} bytes (rerun to resume)");
        std::fs::rename(&part, dest)?;
        Ok(())
    }
}

#[cfg(not(feature = "net"))]
mod imp {
    use std::path::Path;

    use anyhow::{Result, bail};

    use super::Progress;

    pub enum Fetched {
        Json(serde_json::Value),
        NotFound,
    }

    const OFF: &str = "this build of dopplerprint has no network support (dp-core feature `net`)";

    pub fn get_json(_: &str, _: &[(&str, &str)]) -> Result<Fetched> {
        bail!(OFF)
    }

    pub fn download(_: &str, _: &Path, _: Option<u64>, _: Progress<'_>) -> Result<()> {
        bail!(OFF)
    }
}

pub use imp::Fetched;

/// Cap for a download whose size isn't listed in advance.
#[cfg(feature = "net")]
const MAX_UNSIZED: u64 = 100_000_000;

/// GET a JSON document; 404 is `Fetched::NotFound`, other failures are errors.
pub fn get_json(url: &str, query: &[(&str, &str)]) -> Result<Fetched> {
    imp::get_json(url, query)
}

/// Download `url` to `dest`. With a listed `size` the file must match it exactly and a previous `.part` is
/// resumed; without one it is capped at `MAX_UNSIZED`.
pub fn download(url: &str, dest: &Path, size: Option<u64>, progress: Progress<'_>) -> Result<()> {
    imp::download(url, dest, size, progress)
}
