use super::*;
use lanka_extractd::{PageBox, Word};

fn load(json: &str) -> Words {
    let v: serde_json::Value = serde_json::from_str(json).expect("fixture json");
    let pages = v["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .map(|p| PageBox {
            page: u32::try_from(p[0].as_u64().expect("page")).expect("page"),
            width: p[1].as_f64().expect("w") as f32,
            height: p[2].as_f64().expect("h") as f32,
        })
        .collect();
    let words = v["words"]
        .as_array()
        .expect("words")
        .iter()
        .map(|w| Word {
            page: u32::try_from(w[0].as_u64().expect("page")).expect("page"),
            text: w[1].as_str().expect("text").to_owned(),
            bbox: [
                w[2].as_f64().expect("x0") as f32,
                w[3].as_f64().expect("y0") as f32,
                w[4].as_f64().expect("x1") as f32,
                w[5].as_f64().expect("y1") as f32,
            ],
            conf: None,
        })
        .collect();
    Words { pages, words }
}

const F2412: &str = include_str!("../../tests/fixtures/civic/gze-2412-08.p2-5.json");
const F2187: &str = include_str!("../../tests/fixtures/civic/gze-2187-27.p12-15.json");
const F2403: &str = include_str!("../../tests/fixtures/civic/gze-2403-53.p2-3.json");

// ─────────────────────────── a trimmed real fixture ─────────────────────────

fn item(items: &[Item], no: i32) -> &Item {
    items.iter().find(|i| i.item_no == no).expect("item")
}

#[test]
fn real_pages_of_2412_08_parse_into_two_ministers() {
    let s = parse(&load(F2412));
    assert_eq!(s.table_pages, vec![2, 3, 4, 5]);
    assert_eq!(s.portfolios.len(), 2, "{:?}", s.warnings);
    let (d, f) = (&s.portfolios[0], &s.portfolios[1]);

    // heading: number, title without "(Contd.)", first page
    assert_eq!(
        (d.seq, d.title.as_str(), d.first_page),
        (1, "Minister of Defence", 2)
    );
    assert_eq!((f.seq, f.first_page), (2, 5));
    assert_eq!(
        f.title,
        "Minister of Finance, Planning and Economic Development"
    );

    // Column I: 1..31 across the three pages of the heading, nothing lost at the page breaks
    assert_eq!(
        d.functions.iter().map(|i| i.item_no).collect::<Vec<_>>(),
        (1..=31).collect::<Vec<_>>()
    );
    assert_eq!(item(&d.functions, 10).page, 3);
    assert_eq!(item(&d.functions, 24).page, 4);
    // a word split by the line break joins without its hyphen
    assert!(
        item(&d.functions, 29)
            .text
            .ends_with("availability of maritime hydrospatial data"),
        "{}",
        item(&d.functions, 29).text
    );

    // Column II: 1..25, the `fi` ligature printed as two fragments is glued back
    assert_eq!(d.institutions.len(), 25);
    assert_eq!(
        item(&d.institutions, 1).text,
        "Office of the Chief of Defence Staff"
    );
    assert_eq!(
        item(&d.institutions, 8).text,
        "National Authority for the Implementation of Chemical Weapons Convention"
    );
    assert_eq!(
        item(&d.institutions, 25).text,
        "National Building Research Organization"
    );

    // Column III: one item per act, numbered 1..n in reading order
    assert_eq!(d.laws.len(), 26);
    assert_eq!(
        item(&d.laws, 1).text,
        "Chief of Defence Staff Act No. 35 of 2009"
    );
    assert_eq!(item(&d.laws, 7).text, "Firearms Ordinance No. 33 of 1916");
    assert_eq!(
        item(&d.laws, 18).text,
        "Suppression of Unlawful Acts Against the Safety of Maritime Navigation Act No. 42 of 2000"
    );
    assert!(
        item(&d.laws, 26)
            .text
            .starts_with("All other legislations pertaining")
    );
    assert_eq!(
        d.laws.iter().map(|i| i.item_no).collect::<Vec<_>>(),
        (1..=26).collect::<Vec<_>>()
    );

    // the second heading starts clean on its own page
    assert_eq!(item(&f.institutions, 1).text, "General Treasury");
    assert_eq!(item(&f.functions, 1).page, 5);
}

