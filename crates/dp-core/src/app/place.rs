//! `--near` parsing: a point, a point with radius, a bbox, or a place name (geocoded elsewhere).

use super::NeedsInput;

/// Default search radius; rationale in docs/USAGE.md#where.
pub const DEFAULT_RADIUS_KM: f64 = 10.0;
/// Search cost grows with area; past this the coarse lattice alone is ~200k cells.
pub const MAX_RADIUS_KM: f64 = 50.0;
const EXAMPLE: &str = "--near \"Livermore, CA\" or --near 37.68,-121.77";
/// The embedded geoid and the EPSG:5070 lattice cover the contiguous US only.
pub const COVERAGE: (f64, f64, f64, f64) = (-125.5, 24.0, -66.0, 50.0);

#[derive(Clone, Debug, PartialEq)]
pub enum Near {
    Point { lat: f64, lon: f64, radius_km: Option<f64> },
    /// west, south, east, north (degrees).
    BBox([f64; 4]),
    Place(String),
}

/// A resolved circular search area.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Area {
    pub lat: f64,
    pub lon: f64,
    pub radius_km: f64,
    /// For the report: "Livermore, CA (37.6819, -121.7680)", or the centre's coordinates.
    pub label: String,
}

pub fn missing_near() -> NeedsInput {
    NeedsInput(format!(
        "Need roughly where the recording was made: add {EXAMPLE} (within ~{DEFAULT_RADIUS_KM:.0} km; whole-country search isn't supported yet)."
    ))
}

pub fn parse_near(s: &str) -> Result<Near, NeedsInput> {
    let s = s.trim();
    if s.is_empty() {
        return Err(missing_near());
    }
    let nums: Option<Vec<f64>> = s.split(',').map(|x| x.trim().parse::<f64>().ok()).collect();
    let Some(n) = nums else { return Ok(Near::Place(s.to_string())) };
    match n.as_slice() {
        [lat, lon] => point(*lat, *lon, None),
        [lat, lon, r] => point(*lat, *lon, Some(*r)),
        [w, so, e, no] => {
            let ok = w < e && so < no && check_latlon(*so, *w).is_ok() && check_latlon(*no, *e).is_ok();
            if !ok {
                return Err(NeedsInput(format!("--near \"{s}\" isn't a valid box; give west,south,east,north in degrees.")));
            }
            Ok(Near::BBox([*w, *so, *e, *no]))
        }
        _ => Err(NeedsInput(format!("Can't read --near \"{s}\"; use {EXAMPLE}, lat,lon,radius_km or west,south,east,north."))),
    }
}

fn point(lat: f64, lon: f64, radius_km: Option<f64>) -> Result<Near, NeedsInput> {
    check_latlon(lat, lon)?;
    Ok(Near::Point { lat, lon, radius_km })
}

fn check_latlon(lat: f64, lon: f64) -> Result<(), NeedsInput> {
    if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
        return Ok(());
    }
    let hint = if lon.abs() <= 90.0 && lat.abs() <= 180.0 { " (latitude comes first: lat,lon)" } else { "" };
    Err(NeedsInput(format!("--near {lat},{lon} is not a valid position{hint}.")))
}

/// Turn a parsed `Near` (place names already geocoded to a point) into the search area.
pub fn area(near: &Near, label: Option<&str>, radius_override_km: Option<f64>) -> Result<Area, NeedsInput> {
    let (lat, lon, r) = match *near {
        Near::Point { lat, lon, radius_km } => (lat, lon, radius_override_km.or(radius_km).unwrap_or(DEFAULT_RADIUS_KM)),
        Near::BBox([w, s, e, n]) => {
            let (lat, lon) = ((s + n) / 2.0, (w + e) / 2.0);
            let half_diag_km = crate::eval::distance_m(s, w, n, e) / 2000.0;
            (lat, lon, radius_override_km.unwrap_or(half_diag_km))
        }
        Near::Place(_) => unreachable!("place names are geocoded before area()"),
    };
    let label = label.map_or_else(|| format!("{lat:.4}, {lon:.4}"), str::to_string);
    let (w, s, e, n) = COVERAGE;
    if !(s..=n).contains(&lat) || !(w..=e).contains(&lon) {
        return Err(NeedsInput(format!(
            "{label} is outside the area dopplerprint covers so far (the contiguous US)."
        )));
    }
    if !(r > 0.0 && r <= MAX_RADIUS_KM) {
        return Err(NeedsInput(format!(
            "A {r:.0} km search radius is too wide; narrow --near to within {MAX_RADIUS_KM:.0} km (whole-country search isn't supported yet)."
        )));
    }
    Ok(Area { lat, lon, radius_km: r, label })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms() {
        assert_eq!(parse_near("37.68,-121.77").unwrap(), Near::Point { lat: 37.68, lon: -121.77, radius_km: None });
        assert_eq!(parse_near(" 37.68, -121.77, 5 ").unwrap(), Near::Point { lat: 37.68, lon: -121.77, radius_km: Some(5.0) });
        assert_eq!(parse_near("-121.9,37.6,-121.6,37.8").unwrap(), Near::BBox([-121.9, 37.6, -121.6, 37.8]));
        assert_eq!(parse_near("Livermore, CA").unwrap(), Near::Place("Livermore, CA".into()));
    }

    #[test]
    fn rejects_with_one_clear_sentence() {
        let swapped = parse_near("-121.77,37.68").unwrap_err().0;
        assert!(swapped.contains("latitude comes first"), "{swapped}");
        let bbox = parse_near("-121.6,37.6,-121.9,37.8").unwrap_err().0;
        assert!(bbox.contains("west,south,east,north"), "{bbox}");
        assert!(parse_near("1,2,3,4,5").unwrap_err().0.contains("lat,lon,radius_km"));
        let missing = missing_near().0;
        assert!(missing.contains("--near") && missing.contains("whole-country") && !missing.contains('\n'));
    }

    #[test]
    fn areas() {
        let p = parse_near("37.68,-121.77").unwrap();
        let a = area(&p, None, None).unwrap();
        assert_eq!((a.radius_km, a.label.as_str()), (DEFAULT_RADIUS_KM, "37.6800, -121.7700"));
        assert_eq!(area(&p, None, Some(3.0)).unwrap().radius_km, 3.0);
        assert_eq!(area(&parse_near("37.68,-121.77,4").unwrap(), None, None).unwrap().radius_km, 4.0);
        let b = area(&parse_near("-121.9,37.6,-121.6,37.8").unwrap(), Some("box"), None).unwrap();
        assert!((b.lat - 37.7).abs() < 1e-9 && (b.radius_km - 17.5).abs() < 1.5, "{b:?}");
        let wide = area(&p, None, Some(400.0)).unwrap_err().0;
        assert!(wide.contains("whole-country"), "{wide}");
        let paris = area(&parse_near("48.85,2.35").unwrap(), Some("Paris"), None).unwrap_err().0;
        assert!(paris.contains("contiguous US"), "{paris}");
    }
}
