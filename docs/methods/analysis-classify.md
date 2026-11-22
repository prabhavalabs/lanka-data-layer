# Method: `classify@1.0.0` (analysis, tier 1)

Class breaks for styling, the single implementation behind server legends and the browser (the same
`lanka-stats::classify` compiled to WebAssembly, `lanka-stats-wasm`): `quantile` (type 7), `equal`, `jenks`
(Fisher–Jenks optimal partition by dynamic programming; above 2,000 values on an evenly spaced sample of the
sorted data), `stddev` (mean ± k·sd), `headtail` (Jiang 2013: split at the mean while the head is ≤ 40 %),
`pretty` (base R `pretty()`). Breaks are the k − 1 inner thresholds, strictly ascending (ties collapse classes);
a value's class is the number of breaks ≤ it. With `y` given: bivariate classes, code `class_y · k + class_x`.
Validation: numpy quantiles and an independent Jenks programme (1e-8); R `pretty()` textbook cases;
native ≡ WebAssembly bit for bit on the fixtures.