#[test]
fn real_pages_of_2403_53_use_bullets_in_column_iii() {
    let s = parse(&load(F2403));
    assert_eq!(s.portfolios.len(), 1, "{:?}", s.warnings);
    let d = &s.portfolios[0];
    // "1." and the title without a zero: the older numbering style
    assert_eq!((d.seq, d.title.as_str()), (1, "Minister of Defence"));
    // pages 2 and 3 of the gazette: items 1..22 (23 onwards are on page 4)
    assert_eq!(d.functions.len(), 22);
    assert_eq!(d.institutions.len(), 26);
    assert_eq!(
        item(&d.institutions, 26).text,
        "Telecommunication Regulatory Commission of Sri Lanka and Allied Institutions"
    );
    assert_eq!(
        item(&d.laws, 1).text,
        "Chief of Defence Staff Act No. 35 of 2009"
    );
    assert_eq!(item(&d.laws, 7).text, "Firearms Ordinance No. 33 of 1916");
    assert!(
        item(&d.institutions, 9)
            .text
            .starts_with("Sir John Kotelawala Defence University")
    );
}

#[test]
fn the_2020_policy_statement_layout_reads_into_the_same_model() {
    // the three-column parser finds nothing in this layout
    let w = load(F2187);
    assert!(parse(&w).portfolios.is_empty());
    let s = crate::civic::schedule_b::parse(&w);
    assert_eq!(s.portfolios.len(), 1, "{:?}", s.warnings);
    let d = &s.portfolios[0];
    assert_eq!(
        (d.seq, d.title.as_str(), d.first_page),
        (1, "Minister of Defence", 12)
    );
    // the Cabinet minister's Subjects and Functions only
    assert_eq!(d.functions.len(), 4, "{:?}", d.functions);
    assert!(
        item(&d.functions, 1)
            .text
            .starts_with("Providing policy guidance to relevant State Ministry")
    );
    // institutions: the Cabinet minister's 23, then the State Minister's 12, numbered on
    assert_eq!(d.institutions.len(), 35);
    assert_eq!(
        item(&d.institutions, 1).text,
        "Office of the Chief of Defence Staff"
    );
    assert_eq!(
        item(&d.institutions, 23).text,
        "Academy of Financial Studies"
    );
    assert_eq!(
        item(&d.institutions, 24).text,
        "All District Secretariats and Divisional Secretariats"
    );
    assert_eq!(
        item(&d.institutions, 35).text,
        "National Building Research Organization"
    );
    assert_eq!(item(&d.institutions, 24).page, 15);
    // laws: bullets, wrapped lines joined
    assert_eq!(
        item(&d.laws, 1).text,
        "Chief of Defence Staff Act, No. 35 of 2009"
    );
    assert!(
        d.laws
            .iter()
            .any(|l| l.text == "Defence Services Command and Staff College Act, No. 5 of 2008")
    );
    assert!(d.laws.iter().all(|l| !l.text.starts_with('•')));
    let checks = crate::civic::validate::validate(&s, "1.0 Minister of Defence\n", &[], &[]);
    assert!(
        checks
            .iter()
            .find(|c| c.check == "items_sequential")
            .expect("check")
            .ok
    );
}

// ─────────────────────────── synthetic pages ────────────────────────────────

/// Words laid out on a page the way the gazettes do: header centres at
/// 136 / 301 / 466; numbers at 62 / 223 and text at 80 / 241 / 409.
struct Doc {
    words: Vec<Word>,
    pages: Vec<PageBox>,
}

const BAND_X: [(f32, f32); 3] = [(62.0, 80.0), (223.0, 241.0), (409.0, 409.0)];

impl Doc {
    fn new() -> Self {
        Self {
            words: Vec::new(),
            pages: Vec::new(),
        }
    }

    fn page(&mut self, n: u32) {
        self.pages.push(PageBox {
            page: n,
            width: 595.0,
            height: 842.0,
        });
    }

    fn put(&mut self, page: u32, y: f32, x: f32, text: &str) {
        let mut x = x;
        for t in text.split_whitespace() {
            let w = 5.0 * t.chars().count() as f32;
            self.words.push(Word {
                page,
                text: t.to_owned(),
                bbox: [x, y, x + w, y + 9.0],
                conf: None,
            });
            x += w + 3.0;
        }
    }

    /// `1. Text` in a numbered band (number at the edge, text further in).
    fn item(&mut self, page: u32, y: f32, band: usize, no: &str, text: &str) {
        self.put(page, y, BAND_X[band].0, no);
        self.put(page, y, BAND_X[band].1, text);
    }

