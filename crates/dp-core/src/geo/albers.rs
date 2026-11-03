//! EPSG:5070 (NAD83 / Conus Albers) forward/inverse projection and the candidate lattice.
//! NAD83 vs WGS84 differ by ~1 m, below our tolerance, so WGS84 lat/lon are used directly.

/// Integer cell on a square lattice in EPSG:5070 metres. `step_m` halves per refinement level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub ix: i32,
    pub iy: i32,
    pub step_m: u32,
}

/// Projected EPSG:5070 coordinate in metres.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

pub fn forward(_lat_deg: f64, _lon_deg: f64) -> Xy {
    todo!("geo agent: Albers equal-area conic, EPSG:5070 parameters")
}

pub fn inverse(_xy: Xy) -> (f64, f64) {
    todo!("geo agent")
}

impl Cell {
    pub fn center(&self) -> Xy {
        let s = self.step_m as f64;
        Xy { x: (self.ix as f64 + 0.5) * s, y: (self.iy as f64 + 0.5) * s }
    }
    pub fn of(xy: Xy, step_m: u32) -> Cell {
        let s = step_m as f64;
        Cell { ix: (xy.x / s).floor() as i32, iy: (xy.y / s).floor() as i32, step_m }
    }
    /// The 4 children at half the step.
    pub fn children(&self) -> [Cell; 4] {
        let (ix, iy, s) = (self.ix * 2, self.iy * 2, self.step_m / 2);
        [
            Cell { ix, iy, step_m: s },
            Cell { ix: ix + 1, iy, step_m: s },
            Cell { ix, iy: iy + 1, step_m: s },
            Cell { ix: ix + 1, iy: iy + 1, step_m: s },
        ]
    }
}
