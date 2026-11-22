# Method: `zonal@1.0.0` (analysis, tier 1)

Any grid or feature layer summarised to admin units — the alignment step exposed as a result. Grids use
`grid_zonal@1` (`lanka-enrich`, over `lanka_grid::zonal`): `sum` for count layers (every assigned cell),
area-weighted `mean` for intensive layers (cells whose centre is inside a unit), `max`; features use
`features_zonal@1` (`count`, optional `kinds`). The result's `alignment` records the method, parameters and
coverage; the `zonal` frame has one value per unit of the support. Totals are reported for extensive layers.
Guards: `ESTIMATED_INPUT`, `LOW_COVERAGE`, `SYNTHETIC_INPUT`.
