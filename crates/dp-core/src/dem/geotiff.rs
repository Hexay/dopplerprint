//! Decodes one downloaded 1° GeoTIFF tile (f32 heights on a lon/lat grid) and samples it the way GDAL's
//! bilinear warper does. Downloads are untrusted, so the georeferencing is checked against the tile.

use std::io::{Read, Seek};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

use super::tiles::Tile;

const MAX_SIDE: u32 = 12_000;
const GT_RASTER_TYPE: u16 = 1025;
const PIXEL_IS_POINT: u16 = 2;

/// Heights on a regular lon/lat grid; row 0 is north. Pixel (r, c) centre is `(lat0 − r·dlat, lon0 + c·dlon)`.
#[derive(Debug)]
pub struct SourceRaster {
    pub width: usize,
    pub height: usize,
    pub lon0: f64,
    pub lat0: f64,
    pub dlon: f64,
    pub dlat: f64,
    pub nodata: Option<f32>,
    pub data: Vec<f32>,
}

impl SourceRaster {
    /// Reads `path` and checks it is a 1″-class tile covering `tile`.
    pub fn read(path: &Path, tile: Tile) -> Result<SourceRaster> {
        let f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let r = Self::decode(std::io::BufReader::new(f)).with_context(|| format!("decoding {}", path.display()))?;
        r.check_covers(tile).with_context(|| format!("{}", path.display()))?;
        Ok(r)
    }

    pub fn decode<R: Read + Seek>(reader: R) -> Result<SourceRaster> {
        let mut d = Decoder::new(reader)?;
        let (w, h) = d.dimensions()?;
        ensure!(w >= 2 && h >= 2 && w <= MAX_SIDE && h <= MAX_SIDE, "implausible size {w}x{h}");
        let scale = d.get_tag_f64_vec(Tag::ModelPixelScaleTag).context("no ModelPixelScale")?;
        let tie = d.get_tag_f64_vec(Tag::ModelTiepointTag).context("no ModelTiepoint")?;
        ensure!(scale.len() >= 2 && tie.len() >= 6, "short GeoTIFF georeferencing tags");
        let keys = d.get_tag_u16_vec(Tag::GeoKeyDirectoryTag).context("no GeoKeyDirectory")?;
        let nodata = match d.find_tag(Tag::GdalNodata)? {
            Some(v) => Some(v.into_string()?.trim_matches(|c: char| c == '\0' || c.is_whitespace()).parse::<f32>()?),
            None => None,
        };
        let data = match d.read_image()? {
            DecodingResult::F32(v) => v,
            DecodingResult::I16(v) => v.into_iter().map(f32::from).collect(),
            _ => bail!("unsupported sample format {:?}", d.colortype()?),
        };
        ensure!(data.len() == w as usize * h as usize, "multi-band or truncated image");
        let (dlon, dlat) = (scale[0], scale[1]);
        // GDAL honours PixelIsPoint: the tiepoint is then a pixel centre rather than a pixel corner.
        let corner = if geo_key(&keys, GT_RASTER_TYPE) == Some(PIXEL_IS_POINT) { 0.0 } else { 0.5 };
        Ok(SourceRaster {
            width: w as usize,
            height: h as usize,
            lon0: tie[3] + (corner - tie[0]) * dlon,
            lat0: tie[4] - (corner - tie[1]) * dlat,
            dlon,
            dlat,
            nodata,
            data,
        })
    }

    fn check_covers(&self, t: Tile) -> Result<()> {
        let arcsec = 1.0 / 3600.0;
        ensure!((self.dlat - arcsec).abs() < 1e-3 * arcsec, "lat spacing {} is not 1″", self.dlat);
        ensure!(self.dlon > 0.99 * arcsec && self.dlon < 6.01 * arcsec, "lon spacing {} out of range", self.dlon);
        let [w, s, e, n] = self.extent();
        let tol = 2.0 * self.dlon.max(self.dlat);
        ensure!(
            w <= t.lon as f64 + tol && e >= (t.lon + 1) as f64 - tol && s <= t.lat as f64 + tol && n >= (t.lat + 1) as f64 - tol,
            "extent {:?} does not cover tile {:?}",
            [w, s, e, n],
            t
        );
        ensure!(w >= t.lon as f64 - 0.1 && n <= (t.lat + 1) as f64 + 0.1, "extent {:?} is not tile {:?}", [w, s, e, n], t);
        Ok(())
    }

    /// Pixel-area extent `[lon_w, lat_s, lon_e, lat_n]`.
    pub fn extent(&self) -> [f64; 4] {
        let (w, n) = (self.lon0 - 0.5 * self.dlon, self.lat0 + 0.5 * self.dlat);
        [w, n - self.height as f64 * self.dlat, w + self.width as f64 * self.dlon, n]
    }

    fn value(&self, r: isize, c: isize) -> Option<f32> {
        if r < 0 || c < 0 || r as usize >= self.height || c as usize >= self.width {
            return None;
        }
        let v = self.data[r as usize * self.width + c as usize];
        (v.is_finite() && Some(v) != self.nodata).then_some(v)
    }

