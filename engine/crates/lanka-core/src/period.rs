//! Observation periods (docs/architecture-v2/02-standard-model.md §5).
//!
//! A period is `(freq, time_start)`; the text code is rendered and parsed
//! here and nowhere else. Codes:
//!
//! | freq | code | start |
//! |---|---|---|
//! | `A` annual | `YYYY` | Jan 1 |
//! | `F` split/fiscal year | `YYYY/YY` | profile fiscal start month, day 1 |
//! | `H` half-year | `YYYY-Hn` | Jan 1 / Jul 1 |
//! | `S` season | `YYYY-<Season>` | profile season start in the named year |
//! | `Q` quarter | `YYYY-Qn` | first day of the quarter |
//! | `M` month | `YYYY-MM` | day 1 |
//! | `W` ISO week | `YYYY-Www` | Monday |
//! | `D` day | `YYYY-MM-DD` | that day |
//! | `P` point in time | `YYYY-MM-DD`, or `YYYY` for a stock dated by year only | that date (`YYYY` ⇔ Jan 1) |
//!
//! Sub-daily (`T`) timestamps live in `data.obs_hf` and are not periods.
//! Invariant (property-tested): `parse(f, render(p)) == p` for every valid
//! period, and `render(parse(f, s)) == s` for every canonical code `s`.

use std::fmt;

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use crate::profile::{CalendarSpec, ProfileError};

/// SDMX-style frequency code; stored as Postgres `"char"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Freq {
    A,
    F,
    H,
    S,
    Q,
    M,
    W,
    D,
    P,
}

impl Freq {
    pub const ALL: [Self; 9] = [
        Self::A,
        Self::F,
        Self::H,
        Self::S,
        Self::Q,
        Self::M,
        Self::W,
        Self::D,
        Self::P,
    ];

    #[must_use]
    pub const fn as_char(self) -> char {
        match self {
            Self::A => 'A',
            Self::F => 'F',
            Self::H => 'H',
            Self::S => 'S',
            Self::Q => 'Q',
            Self::M => 'M',
            Self::W => 'W',
            Self::D => 'D',
            Self::P => 'P',
        }
    }

    /// Postgres `"char"` value (sqlx maps it to `i8`).
    #[must_use]
    pub const fn as_db(self) -> i8 {
        self.as_char() as i8
    }

    #[must_use]
    pub fn from_db(c: i8) -> Option<Self> {
        u8::try_from(c)
            .ok()
            .and_then(|b| Self::from_char(b as char))
    }

    #[must_use]
    pub fn from_char(c: char) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_char() == c)
    }

    /// Registry `period_type` → frequency.
    #[must_use]
    pub fn from_period_type(t: &str) -> Option<Self> {
        Some(match t {
            "year" => Self::A,
            "fiscal_year" => Self::F,
            "half_year" => Self::H,
            "season" => Self::S,
            "quarter" => Self::Q,
            "month" => Self::M,
            "week" => Self::W,
            "day" => Self::D,
            "snapshot" => Self::P,
            _ => return None,
        })
    }
}

impl fmt::Display for Freq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

/// Calendar parameters from the country profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendars {
    pub fiscal_start_month: u32,
    /// `(id, month, day)`, in profile order.
    pub seasons: Vec<(String, u32, u32)>,
}

impl Default for Calendars {
    /// Calendar-year fiscal years and no seasons.
    fn default() -> Self {
        Self {
            fiscal_start_month: 1,
            seasons: Vec::new(),
        }
    }
}

