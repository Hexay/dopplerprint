//! Basins of the posterior surface: each leaf joins its highest neighbour's basin, and basins whose peak
//! rises less than `persistence_nats` above the saddle to a higher basin are merged (ToMATo, Chazal et al.
//! 2013). A mirror pair is separated by a valley tens of nats deep, so it stays two modes.

use std::collections::HashMap;

use super::surface::{Posterior, Surface};
use crate::geo::albers::{self, Cell, Xy};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Mode {
    /// Leaf index of the peak.
    pub peak: usize,
    pub peak_score: f64,
    pub lat: f64,
    pub lon: f64,
    pub centroid_lat: f64,
    pub centroid_lon: f64,
    /// Posterior mass of the basin.
    pub mass: f64,
    /// Flight tuple of the peak (`Surface::tuples`).
    pub tuple: u32,
    /// Part of the basin inside the highest-density region at the requested level.
    pub region_mass: f64,
    pub region_km2: f64,
}

pub struct Modes {
    /// Heaviest first.
    pub modes: Vec<Mode>,
    /// Mode index per leaf.
    pub label: Vec<usize>,
}

impl Modes {
    /// Mode with the highest peak (what ranking by peak score picks).
    pub fn by_peak(&self) -> Option<usize> {
        (0..self.modes.len()).max_by(|&a, &b| self.modes[a].peak_score.total_cmp(&self.modes[b].peak_score))
    }
}

/// `level` is the density of the highest-density region whose share of each basin is reported.
pub fn modes(s: &Surface, post: &Posterior, temperature: f64, persistence_nats: f64, level: f64) -> Modes {
    let n = s.leaves.len();
    let h: Vec<f64> = s.leaves.iter().map(|l| l.score / temperature).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| h[b].total_cmp(&h[a]));
    let nb = Neighbours::new(s);
    let mut parent: Vec<usize> = (0..n).collect();
    let mut done = vec![false; n];
    for &i in &order {
        let mut roots: Vec<usize> = nb.of(s, i).into_iter().filter(|&j| done[j]).map(|j| find(&mut parent, j)).collect();
        roots.sort_unstable();
        roots.dedup();
        done[i] = true;
        let Some(&top) = roots.iter().max_by(|&&a, &&b| h[a].total_cmp(&h[b])) else { continue };
        parent[i] = top;
        for &r in &roots {
            if r != top && h[r] - h[i] < persistence_nats {
                parent[r] = top;
            }
        }
    }
    collect(s, post, &mut parent, level)
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn collect(s: &Surface, post: &Posterior, parent: &mut [usize], level: f64) -> Modes {
    let n = s.leaves.len();
    let mut index: HashMap<usize, usize> = HashMap::new();
    let mut acc: Vec<(usize, f64, f64, f64, f64, f64)> = Vec::new();
    let mut label = vec![0; n];
    for (i, lab) in label.iter_mut().enumerate() {
        let r = find(parent, i);
        let k = *index.entry(r).or_insert_with(|| {
            acc.push((r, 0.0, 0.0, 0.0, 0.0, 0.0));
            acc.len() - 1
        });
        *lab = k;
        let (m, c) = (post.mass(i), s.leaves[i].cell.center());
        let a = &mut acc[k];
        (a.1, a.2, a.3) = (a.1 + m, a.2 + m * c.x, a.3 + m * c.y);
        if post.density[i] >= level {
            (a.4, a.5) = (a.4 + m, a.5 + post.area[i]);
        }
    }
    let modes: Vec<Mode> = acc
        .iter()
        .map(|&(r, m, x, y, rm, ra)| {
            let (lat, lon) = albers::inverse(s.leaves[r].cell.center());
            let (centroid_lat, centroid_lon) = albers::inverse(Xy { x: x / m, y: y / m });
            let l = &s.leaves[r];
            Mode { peak: r, peak_score: l.score, lat, lon, centroid_lat, centroid_lon, mass: m, tuple: l.tuple, region_mass: rm, region_km2: ra }
        })
        .collect();
    let mut by_mass: Vec<usize> = (0..modes.len()).collect();
    by_mass.sort_by(|&a, &b| modes[b].mass.total_cmp(&modes[a].mass));
    let mut rank = vec![0; modes.len()];
    for (new, &old) in by_mass.iter().enumerate() {
        rank[old] = new;
    }
    label.iter_mut().for_each(|l| *l = rank[*l]);
    let modes = by_mass.iter().map(|&i| modes[i].clone()).collect();
    Modes { modes, label }
}