    fn wrap(&mut self, page: u32, y: f32, band: usize, text: &str) {
        self.put(page, y, BAND_X[band].1, text);
    }

    fn heading(&mut self, page: u32, y: f32, text: &str) {
        self.put(page, y, 62.0, text);
    }

    fn header(&mut self, page: u32, y: f32) {
        self.put(page, y, 117.0, "Column I");
        self.put(page, y, 280.0, "Column II");
        self.put(page, y, 443.0, "Column III");
        self.put(page, y + 18.0, 90.0, "Subjects and Functions");
        self.put(page, y + 18.0, 230.0, "Departments, Statutory Institutions");
        self.put(page, y + 18.0, 400.0, "Laws, Acts and Ordinances to be");
        self.put(page, y + 30.0, 250.0, "and Public Corporations");
        self.put(page, y + 30.0, 440.0, "Implemented");
    }

    fn words(self) -> Words {
        Words {
            pages: self.pages,
            words: self.words,
        }
    }
}

#[test]
fn wrapped_lines_join_and_a_contd_heading_continues_the_minister() {
    let mut d = Doc::new();
    // page 1: heading, header, two items (the first wraps over three lines, with a hyphen)
    d.page(1);
    d.put(1, 74.0, 100.0, "PART I : SEC. (I) - GAZETTE EXTRAORDINARY");
    d.heading(1, 140.0, "01. Minister of Alpha");
    d.header(1, 178.0);
    d.item(1, 226.0, 0, "1.", "Provision of services");
    d.wrap(1, 238.0, 0, "for hydro-");
    d.wrap(1, 250.0, 0, "spatial data");
    d.item(1, 262.0, 0, "2.", "Second function");
    d.item(1, 226.0, 1, "1.", "Department of Alpha");
    d.wrap(1, 238.0, 1, "Services");
    d.put(1, 226.0, 409.0, "\u{f0b7}");
    d.put(1, 226.0, 426.0, "Alpha Act No. 1 of 1990");
    // page 2: the same minister continues; the numbers carry on
    d.page(2);
    d.heading(2, 140.0, "01. Minister of Alpha (Contd.)");
    d.header(2, 178.0);
    d.item(2, 226.0, 0, "3.", "Third function");
    d.wrap(2, 238.0, 0, "over two lines");
    d.item(2, 226.0, 1, "2.", "Authority of Alpha");
    // page 3: the next minister, its title wrapped over two lines
    d.page(3);
    d.heading(3, 140.0, "02. Minister of Beta, Gamma and");
    d.heading(3, 152.0, "Delta Affairs");
    d.header(3, 178.0);
    d.item(3, 226.0, 0, "1.", "Only function");
    let s = parse(&d.words());

    assert_eq!(s.portfolios.len(), 2, "{:?}", s.warnings);
    let a = &s.portfolios[0];
    assert_eq!(
        (a.seq, a.title.as_str(), a.first_page),
        (1, "Minister of Alpha", 1)
    );
    assert_eq!(a.functions.len(), 3);
    assert_eq!(
        a.functions[0].text,
        "Provision of services for hydrospatial data"
    );
    assert_eq!(a.functions[2].text, "Third function over two lines");
    assert_eq!(a.functions[2].page, 2);
    assert_eq!(
        a.institutions
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
        ["Department of Alpha Services", "Authority of Alpha"]
    );
    assert_eq!(a.laws.len(), 1);
    assert_eq!(a.laws[0].text, "Alpha Act No. 1 of 1990");

    let b = &s.portfolios[1];
    assert_eq!(
        (b.seq, b.title.as_str(), b.first_page),
        (2, "Minister of Beta, Gamma and Delta Affairs", 3)
    );
    assert_eq!(b.functions.len(), 1);
}

#[test]
fn a_heading_in_the_middle_of_a_page_needs_no_header_row() {
    // the 2018 style: the next minister starts under the last one, without a header
    let mut d = Doc::new();
    d.page(1);
    d.heading(1, 140.0, "(1) Minister of Alpha");
    d.header(1, 178.0);
    d.item(1, 226.0, 0, "1.", "First");
    d.item(1, 226.0, 1, "1.", "Alpha Board");
    d.heading(1, 400.0, "(2) Minister of Beta and");
    d.heading(1, 412.0, "Gamma");
    d.item(1, 440.0, 0, "1.", "Beta function");
    d.item(1, 440.0, 1, "1.", "Beta Department");
    d.item(1, 452.0, 1, "2.", "Beta Board");
    let s = parse(&d.words());
    assert_eq!(s.portfolios.len(), 2, "{:?}", s.warnings);
    assert_eq!(s.portfolios[0].institutions.len(), 1);
    assert_eq!(s.portfolios[1].title, "Minister of Beta and Gamma");
    assert_eq!(s.portfolios[1].institutions.len(), 2);
    assert_eq!(s.portfolios[1].functions[0].text, "Beta function");
}

