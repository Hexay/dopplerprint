//! Track source for the search. In-memory for sims and regional runs; the ADS-B store's
//! segment index implements the same trait for national runs.

use rstar::{AABB, RTree, RTreeObject};

use crate::geo::Vec3;
use crate::physics::{self, Atmosphere, Cpa};
use crate::track::{Track, TrackBounds};

/// Slack (m) beyond `radius_m` within which some track point must lie for `near` to return the track.
pub const POINT_SLACK_M: f64 = 2_000.0;

pub trait Catalog: Sync {
    /// Tracks that pass within `radius_m` (straight-line, ECEF) of `p` at any time in [t0, t1], in
    /// catalogue order (hypothesis enumeration depends on it).
    fn near(&self, p: Vec3, radius_m: f64, t0: f64, t1: f64) -> Vec<Nearby<'_>>;
    fn all(&self) -> &[Track];
}

/// A catalogue track with the chunk bounds used for exact pruning (track::bounds).
#[derive(Clone, Copy)]
pub struct Nearby<'a> {
    pub track: &'a Track,
    pub bounds: &'a TrackBounds,
}

impl Nearby<'_> {
    pub fn cpa(&self, obs: Vec3, atm: &Atmosphere) -> Option<Cpa> {
        physics::cpa_bounded(self.track, self.bounds, obs, atm)
    }
}

/// Catalogue with per-track bounding spheres and an R-tree over point chunks.
pub struct MemCatalog {
    tracks: Vec<Track>,
    spheres: Vec<(Vec3, f64)>,
    bounds: Vec<TrackBounds>,
    tree: RTree<ChunkRef>,
}

struct ChunkRef {
    track: u32,
    chunk: u32,
    env: AABB<[f64; 3]>,
}

impl RTreeObject for ChunkRef {
    type Envelope = AABB<[f64; 3]>;
    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

impl MemCatalog {
    pub fn new(tracks: Vec<Track>) -> Self {
        let spheres = tracks.iter().map(bounding_sphere).collect();
        let bounds: Vec<TrackBounds> = tracks.iter().map(TrackBounds::new).collect();
        let refs = bounds
            .iter()
            .enumerate()
            .flat_map(|(ti, b)| {
                b.chunks.iter().enumerate().map(move |(ci, c)| ChunkRef {
                    track: ti as u32,
                    chunk: ci as u32,
                    env: AABB::from_corners([c.lo.x, c.lo.y, c.lo.z], [c.hi.x, c.hi.y, c.hi.z]),
                })
            })
            .collect();
        Self { tracks, spheres, bounds, tree: RTree::bulk_load(refs) }
    }
}

fn bounding_sphere(t: &Track) -> (Vec3, f64) {
    let n = t.points.len().max(1) as f64;
    let c = t.points.iter().fold(Vec3::ZERO, |a, p| a + p.pos) * (1.0 / n);
    let r = t.points.iter().map(|p| (p.pos - c).norm()).fold(0.0, f64::max);
    (c, r)
}

impl Catalog for MemCatalog {
    /// Same set as testing every track: a point within reach lies in a chunk whose box meets the query cube.
    fn near(&self, p: Vec3, radius_m: f64, t0: f64, t1: f64) -> Vec<Nearby<'_>> {
        let reach = radius_m + POINT_SLACK_M;
        let cube = AABB::from_corners([p.x - reach, p.y - reach, p.z - reach], [p.x + reach, p.y + reach, p.z + reach]);
        let mut hit: Vec<u32> = self
            .tree
            .locate_in_envelope_intersecting(cube)
            .filter(|r| {
                let t = &self.tracks[r.track as usize];
                let c = &self.bounds[r.track as usize].chunks[r.chunk as usize];
                t.points[c.first..=c.last].iter().any(|q| (q.pos - p).norm() <= reach)
            })
            .map(|r| r.track)
            .collect();
        hit.sort_unstable();
        hit.dedup();
        hit.into_iter()
            .map(|i| i as usize)
            .filter(|&i| {
                let (t, (c, r)) = (&self.tracks[i], &self.spheres[i]);
                t.t_end() >= t0 && t.t_start() <= t1 && (p - *c).norm() <= r + radius_m
            })
            .map(|i| Nearby { track: &self.tracks[i], bounds: &self.bounds[i] })
            .collect()
    }
    fn all(&self) -> &[Track] {
        &self.tracks
    }
}
