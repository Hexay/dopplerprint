//! Conservative ECEF boxes over runs of consecutive track points, for exact pruning in range queries
//! (`search::MemCatalog::near`) and closest approach (`physics::cpa`).
//!
//! The Hermite curve between knots a, b strays from the chord lerp(a, b) by at most
//! 0.0963·|b−a| + (4/27)·dt·(|v_a| + |v_b|): p − lerp = (2s³−3s²+s)(a−b) + dt(h10·v_a + h11·v_b), with
//! max|2s³−3s²+s| = 1/(6√3) and max|h10| = max|h11| = 4/27 on [0, 1]. The chord lies in the points' box,
//! so that box grown by the largest stray (+ slack for evaluation just past s = 1 and rounding) holds the
//! curve.

use super::Track;
use crate::geo::Vec3;

/// Segments per chunk.
pub const CHUNK: usize = 16;
/// Metres added to every curve pad (UNIX-time overshoot of s past 1, f64 rounding).
const PAD_SLACK_M: f64 = 1.0;

#[derive(Clone, Debug)]
pub struct Chunk {
    /// Points `first..=last` (shared endpoint with the next chunk); segments `first..last`.
    pub first: usize,
    pub last: usize,
    pub lo: Vec3,
    pub hi: Vec3,
    /// Growth of [lo, hi] that also bounds the Hermite curve of the chunk's segments.
    pub pad: f64,
}

#[derive(Clone, Debug, Default)]
pub struct TrackBounds {
    pub chunks: Vec<Chunk>,
}

impl TrackBounds {
    pub fn new(t: &Track) -> Self {
        let n = t.points.len();
        let mut chunks = Vec::with_capacity(n / CHUNK + 1);
        let mut first = 0;
        while first < n {
            let last = (first + CHUNK).min(n - 1);
            let pts = &t.points[first..=last];
            let lo = pts.iter().fold(pts[0].pos, |m, p| m.min(p.pos));
            let hi = pts.iter().fold(pts[0].pos, |m, p| m.max(p.pos));
            let stray = pts
                .windows(2)
                .map(|w| 0.0963 * (w[1].pos - w[0].pos).norm() + 0.1482 * (w[1].t - w[0].t) * (w[0].vel.norm() + w[1].vel.norm()))
                .fold(0.0, f64::max);
            chunks.push(Chunk { first, last, lo, hi, pad: stray + PAD_SLACK_M });
            if last == n - 1 {
                break;
            }
            first = last;
        }
        Self { chunks }
    }
}

impl Chunk {
    /// Lower bound on |q − p|² over the chunk's points q (exact: same subtractions as the direct range).
    pub fn point_lb2(&self, p: Vec3) -> f64 {
        dist2_box(p, self.lo, self.hi, 0.0)
    }

    /// Lower bound on |c(t) − p|² over the chunk's Hermite curve.
    pub fn curve_lb2(&self, p: Vec3) -> f64 {
        dist2_box(p, self.lo, self.hi, self.pad)
    }

    /// Corners of the box holding the chunk's points and curve.
    pub fn curve_box(&self) -> (Vec3, Vec3) {
        let pad = Vec3::new(self.pad, self.pad, self.pad);
        (self.lo - pad, self.hi + pad)
    }
}

fn dist2_box(p: Vec3, lo: Vec3, hi: Vec3, pad: f64) -> f64 {
    let d = |x: f64, l: f64, h: f64| (l - pad - x).max(x - (h + pad)).max(0.0);
    let (dx, dy, dz) = (d(p.x, lo.x, hi.x), d(p.y, lo.y, hi.y), d(p.z, lo.z, hi.z));
    dx * dx + dy * dy + dz * dz
}