#[test]
fn an_item_that_only_looks_like_a_heading_stays_an_item() {
    // `13. Minister of …` inside minister 1 is item 13, not heading 13
    let mut d = Doc::new();
    d.page(1);
    d.heading(1, 140.0, "01. Minister of Alpha");
    d.header(1, 178.0);
    d.item(1, 226.0, 0, "1.", "First");
    d.item(1, 238.0, 0, "2.", "Minister of Beta to be consulted");
    d.item(1, 250.0, 0, "3.", "Third");
    let s = parse(&d.words());
    assert_eq!(s.portfolios.len(), 1, "{:?}", s.warnings);
    assert_eq!(s.portfolios[0].functions.len(), 3);
}

#[test]
fn a_gap_in_the_numbering_is_read_as_printed_and_fails_validation() {
    let mut d = Doc::new();
    d.page(1);
    d.heading(1, 140.0, "01. Minister of Alpha");
    d.header(1, 178.0);
    d.item(1, 226.0, 0, "1.", "First");
    d.item(1, 238.0, 0, "2.", "Second");
    d.item(1, 250.0, 0, "4.", "Fourth, the third was never printed");
    d.item(1, 226.0, 1, "1.", "Alpha Board");
    let s = parse(&d.words());
    let nos: Vec<i32> = s.portfolios[0]
        .functions
        .iter()
        .map(|i| i.item_no)
        .collect();
    assert_eq!(nos, vec![1, 2, 4]);
    let checks = crate::civic::validate::validate(&s, "01. Minister of Alpha\nColumn I", &[], &[]);
    let bad = checks
        .iter()
        .find(|c| c.check == "items_sequential")
        .expect("check");
    assert!(!bad.ok);
    assert!(bad.detail.contains("expected 3, found 4"), "{}", bad.detail);
}

#[test]
fn column_iii_without_bullets_splits_at_the_end_of_an_act_name() {
    let mut d = Doc::new();
    d.page(1);
    d.heading(1, 140.0, "01. Minister of Alpha");
    d.header(1, 178.0);
    d.item(1, 226.0, 0, "1.", "First");
    d.put(1, 226.0, 409.0, "Chief of Defence Staff Act No.");
    d.put(1, 238.0, 409.0, "35 of 2009");
    d.put(1, 250.0, 409.0, "Army Act No. 17 of 1949");
    d.put(1, 262.0, 409.0, "Offensive Weapons Act, No. 18");
    d.put(1, 274.0, 409.0, "of 1966");
    d.put(1, 286.0, 409.0, "Excise Ordinance (Chapter 52)");
    d.put(1, 298.0, 409.0, "Securities and Exchange Commission Act");
    d.put(1, 310.0, 409.0, "No. 19 of 2021");
    let s = parse(&d.words());
    let laws: Vec<&str> = s.portfolios[0]
        .laws
        .iter()
        .map(|i| i.text.as_str())
        .collect();
    assert_eq!(
        laws,
        [
            "Chief of Defence Staff Act No. 35 of 2009",
            "Army Act No. 17 of 1949",
            "Offensive Weapons Act, No. 18 of 1966",
            "Excise Ordinance (Chapter 52)",
            "Securities and Exchange Commission Act No. 19 of 2021",
        ]
    );
}

#[test]
fn a_note_under_a_list_is_not_part_of_its_last_item() {
    let mut d = Doc::new();
    d.page(1);
    d.heading(1, 140.0, "01. Minister of Alpha");
    d.header(1, 178.0);
    d.item(1, 226.0, 0, "1.", "First");
    d.item(1, 226.0, 1, "1.", "Alpha Board");
    d.wrap(1, 238.0, 1, "of Sri Lanka");
    d.item(1, 250.0, 1, "2.", "Beta Ltd");
    // back at the number's edge, under hanging text: a note
    d.put(1, 262.0, 223.0, "Institution being reviewed");
    let s = parse(&d.words());
    let inst: Vec<&str> = s.portfolios[0]
        .institutions
        .iter()
        .map(|i| i.text.as_str())
        .collect();
    assert_eq!(inst, ["Alpha Board of Sri Lanka", "Beta Ltd"]);
    assert!(
        s.warnings
            .iter()
            .any(|w| w.contains("Institution being reviewed")),
        "{:?}",
        s.warnings
    );
}

