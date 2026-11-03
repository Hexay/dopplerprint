//! Make sure the cache holds the ADS-B days a search needs: adsb.lol `globe_history_YYYY` release parts are
//! downloaded (after asking), ingested with `adsb::ingest_day`, and the raw parts deleted.

use anyhow::{Context, Result, bail};

use super::NeedsInput;
use super::cache::{Cache, gb};
use super::http::{self, Fetched};
use super::ui::Ui;
use crate::adsb::{IngestOptions, ingest_day};

/// docs/DATA.md: a ~3.5 GB day becomes a ~1.5 GB store.
const STORE_PER_RAW: f64 = 0.45;
const TYPES_URL: &str = "https://raw.githubusercontent.com/wiedehopf/tar1090-db/master/db/icao_aircraft_types.js";
const REGISTRY_URL: &str = "https://raw.githubusercontent.com/wiedehopf/tar1090-db/csv/aircraft.csv.gz";

#[derive(Clone, Debug)]
pub struct FetchOptions {
    pub offline: bool,
    pub yes: bool,
    pub keep_raw: bool,
    pub max_cache_gb: f64,
}

#[derive(Clone, Debug)]
struct Asset {
    name: String,
    size: u64,
    url: String,
}

#[derive(Clone, Debug)]
struct Day {
    date: String,
    tag: String,
    parts: Vec<Asset>,
}

impl Day {
    fn bytes(&self) -> u64 {
        self.parts.iter().map(|a| a.size).sum()
    }
}

/// adsb.lol release tag for a UTC date (`YYYY-MM-DD`).
pub fn release_tag(date: &str) -> String {
    format!("v{}-planes-readsb-prod-0", date.replace('-', "."))
}

fn release(date: &str) -> Result<Day> {
    let tag = release_tag(date);
    let url = format!("https://api.github.com/repos/adsblol/globe_history_{}/releases/tags/{tag}", &date[..4]);
    let v = match http::get_json(&url, &[]).context("asking GitHub for the adsb.lol release")? {
        Fetched::Json(v) => v,
        Fetched::NotFound => bail!("adsb.lol has no archive for {date} (yet); days are published a day or two later"),
    };
    let mut parts: Vec<Asset> = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            let is_part = name.rsplit_once(".tar.a").is_some_and(|(_, s)| s.len() == 1);
            let safe = !name.contains(['/', '\\']) && !name.starts_with('.');
            (is_part && safe).then(|| Asset { size: a["size"].as_u64().unwrap_or(0), url: a["browser_download_url"].as_str().unwrap_or("").into(), name })
        })
        .collect();
    parts.sort_by(|a, b| a.name.cmp(&b.name));
    if parts.is_empty() || parts.iter().any(|p| p.size == 0 || !p.url.starts_with("https://github.com/")) {
        bail!("the adsb.lol release {tag} doesn't list usable .tar parts");
    }
    Ok(Day { date: date.into(), tag, parts })
}

