//! Fitted posterior calibration: temperature T and per-stratum split-conformal HPD levels q̂.
//! Values live in calibration.toml (written by `dopplerprint calibrate`, provenance inside).

use std::borrow::Cow;
use std::sync::LazyLock;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Calibration {
    /// z-units per e-fold of likelihood.
    pub temperature: f64,
    /// Uniform mixture weight ε over the region.
    pub floor: f64,
    pub persistence_nats: f64,
    pub provenance: String,
    #[serde(default)]
    pub stratum: Vec<Stratum>,
}

/// Cases with `min_events..=max_events` events heard.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Stratum {
    pub name: String,
    pub min_events: usize,
    pub max_events: usize,
    /// Calibration cases behind q̂, and where they came from ("real", "sim", "real+sim").
    pub n: usize,
    pub source: String,
    pub nominal: Vec<f64>,
    /// HPD mass to report for each nominal level; 1.0 when n is too small for that level.
    pub q_hat: Vec<f64>,
}

static CAL: LazyLock<Calibration> =
    LazyLock::new(|| toml::from_str(include_str!("calibration.toml")).expect("search/calibration.toml parses"));
static ORACLE: LazyLock<Calibration> =
    LazyLock::new(|| toml::from_str(include_str!("calibration_oracle.toml")).expect("search/calibration_oracle.toml parses"));

/// The shipped table, fitted on discovered events: what the app uses.
pub fn get() -> &'static Calibration {
    &CAL
}

/// Fitted on oracle (truth-label) events; eval baselines only.
pub fn oracle() -> &'static Calibration {
    &ORACLE
}

/// Which table an eval scores coverage with (`[search] calibration`): "auto" (default), "discovered",
/// "oracle", or a TOML path. The app ignores it and always uses `get()`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Choice {
    /// `oracle()` when every searched event is a truth label, else `get()`.
    #[default]
    Auto,
    Discovered,
    Oracle,
    File(std::path::PathBuf),
}

impl From<String> for Choice {
    fn from(s: String) -> Self {
        match s.as_str() {
            "auto" => Choice::Auto,
            "discovered" => Choice::Discovered,
            "oracle" => Choice::Oracle,
            _ => Choice::File(s.into()),
        }
    }
}

impl From<Choice> for String {
    fn from(c: Choice) -> Self {
        match c {
            Choice::Auto => "auto".into(),
            Choice::Discovered => "discovered".into(),
            Choice::Oracle => "oracle".into(),
            Choice::File(p) => p.display().to_string(),
        }
    }
}

impl Choice {
    pub fn is_auto(&self) -> bool {
        *self == Choice::Auto
    }

    pub fn resolve(&self, oracle_events: bool) -> anyhow::Result<Cow<'static, Calibration>> {
        Ok(match self {
            Choice::Auto if oracle_events => Cow::Borrowed(oracle()),
            Choice::Auto | Choice::Discovered => Cow::Borrowed(get()),
            Choice::Oracle => Cow::Borrowed(oracle()),
            Choice::File(p) => Cow::Owned(Calibration::from_file(p)?),
        })
    }
}

impl Calibration {
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn stratum(&self, n_events: usize) -> Option<&Stratum> {
        self.stratum.iter().find(|s| (s.min_events..=s.max_events).contains(&n_events))
    }

    /// Calibrated HPD mass for `nominal` coverage, or `None` when no stratum calibrates that level.
    pub fn q_hat(&self, n_events: usize, nominal: f64) -> Option<f64> {
        let s = self.stratum(n_events)?;
        s.nominal.iter().position(|x| (x - nominal).abs() < 1e-9).map(|i| s.q_hat[i])
    }
}

#[cfg(test)]
mod tests {
    use super::{Choice, get, oracle};

    #[test]
    fn shipped_calibrations_parse() {
        for c in [get(), oracle()] {
            assert!(c.temperature > 0.0 && c.floor > 0.0 && c.floor < 1.0);
            for s in &c.stratum {
                assert_eq!(s.nominal.len(), s.q_hat.len());
            }
        }
    }

    #[test]
    fn choice_picks_table_per_event_mode() {
        let is = |c: &Choice, oracle_events: bool, want: &super::Calibration| {
            std::ptr::eq(c.resolve(oracle_events).unwrap().as_ref(), want)
        };
        assert!(is(&Choice::Auto, true, oracle()) && is(&Choice::Auto, false, get()));
        assert!(is(&Choice::Discovered, true, get()) && is(&Choice::Oracle, false, oracle()));
        let parse = |s: &str| toml::from_str::<std::collections::HashMap<String, Choice>>(&format!("c = \"{s}\"")).unwrap()["c"].clone();
        assert_eq!(parse("oracle"), Choice::Oracle);
        assert_eq!(parse("discovered"), Choice::Discovered);
        assert_eq!(parse("x/cal.toml"), Choice::File("x/cal.toml".into()));
        assert!(Choice::File("/nonexistent.toml".into()).resolve(true).is_err());
    }
}
