//! Votes → per-cell best clock window → connected patches → ranked region proposals.

use std::collections::HashMap;

use rayon::prelude::*;

use super::footprint::{Vote, bin_start};
use super::{GlobalParams, RegionProposal};
use crate::geo::albers::{self, Xy};
use crate::search::pipeline::Region;
use crate::track::Track;

/// Best clock window of one lattice cell. `votes` indexes the sorted vote array.
#[derive(Clone, Copy, Debug)]
pub(super) struct CellBest {
    pub ix: i32,
    pub iy: i32,
    pub n: u16,
    pub score: f32,
    /// Window start bin; the window is [bin, bin + clock_tol_s / clock_bin_s].
    pub bin: i32,
    pub votes: (u32, u32),
}

fn window_bins(p: &GlobalParams) -> i32 {
    (p.clock_tol_s / p.clock_bin_s).round() as i32
}

/// Sorts `votes` in place; every returned cell references its slice of them.
pub(super) fn best_per_cell(votes: &mut [Vote], n_events: usize, p: &GlobalParams) -> Vec<CellBest> {
    votes.par_sort_unstable_by(|a, b| a.key().cmp(&b.key()).then(b.w.total_cmp(&a.w)));
    let mut starts = vec![0usize];
    for i in 1..votes.len() {
        if (votes[i].ix, votes[i].iy) != (votes[i - 1].ix, votes[i - 1].iy) {
            starts.push(i);
        }
    }
    starts.push(votes.len());
    let votes: &[Vote] = votes;
    let wb = window_bins(p);
    starts
        .par_windows(2)
        .filter(|w| w[1] > w[0])
        .map(|w| {
            let g = &votes[w[0]..w[1]];
            let (n, score, bin) = best_window(g, n_events, wb);
            CellBest { ix: g[0].ix, iy: g[0].iy, n, score, bin, votes: (w[0] as u32, w[1] as u32) }
        })
        .collect()
}

/// Clock window maximising (events explained, summed best weight) over a cell's votes.
fn best_window(g: &[Vote], n_events: usize, wb: i32) -> (u16, f32, i32) {
    let mut best = (0u16, f32::NEG_INFINITY, 0i32);
    let mut per_event = vec![0f32; n_events];
    let mut bins: Vec<i32> = g.iter().map(|v| v.bin).collect();
    bins.sort_unstable();
    bins.dedup();
    for &b in &bins {
        per_event.iter_mut().for_each(|x| *x = 0.0);
        for v in g.iter().filter(|v| v.bin >= b && v.bin <= b + wb) {
            let slot = &mut per_event[v.event as usize];
            *slot = slot.max(v.w);
        }
        let n = per_event.iter().filter(|w| **w > 0.0).count() as u16;
        let s: f32 = per_event.iter().sum();
        if (n, s) > (best.0, best.1) {
            best = (n, s, b);
        }
    }
    best
}

/// Connected patches of cells explaining ≥ n_keep events with overlapping clock windows, greedily
/// covered by regions of at most `max_radius_m`. Returns the kept proposals best first, n_max and the total count.
pub(super) fn proposals(
    cells: &[CellBest],
    votes: &[Vote],
    tracks: &[Track],
    n_events: usize,
    prior: (f64, f64),
    cell_m: f64,
    p: &GlobalParams,
) -> (Vec<RegionProposal>, usize, usize) {
    let n_max = cells.iter().map(|c| c.n as usize).max().unwrap_or(0);
    let n_keep = if n_max >= 2 + p.event_slack { n_max - p.event_slack } else { n_max }.max(1);
    let kept: Vec<&CellBest> = cells.iter().filter(|c| c.n as usize >= n_keep).collect();
    let index: HashMap<(i32, i32), usize> = kept.iter().enumerate().map(|(i, c)| ((c.ix, c.iy), i)).collect();
    let wb = window_bins(p);
    let mut comp = vec![usize::MAX; kept.len()];
    let mut out = Vec::new();
    for root in 0..kept.len() {
        if comp[root] != usize::MAX {
            continue;
        }
        let mut members = vec![root];
        comp[root] = root;
        let mut k = 0;
        while k < members.len() {
            let c = kept[members[k]];
            k += 1;
            for (dx, dy) in NEIGHBOURS {
                let Some(&j) = index.get(&(c.ix + dx, c.iy + dy)) else { continue };
                if comp[j] == usize::MAX && (kept[j].bin - c.bin).abs() <= wb {
                    comp[j] = root;
                    members.push(j);
                }
            }
        }
        let patch: Vec<&CellBest> = members.iter().map(|&i| kept[i]).collect();
        cover(&patch, prior, cell_m, p, &mut out);
    }
    out.sort_by(|a, b| b.0.n_events.cmp(&a.0.n_events).then(b.0.score.total_cmp(&a.0.score)));
    let total = out.len();
    out.truncate(p.keep_proposals);
    let props = out
        .into_iter()
        .map(|(mut r, seed)| {
            r.tracks = supporting(&seed, votes, tracks, n_events, wb);
            r.flights = r.tracks.iter().map(|ev| ev.iter().map(|&t| tracks[t as usize].id.clone()).collect()).collect();
            r
        })
        .collect();
    (props, n_max, total)
}

