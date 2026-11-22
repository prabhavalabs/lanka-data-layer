# Method: `describe@1.0.0` (analysis, tier 1)

Univariate summary of one variable on a support: n, missing, mean, sample sd, type-7 quantiles (p0, p5, p25, p50,
p75, p95, p100; Hyndman & Fan 1996), MAD, adjusted skewness G1 and excess kurtosis G2 (Joanes & Gill 1998), the
count of values ≤ 0 (whether a log transform is possible) and an equal-width histogram (`bins`, default Sturges
clamped to 5–50). Outputs: `values` frame (choropleth) and `histogram` chart. Min n: 1 (block), 30 (warn).
Validation: numpy percentiles, scipy `skew`/`kurtosis(bias=False)` to 1e-8. Moran's I of the variable comes with
contiguity weights (WP-30).
