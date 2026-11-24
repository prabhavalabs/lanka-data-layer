//! Institution identity (13 §2.1): the entity key, the kind guessed from the
//! name, and `registry/civic/aliases.yaml`, which maps a renamed or reworded
//! institution to one key.
//!
//! The key is the name lower-cased, with `&` as `and`, `Ltd`, `Ltd.` and
//! `(Pvt)` as `limited`, a leading `the` dropped, punctuation and extra
//! spaces removed, then slugged with hyphens.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;

/// The kinds `civic.entities.kind` allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Department,
    StatutoryBody,
    Corporation,
    Company,
    Fund,
    University,
    Force,
    Other,
}

impl Kind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Department => "department",
            Self::StatutoryBody => "statutory_body",
            Self::Corporation => "corporation",
            Self::Company => "company",
            Self::Fund => "fund",
            Self::University => "university",
            Self::Force => "force",
            Self::Other => "other",
        }
    }
}

/// Zero-width and joiner characters some PDFs carry inside words.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{ad}'
    )
}

/// Joins words the PDF broke with a hyphen (`Ter-minal`, `Non- Governmental`)
/// so they read the same however the line happened to wrap: a hyphen between
/// two letters, with or without a space after it, disappears.
fn join_hyphens(name: &str) -> String {
    let cs: Vec<char> = name.chars().filter(|c| !is_invisible(*c)).collect();
    let mut out = String::with_capacity(name.len());
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if matches!(c, '-' | '\u{2010}' | '\u{2011}')
            && out.chars().next_back().is_some_and(char::is_alphabetic)
        {
            let mut j = i + 1;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
            if cs.get(j).is_some_and(|n| n.is_alphabetic()) {
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Splits into lower-case alphanumeric words; apostrophes vanish (so
/// `Employees’ Trust Fund` and `Employees Trust Fund` agree), hyphens inside
/// a word vanish (see [`join_hyphens`]), everything else separates.
fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in join_hyphens(name).chars() {
        if matches!(ch, '\'' | '’' | '‘' | '`') {
            continue;
        }
        if ch == '&' {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push("and".to_owned());
        } else if ch.is_alphanumeric() {
            cur.extend(ch.to_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Does a wording look broken by a line wrap (`Ter-minal`: a hyphen into a
/// lower-case letter)? Used to prefer the cleaner of two wordings as the name.
#[must_use]
pub fn has_soft_hyphen(name: &str) -> bool {
    let cs: Vec<char> = name.chars().collect();
    cs.windows(3)
        .any(|w| w[0].is_alphabetic() && w[1] == '-' && w[2].is_lowercase())
}

/// The entity key of a name, before aliases.
#[must_use]
pub fn entity_key(name: &str) -> String {
    let mut w = words(name);
    if w.first().is_some_and(|f| f == "the") && w.len() > 1 {
        w.remove(0);
    }
    // Ltd, Ltd. and (Pvt) say "limited"; "(Pvt) Ltd" collapses to one
    let mut out: Vec<String> = Vec::with_capacity(w.len());
    for t in w {
        let t = match t.as_str() {
            "ltd" | "pvt" => "limited".to_owned(),
            _ => t,
        };
        if t == "limited" && out.last().is_some_and(|l| l == "limited") {
            continue;
        }
        out.push(t);
    }
    out.join("-")
}

/// Slug of a portfolio title without the leading `Minister of`.
#[must_use]
pub fn portfolio_key(title: &str) -> String {
    let t = title.trim();
    let lower = t.to_lowercase();
    let cut = [
        "minister of the ",
        "minister of ",
        "minister for the ",
        "minister for ",
        "ministry of the ",
        "ministry of ",
    ]
    .iter()
    .find(|p| lower.starts_with(**p))
    .map_or(0, |p| p.len());
    // `lower` and `t` share byte offsets only when lowercasing keeps lengths;
    // fall back to the word slug of the whole title otherwise
    let rest = if t.is_char_boundary(cut) && t.len() == lower.len() {
        &t[cut..]
    } else {
        t
    };
    words(rest).join("-")
}

/// The kind guessed from the name. The forces are named; otherwise the
/// rightmost keyword decides (`… Commission of Sri Lanka` is a body,
/// `Coast Guard Department of Sri Lanka` a department).
#[must_use]
pub fn guess_kind(name: &str) -> Kind {
    let w = words(name);
    let joined = w.join(" ");
    for f in [
        "sri lanka army",
        "sri lanka navy",
        "sri lanka air force",
        "sri lanka police",
    ] {
        if joined == f || joined.starts_with(&format!("{f} ")) {
            return Kind::Force;
        }
    }
    for t in w.iter().rev() {
        let k = match t.as_str() {
            "department" | "departments" => Kind::Department,
            "authority" | "board" | "commission" | "council" => Kind::StatutoryBody,
            "corporation" => Kind::Corporation,
            "limited" | "ltd" | "plc" => Kind::Company,
            "fund" => Kind::Fund,
            "university" | "universities" => Kind::University,
            _ => continue,
        };
        return k;
    }
    Kind::Other
}

/// A string that is already a key (`department-of-excise`) stays as it is; a
/// name goes through [`entity_key`]. (Keys are not fed back through the key
/// rules: joining hyphens would turn a slug into one long word.)
fn as_key(s: &str) -> String {
    let t = s.trim();
    let slug = !t.is_empty()
        && t.split('-').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
    if slug { t.to_owned() } else { entity_key(t) }
}

/// `registry/civic/aliases.yaml`: reworded name or key → canonical key.
#[derive(Debug, Clone, Default)]
pub struct Aliases {
    map: HashMap<String, String>,
}

impl Aliases {
    /// Parses the file's text: a YAML map of `alias: canonical key`. Both
    /// sides are put through the key rules, so a name works as well as a slug.
    pub fn parse(yaml: &str) -> Result<Self> {
        let raw: Option<HashMap<String, String>> =
            serde_yaml_ng::from_str(yaml).context("aliases.yaml is not a map of strings")?;
        let mut map = HashMap::new();
        for (from, to) in raw.unwrap_or_default() {
            let (f, t) = (as_key(&from), as_key(&to));
            if f.is_empty() || t.is_empty() {
                bail!("aliases.yaml: empty key in {from:?}: {to:?}");
            }
            if f != t {
                map.insert(f, t);
            }
        }
        // one hop only: a target that is itself an alias would be ambiguous
        for (f, t) in &map {
            if map.contains_key(t) {
                bail!(
                    "aliases.yaml: {f} -> {t}, but {t} is itself an alias; point both at the final key"
                );
            }
        }
        Ok(Self { map })
    }

    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::parse(&s).with_context(|| path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// The canonical key of an institution name.
    #[must_use]
    pub fn key_of(&self, name: &str) -> String {
        let k = entity_key(name);
        self.map.get(&k).cloned().unwrap_or(k)
    }

    /// Does the file already join these two keys (either way round)?
    #[must_use]
    pub fn joins(&self, a: &str, b: &str) -> bool {
        self.map.get(a).is_some_and(|t| t == b) || self.map.get(b).is_some_and(|t| t == a)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_the_contract() {
        assert_eq!(
            entity_key("Department of Meteorology"),
            "department-of-meteorology"
        );
        assert_eq!(entity_key("The Sri Lanka Army"), "sri-lanka-army");
        assert_eq!(
            entity_key("Rakna Arakshana Lanka Ltd."),
            "rakna-arakshana-lanka-limited"
        );
        assert_eq!(
            entity_key("Canwill Holdings (Pvt) Ltd"),
            "canwill-holdings-limited"
        );
        assert_eq!(
            entity_key("Techno Park Development Company (Pvt) Limited"),
            "techno-park-development-company-limited"
        );
        assert_eq!(entity_key("Sugar (Pvt.) Ltd."), "sugar-limited");
        assert_eq!(
            entity_key("Water Supply & Drainage Board"),
            "water-supply-and-drainage-board"
        );
        assert_eq!(
            entity_key("Employees’ Trust Fund"),
            entity_key("Employees' Trust Fund")
        );
        assert_eq!(entity_key("Employees’ Trust Fund"), "employees-trust-fund");
        // a hyphen set by a line wrap, or a zero-width joiner, does not change the key
        assert_eq!(
            entity_key("Ceylon Petroleum Storage Ter-minal Ltd."),
            entity_key("Ceylon Petroleum Storage Terminal Ltd.")
        );
        assert_eq!(
            entity_key("Sri Lanka Th\u{200d}riposha Co. Ltd."),
            entity_key("Sri Lanka Thriposha Co. Ltd.")
        );
        assert_eq!(
            entity_key("Secretariat for Non- Governmental Organizations"),
            entity_key("Secretariat for Non-Governmental Organizations")
        );
        assert_eq!(
            entity_key("Sri Lanka - Japan Friendship"),
            "sri-lanka-japan-friendship"
        );
        assert!(has_soft_hyphen("Authori-ty of Sri Lanka"));
        assert!(!has_soft_hyphen("Non-Governmental Organization"));
        assert_eq!(
            entity_key("  Strike,  Riot, Civil Commotion and Terrorism Fund "),
            "strike-riot-civil-commotion-and-terrorism-fund"
        );
        // a lone "The" is kept (nothing would be left)
        assert_eq!(entity_key("The"), "the");
    }

    #[test]
    fn kinds_are_guessed_from_the_name() {
        use Kind::*;
        for (name, kind) in [
            ("Department of Meteorology", Department),
            ("Coast Guard Department of Sri Lanka", Department),
            ("Sri Lanka Ports Authority", StatutoryBody),
            ("National Lotteries Board", StatutoryBody),
            (
                "Telecommunication Regulatory Commission of Sri Lanka and Allied Institutions",
                StatutoryBody,
            ),
            ("National Disaster Management Council", StatutoryBody),
            ("Sri Lanka Insurance Corporation", Corporation),
            ("Rakna Arakshana Lanka Ltd.", Company),
            ("Sri Lanka Telecom PLC", Company),
            ("Api Wenuwen Api Fund", Fund),
            ("Sir John Kotelawala Defence University", University),
            ("Sri Lanka Army", Force),
            ("Sri Lanka Navy", Force),
            ("Sri Lanka Air Force", Force),
            ("Sri Lanka Police", Force),
            ("National Defence College", Other),
        ] {
            assert_eq!(guess_kind(name), kind, "{name}");
        }
    }

    #[test]
    fn portfolio_keys_drop_minister_of() {
        assert_eq!(portfolio_key("Minister of Defence"), "defence");
        assert_eq!(
            portfolio_key("Minister of Finance, Planning and Economic Development"),
            "finance-planning-and-economic-development"
        );
        assert_eq!(portfolio_key("Minister of the Environment"), "environment");
        assert_eq!(
            portfolio_key("Ministry of Finance & Planning"),
            "finance-and-planning"
        );
        assert_eq!(portfolio_key("Prime Minister"), "prime-minister");
    }

    #[test]
    fn aliases_map_reworded_names_to_one_key() {
        let a = Aliases::parse(
            "\"sri lanka telecom regulatory commission\": telecommunications-regulatory-commission-of-sri-lanka\n\
             \"Telecommunication Regulatory Commission of Sri Lanka and Allied Institutions\": telecommunications-regulatory-commission-of-sri-lanka\n",
        )
        .expect("parse");
        assert_eq!(a.len(), 2);
        assert_eq!(
            a.key_of("Sri Lanka Telecom Regulatory Commission"),
            "telecommunications-regulatory-commission-of-sri-lanka"
        );
        assert_eq!(
            a.key_of("Telecommunications Regulatory Commission of Sri Lanka"),
            "telecommunications-regulatory-commission-of-sri-lanka"
        );
        assert_eq!(a.key_of("Department of Excise"), "department-of-excise");
        // empty file, comment-only file
        assert!(Aliases::parse("# nothing\n{}\n").expect("empty").is_empty());
        assert!(Aliases::parse("").expect("empty").is_empty());
        // chains are refused
        assert!(Aliases::parse("a: b\nb: c\n").is_err());
    }
}