impl Calendars {
    pub fn from_spec(c: &CalendarSpec) -> Result<Self, ProfileError> {
        if !(1..=12).contains(&c.fiscal.start_month) || !(1..=12).contains(&c.agri.start_month) {
            return Err(ProfileError::Invalid(
                "calendar start months must be 1..12".into(),
            ));
        }
        let mut seasons = Vec::new();
        for s in &c.seasons {
            let bad = || ProfileError::Invalid(format!("season {:?}: start must be MM-DD", s.id));
            let (m, d) = s.start.split_once('-').ok_or_else(bad)?;
            let (m, d): (u32, u32) = (m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?);
            // Validate against a leap year so 02-29 is rejected only when impossible.
            NaiveDate::from_ymd_opt(2001, m, d).ok_or_else(bad)?;
            if s.id.is_empty() || !s.id.chars().all(char::is_alphabetic) {
                return Err(ProfileError::Invalid(format!(
                    "season id {:?} must be letters only",
                    s.id
                )));
            }
            seasons.push((s.id.clone(), m, d));
        }
        Ok(Self {
            fiscal_start_month: c.fiscal.start_month,
            seasons,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {freq} period {text:?}: {why}")]
pub struct PeriodError {
    pub freq: String,
    pub text: String,
    pub why: &'static str,
}

/// A canonical period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Period {
    pub freq: Freq,
    pub start: NaiveDate,
}

fn year_of(s: &str) -> Option<i32> {
    let y = s.get(0..4)?;
    if !y.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i32 = y.parse().ok()?;
    (1000..=9999).contains(&y).then_some(y)
}

fn two_digits(s: &str) -> Option<u32> {
    (s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

impl Period {
    /// Builds a period, checking that `start` is canonical for `freq`.
    pub fn new(freq: Freq, start: NaiveDate, cal: &Calendars) -> Result<Self, PeriodError> {
        let p = Self { freq, start };
        let text = p.render(cal);
        let back = Self::parse(freq, &text, cal)?;
        if back == p {
            Ok(p)
        } else {
            Err(PeriodError {
                freq: freq.to_string(),
                text: start.to_string(),
                why: "date is not the start of a period of this frequency",
            })
        }
    }

    /// Parses `text` as a period of frequency `freq`.
    pub fn parse(freq: Freq, text: &str, cal: &Calendars) -> Result<Self, PeriodError> {
        let t = text.trim();
        let err = |why| PeriodError {
            freq: freq.to_string(),
            text: text.to_owned(),
            why,
        };
        let year = year_of(t).ok_or_else(|| err("expected a 4-digit year 1000-9999"))?;
        let rest = &t[4..];
        let ymd =
            |m: u32, d: u32| NaiveDate::from_ymd_opt(year, m, d).ok_or_else(|| err("no such date"));
        let start = match freq {
            Freq::A => {
                if !rest.is_empty() {
                    return Err(err("expected YYYY"));
                }
                ymd(1, 1)?
            }
            Freq::F => {
                let yy = rest
                    .strip_prefix('/')
                    .and_then(two_digits)
                    .ok_or_else(|| err("expected YYYY/YY"))?;
                if yy != u32::try_from((year + 1) % 100).unwrap_or(u32::MAX) {
                    return Err(err("YY must be the year after YYYY"));
                }
                ymd(cal.fiscal_start_month, 1)?
            }
            Freq::H => match rest {
                "-H1" => ymd(1, 1)?,
                "-H2" => ymd(7, 1)?,
                _ => return Err(err("expected YYYY-H1 or YYYY-H2")),
            },
            Freq::S => {
                let name = rest
                    .strip_prefix('-')
                    .ok_or_else(|| err("expected YYYY-<Season>"))?;
                let (_, m, d) = cal
                    .seasons
                    .iter()
                    .find(|(id, _, _)| id == name)
                    .ok_or_else(|| err("unknown season (see the country profile)"))?;
                ymd(*m, *d)?
            }
            Freq::Q => {
                let q: u32 = rest
                    .strip_prefix("-Q")
                    .filter(|q| q.len() == 1)
                    .and_then(|q| q.parse().ok())
                    .filter(|q| (1..=4).contains(q))
                    .ok_or_else(|| err("expected YYYY-Q1..Q4"))?;
                ymd((q - 1) * 3 + 1, 1)?
            }
            Freq::M => {
                let m = rest
                    .strip_prefix('-')
                    .and_then(two_digits)
                    .ok_or_else(|| err("expected YYYY-MM"))?;
                ymd(m, 1)?
            }
            Freq::W => {
                let w = rest
                    .strip_prefix("-W")
                    .and_then(two_digits)
                    .ok_or_else(|| err("expected YYYY-Www"))?;
                NaiveDate::from_isoywd_opt(year, w, Weekday::Mon)
                    .ok_or_else(|| err("no such ISO week"))?
            }
            Freq::D => Self::parse_ymd(year, rest).ok_or_else(|| err("expected YYYY-MM-DD"))?,
            Freq::P => {
                if rest.is_empty() {
                    ymd(1, 1)?
                } else {
                    Self::parse_ymd(year, rest).ok_or_else(|| err("expected YYYY-MM-DD or YYYY"))?
                }
            }
        };
        Ok(Self { freq, start })
    }

    fn parse_ymd(year: i32, rest: &str) -> Option<NaiveDate> {
        let r = rest.strip_prefix('-')?;
        let (m, d) = r.split_once('-')?;
        NaiveDate::from_ymd_opt(year, two_digits(m)?, two_digits(d)?)
    }

    /// Parses a period whose frequency is implied by its shape (`YYYY` is
    /// annual; `YYYY-MM-DD` is a day). Used for range bounds in queries.
    pub fn infer(text: &str, cal: &Calendars) -> Result<Self, PeriodError> {
        let t = text.trim();
        let freq = match t.len() {
            4 => Freq::A,
            7 if t.as_bytes().get(4) == Some(&b'/') => Freq::F,
            7 if t.get(4..6) == Some("-H") => Freq::H,
            7 if t.get(4..6) == Some("-Q") => Freq::Q,
            7 => Freq::M,
            8 if t.get(4..6) == Some("-W") => Freq::W,
            10 => Freq::D,
            _ => Freq::S,
        };
        Self::parse(freq, t, cal)
    }

    /// The canonical text code.
    #[must_use]
    pub fn render(&self, cal: &Calendars) -> String {
        let s = self.start;
        let y = s.year();
        match self.freq {
            Freq::A => format!("{y:04}"),
            Freq::F => format!("{y:04}/{:02}", (y + 1) % 100),
            Freq::H => format!("{y:04}-H{}", if s.month() < 7 { 1 } else { 2 }),
            Freq::S => {
                let name = cal
                    .seasons
                    .iter()
                    .find(|(_, m, d)| *m == s.month() && *d == s.day())
                    .map_or("?", |(id, _, _)| id.as_str());
                format!("{y:04}-{name}")
            }
            Freq::Q => format!("{y:04}-Q{}", (s.month() - 1) / 3 + 1),
            Freq::M => format!("{y:04}-{:02}", s.month()),
            Freq::W => {
                let w = s.iso_week();
                format!("{:04}-W{:02}", w.year(), w.week())
            }
            Freq::D => s.format("%Y-%m-%d").to_string(),
            Freq::P => {
                if s.month() == 1 && s.day() == 1 {
                    format!("{y:04}")
                } else {
                    s.format("%Y-%m-%d").to_string()
                }
            }
        }
    }
}

/// First day of a period whose frequency is implied by its shape.
pub fn period_start(text: &str) -> Result<NaiveDate, PeriodError> {
    Period::infer(text, &Calendars::default()).map(|p| p.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> Calendars {
        Calendars {
            fiscal_start_month: 4,
            seasons: vec![("Maha".into(), 9, 1), ("Yala".into(), 4, 1)],
        }
    }

    fn d(y: i32, m: u32, dd: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, dd).expect("valid date")
    }

    #[test]
    fn parses_all_shapes() {
        let c = cal();
        let cases = [
            (Freq::A, "2024", d(2024, 1, 1)),
            (Freq::F, "2024/25", d(2024, 4, 1)),
            (Freq::F, "1999/00", d(1999, 4, 1)),
            (Freq::H, "2024-H2", d(2024, 7, 1)),
            (Freq::S, "2024-Maha", d(2024, 9, 1)),
            (Freq::S, "2024-Yala", d(2024, 4, 1)),
            (Freq::Q, "2024-Q3", d(2024, 7, 1)),
            (Freq::M, "2026-08", d(2026, 8, 1)),
            (Freq::W, "2026-W47", d(2026, 11, 16)),
            (Freq::W, "2021-W01", d(2021, 1, 4)),
            (Freq::D, "2026-11-20", d(2026, 11, 20)),
            (Freq::P, "2024-12-19", d(2024, 12, 19)),
            (Freq::P, "2024", d(2024, 1, 1)),
        ];
        for (f, s, start) in cases {
            let p = Period::parse(f, s, &c).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert_eq!(p.start, start, "{s}");
            assert_eq!(p.render(&c), s, "{s} renders back");
        }
    }

    #[test]
    fn rejects_bad() {
        let c = cal();
        for (f, s) in [
            (Freq::A, ""),
            (Freq::A, "24"),
            (Freq::A, "2024-01"),
            (Freq::Q, "2024-Q5"),
            (Freq::Q, "2024-Q10"),
            (Freq::M, "2024-13"),
            (Freq::M, "2024-1"),
            (Freq::D, "2024-02-30"),
            (Freq::F, "2024/26"),
            (Freq::S, "2024-Monsoon"),
            (Freq::W, "2026-W54"),
            (Freq::H, "2024-H3"),
            (Freq::A, "abcd"),
            (Freq::A, "0999"),
        ] {
            assert!(Period::parse(f, s, &c).is_err(), "{f} {s}");
        }
    }

    #[test]
    fn infer_shapes() {
        let c = cal();
        for (s, f) in [
            ("2024", Freq::A),
            ("2024/25", Freq::F),
            ("2024-H1", Freq::H),
            ("2024-Q2", Freq::Q),
            ("2024-02", Freq::M),
            ("2024-W02", Freq::W),
            ("2024-02-29", Freq::D),
            ("2024-Maha", Freq::S),
        ] {
            assert_eq!(Period::infer(s, &c).map(|p| p.freq), Ok(f), "{s}");
        }
        assert_eq!(period_start("2024-Q3"), Ok(d(2024, 7, 1)));
    }

    #[test]
    fn new_rejects_non_canonical_starts() {
        let c = cal();
        assert!(Period::new(Freq::M, d(2024, 2, 1), &c).is_ok());
        assert!(Period::new(Freq::M, d(2024, 2, 2), &c).is_err());
        assert!(Period::new(Freq::W, d(2026, 11, 17), &c).is_err());
        assert!(Period::new(Freq::P, d(2024, 1, 1), &c).is_ok());
    }

    /// Property: every canonical period round-trips through its text code.
    /// Deterministic xorshift sampling over 1000-9999 × every frequency.
    #[test]
    fn round_trip_property() {
        let c = cal();
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut checked = 0;
        for _ in 0..20_000 {
            let freq = Freq::ALL[usize::try_from(next() % 9).unwrap_or(0)];
            let year = 1000 + i32::try_from(next() % 9000).unwrap_or(0);
            let doy = u32::try_from(next() % 366).unwrap_or(0) + 1;
            let Some(day) = NaiveDate::from_yo_opt(year, doy) else {
                continue;
            };
            // Snap the random day to the start of its period.
            let start = match freq {
                Freq::A => d(year, 1, 1),
                Freq::F => d(year, c.fiscal_start_month, 1),
                Freq::H => d(year, if day.month() < 7 { 1 } else { 7 }, 1),
                Freq::S => {
                    let (_, m, dd) = &c.seasons[usize::try_from(next() % 2).unwrap_or(0)];
                    d(year, *m, *dd)
                }
                Freq::Q => d(year, (day.month() - 1) / 3 * 3 + 1, 1),
                Freq::M => d(year, day.month(), 1),
                Freq::W => {
                    let w = day.iso_week();
                    if !(1000..=9999).contains(&w.year()) {
                        continue;
                    }
                    NaiveDate::from_isoywd_opt(w.year(), w.week(), Weekday::Mon).expect("iso week")
                }
                Freq::D | Freq::P => day,
            };
            let p = Period { freq, start };
            let text = p.render(&c);
            let back = Period::parse(freq, &text, &c).unwrap_or_else(|e| panic!("{p:?}: {e}"));
            if freq == Freq::P && start.month() == 1 && start.day() == 1 {
                assert_eq!(text.len(), 4, "Jan 1 snapshots render as YYYY");
            }
            assert_eq!(back, p, "{text}");
            assert_eq!(back.render(&c), text);
            checked += 1;
        }
        assert!(checked > 15_000);
    }
}
