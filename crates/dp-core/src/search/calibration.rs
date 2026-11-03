//! Fitted posterior calibration: temperature T and per-stratum split-conformal HPD levels q̂.
//! Values live in calibration.toml (written by `dopplerprint calibrate`, provenance inside).

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

pub fn get() -> &'static Calibration {
    &CAL
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
    #[test]
    fn shipped_calibration_parses() {
        let c = super::get();
        assert!(c.temperature > 0.0 && c.floor > 0.0 && c.floor < 1.0);
        for s in &c.stratum {
            assert_eq!(s.nominal.len(), s.q_hat.len());
        }
    }
}
