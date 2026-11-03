//! Chunk-bound pruning must not change results: bounded CPA == reference CPA, indexed `near` == brute force.

use dp_core::geo::{Geodetic, Vec3, east_north, geodetic_to_ecef, up_vector};
use dp_core::physics::{Atmosphere, cpa, cpa_bounded};
use dp_core::search::{Catalog, MemCatalog};
use dp_core::track::{EngineClass, Track, TrackBounds, TrackPoint};

const T0: f64 = 1.725e9;
const LAT: f64 = 40.87;
const LON: f64 = -73.5;

fn origin() -> Vec3 {
    geodetic_to_ecef(Geodetic { lat_deg: LAT, lon_deg: LON, h_ell_m: 0.0 })
}

/// Deterministic xorshift in [−1, 1).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

/// Looping, climbing path with uneven sampling and noisy reported velocity (so Hermite segments bulge).
fn wander(seed: u64, n: usize) -> Track {
    let (e, nn) = east_north(LAT, LON);
    let up = up_vector(LAT, LON);
    let mut rng = Rng(seed);
    let (cx, cy) = (rng.next() * 8000.0, rng.next() * 8000.0);
    let mut t = 0.0;
    let points = (0..n)
        .map(|_| {
            t += 1.0 + 4.0 * (rng.next() + 1.0);
            let a = t * 0.01 + seed as f64;
            let pos = origin() + e * (cx + 3000.0 * a.cos() + 40.0 * t) + nn * (cy + 2000.0 * (1.7 * a).sin()) + up * (500.0 + 2.0 * t);
            let vel = e * (-30.0 * a.sin() + 40.0 + 15.0 * rng.next()) + nn * (34.0 * (1.7 * a).cos() + 15.0 * rng.next()) + up * 2.0;
            TrackPoint { t: T0 + t, pos, vel }
        })
        .collect();
    Track { id: format!("w{seed}"), icao24: seed as u32, type_code: None, engine: EngineClass::Piston, points }
}

fn observers(rng: &mut Rng, k: usize) -> Vec<Vec3> {
    let (e, n) = east_north(LAT, LON);
    (0..k).map(|_| origin() + e * (rng.next() * 25_000.0) + n * (rng.next() * 25_000.0)).collect()
}

#[test]
fn bounded_cpa_equals_reference() {
    let atm = Atmosphere::default();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for (seed, n) in [(1, 1), (2, 2), (3, 17), (4, 33), (5, 400), (6, 1500)] {
        let track = wander(seed, n);
        let bounds = TrackBounds::new(&track);
        for obs in observers(&mut rng, 300) {
            let want = cpa(&track, obs, &atm);
            assert_eq!(cpa_bounded(&track, bounds.chunks.iter(),obs, &atm, f64::INFINITY), want, "seed {seed} n {n}");
            // Reaches straddling the true range: just below → None, at or above → identical.
            if let Some(w) = want {
                for (reach, hit) in [(w.slant_m * 0.999, false), (w.slant_m * 1.001, true), (w.slant_m * 3.0, true)] {
                    let got = cpa_bounded(&track, bounds.chunks.iter(),obs, &atm, reach);
                    assert_eq!(got, hit.then_some(w), "seed {seed} n {n} reach {reach}");
                }
            }
        }
        // On-track observers: the knot minimum and segment refinement are both exercised at ~0 range.
        for p in track.points.iter().step_by(7) {
            let obs = p.pos + Vec3::new(3.0, -2.0, 1.0);
            assert_eq!(cpa_bounded(&track, bounds.chunks.iter(),obs, &atm, f64::INFINITY), cpa(&track, obs, &atm), "on-track seed {seed}");
        }
    }
}

#[test]
fn indexed_near_equals_brute_force() {
    let tracks: Vec<Track> = (10..40).map(|s| wander(s, 50 + 37 * (s as usize % 9))).collect();
    let cat = MemCatalog::new(tracks.clone());
    let atm = Atmosphere::default();
    let mut rng = Rng(42);
    for obs in observers(&mut rng, 400) {
        for (radius, t0, t1) in [(10_000.0, T0, T0 + 5_000.0), (3_000.0, T0 + 800.0, T0 + 900.0), (25_000.0, f64::NEG_INFINITY, f64::INFINITY)] {
            let near = cat.near(obs, radius, t0, t1);
            let got: Vec<&str> = near.iter().map(|n| n.track.id.as_str()).collect();
            let want: Vec<&str> = tracks.iter().filter(|t| brute_near(t, obs, radius, t0, t1)).map(|t| t.id.as_str()).collect();
            assert_eq!(got, want);
            // CPA over the queried chunk subset equals the full reference CPA (capped reach covers it).
            for n in &near {
                assert_eq!(n.cpa(obs, &atm, f64::INFINITY), cpa(n.track, obs, &atm), "track {}", n.track.id);
            }
        }
    }
}

/// The pre-index predicate (bounding sphere + any point within radius + 2 km).
fn brute_near(t: &Track, p: Vec3, radius: f64, t0: f64, t1: f64) -> bool {
    let n = t.points.len().max(1) as f64;
    let c = t.points.iter().fold(Vec3::ZERO, |a, q| a + q.pos) * (1.0 / n);
    let r = t.points.iter().map(|q| (q.pos - c).norm()).fold(0.0, f64::max);
    t.t_end() >= t0
        && t.t_start() <= t1
        && (p - c).norm() <= r + radius
        && t.points.iter().any(|q| (q.pos - p).norm() <= radius + 2_000.0)
}
