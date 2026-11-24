//! What a gazette says about itself (13 §2.1), read from the notification on
//! its first page: baseline or amendment, the date it takes effect, the
//! baseline it amends and who signed. Pure text functions.

use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Baseline,
    /// Subjects and functions for the Ministers just appointed, not a whole
    /// cabinet ("…to be assigned to the Ministers appointed under Article 43
    /// have been determined"): a run of them makes up a structure.
    Supplement,
    Amendment,
}

impl Kind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Supplement => "supplement",
            Self::Amendment => "amendment",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Info {
    /// `None` when the notification says neither "set out in the schedule"
    /// nor "amended".
    pub kind: Option<Kind>,
    pub effective_from: NaiveDate,
    /// `doc_id` of the first gazette the notification names (`gze-2412-08`).
    pub amends: Option<String>,
    pub head_of_state: Option<String>,
    /// The Articles of the Constitution the notification cites (43, 44, 45).
    pub articles: Vec<u32>,
    /// The notification paragraph, whitespace-normalised.
    pub notification: String,
}

impl Info {
    /// A notification under Article 44 (or 43, the numbering before 1978's
    /// later renumbering) of the Constitution: the Cabinet's schedule.
    /// Article 45 (non-cabinet and state ministers) is not.
    #[must_use]
    pub fn is_cabinet_notification(&self) -> bool {
        self.articles.iter().any(|a| matches!(a, 43 | 44))
    }
}

static AMEND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bamend|\bsubstitut|\bvaried\b|\bvary\b|\bomit|\binsert").expect("amend regex")
});
/// "amended by the substitution thereof of the Schedule given below": a whole
/// new schedule, which is a baseline in effect.
static REPLACES_SCHEDULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bsubstitut\w*[^.;]{0,50}?\bthe\s+schedule\s+(?:given\s+)?below")
        .expect("replacement regex")
});
/// "…to be assigned to the Ministers appointed under Article 43…": the
/// ministers just appointed, and nothing about the number of ministers.
static APPOINTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bMinisters\s+appointed\s+under\s+Articles?\b").expect("appointed regex")
});
static DETERMINED_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bdetermined\s+the\s+number\s+of\b").expect("determined regex")
});
static SET_OUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bset[\s-]+out\s+in\s+the\s+schedule|\bin\s+the\s+schedule\s+(given\s+)?below")
        .expect("set-out regex")
});
static ARTICLES: LazyLock<Regex> = LazyLock::new(|| {
    // "Article 44", "Articles 43 (1), 44 (2) and 46 (1) (a)": the numbers up to "of the Constitution"
    Regex::new(r"(?i)\bArticles?\s+((?:\d{2,3}\s*(?:\([a-z0-9]{1,3}\)\s*)*[,&]?\s*(?:and\s+|respectively\s+)?)+)")
        .expect("articles regex")
});
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(\d{2,3})\b").expect("number regex"));
/// `, President.` of the signature (not `, Presidential Secretariat`).
static SIGNATURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r",\s*President\b").expect("signature regex"));
static GAZETTE_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bGaz(?:ette|\.)[^0-9]{0,40}?\bNos?[.:]?\s*(\d{4})\s*/\s*(\d{1,3})")
        .expect("gazette ref regex")
});
static WEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:with\s+effect\s+from|w\.?\s*e\.?\s*f\.?)\s*[:,]?\s*(.{0,40})")
        .expect("wef regex")
});
static MONTH_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\.?\s+(\d{1,2})(?:st|nd|rd|th)?\s*,?\s+(\d{4})")
        .expect("month-first regex")
});
static DAY_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\d{1,2})(?:st|nd|rd|th)?\s+(?:(?:day\s+)?of\s+)?(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\.?\s*,?\s+(\d{4})")
        .expect("day-first regex")
});
static NUMERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(\d{1,2})[./-](\d{1,2})[./-](\d{4})\b").expect("numeric date regex")
});

fn month_no(s: &str) -> Option<u32> {
    let m = s.to_lowercase();
    [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|p| m.starts_with(p))
    .and_then(|i| u32::try_from(i + 1).ok())
}

/// The first date in `s` (`October 17, 2025`, `17th October 2025`,
/// `17.10.2025`), only if it starts the string's first few words.
#[must_use]
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let mut best: Option<(usize, NaiveDate)> = None;
    let mut keep = |at: usize, d: Option<NaiveDate>| {
        if let Some(d) = d
            && best.is_none_or(|(b, _)| at < b)
        {
            best = Some((at, d));
        }
    };
    if let Some(c) = MONTH_FIRST.captures(s) {
        let at = c.get(0).map_or(0, |m| m.start());
        keep(
            at,
            month_no(&c[1])
                .and_then(|m| NaiveDate::from_ymd_opt(c[3].parse().ok()?, m, c[2].parse().ok()?)),
        );
    }
    if let Some(c) = DAY_FIRST.captures(s) {
        let at = c.get(0).map_or(0, |m| m.start());
        keep(
            at,
            month_no(&c[2])
                .and_then(|m| NaiveDate::from_ymd_opt(c[3].parse().ok()?, m, c[1].parse().ok()?)),
        );
    }
    if let Some(c) = NUMERIC.captures(s) {
        let at = c.get(0).map_or(0, |m| m.start());
        keep(
            at,
            NaiveDate::from_ymd_opt(c[3].parse().ok()?, c[2].parse().ok()?, c[1].parse().ok()?),
        );
    }
    best.map(|(_, d)| d)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

