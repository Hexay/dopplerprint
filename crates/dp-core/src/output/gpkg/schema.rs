//! GeoPackage 1.4 core tables, SRS rows, R*Tree index SQL and the QGIS `layer_styles` table.

pub const WGS84: i32 = 4326;
pub const ALBERS: i32 = 5070;

const WGS84_WKT: &str = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]]"#;
const ALBERS_WKT: &str = r#"PROJCS["NAD83 / Conus Albers",GEOGCS["NAD83",DATUM["North_American_Datum_1983",SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],AUTHORITY["EPSG","6269"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4269"]],PROJECTION["Albers_Conic_Equal_Area"],PARAMETER["latitude_of_center",23],PARAMETER["longitude_of_center",-96],PARAMETER["standard_parallel_1",29.5],PARAMETER["standard_parallel_2",45.5],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","5070"]]"#;

/// (name, srs_id, wkt, description) inserted into `gpkg_spatial_ref_sys`.
pub const SRS: [(&str, i32, &str, &str); 2] = [
    ("WGS 84", WGS84, WGS84_WKT, "WGS 84 geographic"),
    ("NAD83 / Conus Albers", ALBERS, ALBERS_WKT, "CONUS Albers equal-area; the doppler_print lattice"),
];

pub const CORE: &str = r#"
PRAGMA application_id = 1196444487;
PRAGMA user_version = 10400;
CREATE TABLE gpkg_spatial_ref_sys (
  srs_name TEXT NOT NULL, srs_id INTEGER PRIMARY KEY, organization TEXT NOT NULL,
  organization_coordsys_id INTEGER NOT NULL, definition TEXT NOT NULL, description TEXT);
INSERT INTO gpkg_spatial_ref_sys VALUES
  ('Undefined cartesian SRS', -1, 'NONE', -1, 'undefined', 'undefined cartesian coordinate reference system'),
  ('Undefined geographic SRS', 0, 'NONE', 0, 'undefined', 'undefined geographic coordinate reference system');
CREATE TABLE gpkg_contents (
  table_name TEXT NOT NULL PRIMARY KEY, data_type TEXT NOT NULL, identifier TEXT UNIQUE, description TEXT DEFAULT '',
  last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE,
  srs_id INTEGER, CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id));
CREATE TABLE gpkg_geometry_columns (
  table_name TEXT NOT NULL, column_name TEXT NOT NULL, geometry_type_name TEXT NOT NULL,
  srs_id INTEGER NOT NULL, z TINYINT NOT NULL, m TINYINT NOT NULL,
  CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),
  CONSTRAINT fk_gc_tn FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name),
  CONSTRAINT fk_gc_srs FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id));
CREATE TABLE gpkg_extensions (
  table_name TEXT, column_name TEXT, extension_name TEXT NOT NULL, definition TEXT NOT NULL, scope TEXT NOT NULL,
  CONSTRAINT ge_tce UNIQUE (table_name, column_name, extension_name));
CREATE TABLE layer_styles (
  id INTEGER PRIMARY KEY AUTOINCREMENT, f_table_catalog TEXT(256), f_table_schema TEXT(256),
  f_table_name TEXT(256), f_geometry_column TEXT(256), styleName TEXT(30), styleQML TEXT, styleSLD TEXT,
  useAsDefault BOOLEAN, description TEXT, owner TEXT(30), ui TEXT(30),
  update_time DATETIME DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')));
INSERT INTO gpkg_contents (table_name, data_type, identifier, description)
  VALUES ('layer_styles', 'attributes', 'layer_styles', 'QGIS default layer styles');
"#;

/// R*Tree table and the GeoPackage 1.4 maintenance triggers for `<t>.geom` keyed by `fid`. The triggers call
/// ST_* functions that GDAL/QGIS register; they are created after our own bulk insert so it never fires them.
pub fn rtree_sql(t: &str) -> String {
    let r = format!("rtree_{t}_geom");
    let vals = "NEW.fid, ST_MinX(NEW.geom), ST_MaxX(NEW.geom), ST_MinY(NEW.geom), ST_MaxY(NEW.geom)";
    format!(
        r#"
CREATE TRIGGER "{r}_insert" AFTER INSERT ON "{t}" WHEN (new.geom NOT NULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN INSERT OR REPLACE INTO "{r}" VALUES ({vals}); END;
CREATE TRIGGER "{r}_update1" AFTER UPDATE OF geom ON "{t}"
WHEN OLD.fid = NEW.fid AND (NEW.geom NOTNULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN INSERT OR REPLACE INTO "{r}" VALUES ({vals}); END;
CREATE TRIGGER "{r}_update2" AFTER UPDATE OF geom ON "{t}"
WHEN OLD.fid = NEW.fid AND (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN DELETE FROM "{r}" WHERE id = OLD.fid; END;
CREATE TRIGGER "{r}_update3" AFTER UPDATE ON "{t}"
WHEN OLD.fid != NEW.fid AND (NEW.geom NOTNULL AND NOT ST_IsEmpty(NEW.geom))
BEGIN DELETE FROM "{r}" WHERE id = OLD.fid; INSERT OR REPLACE INTO "{r}" VALUES ({vals}); END;
CREATE TRIGGER "{r}_update4" AFTER UPDATE ON "{t}"
WHEN OLD.fid != NEW.fid AND (NEW.geom ISNULL OR ST_IsEmpty(NEW.geom))
BEGIN DELETE FROM "{r}" WHERE id IN (OLD.fid, NEW.fid); END;
CREATE TRIGGER "{r}_delete" AFTER DELETE ON "{t}" WHEN old.geom NOT NULL
BEGIN DELETE FROM "{r}" WHERE id = OLD.fid; END;
INSERT INTO gpkg_extensions VALUES ('{t}', 'geom', 'gpkg_rtree_index', 'http://www.geopackage.org/spec120/#extension_rtree', 'write-only');
"#
    )
}