/// Download and ingest every date in `dates` that the cache doesn't hold yet.
pub fn ensure_days(cache: &Cache, dates: &[String], o: &FetchOptions, ui: &dyn Ui) -> Result<()> {
    let missing: Vec<&String> = dates.iter().filter(|d| !cache.has_day(d)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let list = missing.iter().map(|d| d.as_str()).collect::<Vec<_>>().join(" and ");
    if o.offline {
        return Err(NeedsInput(format!(
            "The cache has no ADS-B data for {list} and --offline is set; run without --offline to download it (~3.5 GB per day)."
        ))
        .into());
    }
    ui.info(&format!("Looking up adsb.lol archives for {list}..."));
    let days: Vec<Day> = missing.iter().map(|d| release(d)).collect::<Result<_>>()?;
    let total: u64 = days.iter().map(Day::bytes).sum();
    let (used, limit) = (cache.size_bytes(), (o.max_cache_gb * 1e9) as u64);
    let need = used + total + (total as f64 * STORE_PER_RAW) as u64;
    if need > limit {
        return Err(NeedsInput(format!(
            "Downloading ADS-B for {list} needs {:.1} GB of cache ({:.1} GB in use, limit {:.0} GB); raise --max-cache-gb or free space in {}.",
            gb(need),
            gb(used),
            o.max_cache_gb,
            cache.root.display()
        ))
        .into());
    }
    let question = format!(
        "Download ADS-B for {list} from adsb.lol: {:.2} GB now, ~{:.1} GB kept in {} after processing?",
        gb(total),
        gb((total as f64 * STORE_PER_RAW) as u64),
        cache.root.display()
    );
    if !o.yes && !ui.confirm(&question) {
        return Err(NeedsInput(format!(
            "Need ADS-B data for {list} ({:.2} GB download); rerun with --yes to download it.",
            gb(total)
        ))
        .into());
    }
    ensure_typedb(cache, ui)?;
    for d in &days {
        fetch_day(cache, d, o.keep_raw, ui)?;
    }
    Ok(())
}

fn fetch_day(cache: &Cache, d: &Day, keep_raw: bool, ui: &dyn Ui) -> Result<()> {
    let dir = cache.raw().join(&d.tag);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let n = d.parts.len();
    for (i, a) in d.parts.iter().enumerate() {
        let label = format!("{} part {}/{n}", d.date, i + 1);
        http::download(&a.url, &dir.join(&a.name), Some(a.size), &mut |done, total| ui.progress(&label, done, total))?;
    }
    ui.info(&format!("Processing {} into the track store (about a minute, ~2.5 GB RAM)...", d.date));
    let opts = IngestOptions { typedb_dir: Some(cache.typedb()), ..IngestOptions::default() };
    let mf = ingest_day(&dir.join(&d.parts[0].name), &cache.store(), &opts)
        .with_context(|| format!("processing the {} archive", d.date))?;
    ui.info(&format!("  {}: {} aircraft, {} tracks", mf.date, mf.aircraft, mf.tracks));
    if mf.date != d.date {
        bail!("the {} archive holds {} traces, not {}", d.tag, mf.date, d.date);
    }
    if !keep_raw {
        std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    Ok(())
}

/// tar1090-db type table and registry (~8 MB), fetched once.
pub fn ensure_typedb(cache: &Cache, ui: &dyn Ui) -> Result<()> {
    let dir = cache.typedb();
    std::fs::create_dir_all(&dir)?;
    for (url, name) in [(TYPES_URL, "icao_aircraft_types.js"), (REGISTRY_URL, "aircraft.csv.gz")] {
        if !dir.join(name).is_file() {
            ui.info(&format!("Fetching aircraft type table {name} (tar1090-db)..."));
            http::download(url, &dir.join(name), None, &mut |_, _| {})?;
        }
    }
    Ok(())
}

/// UTC dates (`YYYY-MM-DD`) touched by `[t0, t1]` (UNIX s).
pub fn dates_between(t0: f64, t1: f64) -> Vec<String> {
    let (d0, d1) = ((t0 / 86_400.0).floor() as i64, (t1 / 86_400.0).floor() as i64);
    (d0..=d1).map(|d| crate::adsb::manifest::civil_date(d as f64 * 86_400.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_dates() {
        assert_eq!(release_tag("2026-05-14"), "v2026.05.14-planes-readsb-prod-0");
        let t = 1_778_778_120.0;
        assert_eq!(dates_between(t - 900.0, t + 3600.0), ["2026-05-14"]);
        assert_eq!(dates_between(t, t + 86_400.0), ["2026-05-14", "2026-05-15"]);
    }

    #[test]
    fn offline_miss_needs_input() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache { root: dir.path().into() };
        let o = FetchOptions { offline: true, yes: false, keep_raw: false, max_cache_gb: 20.0 };
        let e = ensure_days(&cache, &["2026-05-14".into()], &o, &super::super::ui::QuietUi).unwrap_err();
        let msg = &e.downcast_ref::<NeedsInput>().unwrap().0;
        assert!(msg.contains("2026-05-14") && msg.contains("--offline"), "{msg}");
    }
}
