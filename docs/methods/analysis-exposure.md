# Method: `exposure@1.0.0` (analysis, tier 1)

Who or what is inside a hazard mask, per unit: the kernel `exposure_overlay` (`lanka-grid/src/exposure.rs`)
makes one pass over the canonical grid's unit index (`gnd_idx`), a hazard layer (`variables.x`, a grid) and an
exposed layer (`variables.weight`, a grid: population, buildings). A cell is in the mask when its hazard value is
≥ `threshold`; nodata hazard is outside. Per unit (and rolled up exactly to every ancestor) it returns the exposed
total, the exposed quantity inside the mask, the land area and the masked area — shares are ratios of these
sums, never averages of shares. All overlays run on the canonical 0.001° grid (11 §3.2 rule 4); the error is
bounded by the cell size (~111 m).

- **Parameters**: `threshold` (required), `return_period` (recorded; the flood layers of WP-27 carry it as a dim).
- **Estimates**: total, exposed, share, top 10 units. **Output**: `exposure` frame (exposed, total, share,
  area_share).
- **Guards**: `ESTIMATED_INPUT` (e.g. WorldPop), `SYNTHETIC_INPUT`. No relationship guards.
- **Validation**: unit test of the mask, roll-up and shares; zonal totals inherit `grid_zonal`'s checks.
- **Limits**: intervals from hazard class bounds and Monte Carlo over return periods are WP-17's recipe method;
  the grid layers are read from the store per request (~0.6 s for two island layers).