    /// GDAL 4-sample bilinear: neighbours that are nodata or off-tile are dropped and the weights
    /// renormalised. `None` outside the pixel-area extent or with no valid neighbour.
    pub fn sample(&self, lat: f64, lon: f64) -> Option<f64> {
        let fc = (lon - self.lon0) / self.dlon;
        let fr = (self.lat0 - lat) / self.dlat;
        if !(-0.5..=self.width as f64 - 0.5).contains(&fc) || !(-0.5..=self.height as f64 - 0.5).contains(&fr) {
            return None;
        }
        let (c, r) = (fc.floor(), fr.floor());
        let (tc, tr) = (fc - c, fr - r);
        let (c, r) = (c as isize, r as isize);
        let (mut sum, mut wsum) = (0.0, 0.0);
        for (dr, dc, w) in [(0, 0, (1.0 - tr) * (1.0 - tc)), (0, 1, (1.0 - tr) * tc), (1, 0, tr * (1.0 - tc)), (1, 1, tr * tc)] {
            if let Some(v) = self.value(r + dr, c + dc) {
                sum += w * v as f64;
                wsum += w;
            }
        }
        (wsum >= 1e-5).then(|| sum / wsum)
    }
}

/// Value of a short-valued key in a GeoKeyDirectory (header of 4 shorts, then 4 shorts per key).
fn geo_key(keys: &[u16], id: u16) -> Option<u16> {
    keys.get(4..)?.as_chunks::<4>().0.iter().find(|k| k[0] == id && k[1] == 0).map(|k| k[3])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;
    use tiff::encoder::{Compression, TiffEncoder, colortype::Gray32Float};

    /// GeoTIFF shaped like the real tiles: f32, geographic, GDAL nodata tag. Pixel (0, 0) is nodata, the
    /// rest `value(row, col)`.
    pub(crate) fn fixture_with(
        w: u32,
        h: u32,
        tie: [f64; 2],
        step: f64,
        point: bool,
        compression: Compression,
        value: impl Fn(u32, u32) -> f32,
    ) -> Vec<u8> {
        let data: Vec<f32> = (0..w * h).map(|i| if i == 0 { -999999.0 } else { value(i / w, i % w) }).collect();
        let mut buf = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut buf).unwrap().with_compression(compression);
        let mut img = enc.new_image::<Gray32Float>(w, h).unwrap();
        let raster_type = if point { PIXEL_IS_POINT } else { 1 };
        let keys: [u16; 16] = [1, 1, 0, 3, 1024, 0, 1, 2, GT_RASTER_TYPE, 0, 1, raster_type, 2048, 0, 1, 4326];
        let e = img.encoder();
        e.write_tag(Tag::ModelPixelScaleTag, &[step, step, 0.0][..]).unwrap();
        e.write_tag(Tag::ModelTiepointTag, &[0.0, 0.0, 0.0, tie[0], tie[1], 0.0][..]).unwrap();
        e.write_tag(Tag::GeoKeyDirectoryTag, &keys[..]).unwrap();
        e.write_tag(Tag::GdalNodata, "-999999").unwrap();
        img.write_data(&data).unwrap();
        buf.into_inner()
    }

    fn fixture(w: u32, h: u32, tie: [f64; 2], step: f64, point: bool, compression: Compression) -> Vec<u8> {
        fixture_with(w, h, tie, step, point, compression, |r, c| (r * 10 + c) as f32)
    }

    #[test]
    fn decodes_fixture_area_and_point() {
        for compression in [Compression::Lzw, Compression::Deflate(Default::default()), Compression::Uncompressed] {
            let r = SourceRaster::decode(Cursor::new(fixture(4, 3, [-122.0, 38.0], 0.25, false, compression))).unwrap();
            assert_eq!((r.width, r.height, r.nodata), (4, 3, Some(-999999.0)));
            assert_eq!((r.lon0, r.lat0), (-121.875, 37.875));
            assert_eq!(r.extent(), [-122.0, 37.25, -121.0, 38.0]);
            assert_eq!(r.data[5], 11.0);
        }
        let p = SourceRaster::decode(Cursor::new(fixture(4, 3, [-122.0, 38.0], 0.25, true, Compression::Lzw))).unwrap();
        assert_eq!((p.lon0, p.lat0), (-122.0, 38.0));
    }

    #[test]
    fn bilinear_with_nodata_and_edges() {
        let r = SourceRaster::decode(Cursor::new(fixture(4, 3, [0.0, 3.0], 1.0, true, Compression::Lzw))).unwrap();
        // Centres at lon 0..3, lat 3..1; value = 10·row + col, (0, 0) is nodata.
        assert_eq!(r.sample(2.0, 1.0), Some(11.0));
        assert!((r.sample(2.5, 1.5).unwrap() - 6.5).abs() < 1e-12);
        assert!((r.sample(2.5, 0.5).unwrap() - (1.0 + 10.0 + 11.0) / 3.0).abs() < 1e-12, "nodata renormalised");
        assert_eq!(r.sample(1.0, 3.4), Some(23.0), "half pixel past the last centre still samples");
        assert_eq!(r.sample(1.0, 3.6), None);
        assert_eq!(r.sample(3.0, 0.0), None, "only nodata in reach");
    }

    #[test]
    fn rejects_wrong_tile_and_garbage() {
        let r = SourceRaster::decode(Cursor::new(fixture(3600, 2, [-122.0, 38.0], 1.0 / 3600.0, false, Compression::Lzw)));
        let r = r.unwrap();
        assert!(r.check_covers(Tile { lat: 37, lon: -122 }).is_err(), "only two rows: does not cover");
        assert!(SourceRaster::decode(Cursor::new(b"II*\0garbage".to_vec())).is_err());
        assert!(SourceRaster::decode(Cursor::new(b"<html>not a tiff</html>".to_vec())).is_err());
    }
}
