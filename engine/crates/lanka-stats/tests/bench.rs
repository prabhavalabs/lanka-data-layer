//! Kernel timings for 11 §8 (run: `cargo test --release -p lanka-stats --test bench -- --ignored --nocapture`).

use std::time::Instant;

use lanka_stats::compare::{Stat, bootstrap, cohens_d, hodges_lehmann, mann_whitney, welch};
use lanka_stats::rng::Stream;

#[test]
#[ignore = "timing, not a check"]
fn group_compare_kernels_on_50000_points() {
    let mut s = Stream::new(1, 1);
    let a: Vec<f64> = (0..12_000).map(|_| s.unit() * 100.0 - 5.0).collect();
    let b: Vec<f64> = (0..38_000).map(|_| s.unit() * 100.0).collect();
    let t = Instant::now();
    let w = welch(&a, &b, 0.95);
    let m = mann_whitney(&a, &b);
    let (hl, sampled) = hodges_lehmann(&a, &b, 7);
    let d = cohens_d(&a, &b);
    let t1 = t.elapsed();
    let ci = bootstrap(&a, Some(&b), Stat::Median, 999, 0.95, 7, 3);
    let t2 = t.elapsed();
    eprintln!(
        "50,000 points: welch + mann-whitney + hodges-lehmann (sampled {sampled}) + cohen's d {:?}; + 999 bootstrap medians {:?} (diff {:.3}, p {:.3e}, hl {hl:.3}, d {d:.3}, ci {ci:?})",
        t1, t2, w.diff, m.p
    );
}
