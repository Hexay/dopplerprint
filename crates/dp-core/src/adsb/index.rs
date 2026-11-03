//! In-memory R-tree over consecutive track-point legs (ECEF chords) for "legs within R of P during [t0, t1]".

use rstar::{AABB, RTree, RTreeObject};

use crate::geo::{Geodetic, Vec3, geodetic_to_ecef};
use crate::track::Track;

#[derive(Clone, Debug)]
struct Leg {
    track: u32,
    seg: u32,
    a: Vec3,
    b: Vec3,
    ta: f64,
    tb: f64,
}

impl RTreeObject for Leg {
    type Envelope = AABB<[f64; 3]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.a.x, self.a.y, self.a.z], [self.b.x, self.b.y, self.b.z])
    }
}

/// One leg (points `seg`→`seg+1` of `tracks[track]`) within range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentHit {
    pub track: usize,
    pub seg: usize,
    /// Distance from the query point to the straight ECEF chord.
    pub dist_m: f64,
    /// Time at the closest point of the chord (linear in time along the leg).
    pub t_closest: f64,
}

pub struct SegmentIndex {
    tree: RTree<Leg>,
}

impl SegmentIndex {
    /// Index every leg of every track; the caller keeps `tracks` to resolve hits.
    pub fn from_tracks(tracks: &[Track]) -> Self {
        let legs = tracks
            .iter()
            .enumerate()
            .flat_map(|(ti, tr)| {
                tr.points.windows(2).enumerate().map(move |(si, w)| Leg {
                    track: ti as u32,
                    seg: si as u32,
                    a: w[0].pos,
                    b: w[1].pos,
                    ta: w[0].t,
                    tb: w[1].t,
                })
            })
            .collect();
        Self { tree: RTree::bulk_load(legs) }
    }

    pub fn len(&self) -> usize {
        self.tree.size()
    }

    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    /// Legs whose time span overlaps `[t0, t1]` and whose chord passes within `radius_m` of `p`, nearest first.
    pub fn within(&self, p: Vec3, radius_m: f64, t0: f64, t1: f64) -> Vec<SegmentHit> {
        let r = radius_m;
        let env = AABB::from_corners([p.x - r, p.y - r, p.z - r], [p.x + r, p.y + r, p.z + r]);
        let mut hits: Vec<SegmentHit> = self
            .tree
            .locate_in_envelope_intersecting(env)
            .filter(|l| l.tb >= t0 && l.ta <= t1)
            .filter_map(|l| {
                let ab = l.b - l.a;
                let len2 = ab.dot(ab);
                let s = if len2 > 0.0 { ((p - l.a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let dist_m = (l.a + ab * s - p).norm();
                (dist_m <= r).then_some(SegmentHit {
                    track: l.track as usize,
                    seg: l.seg as usize,
                    dist_m,
                    t_closest: l.ta + s * (l.tb - l.ta),
                })
            })
            .collect();
        hits.sort_by(|a, b| a.dist_m.total_cmp(&b.dist_m));
        hits
    }

    /// `within` for a geodetic point (`h_ell_m` is height above the WGS84 ellipsoid).
    pub fn within_geodetic(&self, g: Geodetic, radius_m: f64, t0: f64, t1: f64) -> Vec<SegmentHit> {
        self.within(geodetic_to_ecef(g), radius_m, t0, t1)
    }
}
