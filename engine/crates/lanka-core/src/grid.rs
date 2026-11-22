//! The canonical grid — the single spatial convention of the platform.
//!
//! The free functions implement the v1 contract grid (the constants below);
//! [`Grid`] carries the same formula with parameters from the country
//! profile, and the profile loader refuses a profile whose parameters differ
//! from the compiled contract for the deployment's v1 data.
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

/// Grid parameters (from the country profile). Same formula as the free
/// functions, which are [`Grid::CANONICAL`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub lat_max: f64,
    pub lon_min: f64,
    pub step: f64,
    pub rows: u32,
    pub cols: u32,
}

impl Grid {
    /// The v1 contract grid (`docs/architecture.md` §1).
    pub const CANONICAL: Self = Self {
        lat_max: LAT_MAX,
        lon_min: LON_MIN,
        step: STEP,
        rows: ROWS,
        cols: COLS,
    };

    pub fn from_spec(s: &crate::profile::GridSpec) -> Result<Self, crate::profile::ProfileError> {
        let g = Self {
            lat_max: s.lat_max,
            lon_min: s.lon_min,
            step: s.step_deg,
            rows: s.rows,
            cols: s.cols,
        };
        let ok = g.step > 0.0
            && g.step.is_finite()
            && g.rows > 0
            && g.cols > 0
            && u64::from(g.rows) * u64::from(g.cols) <= u64::from(u32::MAX);
        if ok {
            Ok(g)
        } else {
            Err(crate::profile::ProfileError::Invalid(format!(
                "grid parameters out of range: {s:?}"
            )))
        }
    }

    #[must_use]
    pub fn lat_min(&self) -> f64 {
        self.lat_max - f64::from(self.rows) * self.step
    }

    #[must_use]
    pub fn lon_max(&self) -> f64 {
        self.lon_min + f64::from(self.cols) * self.step
    }

    /// Cell id for a WGS84 coordinate. For [`Grid::CANONICAL`] this is
    /// bit-identical to [`cell_id`].
    #[must_use]
    pub fn cell_id(&self, lat: f64, lon: f64) -> Option<u32> {
        if *self == Self::CANONICAL {
            return cell_id(lat, lon);
        }
        if !lat.is_finite() || !lon.is_finite() {
            return None;
        }
        let inv = 1.0 / self.step;
        let row = ((self.lat_max - lat) * inv).floor();
        let col = ((lon - self.lon_min) * inv).floor();
        if row < 0.0 || row >= f64::from(self.rows) || col < 0.0 || col >= f64::from(self.cols) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (row, col) = (row as u32, col as u32);
        Some(row * self.cols + col)
    }
}
