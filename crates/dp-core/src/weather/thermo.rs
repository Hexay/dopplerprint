//! Sound speed from temperature and humidity, and meteorological wind to east/north components.

/// c = 20.05·√T_s with the acoustic (sonic) virtual temperature T_s = T·(1 + 0.51·q), q specific humidity
/// (kg/kg). Kaimal & Gaynor 1991; the 0.51 is acoustic, not the 0.61 of the density virtual temperature.
pub fn sound_speed(t_k: f64, q: f64) -> f64 {
    20.05 * (t_k * (1.0 + 0.51 * q)).sqrt()
}

/// Specific humidity from relative humidity (%), temperature (K) and pressure (hPa); Bolton 1980 e_s.
pub fn specific_humidity(rh_pct: f64, t_k: f64, p_hpa: f64) -> f64 {
    let t_c = t_k - 273.15;
    let e = (rh_pct / 100.0).clamp(0.0, 1.0) * 6.112 * (17.67 * t_c / (t_c + 243.5)).exp();
    0.622 * e / (p_hpa - 0.378 * e)
}

/// (east, north) from speed and the direction the wind blows FROM (degrees clockwise from north).
pub fn wind_en(speed: f64, from_deg: f64) -> [f64; 2] {
    let a = from_deg.to_radians();
    [-speed * a.sin(), -speed * a.cos()]
}

/// Inverse of `wind_en`: (speed, from-direction in [0, 360)).
pub fn speed_from(en: [f64; 2]) -> (f64, f64) {
    let s = en[0].hypot(en[1]);
    let d = (-en[0]).atan2(-en[1]).to_degrees();
    (s, d.rem_euclid(360.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_air_at_15c_is_340() {
        assert!((sound_speed(288.15, 0.0) - 340.35).abs() < 0.05);
    }

    #[test]
    fn humid_summer_air_is_faster() {
        let q = specific_humidity(100.0, 298.15, 1013.25);
        assert!((q - 0.0198).abs() < 0.0005, "{q}");
        let dc = sound_speed(298.15, q) - sound_speed(298.15, 0.0);
        assert!((1.5..2.0).contains(&dc), "{dc}");
    }

    #[test]
    fn westerly_blows_toward_east() {
        let [e, n] = wind_en(10.0, 270.0);
        assert!((e - 10.0).abs() < 1e-9 && n.abs() < 1e-9);
        let (s, d) = speed_from(wind_en(6.0, 250.0));
        assert!((s - 6.0).abs() < 1e-9 && (d - 250.0).abs() < 1e-9);
    }
}
