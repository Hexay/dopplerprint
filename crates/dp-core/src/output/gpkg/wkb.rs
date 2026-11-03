//! GeoPackage geometry blobs: "GP" header (v0, little-endian, srs id, optional XY envelope) + ISO WKB.

/// Geometry in the layer's SRS axis order: x = lon / easting, y = lat / northing.
#[derive(Clone, Debug, PartialEq)]
pub enum Geom {
    Point([f64; 2]),
    LineStringZ(Vec<[f64; 3]>),
    Polygon(Vec<Vec<[f64; 2]>>),
}

/// [min_x, max_x, min_y, max_y], the GeoPackage envelope order.
pub type Envelope = [f64; 4];

pub const EMPTY_ENVELOPE: Envelope = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];

pub fn union(a: Envelope, b: Envelope) -> Envelope {
    [a[0].min(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].max(b[3])]
}

impl Geom {
    pub fn envelope(&self) -> Envelope {
        let mut e = EMPTY_ENVELOPE;
        let mut add = |x: f64, y: f64| e = union(e, [x, x, y, y]);
        match self {
            Geom::Point([x, y]) => add(*x, *y),
            Geom::LineStringZ(pts) => pts.iter().for_each(|p| add(p[0], p[1])),
            Geom::Polygon(rings) => rings.iter().flatten().for_each(|p| add(p[0], p[1])),
        }
        e
    }

    /// Full GeoPackage binary for this geometry. Points carry no envelope (it would equal the point).
    pub fn to_gpkg(&self, srs_id: i32) -> Vec<u8> {
        let with_env = !matches!(self, Geom::Point(_));
        let flags: u8 = 0b0000_0001 | if with_env { 0b0000_0010 } else { 0 };
        let mut b = vec![b'G', b'P', 0, flags];
        b.extend(srs_id.to_le_bytes());
        if with_env {
            self.envelope().iter().for_each(|v| b.extend(v.to_le_bytes()));
        }
        self.write_wkb(&mut b);
        b
    }

    fn write_wkb(&self, b: &mut Vec<u8>) {
        let f = |b: &mut Vec<u8>, v: f64| b.extend(v.to_le_bytes());
        b.push(1);
        match self {
            Geom::Point([x, y]) => {
                b.extend(1u32.to_le_bytes());
                f(b, *x);
                f(b, *y);
            }
            Geom::LineStringZ(pts) => {
                b.extend(1002u32.to_le_bytes());
                b.extend((pts.len() as u32).to_le_bytes());
                for p in pts {
                    p.iter().for_each(|&v| f(b, v));
                }
            }
            Geom::Polygon(rings) => {
                b.extend(3u32.to_le_bytes());
                b.extend((rings.len() as u32).to_le_bytes());
                for ring in rings {
                    b.extend((ring.len() as u32).to_le_bytes());
                    for p in ring {
                        p.iter().for_each(|&v| f(b, v));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_blob_layout() {
        let b = Geom::Point([1.5, -2.0]).to_gpkg(4326);
        assert_eq!(&b[..4], &[b'G', b'P', 0, 1]);
        assert_eq!(i32::from_le_bytes(b[4..8].try_into().unwrap()), 4326);
        assert_eq!(b[8], 1);
        assert_eq!(u32::from_le_bytes(b[9..13].try_into().unwrap()), 1);
        assert_eq!(b.len(), 8 + 1 + 4 + 16);
    }

    #[test]
    fn linestring_z_has_envelope() {
        let g = Geom::LineStringZ(vec![[0.0, 1.0, 5.0], [2.0, -1.0, 6.0]]);
        let b = g.to_gpkg(4326);
        assert_eq!(b[3], 0b11);
        assert_eq!(g.envelope(), [0.0, 2.0, -1.0, 1.0]);
        assert_eq!(u32::from_le_bytes(b[41..45].try_into().unwrap()), 1002);
        assert_eq!(b.len(), 8 + 32 + 1 + 4 + 4 + 2 * 24);
    }
}
