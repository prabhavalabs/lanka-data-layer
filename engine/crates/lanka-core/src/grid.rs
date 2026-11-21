//! Canonical Sri Lanka grid — the single spatial convention of the platform.
//!
//! Contract: `docs/architecture.md` §1. The same constants and formulas exist
//! in `shared/src/grid.ts` and `foundry/src/grid.ts`; all copies are pinned
//! by the same test vectors (`shared/src/grid.test.ts`, mirrored in this
//! crate's `tests/grid_vectors.rs`). The formula is pure IEEE-754 double math
//! with no epsilon — every implementation must reproduce the exact same ids.

/// Northern edge (exclusive), degrees.
pub const LAT_MAX: f64 = 10.0;
/// Southern edge (inclusive), degrees.
pub const LAT_MIN: f64 = 5.8;
/// Western edge (inclusive), degrees.
pub const LON_MIN: f64 = 79.4;
/// Eastern edge (exclusive), degrees.
pub const LON_MAX: f64 = 82.1;
/// Cell size in degrees (~111 m).
pub const STEP: f64 = 0.001;
pub const ROWS: u32 = 4200;
pub const COLS: u32 = 2700;
/// Largest valid cell id (`ROWS * COLS - 1` = 11,339,999).
pub const MAX_CELL_ID: u32 = ROWS * COLS - 1;

/// Cell id for a WGS84 coordinate, or `None` when outside coverage.
#[must_use]
pub fn cell_id(lat: f64, lon: f64) -> Option<u32> {
    if !lat.is_finite() || !lon.is_finite() {
        return None;
    }
    if !(LAT_MIN..LAT_MAX).contains(&lat) || !(LON_MIN..LON_MAX).contains(&lon) {
        return None;
    }
    // Identical operation order to the TS implementation: (max - lat) * 1000.
    let row = ((LAT_MAX - lat) * 1000.0).floor();
    let col = ((lon - LON_MIN) * 1000.0).floor();
    // Guard float artifacts at the bbox edges (e.g. (10.0-5.8)*1000 == 4200.0000000000005).
    if row < 0.0 || row >= f64::from(ROWS) || col < 0.0 || col >= f64::from(COLS) {
        return None;
    }
    // Both are integral and in range, so the casts are exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (row, col) = (row as u32, col as u32);
    Some(row * COLS + col)
}

/// Center coordinate `(lat, lon)` of a cell. Inverse of [`cell_id`] up to cell resolution.
#[must_use]
pub fn cell_center(id: u32) -> Option<(f64, f64)> {
    if id > MAX_CELL_ID {
        return None;
    }
    let row = f64::from(id / COLS);
    let col = f64::from(id % COLS);
    Some((LAT_MAX - (row + 0.5) * STEP, LON_MIN + (col + 0.5) * STEP))
}

/// Bounding box `[west, south, east, north]` of a cell.
#[must_use]
pub fn cell_bounds(id: u32) -> Option<[f64; 4]> {
    if id > MAX_CELL_ID {
        return None;
    }
    let row = f64::from(id / COLS);
    let col = f64::from(id % COLS);
    let north = LAT_MAX - row * STEP;
    let west = LON_MIN + col * STEP;
    Some([west, north - STEP, west + STEP, north])
}

/// `(row, col)` of a cell id.
#[must_use]
pub const fn row_col(id: u32) -> Option<(u32, u32)> {
    if id > MAX_CELL_ID {
        None
    } else {
        Some((id / COLS, id % COLS))
    }
}
