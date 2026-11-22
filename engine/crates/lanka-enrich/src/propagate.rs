//! Uncertainty, status and flag propagation (04 §5), enforced by the runner
//! after every step, independent of what the method claims.
//!
//! | property | rule |
//! |---|---|
//! | basis | max over contributors and the method's `output_basis` (`reported < computed < estimated < forecast`) |
//! | status | `suppressed` if any contributor is; else `missing`; else `low_reliability`; else `provisional`; else what the method said |
//! | flags | OR of contributors' propagating flags, the method's mandatory flags and the row's own |
//! | lo/hi | an `Interval` method must give bounds for every non-null value |
//!
//! A row whose status requires a null value (`missing`, `suppressed`) has
//! its value and interval cleared: a derived value never reveals what an
//! input withheld.

use anyhow::{Result, bail};
use lanka_core::flags::PROPAGATING;
use lanka_core::{Basis, Flags, Status};

use crate::method::{ContribKind, Inputs, MethodInfo, OutRow, Row, UncertaintyKind};

/// Severity of a status for propagation (higher wins). Statuses that are
/// properties of one series (`revised`, `break`, `nil`) do not propagate.
#[must_use]
pub const fn severity(s: Status) -> u8 {
    match s {
        Status::Suppressed => 5,
        Status::Missing => 4,
        Status::LowReliability => 3,
        Status::Provisional => 2,
        Status::Normal | Status::Revised | Status::Break | Status::Nil => 0,
    }
}

/// The status a derived row inherits from its contributors and its own claim.
#[must_use]
pub fn combine_status(own: Status, inputs: impl IntoIterator<Item = Status>) -> Status {
    let mut worst = match own {
        Status::Revised | Status::Break => Status::Normal,
        s => s,
    };
    for s in inputs {
        if severity(s) > severity(worst) {
            worst = s;
        }
    }
    worst
}

/// Applies the rules to one row given its contributors' `(basis, status, flags)`.
pub fn apply(info: &MethodInfo, row: &mut Row, inputs: &[(Basis, Status, Flags)]) -> Result<()> {
    let mut basis = row.basis.max(info.output_basis);
    let mut flags = row.flags.union(info.mandatory_flags);
    for (b, _, f) in inputs {
        basis = basis.max(*b);
        flags = flags.union(Flags::from_db_lossy(i32::from_ne_bytes(
            (f.bits() & PROPAGATING).to_ne_bytes(),
        )));
    }
    row.basis = basis;
    row.flags = flags;
    row.status = combine_status(row.status, inputs.iter().map(|i| i.1));
    if row.status.requires_null() {
        row.value = None;
        row.lo = None;
        row.hi = None;
    }
    if info.uncertainty == UncertaintyKind::Interval
        && row.value.is_some()
        && (row.lo.is_none() || row.hi.is_none())
    {
        bail!(
            "{} returned {} {} {} without an interval (uncertainty = interval)",
            info.reference(),
            row.indicator,
            row.geo,
            row.start
        );
    }
    Ok(())
}

