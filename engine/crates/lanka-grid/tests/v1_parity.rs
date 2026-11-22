//! WorldPop 2025 on the canonical grid equals v1 `cells` cell for cell.
//!
//! Runs when `LANKA_V1_PARITY_DIR` holds `cells.csv` (`cell_id,pop` exported
//! from the v1 `lanka.sqlite` with `printf('%.17g', pop)`) and
//! `LANKA_WORLDPOP_TIF` points at `lka_pop_2025_UC_1km_R2024B_UA_v1.tif`:
//!
//! ```text
//! sqlite3 -csv lanka.sqlite "select cell_id, printf('%.17g', pop) from cells order by cell_id" > cells.csv
//! ```

use std::path::PathBuf;

use lanka_core::grid::Grid;
use lanka_grid::geotiff::GeoTiff;
use lanka_grid::resample::split_sum;

#[test]
fn worldpop_2025_equals_v1_cells() {
    let (Some(dir), Some(tif)) = (
        std::env::var_os("LANKA_V1_PARITY_DIR"),
        std::env::var_os("LANKA_WORLDPOP_TIF"),
    ) else {
        eprintln!("skipped: set LANKA_V1_PARITY_DIR and LANKA_WORLDPOP_TIF");
        return;
    };
    let g = Grid::CANONICAL;
    let mut t = GeoTiff::open(&PathBuf::from(tif)).expect("open tif");
    assert!(!t.pixel_is_point);
    let band = t.read_all().expect("read");
    let (grid, rep) = split_sum(&g, &t.georef, &band, 0, t.nodata);
    let csv = std::fs::read_to_string(PathBuf::from(dir).join("cells.csv")).expect("cells.csv");
    let mut v1 = vec![None::<f64>; lanka_grid::cells(&g)];
    let mut n_v1 = 0usize;
    for line in csv.lines() {
        let (id, pop) = line.split_once(',').expect("csv");
        v1[id.parse::<usize>().expect("id")] = Some(pop.parse::<f64>().expect("pop"));
        n_v1 += 1;
    }
    let mut n_v2 = 0usize;
    let (mut exact, mut f32_equal, mut missing, mut extra) = (0usize, 0usize, 0usize, 0usize);
    let mut max_rel = 0.0f64;
    for (i, &v) in grid.iter().enumerate() {
        let positive = v.is_finite() && v > 0.0;
        n_v2 += usize::from(positive);
        match (v1[i], positive) {
            (Some(a), true) => {
                #[allow(clippy::cast_possible_truncation)]
                if (a as f32) == v {
                    f32_equal += 1;
                }
                if a == f64::from(v) {
                    exact += 1;
                }
                max_rel = max_rel.max((a - f64::from(v)).abs() / a);
            }
            (Some(_), false) => missing += 1,
            (None, true) => extra += 1,
            (None, false) => {}
        }
    }
    eprintln!(
        "v1 cells {n_v1}, v2 positive cells {n_v2}, f32-equal {f32_equal}, f64-exact {exact}, \
         missing {missing}, extra {extra}, max rel diff {max_rel:e}; {rep:?}"
    );
    assert_eq!(n_v1, 5_468_498);
    assert_eq!(n_v2, n_v1);
    assert_eq!(missing, 0);
    assert_eq!(extra, 0);
    assert_eq!(
        f32_equal, n_v1,
        "every cell equals v1 at f32 storage precision"
    );
}
