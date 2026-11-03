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
#[derive(Clone)]
pub struct Nearby<'a> {
    pub track: &'a Track,
    /// Position of `track` in `Catalog::all()`.
    pub index: u32,
    pub bounds: &'a TrackBounds,
    /// Ascending indices of the chunks whose curve box meets the query cube; every other chunk's curve
    /// lies farther than `query_m` from the query point.
    pub chunks: Vec<u32>,
    /// `radius_m + POINT_SLACK_M` of the query. A returned track has a knot this close, so its CPA is too.
    pub query_m: f64,
}

impl Nearby<'_> {
    /// CPA from the query point if within `reach_m`, else `None`. Reach is capped at `query_m`, where
    /// the result is unchanged (see `query_m`) and the omitted chunks cannot matter.
    pub fn cpa(&self, obs: Vec3, atm: &Atmosphere, reach_m: f64) -> Option<Cpa> {
        let chunks = self.chunks.iter().map(|&c| &self.bounds.chunks[c as usize]);
        physics::cpa_bounded(self.track, chunks, obs, atm, reach_m.min(self.query_m))
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
                b.chunks.iter().enumerate().map(move |(ci, c)| {
                    let (lo, hi) = c.curve_box();
                    ChunkRef { track: ti as u32, chunk: ci as u32, env: AABB::from_corners([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]) }
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
    /// Same set as testing every track: a point within reach lies in a chunk whose (curve) box meets the
    /// query cube.
    fn near(&self, p: Vec3, radius_m: f64, t0: f64, t1: f64) -> Vec<Nearby<'_>> {
        let reach = radius_m + POINT_SLACK_M;
        // +1 m so rounding at the cube face cannot drop a chunk `Nearby::cpa` needs.
        let h = reach + 1.0;
        let cube = AABB::from_corners([p.x - h, p.y - h, p.z - h], [p.x + h, p.y + h, p.z + h]);
        let mut hits: Vec<(u32, u32)> = self.tree.locate_in_envelope_intersecting(cube).map(|r| (r.track, r.chunk)).collect();
        hits.sort_unstable();
        hits.chunk_by(|a, b| a.0 == b.0)
            .filter_map(|run| {
                let i = run[0].0 as usize;
                let (t, b, (c, r)) = (&self.tracks[i], &self.bounds[i], &self.spheres[i]);
                let in_time = t.t_end() >= t0 && t.t_start() <= t1 && (p - *c).norm() <= r + radius_m;
                let point_near = |&(_, ci): &(u32, u32)| {
                    let ch = &b.chunks[ci as usize];
                    t.points[ch.first..=ch.last].iter().any(|q| (q.pos - p).norm() <= reach)
                };
                (in_time && run.iter().any(point_near))
                    .then(|| Nearby {
                        track: t,
                        index: i as u32,
                        bounds: b,
                        chunks: run.iter().map(|h| h.1).collect(),
                        query_m: reach,
                    })
            })
            .collect()
    }
    fn all(&self) -> &[Track] {
        &self.tracks
    }
}
