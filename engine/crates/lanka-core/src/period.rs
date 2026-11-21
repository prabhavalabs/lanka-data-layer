//! Observation periods: `YYYY`, `YYYY-Qn`, `YYYY-MM`, `YYYY-MM-DD`.
//! `period_start` is the sortable first day of the period.

use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid period {0:?}: expected YYYY, YYYY-Qn, YYYY-MM or YYYY-MM-DD")]
pub struct PeriodError(pub String);

/// First day of a period string.
pub fn period_start(period: &str) -> Result<NaiveDate, PeriodError> {
    let err = || PeriodError(period.to_owned());
    let p = period.trim();
    let year: i32 = p.get(0..4).and_then(|y| y.parse().ok()).ok_or_else(err)?;
    if !(1000..=9999).contains(&year) {
        return Err(err());
    }
    let rest = &p[4..];
    let (month, day) = if rest.is_empty() {
        (1, 1)
    } else if let Some(q) = rest.strip_prefix("-Q") {
        let q: u32 = q.parse().map_err(|_| err())?;
        if !(1..=4).contains(&q) {
            return Err(err());
        }
        ((q - 1) * 3 + 1, 1)
    } else if rest.len() == 3 && rest.starts_with('-') {
        (rest[1..].parse().map_err(|_| err())?, 1)
    } else if rest.len() == 6 && rest.starts_with('-') && rest.as_bytes()[3] == b'-' {
        (
            rest[1..3].parse().map_err(|_| err())?,
            rest[4..].parse().map_err(|_| err())?,
        )
    } else {
        return Err(err());
    };
    NaiveDate::from_ymd_opt(year, month, day).ok_or_else(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, dd: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, dd).expect("valid date")
    }

    #[test]
    fn parses_all_shapes() {
        assert_eq!(period_start("2024"), Ok(d(2024, 1, 1)));
        assert_eq!(period_start("2024-Q3"), Ok(d(2024, 7, 1)));
        assert_eq!(period_start("2026-08"), Ok(d(2026, 8, 1)));
        assert_eq!(period_start("2026-11-20"), Ok(d(2026, 11, 20)));
    }

    #[test]
    fn rejects_bad() {
        for p in [
            "",
            "24",
            "2024-Q5",
            "2024-13",
            "2024-02-30",
            "2024/25",
            "abcd",
        ] {
            assert!(period_start(p).is_err(), "{p}");
        }
    }
}
