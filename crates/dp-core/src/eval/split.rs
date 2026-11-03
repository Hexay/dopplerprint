//! Frozen dev/test split of corpus cases (experiments/split.toml). Test cases carry a fingerprint of
//! their truth.json so a silently edited truth is refused rather than scored.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    /// Corpus cases not in the test list, plus sim. The default for iteration.
    #[default]
    Dev,
    /// Only frozen test cases; use when promoting a variant to default.
    Test,
    All,
}

#[derive(Debug, Default, serde::Deserialize)]
struct SplitFile {
    #[serde(default)]
    test: BTreeMap<String, String>,
}

/// Case id → expected truth.json fingerprint. A missing file means no case is held out.
pub fn load(path: &Path) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let f: SplitFile = toml::from_str(&std::fs::read_to_string(path)?).with_context(|| format!("{}", path.display()))?;
    Ok(f.test)
}

pub fn wanted(split: Split, test_ids: &BTreeMap<String, String>, id: &str, is_sim: bool) -> bool {
    let held_out = test_ids.contains_key(id);
    match split {
        Split::Dev => !held_out,
        Split::Test => held_out && !is_sim,
        Split::All => true,
    }
}

/// FNV-1a 64 of the file bytes: a drift check, not a security hash.
pub fn fingerprint(path: &Path) -> Result<String> {
    let h = std::fs::read(path)?
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |a, &b| (a ^ b as u64).wrapping_mul(0x100_0000_01b3));
    Ok(format!("fnv64:{h:016x}"))
}

pub fn verify(case_dir: &Path, expected: &str) -> Result<()> {
    let got = fingerprint(&case_dir.join("truth.json"))?;
    if got != expected {
        bail!("frozen test case changed: truth.json is {got}, split.toml expects {expected}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_and_test_partition_the_corpus() {
        let ids: BTreeMap<String, String> = [("a".to_string(), "x".to_string())].into();
        assert!(!wanted(Split::Dev, &ids, "a", false) && wanted(Split::Dev, &ids, "b", false));
        assert!(wanted(Split::Test, &ids, "a", false) && !wanted(Split::Test, &ids, "sim_b", true));
        assert!(wanted(Split::All, &ids, "a", false) && wanted(Split::All, &ids, "b", false));
    }
}
