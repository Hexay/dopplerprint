//! GeoJSON FeatureCollection of tracks (one LineString each) for eyeballing a query in QGIS.

use serde_json::{Value, json};

use crate::geo::ecef_to_geodetic;
use crate::track::Track;

pub fn tracks_geojson(tracks: &[Track]) -> Value {
    let features: Vec<Value> = tracks
        .iter()
        .map(|tr| {
            let coords: Vec<Value> = tr
                .points
                .iter()
                .map(|p| {
                    let g = ecef_to_geodetic(p.pos);
                    json!([round(g.lon_deg, 6), round(g.lat_deg, 6), round(g.h_ell_m, 1)])
                })
                .collect();
            let speeds: Vec<f64> = tr.points.iter().map(|p| p.vel.norm()).collect();
            let mean_speed = speeds.iter().sum::<f64>() / speeds.len().max(1) as f64;
            json!({
                "type": "Feature",
                "geometry": {"type": "LineString", "coordinates": coords},
                "properties": {
                    "id": tr.id,
                    "icao24": super::model::format_icao(tr.icao24),
                    "type_code": tr.type_code,
                    "engine": format!("{:?}", tr.engine),
                    "t_start": tr.t_start(),
                    "t_end": tr.t_end(),
                    "n_points": tr.points.len(),
                    "mean_speed_mps": round(mean_speed, 1),
                }
            })
        })
        .collect();
    json!({"type": "FeatureCollection", "features": features})
}

fn round(v: f64, digits: i32) -> f64 {
    let s = 10f64.powi(digits);
    (v * s).round() / s
}
