//! 1°×1° DEM source tiles, their names and URLs. Mirrors python/dopplerprint/prep/dem_sources.py.

/// Terrain source, in the default preference order (docs/GEO.md "DEM raster").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DemSource {
    /// USGS 3DEP 1″ bare earth, NAVD88.
    Tdep1,
    /// Copernicus GLO-30 DSM (canopy and buildings included), EGM2008.
    Glo30,
}

pub const DEFAULT_SOURCES: [DemSource; 2] = [DemSource::Tdep1, DemSource::Glo30];

const TDEP_URL: &str = "https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1/TIFF/current";
const GLO30_URL: &str = "https://copernicus-dem-30m.s3.amazonaws.com";

/// 1°×1° tile with south-west corner (lat, lon).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tile {
    pub lat: i32,
    pub lon: i32,
}

impl Tile {
    // 3DEP names tiles by their north-west corner.
    fn tdep_name(self) -> String {
        let north = self.lat + 1;
        let ns = if north >= 0 { format!("n{north:02}") } else { format!("s{:02}", -north) };
        let ew = if self.lon < 0 { format!("w{:03}", -self.lon) } else { format!("e{:03}", self.lon) };
        ns + &ew
    }

    fn glo30_name(self) -> String {
        let ns = if self.lat >= 0 { format!("N{:02}", self.lat) } else { format!("S{:02}", -self.lat) };
        let ew = if self.lon >= 0 { format!("E{:03}", self.lon) } else { format!("W{:03}", -self.lon) };
        format!("Copernicus_DSM_COG_10_{ns}_00_{ew}_00_DEM")
    }
}

impl DemSource {
    pub fn label(self) -> &'static str {
        match self {
            DemSource::Tdep1 => "3dep",
            DemSource::Glo30 => "glo30",
        }
    }

    /// Cache file name; distinct across sources, so all tiles share one directory.
    pub fn file_name(self, t: Tile) -> String {
        match self {
            DemSource::Tdep1 => format!("USGS_1_{}.tif", t.tdep_name()),
            DemSource::Glo30 => format!("{}.tif", t.glo30_name()),
        }
    }

    pub fn url(self, t: Tile) -> String {
        match self {
            DemSource::Tdep1 => {
                let n = t.tdep_name();
                format!("{TDEP_URL}/{n}/USGS_1_{n}.tif")
            }
            DemSource::Glo30 => {
                let n = t.glo30_name();
                format!("{GLO30_URL}/{n}/{n}.tif")
            }
        }
    }
}

/// Tiles touching `[lon_w, lat_s, lon_e, lat_n]`, south-to-north then west-to-east. Later tiles overwrite
/// earlier ones where they overlap, the same order the Python prep warps them in.
pub fn tiles_for_bbox(bbox: [f64; 4]) -> Vec<Tile> {
    let [lon_w, lat_s, lon_e, lat_n] = bbox;
    let (la0, la1) = (lat_s.floor() as i32, lat_n.ceil() as i32);
    let (lo0, lo1) = (lon_w.floor() as i32, lon_e.ceil() as i32);
    (la0..la1).flat_map(|lat| (lo0..lo1).map(move |lon| Tile { lat, lon })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_python_prep() {
        let t = Tile { lat: 37, lon: -122 };
        assert_eq!(DemSource::Tdep1.file_name(t), "USGS_1_n38w122.tif");
        assert_eq!(
            DemSource::Tdep1.url(t),
            "https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1/TIFF/current/n38w122/USGS_1_n38w122.tif"
        );
        assert_eq!(DemSource::Glo30.file_name(t), "Copernicus_DSM_COG_10_N37_00_W122_00_DEM.tif");
        assert_eq!(
            DemSource::Glo30.url(t),
            "https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_N37_00_W122_00_DEM/\
             Copernicus_DSM_COG_10_N37_00_W122_00_DEM.tif"
        );
        let sw = Tile { lat: -2, lon: 5 };
        assert_eq!(DemSource::Tdep1.file_name(sw), "USGS_1_s01e005.tif");
        assert_eq!(DemSource::Glo30.file_name(sw), "Copernicus_DSM_COG_10_S02_00_E005_00_DEM.tif");
    }

    #[test]
    fn bbox_tiles_cover_and_order() {
        let t = tiles_for_bbox([-122.07, 37.44, -121.5, 38.0]);
        assert_eq!(t, vec![Tile { lat: 37, lon: -123 }, Tile { lat: 37, lon: -122 }]);
        let t = tiles_for_bbox([-122.07, 37.44, -121.5, 38.01]);
        assert_eq!(t.len(), 4);
        assert_eq!(t[2], Tile { lat: 38, lon: -123 });
        assert_eq!(tiles_for_bbox([10.2, 45.1, 10.3, 45.2]), vec![Tile { lat: 45, lon: 10 }]);
    }
}
