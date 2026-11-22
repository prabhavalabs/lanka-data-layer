//! Words with page and bounding box: the one shape every document reader in
//! the sandbox produces (03 §3.1). Parsers here are pure Rust and run in the
//! engine process on the tool output, which is untrusted text: they only
//! scan for a handful of tags, never build a DOM, and cap what they accept.
//!
//! Coordinates are PDF points with a top-left origin (`pdftotext
//! -bbox-layout` convention); OCR pixel boxes are converted with the render
//! resolution so both readers share one coordinate system.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// Most words a single call may return (a dense A4 page has ~2,000).
const MAX_WORDS: usize = 2_000_000;

/// One printed token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// 1-based page number.
    pub page: u32,
    pub text: String,
    /// `[x0, y0, x1, y1]` in points, origin top-left.
    pub bbox: [f32; 4],
    /// OCR confidence 0..1 (`None` for a text layer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conf: Option<f32>,
}

impl Word {
    #[must_use]
    pub fn height(&self) -> f32 {
        self.bbox[3] - self.bbox[1]
    }

    #[must_use]
    pub fn cx(&self) -> f32 {
        (self.bbox[0] + self.bbox[2]) / 2.0
    }

    #[must_use]
    pub fn cy(&self) -> f32 {
        (self.bbox[1] + self.bbox[3]) / 2.0
    }
}

/// Page geometry in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PageBox {
    pub page: u32,
    pub width: f32,
    pub height: f32,
}

/// Everything read from a document (or a page range of it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Words {
    pub pages: Vec<PageBox>,
    pub words: Vec<Word>,
}

impl Words {
    #[must_use]
    pub fn page(&self, n: u32) -> Option<&PageBox> {
        self.pages.iter().find(|p| p.page == n)
    }

    pub fn on_page(&self, n: u32) -> impl Iterator<Item = &Word> {
        self.words.iter().filter(move |w| w.page == n)
    }
}

/// Decodes the XML entities poppler and tesseract emit.
#[must_use]
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(j) = tail.find(';').filter(|j| *j <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                u32::from_str_radix(&ent[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The value of `name="…"` (or `name='…'`) inside one tag's text.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = tag[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let after = &tag[at + name.len()..];
        if before_ok && let Some(rest) = after.strip_prefix('=') {
            let q = rest.chars().next()?;
            if q == '"' || q == '\'' {
                let body = &rest[1..];
                return body.find(q).map(|e| &body[..e]);
            }
        }
        from = at + name.len();
    }
    None
}

fn num(tag: &str, name: &str) -> Result<f32> {
    let v = attr(tag, name).with_context(|| format!("missing {name} in {tag:.80}"))?;
    let f: f32 = v
        .parse()
        .with_context(|| format!("bad number {v:?} for {name}"))?;
    ensure!(f.is_finite(), "non-finite {name}");
    Ok(f)
}

/// Strips any tags inside a word's text (`<strong>` in hOCR).
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = false;
    for c in s.chars() {
        match c {
            '<' => depth = true,
            '>' => depth = false,
            _ if !depth => out.push(c),
            _ => {}
        }
    }
    out
}

/// Parses `pdftotext -bbox-layout` (or `-bbox`) XHTML. `first_page` is the
/// number of the first page in the output (the `-f` argument).
pub fn parse_bbox_layout(xhtml: &str, first_page: u32) -> Result<Words> {
    let mut out = Words::default();
    let mut page = first_page.saturating_sub(1);
    let mut rest = xhtml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let end = rest.find('>').context("unterminated tag")?;
        let tag = &rest[..end];
        let after = &rest[end + 1..];
        if tag.starts_with("page ") || tag == "page" {
            page += 1;
            out.pages.push(PageBox {
                page,
                width: num(tag, "width")?,
                height: num(tag, "height")?,
            });
        } else if tag.starts_with("word ") {
            ensure!(page >= first_page, "word before the first <page>");
            let close = after.find("</word>").context("unterminated <word>")?;
            let text = decode_entities(&after[..close]);
            let bbox = [
                num(tag, "xMin")?,
                num(tag, "yMin")?,
                num(tag, "xMax")?,
                num(tag, "yMax")?,
            ];
            if !text.trim().is_empty() {
                ensure!(out.words.len() < MAX_WORDS, "more than {MAX_WORDS} words");
                out.words.push(Word {
                    page,
                    text: text.trim().to_owned(),
                    bbox,
                    conf: None,
                });
            }
            rest = &after[close + "</word>".len()..];
            continue;
        }
        rest = after;
    }
    if out.pages.is_empty() && !xhtml.contains("<doc") {
        bail!("not pdftotext -bbox output");
    }
    Ok(out)
}

