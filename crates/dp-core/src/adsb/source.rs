//! Day-archive inputs: split tar parts (`*.tar.aa`, `*.tar.ab`, …), a single tar, or a directory of
//! extracted `trace_full_*.json` files. Streams one trace file at a time.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

pub struct TraceFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub enum Archive {
    /// Tar parts concatenated in order.
    Tar(Vec<PathBuf>),
    TraceDir(Vec<PathBuf>),
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SourceStats {
    pub trace_files: u64,
    pub other_entries: u64,
    pub compressed_bytes: u64,
    /// The tar ended mid-entry (e.g. an incomplete download); everything before it was used.
    pub truncated: bool,
}

fn is_trace_name(name: &str) -> bool {
    name.starts_with("trace_full_") && name.ends_with(".json")
}

/// `x.tar.aa` → `Some("x.tar.")`: the shared prefix of split parts.
fn split_prefix(name: &str) -> Option<&str> {
    let (stem, suffix) = name.rsplit_once(".tar.")?;
    (suffix.len() == 2 && suffix.bytes().all(|b| b.is_ascii_lowercase())).then(|| &name[..stem.len() + 5])
}

fn sorted_dir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    v.sort();
    Ok(v)
}

fn file_name(p: &Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

fn tar_parts_like(first: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let Some(prefix) = split_prefix(file_name(first)) else { return Ok(vec![first.to_path_buf()]) };
    let dir = first.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let parts: Vec<PathBuf> = sorted_dir(dir)?
        .into_iter()
        .filter(|p| split_prefix(file_name(p)) == Some(prefix))
        .collect();
    Ok(parts)
}

fn walk_traces(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for p in sorted_dir(dir)? {
        if p.is_dir() {
            walk_traces(&p, out)?;
        } else if is_trace_name(file_name(&p)) {
            out.push(p);
        }
    }
    Ok(())
}

impl Archive {
    pub fn open(path: &Path) -> anyhow::Result<Archive> {
        if path.is_file() {
            return Ok(Archive::Tar(tar_parts_like(path)?));
        }
        if !path.is_dir() {
            bail!("archive {} does not exist", path.display());
        }
        let tars: Vec<PathBuf> = sorted_dir(path)?
            .into_iter()
            .filter(|p| p.is_file() && (file_name(p).ends_with(".tar") || split_prefix(file_name(p)).is_some()))
            .collect();
        if let Some(first) = tars.first() {
            let parts = tar_parts_like(first)?;
            if parts.len() != tars.len() {
                bail!("{} holds tars from more than one release; pass one explicitly", path.display());
            }
            return Ok(Archive::Tar(parts));
        }
        let mut files = Vec::new();
        walk_traces(path, &mut files)?;
        if files.is_empty() {
            bail!("no tar parts or trace_full_*.json files under {}", path.display());
        }
        Ok(Archive::TraceDir(files))
    }

    /// Call `f` for each trace file in archive order; `f` returns `false` to stop early.
    pub fn for_each(&self, mut f: impl FnMut(TraceFile) -> anyhow::Result<bool>) -> anyhow::Result<SourceStats> {
        let mut st = SourceStats::default();
        match self {
            Archive::TraceDir(files) => {
                for p in files {
                    let bytes = std::fs::read(p).with_context(|| format!("reading {}", p.display()))?;
                    st.trace_files += 1;
                    st.compressed_bytes += bytes.len() as u64;
                    if !f(TraceFile { name: file_name(p).to_string(), bytes })? {
                        break;
                    }
                }
            }
            Archive::Tar(parts) => {
                let mut reader: Box<dyn Read> = Box::new(std::io::empty());
                for p in parts {
                    let file = File::open(p).with_context(|| format!("opening {}", p.display()))?;
                    reader = Box::new(reader.chain(file));
                }
                let mut ar = tar::Archive::new(BufReader::with_capacity(1 << 20, reader));
                for entry in ar.entries()? {
                    let Ok(mut entry) = entry else {
                        st.truncated = true;
                        break;
                    };
                    let path = entry.path().map(|p| p.into_owned()).unwrap_or_default();
                    let name = file_name(&path).to_string();
                    if !entry.header().entry_type().is_file() || !is_trace_name(&name) {
                        st.other_entries += 1;
                        continue;
                    }
                    let mut bytes = Vec::with_capacity(entry.size() as usize);
                    if entry.read_to_end(&mut bytes).is_err() {
                        st.truncated = true;
                        break;
                    }
                    st.trace_files += 1;
                    st.compressed_bytes += bytes.len() as u64;
                    if !f(TraceFile { name, bytes })? {
                        break;
                    }
                }
            }
        }
        Ok(st)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_part_names() {
        assert_eq!(split_prefix("v2026.09.08-planes-readsb-staging-0.tar.ab"), Some("v2026.09.08-planes-readsb-staging-0.tar."));
        assert_eq!(split_prefix("prod-0.tar.aa.head30M"), None);
        assert_eq!(split_prefix("x.tar"), None);
    }
}
