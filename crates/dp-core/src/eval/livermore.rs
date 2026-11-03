//! Adapter for Livermore-style corpus cases: no published audio start, a UTC ridge CSV and a registration.
//! The audio start is recovered by sliding the published ridge over the recording's own spectrogram
//! inside the truth start window; the CPA time comes from the truth flight's ADS-B track at the truth spot.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::case::{Posed, first_file, load_tracks, oracle};
use crate::adsb::manifest::{civil_date, parse_utc};
use crate::audio::spectrogram::log_spectrogram;
use crate::audio::{self, LogSpectrogram, SpecParams};
use crate::config::EvalParams;
use crate::dem::{Dem, geoid_undulation};
use crate::geo::{Geodetic, geodetic_to_ecef};
use crate::physics::{self, Atmosphere};

pub fn is_livermore(truth: &Value) -> bool {
    truth.get("recording_start_unix").is_none() && truth.get("recording_start_window_utc").is_some()
}

pub fn pose(dir: &Path, truth: &Value, p: &EvalParams) -> Result<Posed> {
    let id = dir.file_name().unwrap().to_string_lossy().into_owned();
    let reg = truth["flight_ids"][0]["registration"].as_str().context("needs flight_ids[0].registration")?;
    let icao = n_number_icao(reg).with_context(|| format!("not a US N-number: {reg}"))?;
    let win = &truth["recording_start_window_utc"];
    let parse = |v: &Value| v.as_str().and_then(parse_utc);
    let (w0, w1) = (parse(&win[0]).context("start window")?, parse(&win[1]).context("start window")?);
    let ridge = read_ridge(&dir.join("source/frequency_vs_time_used.csv"))?;
    let audio = audio::decode(&first_file(&dir.join("audio"))?)?;
    let spec = log_spectrogram(&audio, &SpecParams::default());
    let t0 = align_start(&spec, &ridge, (w0 - 1.0, w1 + 1.0));
    let (lat, lon) = (truth["lat"].as_f64().context("lat")?, truth["lon"].as_f64().context("lon")?);
    let dem_path = p.data_dir.join("dem/5070/livermore_30m.u16");
    let dem = dem_path.exists().then(|| Dem::open(&dem_path)).transpose()?;
    let ground = dem.as_ref().and_then(|d| d.height_ellipsoid_latlon(lat, lon)).unwrap_or_else(|| geoid_undulation(lat, lon));
    let obs = geodetic_to_ecef(Geodetic { lat_deg: lat, lon_deg: lon, h_ell_m: ground + 1.5 });
    let dur = audio.duration_s();
    let tracks = load_tracks(p, &civil_date(t0), lat, lon, t0, dur)?;
    let t_cpa = tracks
        .iter()
        .filter(|t| t.icao24 == icao)
        .filter_map(|t| physics::cpa(t, obs, &Atmosphere::default()))
        .filter(|c| c.t_arrive >= t0 - 30.0 && c.t_arrive <= t0 + dur + 30.0)
        .min_by(|a, b| a.slant_m.total_cmp(&b.slant_m))
        .map_or_else(|| steepest_descent(&ridge), |c| c.t_arrive);
    let f0 = interp(&ridge, t_cpa);
    let (r0, r1) = (ridge[0].0 - t0, ridge[ridge.len() - 1].0 - t0);
    if !f0.is_finite() || r1 <= r0 {
        bail!("ridge does not cover the CPA");
    }
    Ok(Posed {
        id,
        audio,
        truth_lat: lat,
        truth_lon: lon,
        truth_h_ell: Some(ground),
        t_start_true: t0,
        events: vec![oracle(((r0 - 1.0).max(0.0), (r1 + 1.0).min(dur)), t_cpa - t0, f0, p)],
        truth_flights: vec![Some(format!("{icao:06x}"))],
        tracks,
        dem,
        sim_atmosphere: None,
    })
}

/// (UNIX s, Hz) rows of the publisher's ridge.
fn read_ridge(path: &Path) -> Result<Vec<(f64, f64)>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
    let rows: Vec<(f64, f64)> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let (t, f) = l.split_once(',')?;
            Some((parse_utc(t.trim())?, f.trim().parse().ok()?))
        })
        .collect();
    if rows.len() < 10 {
        bail!("ridge CSV has {} rows", rows.len());
    }
    Ok(rows)
}

