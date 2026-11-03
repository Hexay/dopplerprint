//! Generic GeoPackage table: feature layer (with R*Tree and optional default style) or attribute table.

use anyhow::Result;
use rusqlite::{Connection, params_from_iter, types::Value};

use super::schema::rtree_sql;
use super::wkb::{EMPTY_ENVELOPE, Geom, union};

#[derive(Clone, Copy, Debug)]
pub struct GeomSpec {
    /// GeoPackage geometry type name: POINT, LINESTRING, POLYGON.
    pub type_name: &'static str,
    pub srs_id: i32,
    pub z: bool,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub description: String,
    /// None for an attribute-only table.
    pub geom: Option<GeomSpec>,
    /// (column name, SQL type).
    pub columns: Vec<(String, &'static str)>,
    pub rows: Vec<(Option<Geom>, Vec<Value>)>,
    /// QGIS QML installed as the layer's default style.
    pub style_qml: Option<&'static str>,
}

impl Layer {
    pub fn new(name: &str, description: &str, geom: Option<GeomSpec>, style_qml: Option<&'static str>) -> Layer {
        Layer { name: name.into(), description: description.into(), geom, columns: vec![], rows: vec![], style_qml }
    }

    pub fn column(&mut self, name: impl Into<String>, sql_type: &'static str) -> &mut Self {
        self.columns.push((name.into(), sql_type));
        self
    }
}

pub fn write_layer(db: &Connection, layer: &Layer) -> Result<()> {
    let t = &layer.name;
    let geom_col = layer.geom.map_or(String::new(), |g| format!(", geom {}", g.type_name));
    let col_defs: String = layer.columns.iter().map(|(c, ty)| format!(", \"{c}\" {ty}")).collect();
    db.execute_batch(&format!("CREATE TABLE \"{t}\" (fid INTEGER PRIMARY KEY AUTOINCREMENT{geom_col}{col_defs});"))?;

    let names: Vec<String> =
        layer.geom.map(|_| "geom".to_string()).into_iter().chain(layer.columns.iter().map(|(c, _)| format!("\"{c}\""))).collect();
    let marks = vec!["?"; names.len()].join(", ");
    let mut ins = db.prepare(&format!("INSERT INTO \"{t}\" ({}) VALUES ({marks})", names.join(", ")))?;
    let mut bbox = EMPTY_ENVELOPE;
    let mut envelopes = Vec::with_capacity(layer.rows.len());
    for (geom, vals) in &layer.rows {
        anyhow::ensure!(vals.len() == layer.columns.len(), "{t}: row has {} values for {} columns", vals.len(), layer.columns.len());
        let mut all = Vec::with_capacity(vals.len() + 1);
        if let Some(spec) = layer.geom {
            all.push(geom.as_ref().map_or(Value::Null, |g| Value::Blob(g.to_gpkg(spec.srs_id))));
        }
        all.extend(vals.iter().cloned());
        ins.execute(params_from_iter(all))?;
        if let Some(g) = geom {
            let e = g.envelope();
            bbox = union(bbox, e);
            envelopes.push((db.last_insert_rowid(), e));
        }
    }

    let Some(spec) = layer.geom else {
        db.execute(
            "INSERT INTO gpkg_contents (table_name, data_type, identifier, description) VALUES (?1, 'attributes', ?1, ?2)",
            (t, &layer.description),
        )?;
        return Ok(());
    };
    let b = if envelopes.is_empty() { [0.0; 4] } else { bbox };
    db.execute(
        "INSERT INTO gpkg_contents (table_name, data_type, identifier, description, min_x, min_y, max_x, max_y, srs_id)
         VALUES (?1, 'features', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (t, &layer.description, b[0], b[2], b[1], b[3], spec.srs_id),
    )?;
    db.execute(
        "INSERT INTO gpkg_geometry_columns VALUES (?1, 'geom', ?2, ?3, ?4, 0)",
        (t, spec.type_name, spec.srs_id, spec.z as i32),
    )?;
    db.execute_batch(&format!("CREATE VIRTUAL TABLE \"rtree_{t}_geom\" USING rtree(id, minx, maxx, miny, maxy);"))?;
    let mut rt = db.prepare(&format!("INSERT INTO \"rtree_{t}_geom\" VALUES (?1, ?2, ?3, ?4, ?5)"))?;
    for (fid, e) in envelopes {
        rt.execute((fid, e[0], e[1], e[2], e[3]))?;
    }
    db.execute_batch(&rtree_sql(t))?;
    if let Some(qml) = layer.style_qml {
        db.execute(
            "INSERT INTO layer_styles (f_table_catalog, f_table_schema, f_table_name, f_geometry_column, styleName,
               styleQML, styleSLD, useAsDefault, description, owner, ui)
             VALUES ('', '', ?1, 'geom', ?1, ?2, '', 1, 'doppler_print default', '', NULL)",
            (t, qml),
        )?;
    }
    Ok(())
}
