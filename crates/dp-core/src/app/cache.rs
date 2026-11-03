//! The user cache: `--cache`, else `$DOPPLERPRINT_CACHE`, else `$XDG_CACHE_HOME/dopplerprint`, else
//! `~/.cache/dopplerprint`. Layout: docs/USAGE.md#cache.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Cache {
    pub root: PathBuf,
}

impl Cache {
    pub fn resolve(explicit: Option<&Path>) -> Result<Cache> {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let root = explicit
            .map(Path::to_path_buf)
            .or_else(|| env("DOPPLERPRINT_CACHE"))
            .or_else(|| env("XDG_CACHE_HOME").map(|p| p.join("dopplerprint")))
            .or_else(|| env("HOME").map(|p| p.join(".cache/dopplerprint")))
            .context("no cache directory: pass --cache DIR or set DOPPLERPRINT_CACHE")?;
        Ok(Cache { root })
    }

    pub fn store(&self) -> PathBuf {
        self.root.join("adsb/store")
    }

    pub fn raw(&self) -> PathBuf {
        self.root.join("adsb/raw")
    }

    pub fn typedb(&self) -> PathBuf {
        self.root.join("typedb")
    }

    pub fn dem_dir(&self) -> PathBuf {
        self.root.join("dem")
    }

    pub fn weather_dir(&self) -> PathBuf {
        self.root.join("weather")
    }

    pub fn geocode_file(&self) -> PathBuf {
        self.root.join("geocode.json")
    }

    pub fn has_day(&self, date: &str) -> bool {
        crate::adsb::manifest::day_dir(&self.store(), date).join("manifest.json").is_file()
    }

    /// Total bytes under the cache root (0 if it doesn't exist yet).
    pub fn size_bytes(&self) -> u64 {
        dir_size(&self.root)
    }
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.filter_map(|e| e.ok())
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

pub fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1e9
}
