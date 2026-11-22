//! Pinned grid test vectors — mirror of `shared/src/grid.test.ts` (and
//! `foundry/test/grid.test.ts`). All implementations must produce identical
//! ids; if one of these changes, every copy and docs/architecture.md §1 must.

use lanka_core::grid::{COLS, MAX_CELL_ID, STEP, cell_bounds, cell_center, cell_id, row_col};

#[test]
fn colombo_fort() {
    // row = floor((10.0 - 6.9344) * 1000) = 3065; col = floor((79.8428 - 79.4) * 1000) = 442
    assert_eq!(cell_id(6.9344, 79.8428), Some(3065 * COLS + 442));
}

#[test]
fn point_pedro_northern_tip() {
    // col is 834, not 835: (80.235 - 79.4) * 1000 == 834.9999… in IEEE 754.
    assert_eq!(cell_id(9.8355, 80.235), Some(164 * COLS + 834));
}

#[test]
fn dondra_head_southern_tip() {
    assert_eq!(cell_id(5.9236, 80.5883), Some(4076 * COLS + 1188));
}

#[test]
fn out_of_coverage_is_none() {
    assert_eq!(cell_id(4.0, 80.0), None); // south of bbox
    assert_eq!(cell_id(7.0, 83.0), None); // east of bbox
    assert_eq!(cell_id(10.0, 80.0), None); // latMax is exclusive
    assert_eq!(cell_id(f64::NAN, 80.0), None);
    assert_eq!(cell_id(7.0, f64::INFINITY), None);
}

#[test]
fn bbox_corners() {
    assert_eq!(cell_id(9.9999, 79.4), Some(0));
    assert_eq!(cell_id(5.8005, 82.0995), Some(MAX_CELL_ID));
    // exact south edge falls outside (half-open bbox)
    assert_eq!(cell_id(5.8, 82.0), None);
}

#[test]
fn center_inverts_cell_id() {
    let id = cell_id(6.9344, 79.8428).expect("in coverage");
    let (lat, lon) = cell_center(id).expect("valid id");
    assert!((lat - 6.9344).abs() <= STEP);
    assert!((lon - 79.8428).abs() <= STEP);
    assert_eq!(cell_id(lat, lon), Some(id));
}

#[test]
fn bounds_contain_center() {
    let id = cell_id(7.2906, 80.6337).expect("Kandy in coverage");
    let [w, s, e, n] = cell_bounds(id).expect("valid id");
    let (lat, lon) = cell_center(id).expect("valid id");
    assert!(lon > w && lon < e && lat > s && lat < n);
}

#[test]
fn invalid_ids_are_none() {
    assert_eq!(cell_bounds(MAX_CELL_ID + 1), None);
    assert_eq!(cell_center(MAX_CELL_ID + 1), None);
    assert_eq!(row_col(MAX_CELL_ID), Some((4199, 2699)));
    assert_eq!(MAX_CELL_ID, 11_339_999);
}

/// Every cell center must map back to its own id (exhaustive over a
/// stride of the grid; the full 11.34M sweep runs in release CI).
#[test]
fn center_roundtrip_sweep() {
    for id in (0..=MAX_CELL_ID).step_by(997) {
        let (lat, lon) = cell_center(id).expect("valid id");
        assert_eq!(cell_id(lat, lon), Some(id), "cell {id}");
    }
}