static NOTIF_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)republic\s+of\s+sri\s+lanka\s+notifications?\b|government\s+notifications?\b")
        .expect("notification heading regex")
});

/// The notification: the text after the last `… Sri Lanka Notification`
/// heading that precedes the signature, up to the signature.
fn notification_of(flat: &str) -> String {
    let end = SIGNATURE.find(flat).map_or(flat.len(), |m| m.start());
    let head = &flat[..end];
    let start = NOTIF_HEAD.find_iter(head).last().map_or(0, |m| m.end());
    head[start..].trim().to_owned()
}

fn title_case(w: &str) -> String {
    let mut cs = w.chars();
    cs.next().map_or_else(String::new, |f| {
        f.to_uppercase().collect::<String>() + &cs.as_str().to_lowercase()
    })
}

/// Small capitals print as `M AITHRIPALA S IRISENA` or `R anil W ickremesinghe`:
/// a lone capital joins the word after it.
fn glue_small_caps(tokens: &[&str]) -> Vec<String> {
    let letters = |t: &str| t.chars().count() > 1 && t.chars().all(char::is_alphabetic);
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        let lone = t.chars().count() == 1 && t.chars().all(char::is_uppercase);
        match tokens.get(i + 1) {
            Some(n) if lone && letters(n) => {
                out.push(format!("{t}{n}"));
                i += 2;
            }
            _ => {
                out.push(t.to_owned());
                i += 1;
            }
        }
    }
    out
}

/// The name before `, President.` — a run of capitalised words, at most six.
#[must_use]
pub fn head_of_state(flat: &str) -> Option<String> {
    let at = SIGNATURE.find(flat)?.start();
    // a comma set apart by the small-capitals spacing (`Wickremesinghe ,`) is dropped
    let raw: Vec<&str> = flat[..at]
        .split_whitespace()
        .filter(|t| !t.chars().all(|c| matches!(c, ',' | ';')))
        .collect();
    let tail = &raw[raw.len().saturating_sub(10)..];
    let toks = glue_small_caps(tail);
    let mut name: Vec<&str> = Vec::new();
    for t in toks.iter().rev().take(6) {
        let core = t.trim_end_matches([',', ';']);
        let initial = core.ends_with('.') && core.chars().count() <= 3;
        let word = core
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, '’' | '\'' | '.'))
            && core.chars().next().is_some_and(char::is_uppercase)
            && (!core.ends_with('.') || initial);
        if !word {
            break;
        }
        name.push(core);
    }
    name.reverse();
    if name.len() < 2 {
        return None;
    }
    let all_caps = name.iter().all(|w| {
        w.chars()
            .filter(|c| c.is_alphabetic())
            .all(char::is_uppercase)
    });
    let out: Vec<String> = name
        .iter()
        .map(|w| {
            if all_caps && w.chars().filter(|c| c.is_alphabetic()).count() > 2 {
                title_case(w)
            } else {
                (*w).to_owned()
            }
        })
        .collect();
    Some(out.join(" "))
}

