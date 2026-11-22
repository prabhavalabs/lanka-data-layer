# Method: `group_compare@1.0.0` (analysis, tier 1)

Difference of a measured variable (`y`) between units inside and outside a zone. In WP-29 the zone is a variable
on the same areal support (`variables.group`): a unit is inside when it is at or above `zone.threshold`
(> 0 when the threshold is 0, i.e. "any share"). Point support with distance rings ("inside", "0–250 m", …)
and within-parent comparisons arrive with point layers (WP-27/WP-33). Code:
`lanka-analysis/src/methods/group_compare.rs`.

- **Statistics**: Welch t (difference of means, Welch–Satterthwaite df, 95 % interval), Mann–Whitney U (normal
  approximation with tie and continuity corrections = scipy `method="asymptotic"`), Hodges–Lehmann shift (median
  of all pairwise differences; a seeded sample of 4 M pairs above that), Cohen's d (pooled sd), percentile
  bootstrap intervals (seeded, `bootstrap` resamples, default 999) of each group's median and of the median
  difference.
- **Min n**: 10 per group (block), 30 per group for p-values.
- **Outputs**: `units` frame (group label, y, zone value) with a categorical layer; `groups` chart.
- **Guards**: `NOT_CAUSAL`, `MIN_N`, `SMALL_N`, `MAUP`, `ECOLOGICAL`, `ESTIMATED_INPUT`, `MODEL_ON_MODEL`,
  `PERIOD_MISMATCH`, `LOW_COVERAGE`, `SUPPRESSED_CELLS`, `SYNTHETIC_INPUT`, `COMMUNITY_INPUT`.
- **Validation**: scipy `ttest_ind(equal_var=False)`, `mannwhitneyu`, numpy Hodges–Lehmann, Cohen's d to 1e-8;
  bootstrap bit-exact against the ported RNG stream.