/// `bbox x0 y0 x1 y1` and `x_wconf N` out of an hOCR `title` attribute.
fn hocr_title(title: &str) -> (Option<[f32; 4]>, Option<f32>) {
    let mut bbox = None;
    let mut conf = None;
    for part in title.split(';') {
        let mut it = part.split_whitespace();
        match it.next() {
            Some("bbox") => {
                let v: Vec<f32> = it.filter_map(|t| t.parse().ok()).collect();
                if v.len() == 4 {
                    bbox = Some([v[0], v[1], v[2], v[3]]);
                }
            }
            Some("x_wconf") => conf = it.next().and_then(|t| t.parse::<f32>().ok()),
            _ => {}
        }
    }
    (bbox, conf)
}

/// Parses tesseract hOCR for one rendered page. Pixel boxes become points
/// (`px × 72 / dpi`); `x_wconf` (0–100) becomes 0–1.
pub fn parse_hocr(hocr: &str, page: u32, dpi: u32) -> Result<Words> {
    ensure!((36..=600).contains(&dpi), "dpi {dpi} out of range");
    #[allow(clippy::cast_precision_loss)]
    let k = 72.0 / dpi as f32;
    let mut out = Words::default();
    let mut rest = hocr;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let end = rest.find('>').context("unterminated tag")?;
        let tag = &rest[..end];
        let after = &rest[end + 1..];
        let class = attr(tag, "class").unwrap_or("");
        if class == "ocr_page" && out.pages.is_empty() {
            let (bbox, _) = hocr_title(attr(tag, "title").unwrap_or(""));
            let b = bbox.context("ocr_page without bbox")?;
            out.pages.push(PageBox {
                page,
                width: (b[2] - b[0]) * k,
                height: (b[3] - b[1]) * k,
            });
        } else if class == "ocrx_word" {
            let close = after.find("</span>").context("unterminated ocrx_word")?;
            let text = decode_entities(&strip_tags(&after[..close]));
            let (bbox, conf) = hocr_title(attr(tag, "title").unwrap_or(""));
            if let Some(b) = bbox
                && !text.trim().is_empty()
            {
                ensure!(out.words.len() < MAX_WORDS, "more than {MAX_WORDS} words");
                out.words.push(Word {
                    page,
                    text: text.trim().to_owned(),
                    bbox: [b[0] * k, b[1] * k, b[2] * k, b[3] * k],
                    conf: conf.map(|c| (c / 100.0).clamp(0.0, 1.0)),
                });
            }
            rest = &after[close..];
            continue;
        }
        rest = after;
    }
    ensure!(!out.pages.is_empty(), "hOCR has no ocr_page");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pdftotext_bbox_layout() {
        let x = r#"<doc>
  <page width="595.5" height="842.25">
    <flow><block><line>
      <word xMin="10" yMin="20" xMax="30" yMax="31">Sri</word>
      <word xMin="32" yMin="20" xMax="60" yMax="31">Lanka&amp;Co</word>
    </line></block></flow>
  </page>
  <page width="100" height="200">
      <word xMin="1" yMin="2" xMax="3" yMax="4">21,919</word>
  </page>
</doc>"#;
        let w = parse_bbox_layout(x, 3).expect("parse");
        assert_eq!(w.pages.len(), 2);
        assert_eq!(w.pages[1].page, 4);
        assert_eq!(w.words[1].text, "Lanka&Co");
        assert_eq!(w.words[2].page, 4);
        assert_eq!(w.words[2].bbox, [1.0, 2.0, 3.0, 4.0]);
        assert!(parse_bbox_layout("garbage", 1).is_err());
    }

    #[test]
    fn parses_hocr_into_points() {
        let h = "<div class='ocr_page' id='page_1' title='image \"p.png\"; bbox 0 0 1500 3000; ppageno 0'>\
                 <span class='ocrx_word' id='word_1_1' title='bbox 150 300 450 330; x_wconf 91'><strong>Table</strong></span>\
                 <span class='ocrx_word' id='word_1_2' title='bbox 500 300 600 330; x_wconf 40'>4.1&#97;</span></div>";
        let w = parse_hocr(h, 2, 150).expect("parse");
        assert_eq!(w.pages[0].width, 720.0);
        assert_eq!(w.words[0].text, "Table");
        assert_eq!(w.words[0].bbox, [72.0, 144.0, 216.0, 158.4]);
        assert_eq!(w.words[1].text, "4.1a");
        assert_eq!(w.words[1].conf, Some(0.4));
        assert_eq!(w.words[1].page, 2);
    }

    #[test]
    fn entities_are_bounded_and_lenient() {
        assert_eq!(
            decode_entities("a &lt; b &#x41; &bogus; &"),
            "a < b A &bogus; &"
        );
    }
}