/// Leaves touching each leaf (edges or corners), across refinement levels.
struct Neighbours {
    index: HashMap<Cell, usize>,
    steps: Vec<u32>,
}

impl Neighbours {
    fn new(s: &Surface) -> Self {
        let index: HashMap<Cell, usize> = s.leaves.iter().enumerate().map(|(i, l)| (l.cell, i)).collect();
        let mut steps: Vec<u32> = s.leaves.iter().map(|l| l.cell.step_m).collect();
        steps.sort_unstable();
        steps.dedup();
        Neighbours { index, steps }
    }

    fn at(&self, p: Xy) -> Option<usize> {
        self.steps.iter().find_map(|&st| self.index.get(&Cell::of(p, st)).copied())
    }

    /// Probes a ring half a finest step outside the cell, one probe per finest step of perimeter.
    fn of(&self, s: &Surface, i: usize) -> Vec<usize> {
        let c = s.leaves[i].cell;
        let f = self.steps[0] as f64;
        let st = c.step_m as f64;
        let (x0, y0) = (c.ix as f64 * st, c.iy as f64 * st);
        let k = (st / f).round() as i32;
        let mut out = Vec::new();
        for j in -1..=k {
            let t = (j as f64 + 0.5) * f;
            for p in [(x0 + t, y0 - 0.5 * f), (x0 + t, y0 + st + 0.5 * f), (x0 - 0.5 * f, y0 + t), (x0 + st + 0.5 * f, y0 + t)] {
                if let Some(n) = self.at(Xy { x: p.0, y: p.1 })
                    && n != i
                    && !out.contains(&n)
                {
                    out.push(n);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::surface::Leaf;

    fn surface(scores: &[(i32, i32, f64)]) -> Surface {
        let leaves = scores.iter().map(|&(ix, iy, score)| Leaf { cell: Cell { ix, iy, step_m: 100 }, score, tuple: 0 }).collect();
        Surface { leaves, tuples: vec![vec!["a".into()]], region_km2: 1.0 }
    }

    #[test]
    fn mirror_pair_is_two_modes_and_ripples_merge() {
        // Row of 9 cells: peak 10 at x=1 (with a 0.3 ripple at x=3), valley, peak 9 at x=7.
        let row = [8.0, 10.0, 9.0, 9.3, 2.0, 0.0, 3.0, 9.0, 7.0];
        let s = surface(&row.iter().enumerate().map(|(i, &v)| (i as i32, 0, v)).collect::<Vec<_>>());
        let p = s.posterior(1.0, 0.0);
        let m = modes(&s, &p, 1.0, 2.0, p.level(0.9));
        assert_eq!(m.modes.len(), 2);
        assert_eq!(s.leaves[m.modes[0].peak].cell.ix, 1);
        assert_eq!(s.leaves[m.modes[1].peak].cell.ix, 7);
        assert_eq!(m.label[3], 0);
        assert_eq!(m.label[8], 1);
        let total: f64 = m.modes.iter().map(|x| x.mass).sum();
        assert!((total - 1.0).abs() < 1e-9 && m.modes[0].mass > m.modes[1].mass);
        assert_eq!(m.by_peak(), Some(0));
    }

    #[test]
    fn neighbours_cross_levels() {
        let mut s = surface(&[(0, 0, 1.0)]);
        let kids = Cell { ix: 1, iy: 0, step_m: 100 }.children();
        s.leaves.extend(kids.iter().map(|&cell| Leaf { cell, score: 1.0, tuple: 0 }));
        let nb = Neighbours::new(&s);
        let mut n = nb.of(&s, 0);
        n.sort_unstable();
        assert_eq!(n, [1, 3]);
    }
}