const NEIGHBOURS: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

fn centre(c: &CellBest, cell_m: f64) -> Xy {
    Xy { x: (c.ix as f64 + 0.5) * cell_m, y: (c.iy as f64 + 0.5) * cell_m }
}

/// Greedy cover: the best unassigned cell seeds a region that absorbs every cell within max_radius;
/// the radius only has to reach cells of the seed's tier.
fn cover(patch: &[&CellBest], prior: (f64, f64), cell_m: f64, p: &GlobalParams, out: &mut Vec<(RegionProposal, CellBest)>) {
    let mut order: Vec<&CellBest> = patch.to_vec();
    order.sort_by(|a, b| b.n.cmp(&a.n).then(b.score.total_cmp(&a.score)));
    let at: HashMap<(i32, i32), usize> = order.iter().enumerate().map(|(i, c)| ((c.ix, c.iy), i)).collect();
    let mut taken = vec![false; order.len()];
    let reach = (p.max_radius_m - p.margin_m).max(cell_m);
    let span = (reach / cell_m).floor() as i32;
    for s in 0..order.len() {
        if taken[s] {
            continue;
        }
        let seed = order[s];
        let sc = centre(seed, cell_m);
        let (mut r, mut n_cells, mut b_lo, mut b_hi) = (0.0f64, 0usize, seed.bin, seed.bin);
        let mut cells = Vec::new();
        let near = (-span..=span).flat_map(|dx| (-span..=span).map(move |dy| (seed.ix + dx, seed.iy + dy)));
        for j in near.filter_map(|k| at.get(&k).copied()) {
            let c = order[j];
            let xy = centre(c, cell_m);
            let dist = (xy.x - sc.x).hypot(xy.y - sc.y);
            if taken[j] || dist > reach {
                continue;
            }
            taken[j] = true;
            if c.n == seed.n {
                r = r.max(dist);
                n_cells += 1;
                if cells.len() < p.rerank_cells {
                    cells.push(albers::inverse(xy));
                }
                b_lo = b_lo.min(c.bin);
                b_hi = b_hi.max(c.bin);
            }
        }
        let (lat, lon) = albers::inverse(sc);
        let radius_m = (r + 0.71 * cell_m + p.margin_m).clamp(p.min_radius_m, p.max_radius_m);
        let prop = RegionProposal {
            region: Region { lat_deg: lat, lon_deg: lon, radius_m },
            score: seed.score as f64,
            n_events: seed.n as usize,
            clock_range: (bin_start(b_lo, prior, p), bin_start(b_hi + window_bins(p) + 1, prior, p).min(prior.1)),
            flights: Vec::new(),
            tracks: Vec::new(),
            quick: None,
            n_cells,
            cells,
        };
        out.push((prop, *seed));
    }
}

/// Up to three distinct track indices per event inside the seed's clock window, best weight first.
fn supporting(c: &CellBest, votes: &[Vote], tracks: &[Track], n_events: usize, wb: i32) -> Vec<Vec<u32>> {
    let mut per: Vec<Vec<(f32, u32)>> = vec![Vec::new(); n_events];
    for v in &votes[c.votes.0 as usize..c.votes.1 as usize] {
        if v.bin >= c.bin && v.bin <= c.bin + wb {
            per[v.event as usize].push((v.w, v.track));
        }
    }
    per.into_iter()
        .map(|mut l| {
            l.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut ids: Vec<u32> = Vec::new();
            for (_, t) in l {
                let id = &tracks[t as usize].id;
                if !ids.iter().any(|&i| tracks[i as usize].id == *id) && ids.len() < 3 {
                    ids.push(t);
                }
            }
            ids
        })
        .collect()
}
