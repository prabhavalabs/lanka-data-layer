//! Lanka Data Layer distributions (L4). WP-04 ships vector tilesets
//! (`tiles`: Postgres → tippecanoe → PMTiles in the store) and the PMTiles
//! reader the server uses for `/v2/tiles/{tileset}/{z}/{x}/{y}.mvt`; dataset
//! and release distributions (Parquet, CSV, GeoParquet) come with WP-09.

pub mod dem;
/// Evidence sheets live in `lanka_ingest::evidence` (the worker builds them too).
pub use lanka_ingest::evidence;
pub mod pmtiles;
pub mod tiles;
pub mod tippecanoe;
