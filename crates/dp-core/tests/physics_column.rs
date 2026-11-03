use dp_core::geo::{Geodetic, east_north, geodetic_to_ecef, up_vector};
use dp_core::physics::cv::StraightLine;
use dp_core::physics::{Atmosphere, Column, cpa, emission};

const T0: f64 = 1.725e9;
const LAT: f64 = 40.87;
const LON: f64 = -96.7;
const H_OBS: f64 = 400.0;

#[test]
fn column_emission_equals_uniform_layer_mean() {
    let obs = geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: H_OBS });
    let (east, north) = east_north(LAT, LON);
    let up = up_vector(LAT, LON);
    let line = StraightLine { obs, dir: north, offset: up * 900.0 + east * 300.0, speed: 120.0, t_cpa_emit: T0, half_span_s: 300.0, dt: 1.0 };
    let tr = line.track("cv");
    let nodes = [(H_OBS, 345.0, east * 1.0), (H_OBS + 300.0, 341.0, east * 12.0), (H_OBS + 2000.0, 333.0, north * 20.0)];
    let col = Column::new(obs, up, H_OBS, &nodes, (H_OBS, H_OBS + 1000.0)).unwrap();
    let r = col.reference();
    let layered = Atmosphere { c: r.c, wind: r.wind, column: Some(col) };
    for i in -20..=20 {
        let t = T0 + 3.0 + i as f64 * 5.0;
        let e = emission(&tr, obs, t, &layered).unwrap();
        let m = col.layer_mean(H_OBS, col.height(e.pos));
        let u = emission(&tr, obs, t, &Atmosphere::uniform(m.c, m.wind)).unwrap();
        assert!((e.tau - u.tau).abs() < 1e-6 && (e.factor - u.factor).abs() < 1e-7, "t={t}: {e:?} vs {u:?}");
    }
    let (cl, cu) = (cpa(&tr, obs, &layered).unwrap(), cpa(&tr, obs, &Atmosphere::uniform(r.c, r.wind)).unwrap());
    assert!((cl.t_arrive - cu.t_arrive).abs() > 1e-4, "the 900 m layer differs from the 1000 m reference");
}

#[test]
fn offsets_on_the_atmosphere_shift_every_layer() {
    let obs = geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: H_OBS });
    let (east, _) = east_north(LAT, LON);
    let up = up_vector(LAT, LON);
    let col = Column::new(obs, up, H_OBS, &[(H_OBS, 340.0, east), (H_OBS + 3000.0, 330.0, east * 15.0)], (H_OBS, H_OBS + 500.0)).unwrap();
    let r = col.reference();
    let base = Atmosphere { c: r.c, wind: r.wind, column: Some(col) };
    let shifted = Atmosphere { c: r.c + 4.0, wind: r.wind + east * 2.0, ..base };
    let src = obs + up * 2000.0;
    let (a, b) = (base.medium(obs, src), shifted.medium(obs, src));
    assert!((b.c - a.c - 4.0).abs() < 1e-9 && ((b.wind - a.wind).dot(east) - 2.0).abs() < 1e-9);
}
