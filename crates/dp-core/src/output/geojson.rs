//! RFC 7946 GeoJSON (WGS84 lon/lat) of the result layers; each feature carries `properties.kind`.

use serde_json::{Value, json};

use super::features::{Results, normalise};
use super::gpkg::layers::cell_ring;
use crate::geo::albers::{self, Xy};

fn feature(kind: &str, geometry: Value, mut props: Value) -> Value {
    props["kind"] = json!(kind);
    json!({ "type": "Feature", "geometry": geometry, "properties": props })
}

pub fn feature_collection(r: &Results) -> Value {
    let mut out = Vec::new();
    for (c, sn) in r.cells.iter().zip(normalise(r.cells.iter().map(|c| c.score))) {
        let ring: Vec<[f64; 2]> = cell_ring(&c.cell)
            .into_iter()
            .map(|[x, y]| {
                let (lat, lon) = albers::inverse(Xy { x, y });
                [lon, lat]
            })
            .collect();
        let props = json!({ "ix": c.cell.ix, "iy": c.cell.iy, "step_m": c.cell.step_m, "level": c.level,
            "score": c.score, "score_norm": sn, "event_scores": c.event_scores, "in_cr90": c.in_cr90 });
        out.push(feature("cell", json!({ "type": "Polygon", "coordinates": [ring] }), props));
    }
    for (c, sn) in r.candidates.iter().zip(normalise(r.candidates.iter().map(|c| c.score))) {
        let mut props = serde_json::to_value(c).expect("serialisable");
        props["score_norm"] = json!(sn);
        out.push(feature("candidate", json!({ "type": "Point", "coordinates": [c.lon_deg, c.lat_deg] }), props));
    }
    for t in &r.tracks {
        let coords: Vec<[f64; 3]> = t.points.iter().map(|p| [p[1], p[0], p[2]]).collect();
        let props = json!({ "id": t.id, "icao24": t.icao24, "type_code": t.type_code, "engine": t.engine,
            "t_start": t.t_start, "t_end": t.t_end });
        out.push(feature("track", json!({ "type": "LineString", "coordinates": coords }), props));
    }
    for e in &r.events {
        out.push(feature("event", Value::Null, serde_json::to_value(e).expect("serialisable")));
    }
    for g in &r.regions {
        let ring: Vec<[f64; 2]> = g.ring.iter().map(|p| [p[1], p[0]]).collect();
        let props = json!({ "rank": g.rank, "region": g.kind.as_str(), "mass": g.mass, "area_km2": g.area_km2 });
        out.push(feature("region", json!({ "type": "Polygon", "coordinates": [ring] }), props));
    }
    json!({ "type": "FeatureCollection", "run": r.run, "features": out })
}

pub fn write(path: &std::path::Path, r: &Results) -> anyhow::Result<()> {
    std::fs::write(path, serde_json::to_vec(&feature_collection(r))?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::sample::demo_results;

    #[test]
    fn kinds_and_cell_corners() {
        let r = demo_results();
        let fc = feature_collection(&r);
        let feats = fc["features"].as_array().unwrap();
        let count = |k: &str| feats.iter().filter(|f| f["properties"]["kind"] == k).count();
        assert_eq!(count("cell"), r.cells.len());
        assert_eq!(count("candidate"), 3);
        assert_eq!(count("track"), 2);
        assert_eq!(count("event"), 2);
        assert_eq!(count("region"), 2);
        let ring = feats[0]["geometry"]["coordinates"][0].as_array().unwrap();
        assert_eq!(ring.len(), 5);
        assert_eq!(ring[0], ring[4]);
        let lon = ring[0][0].as_f64().unwrap();
        assert!((-74.0..-73.0).contains(&lon));
    }
}
