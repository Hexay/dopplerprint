//! Decides, per tile, which cached or remote source file feeds the raster (first available in preference
//! order, as dem_sources.py does) and what has to be downloaded.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::tiles::{DemSource, Tile};

/// A source file the build needs; `bytes` is the server's size when known.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedFile {
    pub tile: Tile,
    pub source: DemSource,
    pub url: String,
    pub path: PathBuf,
    pub bytes: Option<u64>,
}

/// Downloads `ensure_dem` is about to make; handed to the confirmation hook.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DownloadPlan {
    pub files: Vec<PlannedFile>,
}

impl DownloadPlan {
    /// Sum of the known sizes.
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().filter_map(|f| f.bytes).sum()
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Resolved {
    Cached(DemSource, PathBuf),
    /// No source has the tile (open ocean); its nodes stay nodata.
    Ocean,
    Download(PlannedFile),
    /// Not cached and not checked, because we are offline.
    Unknown(PlannedFile),
}

fn missing_marker(path: &Path) -> PathBuf {
    path.with_extension("missing")
}

/// First source with a cached file. A higher-preference source that is neither cached nor known missing is
/// probed (`online`) or reported as `Unknown`, never skipped, so the result does not depend on cache state.
pub(super) fn resolve_tile(tile: Tile, dir: &Path, sources: &[DemSource], online: bool) -> Result<Resolved> {
    for &source in sources {
        let path = dir.join(source.file_name(tile));
        if path.is_file() {
            return Ok(Resolved::Cached(source, path));
        }
        if missing_marker(&path).exists() {
            continue;
        }
        let planned = PlannedFile { tile, source, url: source.url(tile), path, bytes: None };
        if !online {
            return Ok(Resolved::Unknown(planned));
        }
        match probe(&planned.url)? {
            Some(bytes) => return Ok(Resolved::Download(PlannedFile { bytes, ..planned })),
            None => mark_missing(&planned.path)?,
        }
    }
    Ok(Resolved::Ocean)
}

/// Records a 403/404 so later runs (and offline ones) skip this source for the tile.
pub(super) fn mark_missing(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path.parent().expect("tile paths live in a directory"))?;
    std::fs::write(missing_marker(path), b"")?;
    Ok(())
}

/// `Some(size)` if the URL exists, `None` if the server says it does not.
#[cfg(feature = "net")]
fn probe(url: &str) -> Result<Option<Option<u64>>> {
    use super::download::{Remote, probe};
    Ok(match probe(url)? {
        Remote::Available(n) => Some(n),
        Remote::Missing => None,
    })
}

#[cfg(not(feature = "net"))]
fn probe(url: &str) -> Result<Option<Option<u64>>> {
    anyhow::bail!("{url}: built without the `net` feature, cannot download")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_markers_offline() {
        let dir = tempfile::tempdir().unwrap();
        let t = Tile { lat: 37, lon: -122 };
        let srcs = [DemSource::Tdep1, DemSource::Glo30];
        let glo = dir.path().join(DemSource::Glo30.file_name(t));
        std::fs::write(&glo, b"II*\0").unwrap();
        match resolve_tile(t, dir.path(), &srcs, false).unwrap() {
            Resolved::Unknown(p) => assert_eq!(p.source, DemSource::Tdep1, "3DEP unknown is not skipped"),
            r => panic!("{r:?}"),
        }
        mark_missing(&dir.path().join(DemSource::Tdep1.file_name(t))).unwrap();
        assert_eq!(resolve_tile(t, dir.path(), &srcs, false).unwrap(), Resolved::Cached(DemSource::Glo30, glo.clone()));
        std::fs::remove_file(&glo).unwrap();
        mark_missing(&glo).unwrap();
        assert_eq!(resolve_tile(t, dir.path(), &srcs, false).unwrap(), Resolved::Ocean);
        let tdep = dir.path().join(DemSource::Tdep1.file_name(t));
        std::fs::write(&tdep, b"II*\0").unwrap();
        assert_eq!(resolve_tile(t, dir.path(), &srcs, false).unwrap(), Resolved::Cached(DemSource::Tdep1, tdep));
    }

    #[test]
    fn plan_total() {
        let f = |b| PlannedFile {
            tile: Tile { lat: 0, lon: 0 },
            source: DemSource::Glo30,
            url: String::new(),
            path: PathBuf::new(),
            bytes: b,
        };
        assert_eq!(DownloadPlan { files: vec![f(Some(5)), f(None), f(Some(7))] }.total_bytes(), 12);
    }
}