#[test]
fn a_hyphen_at_a_line_end_is_dropped_before_lower_case_and_kept_before_a_capital() {
    let mut t = String::from("Petroleum Development Authori-");
    append(&mut t, "ty of Sri Lanka");
    assert_eq!(t, "Petroleum Development Authority of Sri Lanka");
    let mut t = String::from("Sir John Kotelawala-");
    append(&mut t, "Defence University");
    assert_eq!(t, "Sir John Kotelawala-Defence University");
    // a dash set apart from its word is not a split word
    let mut t = String::from("Miloda Institute -");
    append(&mut t, "as a Faculty");
    assert_eq!(t, "Miloda Institute - as a Faculty");
}

#[test]
fn number_slips_the_gazettes_print_are_read() {
    // a stray mark, a letter for a digit, a comma before the stop, a missing stop
    assert_eq!(lead_number("`7.", Some(7)), Some((7, String::new())));
    assert_eq!(lead_number("`7.", Some(9)), None);
    assert_eq!(lead_number("3l.", Some(31)), Some((31, String::new())));
    assert_eq!(lead_number("3l.", Some(5)), None);
    assert_eq!(lead_number("23,.", None), Some((23, String::new())));
    assert_eq!(lead_number("15,", Some(15)), Some((15, String::new())));
    assert_eq!(lead_number("15,", Some(2)), None);
    assert_eq!(lead_number("1", Some(1)), Some((1, String::new())));
    assert_eq!(lead_number("1", Some(2)), None);
    assert_eq!(
        lead_number("10.Matters", None),
        Some((10, "Matters".to_owned()))
    );
    assert_eq!(lead_number("1.5", None), None);
}

#[test]
fn act_names_end_where_they_say_they_do() {
    assert!(law_complete("Army Act No. 17 of 1949"));
    assert!(law_complete("Excise Ordinance (Chapter 52)"));
    assert!(law_complete(
        "Sri Lanka Telecommunications Act, No. 25 of 1991 and its Amendments"
    ));
    assert!(!law_complete("Firearms Ordinance No. 33 of"));
    assert!(!law_complete("Chief of Defence Staff Act No."));
}

#[test]
#[ignore = "development aid: LANKA_CIVIC_PDF=/path/to.pdf cargo test -p lanka-ingest dump -- --ignored --nocapture"]
fn dump() {
    let pdf = std::env::var("LANKA_CIVIC_PDF").expect("LANKA_CIVIC_PDF");
    let out = std::process::Command::new("pdftotext")
        .args(["-bbox-layout", &pdf, "-"])
        .output()
        .expect("pdftotext");
    let words = lanka_extractd::words::parse_bbox_layout(&String::from_utf8_lossy(&out.stdout), 1)
        .expect("bbox");
    let mut s = parse(&words);
    if s.portfolios.is_empty() {
        s = crate::civic::schedule_b::parse(&words);
    }
    let full = std::env::var("LANKA_CIVIC_FULL").is_ok();
    for p in &s.portfolios {
        println!(
            "{:>2} {:<60} p{:<3} fn {:>2} inst {:>2} law {:>2}",
            p.seq,
            p.title,
            p.first_page,
            p.functions.len(),
            p.institutions.len(),
            p.laws.len()
        );
        if full {
            for i in &p.institutions {
                println!("      I {:>2}. {}", i.item_no, i.text);
            }
            for i in &p.laws {
                println!("      L {:>2}. {}", i.item_no, i.text);
            }
            for i in &p.functions {
                println!("      F {:>2}. {}", i.item_no, i.text);
            }
        }
    }
    for w in &s.warnings {
        println!("WARN {w}");
    }
    let text = std::fs::read_to_string(std::env::var("LANKA_CIVIC_TEXT").unwrap_or_default())
        .unwrap_or_default();
    for c in crate::civic::validate::validate(&s, &text, &[], &[]) {
        println!(
            "{} {} {}",
            if c.ok { "ok  " } else { "FAIL" },
            c.check,
            c.detail
        );
    }
}