/// Looks up every contributor of `rows` in `inputs` and applies [`apply`].
pub async fn enforce(info: &MethodInfo, inputs: &dyn Inputs, rows: &mut [OutRow]) -> Result<()> {
    // Inputs cache their tables, so the per-contributor lookups are cheap.
    for out in rows.iter_mut() {
        let mut ins = Vec::with_capacity(out.contrib.len());
        for c in &out.contrib {
            match c.kind {
                ContribKind::Obs => {
                    let t = inputs.obs(&c.port).await?;
                    if let Some(r) = t.get(c.idx as usize) {
                        ins.push((r.basis, r.status, r.flags));
                    } else {
                        bail!("contributor {} of port {} out of range", c.idx, c.port);
                    }
                }
                ContribKind::Feature => {
                    let t = inputs.features(&c.port).await?;
                    if let Some(f) = t.get(c.idx as usize) {
                        ins.push((Basis::Reported, Status::Normal, f.flags));
                    } else {
                        bail!("feature {} of port {} out of range", c.idx, c.port);
                    }
                }
                ContribKind::Unit | ContribKind::Cells => {}
            }
        }
        apply(info, &mut out.row, &ins)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use lanka_core::Freq;
    use proptest::prelude::*;

    use super::*;
    use crate::method::{PortSpec, Version};

    const INFO: MethodInfo = MethodInfo {
        id: "t",
        version: Version::new(1, 0, 0),
        summary: "",
        citation: "",
        inputs: &[] as &[PortSpec],
        params: "{}",
        output_basis: Basis::Computed,
        uncertainty: UncertaintyKind::None,
        mandatory_flags: Flags::PARTIAL_COVERAGE,
        tools: &[],
        partial_recompute: false,
    };

    fn row() -> Row {
        Row::computed(
            "x",
            "ZZ1",
            1,
            Freq::A,
            NaiveDate::from_ymd_opt(2024, 1, 1).expect("date"),
            "",
            Some(5.0),
        )
    }

    #[test]
    fn basis_is_never_more_certain_than_inputs() {
        let mut r = row();
        apply(
            &INFO,
            &mut r,
            &[
                (Basis::Reported, Status::Normal, Flags::EMPTY),
                (Basis::Estimated, Status::Normal, Flags::EMPTY),
            ],
        )
        .expect("apply");
        assert_eq!(r.basis, Basis::Estimated);
        let mut r = row();
        r.basis = Basis::Reported;
        apply(
            &INFO,
            &mut r,
            &[(Basis::Reported, Status::Normal, Flags::EMPTY)],
        )
        .expect("apply");
        assert_eq!(r.basis, Basis::Computed, "the method's floor applies");
    }

    #[test]
    fn suppressed_input_suppresses_and_clears_the_value() {
        let mut r = row();
        apply(
            &INFO,
            &mut r,
            &[
                (Basis::Reported, Status::Provisional, Flags::EMPTY),
                (Basis::Reported, Status::Suppressed, Flags::EMPTY),
                (Basis::Reported, Status::LowReliability, Flags::EMPTY),
            ],
        )
        .expect("apply");
        assert_eq!((r.status, r.value), (Status::Suppressed, None));
        let mut r = row();
        apply(
            &INFO,
            &mut r,
            &[
                (Basis::Reported, Status::Provisional, Flags::EMPTY),
                (Basis::Reported, Status::Revised, Flags::EMPTY),
            ],
        )
        .expect("apply");
        assert_eq!(r.status, Status::Provisional);
        assert_eq!(r.value, Some(5.0));
    }

    #[test]
    fn flags_propagate_including_space_and_disaggregation_bits() {
        let mut r = row();
        apply(
            &INFO,
            &mut r,
            &[
                (Basis::Reported, Status::Normal, Flags::INTERPOLATED_SPACE),
                (
                    Basis::Reported,
                    Status::Normal,
                    Flags::DISAGGREGATED.union(Flags::OCR),
                ),
                (Basis::Reported, Status::Normal, Flags::UNOFFICIAL_GEOM),
            ],
        )
        .expect("apply");
        for f in [
            Flags::INTERPOLATED_SPACE,
            Flags::DISAGGREGATED,
            Flags::OCR,
            Flags::UNOFFICIAL_GEOM,
            Flags::PARTIAL_COVERAGE,
        ] {
            assert!(r.flags.contains(f), "{f:?} missing from {:?}", r.flags);
        }
        assert_eq!(
            r.flags.bits() & (1 << 16),
            1 << 16,
            "bit 16 INTERPOLATED_SPACE"
        );
        assert_eq!(r.flags.bits() & (1 << 17), 1 << 17, "bit 17 DISAGGREGATED");
    }

    #[test]
    fn interval_methods_must_give_bounds() {
        let mut info = INFO;
        info.uncertainty = UncertaintyKind::Interval;
        let mut r = row();
        assert!(apply(&info, &mut r, &[]).is_err());
        r.lo = Some(4.0);
        r.hi = Some(6.0);
        apply(&info, &mut r, &[]).expect("bounded");
    }

    fn basis() -> impl Strategy<Value = Basis> {
        prop::sample::select(Basis::ALL.to_vec())
    }
    fn status() -> impl Strategy<Value = Status> {
        prop::sample::select(Status::ALL.to_vec())
    }

    proptest! {
        #[test]
        fn propagation_is_monotone_and_order_free(
            ins in prop::collection::vec((basis(), status(), 0u32..(1 << 18)), 0..12),
        ) {
            let ins: Vec<(Basis, Status, Flags)> = ins
                .into_iter()
                .map(|(b, s, f)| (b, s, Flags::from_db_lossy(i32::from_ne_bytes(f.to_ne_bytes()))))
                .collect();
            let mut a = row();
            apply(&INFO, &mut a, &ins).expect("apply");
            let mut rev = ins.clone();
            rev.reverse();
            let mut b = row();
            apply(&INFO, &mut b, &rev).expect("apply");
            prop_assert_eq!(&a, &b);
            for (bs, s, f) in &ins {
                prop_assert!(a.basis >= *bs);
                prop_assert!(severity(a.status) >= severity(*s));
                prop_assert!(a.flags.contains(*f), "every registered input flag propagates");
            }
            prop_assert!(a.flags.contains(Flags::PARTIAL_COVERAGE));
            if a.status.requires_null() {
                prop_assert!(a.value.is_none());
            }
        }
    }
}