/// Reads the first page of an English gazette. `issued_on` is the fallback
/// for every date.
#[must_use]
pub fn analyse(page1: &str, issued_on: NaiveDate) -> Info {
    let flat = squash(page1);
    let notification = notification_of(&flat);
    let articles: Vec<u32> = {
        let mut v: Vec<u32> = ARTICLES
            .captures_iter(&notification)
            .flat_map(|c| {
                NUMBER
                    .captures_iter(&c[1])
                    .filter_map(|n| n[1].parse::<u32>().ok())
                    .collect::<Vec<_>>()
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let kind = if REPLACES_SCHEDULE.is_match(&notification) {
        Some(Kind::Baseline)
    } else if APPOINTED.is_match(&notification) && !DETERMINED_NUMBER.is_match(&notification) {
        Some(Kind::Supplement)
    } else if AMEND.is_match(&notification) {
        Some(Kind::Amendment)
    } else if SET_OUT.is_match(&notification) {
        Some(Kind::Baseline)
    } else {
        None
    };
    let amends = GAZETTE_REF.captures(&notification).and_then(|c| {
        let b: u32 = c[2].parse().ok()?;
        Some(format!("gze-{}-{b:02}", &c[1]))
    });
    let effective_from = match kind {
        Some(Kind::Amendment) => WEF
            .captures(&notification)
            .and_then(|c| parse_date(&c[1]))
            .unwrap_or(issued_on),
        _ => {
            // the date of the notification: the date after the signature
            let after = SIGNATURE.find(&flat).map_or("", |m| &flat[m.start()..]);
            parse_date(after)
                .filter(|d| (*d - issued_on).num_days().abs() <= 45)
                .unwrap_or(issued_on)
        }
    };
    let plausible = (1948..=issued_on.year() + 1).contains(&effective_from.year());
    Info {
        kind,
        effective_from: if plausible { effective_from } else { issued_on },
        amends,
        head_of_state: head_of_state(&flat),
        articles,
        notification,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("date")
    }

    const BASELINE_2412: &str = "The Gazette of the Democratic Socialist Republic of Sri Lanka EXTRAORDINARY No. 2412/08 - MONDAY, NOVEMBER 25, 2024 \
        PART I : SECTION (I) — GENERAL Government Notifications THE CONSTITUTION OF THE DEMOCRATIC SOCIALIST REPUBLIC OF SRI LANKA Notification \
        WHEREAS I have in the exercise of powers vested in me in terms of Paragraph (1) of Article 44 of the Constitution of the Democratic Socialist Republic of Sri Lanka, \
        determined the number of Ministers of the Cabinet of Ministers and the Ministries and the assignment of subjects and functions and Departments, Statutory Institutions and Public Corporations to the said Ministers; \
        It is now hereby notified that the subjects and functions and Departments, Statutory Institutions and Public Corporations in the charge of various Ministers shall be as set out in the schedule given below, from the date of this Notification. \
        ANURA KUMARA DISSANAYAKE, President. Presidential Secretariat, Colombo 01, November 25, 2024.";

    const AMENDMENT_2458: &str = "No. 2458/65 - SATURDAY, OCTOBER 18, 2025 THE CONSTITUTION OF THE DEMOCRATIC SOCIALIST REPUBLIC OF SRI LANKA Notification \
        IT is hereby notified that the notification issued by me in the exercise of powers vested in me under paragraph (1) of Article 44 of the Constitution and published in the Gazette Extraordinary No. 2412/08 of November 25, 2024 \
        is amended with effect from October 17, 2025 in the manner set out in the schedule below. Anura Kumara Dissanayake, President. Presidential Secretariat, Colombo 01, October 17th, 2025.";

    #[test]
    fn baseline_is_read_from_its_notification() {
        let i = analyse(BASELINE_2412, d(2024, 11, 25));
        assert_eq!(i.kind, Some(Kind::Baseline));
        assert_eq!(i.effective_from, d(2024, 11, 25));
        assert_eq!(i.amends, None);
        assert_eq!(i.head_of_state.as_deref(), Some("Anura Kumara Dissanayake"));
        assert_eq!(i.articles, vec![44]);
        assert!(i.is_cabinet_notification());
        assert!(i.notification.starts_with("WHEREAS I have"));
    }

    #[test]
    fn amendment_names_its_baseline_and_date() {
        let i = analyse(AMENDMENT_2458, d(2025, 10, 18));
        assert_eq!(i.kind, Some(Kind::Amendment));
        assert_eq!(i.effective_from, d(2025, 10, 17));
        assert_eq!(i.amends.as_deref(), Some("gze-2412-08"));
        assert_eq!(i.head_of_state.as_deref(), Some("Anura Kumara Dissanayake"));
    }

    #[test]
    fn a_notification_that_substitutes_the_whole_schedule_is_a_baseline() {
        let t = "Notification IT is hereby notified that the Notification issued by me under Paragraph (1) (a) of Article 44 of the Constitution and published in the Gazette Extraordinary No. 1896/28 of January 10, 2015 is hereby amended by the substitution thereof of the Schedule given below. Maithripala Sirisena, President. Presidential Secretariat, Colombo 01, 18th January, 2015.";
        let i = analyse(t, d(2015, 1, 18));
        assert_eq!(i.kind, Some(Kind::Baseline));
        assert_eq!(i.effective_from, d(2015, 1, 18));
        assert_eq!(i.head_of_state.as_deref(), Some("Maithripala Sirisena"));
    }

    #[test]
    fn a_notification_about_the_ministers_just_appointed_is_a_supplement() {
        let t = "Notification IT is hereby notified that whereas the subjects and functions and departments, state corporations and statutory institutions to be assigned to the Ministers appointed under Article 43 of the Constitution of the Democratic Socialist Republic of Sri Lanka have been determined; the subjects, functions, departments shall be as set out in the Schedule given below with effect from the date of this Notification : M AITHRIPALA S IRISENA , President. Presidential Secretariat, Colombo 01, November 05, 2018.";
        let i = analyse(t, d(2018, 11, 5));
        assert_eq!(i.kind, Some(Kind::Supplement));
        assert_eq!(i.effective_from, d(2018, 11, 5));
        assert!(i.is_cabinet_notification());
        // "determined the number of Ministers" stays a baseline
        let b = "Notification WHEREAS, I have, in terms of paragraph (1) of Article 43, determined the number of Ministers of the Cabinet of Ministers and the Ministries and the assignment of subjects to the Ministers appointed under Article 43. It is now hereby notified that the subjects shall be as set out in the Schedule given below. M AITHRIPALA S IRISENA , President.";
        assert_eq!(analyse(b, d(2018, 12, 28)).kind, Some(Kind::Baseline));
    }

    #[test]
    fn gazette_numbers_may_be_written_with_a_colon() {
        let t = "Notification IT is hereby notified that the notification published in the Gazette Extraordinary No: 1933/13 of 21 st September, 2015 and amended from time to time, is further amended with effect from June 07, 2017 in the manner set out in the schedule below: Maithripala Sirisena, President.";
        let i = analyse(t, d(2017, 6, 9));
        assert_eq!(i.amends.as_deref(), Some("gze-1933-13"));
        assert_eq!(i.effective_from, d(2017, 6, 7));
    }

    #[test]
    fn an_amendment_without_a_date_takes_the_issue_date() {
        let t = "Notification It is hereby notified that the notification published in Gazette Extraordinary No. 2289/43 of July 22, 2022 is further amended in the manner set out in the schedule below. Ranil Wickremesinghe, President.";
        let i = analyse(t, d(2023, 10, 23));
        assert_eq!(i.kind, Some(Kind::Amendment));
        assert_eq!(i.effective_from, d(2023, 10, 23));
        assert_eq!(i.amends.as_deref(), Some("gze-2289-43"));
    }

    #[test]
    fn several_articles_are_all_read() {
        let t = "Notification WHEREAS I have, in terms of Articles 43 (1), 44 (2) and 46 (1) (a) and (b) respectively of the Constitution, determined the Ministers. It is hereby notified that the subjects shall be as set-out in the schedule given below. Gotabaya Rajapaksa, President.";
        let i = analyse(t, d(2020, 8, 9));
        assert_eq!(i.articles, vec![43, 44, 46]);
        assert_eq!(i.kind, Some(Kind::Baseline));
        assert!(i.is_cabinet_notification());
    }

    #[test]
    fn article_45_is_not_a_cabinet_notification() {
        let t = "Notification I have, in the exercise of powers vested in me in terms of Article 45(2) of the Constitution, determined the assignment of subjects as set out in the Schedule given below to Hon. Someone, Non Cabinet Minister of Development Projects with effect from August 23, 2024. Ranil Wickremesinghe, President.";
        let i = analyse(t, d(2024, 8, 23));
        assert_eq!(i.articles, vec![45]);
        assert!(!i.is_cabinet_notification());
    }

    #[test]
    fn dates_are_read_in_the_gazette_styles() {
        assert_eq!(
            parse_date("October 17, 2025 in the manner"),
            Some(d(2025, 10, 17))
        );
        assert_eq!(parse_date("17th October 2025"), Some(d(2025, 10, 17)));
        assert_eq!(parse_date("2nd of January, 2019"), Some(d(2019, 1, 2)));
        assert_eq!(parse_date("2nd day of January, 2019"), Some(d(2019, 1, 2)));
        assert_eq!(parse_date("17.10.2025"), Some(d(2025, 10, 17)));
        assert_eq!(parse_date("Sept. 5, 2018"), Some(d(2018, 9, 5)));
        assert_eq!(parse_date("no date here"), None);
    }

    #[test]
    fn head_of_state_needs_a_capitalised_run() {
        assert_eq!(
            head_of_state("from the date of this Notification. Mahinda Rajapaksa, President."),
            Some("Mahinda Rajapaksa".to_owned())
        );
        assert_eq!(
            head_of_state("Notification. MAITHRIPALA SIRISENA, President. Colombo"),
            Some("Maithripala Sirisena".to_owned())
        );
        assert_eq!(head_of_state("see item 4, President. x"), None);
        assert_eq!(
            head_of_state(
                "this Notification. M AITHRIPALA S IRISENA , President. Presidential Secretariat"
            ),
            Some("Maithripala Sirisena".to_owned())
        );
        // a title is not a signature
        assert_eq!(head_of_state("Colombo 01, Presidential Secretariat"), None);
        assert_eq!(
            head_of_state(
                "this Notification. A nura K umara D issanayake , President. Presidential"
            ),
            Some("Anura Kumara Dissanayake".to_owned())
        );
    }
}
