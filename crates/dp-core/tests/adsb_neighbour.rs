use arrow_array::types::{Int32Type, UInt8Type};
use dp_core::adsb::AltSource;
use dp_core::adsb::model::CleanPoint;
use dp_core::adsb::neighbour::{OffsetAgg, off_key};
use dp_core::adsb::schema::{PointRow, column, points_batch};

const DAY0: f64 = 1_788_825_600.0;

fn row(alt_m: f64, alt_source: AltSource, lat: f64) -> PointRow {
    let p = CleanPoint { t: DAY0 + 3700.0, lat, lon: -83.5, alt_m, alt_source, ve: 0.0, vn: 0.0, vu: 0.0, src: 0, flags: 0 };
    PointRow { icao: 1, track_no: 0, p }
}

#[test]
fn baro_rows_take_the_neighbour_offset_only_with_enough_aircraft() {
    let key = off_key(DAY0 + 3600.0, 40.2, -83.2, 500.0, DAY0);
    let mut agg = OffsetAgg::default();
    agg.add(&[(key, 40.0)]);
    agg.add(&[(key, 60.0)]);
    let lonely = off_key(DAY0 + 3600.0, 41.2, -83.2, 500.0, DAY0);
    agg.add(&[(lonely, 99.0)]);
    let grid = agg.into_grid();
    assert_eq!(grid.len(), 1);

    let rows = [row(450.0, AltSource::Baro, 40.5), row(450.0, AltSource::Geometric, 40.5), row(450.0, AltSource::Baro, 41.5)];
    let out = grid.apply_points(points_batch(&rows), DAY0).unwrap();
    let (alt, src) = (column::<Int32Type>(&out, "alt_dm"), column::<UInt8Type>(&out, "alt_source"));
    assert_eq!((alt.value(0), src.value(0)), (5000, AltSource::BaroNeighbour as u8));
    assert_eq!((alt.value(1), src.value(1)), (4500, AltSource::Geometric as u8));
    assert_eq!((alt.value(2), src.value(2)), (4500, AltSource::Baro as u8));
    assert_eq!(grid.applied.load(std::sync::atomic::Ordering::Relaxed), 1);
}