/// Audio start (UNIX s) maximising whitened power along the ridge, searched over `range` at 20 ms.
pub fn align_start(spec: &LogSpectrogram, ridge: &[(f64, f64)], range: (f64, f64)) -> f64 {
    let power = |s: f64| -> f64 {
        ridge
            .iter()
            .filter_map(|&(t, f)| {
                let fr = spec.frame_of(t - s).round();
                let b = spec.bin_of(f).round();
                (fr >= 0.0 && (fr as usize) < spec.n_t && b >= 2.0 && (b as usize) + 2 < spec.n_f).then(|| {
                    let row = spec.row(fr as usize);
                    row[b as usize - 2..=b as usize + 2].iter().copied().fold(f32::MIN, f32::max) as f64
                })
            })
            .sum()
    };
    let n = ((range.1 - range.0) / 0.02).ceil() as usize;
    (0..=n).map(|i| range.0 + i as f64 * 0.02).max_by(|a, b| power(*a).total_cmp(&power(*b))).unwrap_or(range.0)
}

fn interp(ridge: &[(f64, f64)], t: f64) -> f64 {
    let i = ridge.partition_point(|r| r.0 < t);
    if i == 0 || i == ridge.len() {
        return f64::NAN;
    }
    let ((ta, fa), (tb, fb)) = (ridge[i - 1], ridge[i]);
    fa + (fb - fa) * (t - ta) / (tb - ta).max(1e-9)
}

/// Fallback CPA time when the truth flight is missing from the store: steepest drop of the
/// 2 s-smoothed ridge (that is the CPA emission instant, early by slant/c).
fn steepest_descent(ridge: &[(f64, f64)]) -> f64 {
    let smooth = |t: f64| {
        let w: Vec<f64> = ridge.iter().filter(|r| (r.0 - t).abs() <= 1.0).map(|r| r.1).collect();
        w.iter().sum::<f64>() / w.len().max(1) as f64
    };
    ridge
        .windows(2)
        .map(|w| ((w[0].0 + w[1].0) / 2.0, smooth(w[1].0) - smooth(w[0].0)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(ridge[0].0, |x| x.0)
}

/// FAA N-number → ICAO 24-bit address (the US block's fixed enumeration).
pub fn n_number_icao(reg: &str) -> Option<u32> {
    const L: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const SUFFIX: u32 = 1 + 24 * 25;
    const B4: u32 = 1 + 24 + 10;
    const B3: u32 = 10 * B4 + SUFFIX;
    const B2: u32 = 10 * B3 + SUFFIX;
    const B1: u32 = 10 * B2 + SUFFIX;
    let s = reg.strip_prefix(['N', 'n'])?.to_ascii_uppercase().into_bytes();
    let letter = |c: u8| L.iter().position(|&x| x == c).map(|i| i as u32);
    let digit = |c: u8| c.is_ascii_digit().then(|| (c - b'0') as u32);
    let suffix = |r: &[u8]| -> Option<u32> {
        match r {
            [a] => Some(25 * letter(*a)? + 1),
            [a, b] => Some(25 * letter(*a)? + 1 + letter(*b)? + 1),
            _ => None,
        }
    };
    if s.is_empty() || s.len() > 5 || s[0] == b'0' {
        return None;
    }
    let mut out = 0xA0_0001 + (digit(s[0])? - 1) * B1;
    for (i, b) in [(1, B2), (2, B3), (3, B4)] {
        if s.len() == i {
            return Some(out);
        }
        if s[i].is_ascii_alphabetic() {
            return Some(out + suffix(&s[i..])?);
        }
        out += digit(s[i])? * b + SUFFIX;
    }
    match s.get(4) {
        None => Some(out),
        Some(&c) if c.is_ascii_alphabetic() => Some(out + 1 + letter(c)?),
        Some(&c) => Some(out + 1 + 24 + digit(c)?),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn n_numbers_match_registry() {
        for (n, hex) in [("N1387U", 0xA09C49), ("N172EL", 0xA1210A), ("N413AK", 0xA4DF4F), ("N7PG", 0xA95128)] {
            assert_eq!(super::n_number_icao(n), Some(hex), "{n}");
        }
    }
}
