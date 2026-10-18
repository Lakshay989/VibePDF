//! SPEC: P7-OCR-009 (P7.C1b1) — read a Word document (`.docx`) into the flow
//! model, for laying out as a PDF.
//!
//! **What is read.** The body's paragraphs in order; each run's font, size,
//! bold, italic, underline, strike-through, colour, capitals and
//! super/subscript; paragraph alignment, indents, spacing and line spacing;
//! styles, through their `basedOn` chains and the document defaults; theme
//! fonts; list numbering and bullets; inline images; page size and margins.
//! It descends into hyperlinks, content controls and fields (showing a field's
//! saved result, not its code), and shows tracked changes as if accepted.
//!
//! **What is simplified, and said.** Tables (until P7.C1b2) become their cells'
//! paragraphs; text boxes follow the paragraph they're anchored in; floating
//! images are placed inline; equations become their text. Headers, footers,
//! footnotes and comments are left out. Each of these adds one warning, so a
//! person can see what changed — nothing is dropped without a word.

use std::collections::{BTreeSet, HashMap};

use crate::error::CommandError;
use crate::pdf::flow::{Align, FlowBlock, FlowDocument, FlowImage, LineSpacing, PageSetup, Paragraph, Run, RunStyle};
use crate::pdf::ooxml::ZipArchive;
use crate::pdf::xml_tree::{parse, Element, A, MC, PKG_REL, R, V, W, WP};

const MATH: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const EMU_PER_POINT: f32 = 12_700.0;

/// Twentieths of a point, the unit of most `WordprocessingML` lengths.
fn twips(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().map(|t| t / 20.0)
}

/// A `WordprocessingML` on/off property: present means on, unless its value says off.
fn on_off(element: &Element) -> bool {
    !matches!(element.attr(W, "val"), Some("0" | "false" | "off" | "none"))
}

// ---------------------------------------------------------------------------
// Properties, layered: document defaults, then styles, then direct formatting.

#[derive(Clone, Debug, Default, PartialEq)]
struct RProps {
    family: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    caps: Option<bool>,
    vanish: Option<bool>,
    size: Option<f32>,
    color: Option<[u8; 3]>,
    /// 1 superscript, -1 subscript, 0 baseline.
    vert: Option<i8>,
}

macro_rules! layer {
    ($base:expr, $over:expr, $($field:ident),*) => {
        $( if $over.$field.is_some() { $base.$field = $over.$field.clone(); } )*
    };
}

impl RProps {
    fn over(&mut self, other: &Self) {
        layer!(self, other, family, bold, italic, underline, strike, caps, vanish, size, color, vert);
    }

    fn run_style(&self) -> RunStyle {
        let defaults = RunStyle::default();
        let size = self.size.unwrap_or(defaults.size);
        let (size, rise) = match self.vert.unwrap_or(0) {
            1 => (size * 0.65, size * 0.33),
            -1 => (size * 0.65, -size * 0.14),
            _ => (size, 0.0),
        };
        RunStyle {
            family: self.family.clone().unwrap_or(defaults.family),
            bold: self.bold.unwrap_or(false),
            italic: self.italic.unwrap_or(false),
            underline: self.underline.unwrap_or(false),
            strike: self.strike.unwrap_or(false),
            size,
            color: self.color.unwrap_or([0, 0, 0]),
            rise,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct PProps {
    align: Option<Align>,
    indent_left: Option<f32>,
    indent_right: Option<f32>,
    first_line: Option<f32>,
    space_before: Option<f32>,
    space_after: Option<f32>,
    line: Option<LineSpacing>,
    numbering: Option<(String, usize)>,
    page_break_before: Option<bool>,
}

impl PProps {
    fn over(&mut self, other: &Self) {
        layer!(self, other, align, indent_left, indent_right, first_line, space_before, space_after, line, numbering, page_break_before);
    }
}

#[derive(Clone, Debug, Default)]
struct Theme {
    minor: Option<String>,
    major: Option<String>,
}

fn parse_rpr(rpr: &Element, theme: &Theme) -> RProps {
    let mut out = RProps::default();
    for e in rpr.elements().filter(|e| e.ns == W) {
        match e.name.as_str() {
            "rFonts" => {
                out.family = e
                    .attr(W, "ascii")
                    .or_else(|| e.attr(W, "hAnsi"))
                    .map(str::to_owned)
                    .or_else(|| match e.attr(W, "asciiTheme").or_else(|| e.attr(W, "hAnsiTheme")) {
                        Some(t) if t.starts_with("major") => theme.major.clone(),
                        Some(_) => theme.minor.clone(),
                        None => None,
                    });
            }
            "b" => out.bold = Some(on_off(e)),
            "i" => out.italic = Some(on_off(e)),
            "u" => out.underline = Some(!matches!(e.attr(W, "val"), Some("none" | "0" | "false"))),
            "strike" | "dstrike" => out.strike = Some(on_off(e)),
            "caps" | "smallCaps" => out.caps = Some(on_off(e)),
            "vanish" => out.vanish = Some(on_off(e)),
            "sz" => out.size = e.attr(W, "val").and_then(|v| v.parse::<f32>().ok()).map(|h| h / 2.0),
            "color" => {
                out.color = Some(e.attr(W, "val").and_then(hex_colour).unwrap_or([0, 0, 0]));
            }
            "vertAlign" => {
                out.vert = Some(match e.attr(W, "val") {
                    Some("superscript") => 1,
                    Some("subscript") => -1,
                    _ => 0,
                });
            }
            _ => {}
        }
    }
    out
}

fn hex_colour(value: &str) -> Option<[u8; 3]> {
    if value.len() != 6 {
        return None; // "auto", which is black on white paper
    }
    let channel = |i: usize| u8::from_str_radix(value.get(i..i + 2)?, 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

fn parse_ppr(ppr: &Element) -> PProps {
    let mut out = PProps::default();
    for e in ppr.elements().filter(|e| e.ns == W) {
        match e.name.as_str() {
            "jc" => {
                out.align = match e.attr(W, "val") {
                    Some("center") => Some(Align::Center),
                    Some("right" | "end") => Some(Align::Right),
                    Some("both" | "distribute" | "justify" | "lowKashida" | "mediumKashida" | "highKashida" | "thaiDistribute") => {
                        Some(Align::Justify)
                    }
                    Some(_) => Some(Align::Left),
                    None => None,
                };
            }
            "ind" => {
                out.indent_left = e.attr(W, "left").or_else(|| e.attr(W, "start")).and_then(twips).or(out.indent_left);
                out.indent_right = e.attr(W, "right").or_else(|| e.attr(W, "end")).and_then(twips).or(out.indent_right);
                // `first-line` is the pre-standard spelling, signed (negative
                // hangs); macOS's own DOCX writer still emits it.
                if let Some(hanging) = e.attr(W, "hanging").and_then(twips) {
                    out.first_line = Some(-hanging);
                } else if let Some(first) = e.attr(W, "firstLine").or_else(|| e.attr(W, "first-line")).and_then(twips) {
                    out.first_line = Some(first);
                }
            }
            "spacing" => {
                out.space_before = e.attr(W, "before").and_then(twips).or(out.space_before);
                out.space_after = e.attr(W, "after").and_then(twips).or(out.space_after);
                if let Some(line) = e.attr(W, "line").and_then(|v| v.parse::<f32>().ok()) {
                    out.line = Some(match e.attr(W, "lineRule") {
                        Some("exact") => LineSpacing::Exactly(line / 20.0),
                        Some("atLeast") => LineSpacing::AtLeast(line / 20.0),
                        _ => LineSpacing::Multiple(line / 240.0),
                    });
                }
            }
            "numPr" => {
                let level = e.child(W, "ilvl").and_then(|l| l.attr(W, "val")).and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(id) = e.child(W, "numId").and_then(|n| n.attr(W, "val")) {
                    out.numbering = Some((id.to_owned(), level));
                }
            }
            "pageBreakBefore" => out.page_break_before = Some(on_off(e)),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Styles and numbering

#[derive(Clone, Debug, Default)]
struct Style {
    based_on: Option<String>,
    ppr: PProps,
    rpr: RProps,
}

#[derive(Clone, Debug)]
struct Level {
    start: u32,
    format: String,
    text: String,
    ppr: PProps,
    rpr: RProps,
}

#[derive(Default)]
struct Numbering {
    abstracts: HashMap<String, Vec<Option<Level>>>,
    /// numId → (abstractNumId, start overrides by level).
    nums: HashMap<String, (String, HashMap<usize, u32>)>,
    /// numId → each level's current count, `None` until used.
    counters: HashMap<String, [Option<u32>; 9]>,
}

impl Numbering {
    fn level(&self, num: &str, level: usize) -> Option<&Level> {
        let (abstract_id, _) = self.nums.get(num)?;
        self.abstracts.get(abstract_id)?.get(level)?.as_ref()
    }

    /// Advance `num` at `level` and return its label text.
    fn next_label(&mut self, num: &str, level: usize) -> Option<String> {
        let (abstract_id, overrides) = self.nums.get(num)?.clone();
        let levels = self.abstracts.get(&abstract_id)?.clone();
        let this = levels.get(level)?.as_ref()?;
        let counters = self.counters.entry(num.to_owned()).or_insert([None; 9]);
        let start = |l: usize| overrides.get(&l).copied().or_else(|| levels.get(l).and_then(Option::as_ref).map(|v| v.start)).unwrap_or(1);
        counters[level] = Some(counters[level].map_or_else(|| start(level), |n| n + 1));
        for deeper in counters.iter_mut().skip(level + 1) {
            *deeper = None;
        }
        if this.format == "bullet" {
            let mark: String = this.text.chars().map(symbol).collect();
            return Some(if mark.trim().is_empty() { "•".into() } else { mark });
        }
        if this.format == "none" {
            return Some(this.text.clone()).filter(|t| !t.contains('%'));
        }
        let mut label = this.text.clone();
        for (l, current) in counters.iter().enumerate() {
            let placeholder = format!("%{}", l + 1);
            if label.contains(&placeholder) {
                let format = levels.get(l).and_then(Option::as_ref).map_or("decimal", |v| v.format.as_str());
                let value = current.unwrap_or_else(|| start(l));
                label = label.replace(&placeholder, &number(value, format));
            }
        }
        Some(label)
    }
}

/// `value` in a list's number format.
fn number(value: u32, format: &str) -> String {
    let letters = |upper: bool| {
        // a..z, then aa..zz, as Word counts.
        let index = (value.max(1) - 1) % 26;
        let repeat = (value.max(1) - 1) / 26 + 1;
        let c = char::from(b'a' + u8::try_from(index).unwrap_or(0));
        let c = if upper { c.to_ascii_uppercase() } else { c };
        std::iter::repeat_n(c, repeat as usize).collect::<String>()
    };
    match format {
        "lowerLetter" => letters(false),
        "upperLetter" => letters(true),
        "lowerRoman" => roman(value).to_lowercase(),
        "upperRoman" => roman(value),
        "decimalZero" => format!("{value:02}"),
        _ => value.to_string(),
    }
}

fn roman(mut value: u32) -> String {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut out = String::new();
    for (n, s) in NUMERALS {
        while value >= n {
            out.push_str(s);
            value -= n;
        }
    }
    out
}

/// A Symbol/Wingdings private-use character, as the Unicode one it draws.
/// Word writes bullets this way; most fonts have no glyph at those codes.
fn symbol(c: char) -> char {
    match u32::from(c) {
        0xF0A7 | 0xF06E => '▪',
        0xF0A8 | 0xF0A1 => '○',
        0xF0D8 => '➢',
        0xF076 => '❖',
        0xF0FC => '✓',
        0xF0FB => '✗',
        0xF0E0 => '→',
        0xF02D | 0xF0BE => '–',
        0xF000..=0xF0FF => '•',
        _ => c,
    }
}

// ---------------------------------------------------------------------------

struct Reader<'z> {
    zip: &'z ZipArchive<'z>,
    name: String,
    styles: HashMap<String, Style>,
    default_paragraph: Option<String>,
    doc_ppr: PProps,
    doc_rpr: RProps,
    theme: Theme,
    numbering: Numbering,
    /// Relationship id → package path, for the main document part.
    rels: HashMap<String, (String, bool)>,
    blocks: Vec<FlowBlock>,
    warnings: BTreeSet<&'static str>,
    notes: Vec<String>,
    /// Text-box paragraphs waiting until their anchoring paragraph ends.
    pending: Vec<Element>,
    images: usize,
}

/// A paragraph in progress: its settled properties and the runs so far.
struct Building {
    template: Paragraph,
    runs: Vec<Run>,
    /// Whether any part of this paragraph has been emitted yet.
    emitted: bool,
}

/// Where a part named in a relationship lives in the package.
fn resolve_target(base_dir: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_owned();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

impl Reader<'_> {
    fn warn(&mut self, message: &'static str) {
        self.warnings.insert(message);
    }

    fn xml(&self, path: &str) -> Result<Option<Element>, CommandError> {
        if !self.zip.contains(path) {
            return Ok(None);
        }
        let bytes = self.zip.read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        parse(text.trim_start_matches('\u{feff}')).map(Some)
    }

    fn style_chain(&self, id: &str) -> (PProps, RProps) {
        let mut chain = Vec::new();
        let mut at = Some(id.to_owned());
        while let Some(current) = at {
            if chain.len() > 32 || chain.contains(&current) {
                break; // a cycle in basedOn
            }
            let next = self.styles.get(&current).and_then(|s| s.based_on.clone());
            chain.push(current);
            at = next;
        }
        let (mut ppr, mut rpr) = (PProps::default(), RProps::default());
        for id in chain.iter().rev() {
            if let Some(style) = self.styles.get(id) {
                ppr.over(&style.ppr);
                rpr.over(&style.rpr);
            }
        }
        (ppr, rpr)
    }

    fn load_styles(&mut self, path: &str) -> Result<(), CommandError> {
        let Some(root) = self.xml(path)? else { return Ok(()) };
        if let Some(defaults) = root.child(W, "docDefaults") {
            if let Some(rpr) = defaults.child(W, "rPrDefault").and_then(|d| d.child(W, "rPr")) {
                self.doc_rpr = parse_rpr(rpr, &self.theme);
            }
            if let Some(ppr) = defaults.child(W, "pPrDefault").and_then(|d| d.child(W, "pPr")) {
                self.doc_ppr = parse_ppr(ppr);
            }
        }
        for style in root.elements().filter(|e| e.is(W, "style")) {
            let Some(id) = style.attr(W, "styleId") else { continue };
            if style.attr(W, "type") == Some("paragraph") && matches!(style.attr(W, "default"), Some("1" | "true")) {
                self.default_paragraph = Some(id.to_owned());
            }
            self.styles.insert(
                id.to_owned(),
                Style {
                    based_on: style.child(W, "basedOn").and_then(|b| b.attr(W, "val")).map(str::to_owned),
                    ppr: style.child(W, "pPr").map(parse_ppr).unwrap_or_default(),
                    rpr: style.child(W, "rPr").map(|r| parse_rpr(r, &self.theme)).unwrap_or_default(),
                },
            );
        }
        Ok(())
    }

    fn load_numbering(&mut self, path: &str) -> Result<(), CommandError> {
        let Some(root) = self.xml(path)? else { return Ok(()) };
        for abstract_num in root.elements().filter(|e| e.is(W, "abstractNum")) {
            let Some(id) = abstract_num.attr(W, "abstractNumId") else { continue };
            let mut levels: Vec<Option<Level>> = vec![None; 9];
            for lvl in abstract_num.elements().filter(|e| e.is(W, "lvl")) {
                let Some(index) = lvl.attr(W, "ilvl").and_then(|v| v.parse::<usize>().ok()).filter(|&i| i < 9) else { continue };
                let val = |name: &str| lvl.child(W, name).and_then(|e| e.attr(W, "val")).map(str::to_owned);
                levels[index] = Some(Level {
                    start: val("start").and_then(|v| v.parse().ok()).unwrap_or(1),
                    format: val("numFmt").unwrap_or_else(|| "decimal".into()),
                    text: val("lvlText").unwrap_or_default(),
                    ppr: lvl.child(W, "pPr").map(parse_ppr).unwrap_or_default(),
                    rpr: lvl.child(W, "rPr").map(|r| parse_rpr(r, &self.theme)).unwrap_or_default(),
                });
            }
            self.numbering.abstracts.insert(id.to_owned(), levels);
        }
        for num in root.elements().filter(|e| e.is(W, "num")) {
            let (Some(id), Some(abstract_id)) =
                (num.attr(W, "numId"), num.child(W, "abstractNumId").and_then(|a| a.attr(W, "val")))
            else {
                continue;
            };
            let overrides = num
                .elements()
                .filter(|e| e.is(W, "lvlOverride"))
                .filter_map(|o| {
                    let level = o.attr(W, "ilvl")?.parse().ok()?;
                    let start = o.child(W, "startOverride")?.attr(W, "val")?.parse().ok()?;
                    Some((level, start))
                })
                .collect();
            self.numbering.nums.insert(id.to_owned(), (abstract_id.to_owned(), overrides));
        }
        Ok(())
    }

    fn load_theme(&mut self, path: &str) -> Result<(), CommandError> {
        let Some(root) = self.xml(path)? else { return Ok(()) };
        let face = |which: &str| {
            root.descendant(A, which)
                .and_then(|f| f.child(A, "latin"))
                .and_then(|l| l.attr("", "typeface"))
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        };
        self.theme = Theme { minor: face("minorFont"), major: face("majorFont") };
        Ok(())
    }

    // --- the body ---------------------------------------------------------

    fn block(&mut self, element: &Element) -> Result<(), CommandError> {
        if element.ns == W {
            match element.name.as_str() {
                "p" => return self.paragraph(element),
                "tbl" => {
                    self.warn("Tables are shown as their cells' paragraphs, one after another, until table layout is supported.");
                    for row in element.elements().filter(|e| e.is(W, "tr")) {
                        for cell in row.elements().filter(|e| e.is(W, "tc")) {
                            for child in cell.elements() {
                                self.block(child)?;
                            }
                        }
                    }
                }
                "sdt" => {
                    if let Some(content) = element.child(W, "sdtContent") {
                        for child in content.elements() {
                            self.block(child)?;
                        }
                    }
                }
                "customXml" | "ins" | "moveTo" => {
                    for child in element.elements() {
                        self.block(child)?;
                    }
                }
                _ => {}
            }
        } else if element.is(MC, "AlternateContent") {
            if let Some(choice) = element.child(MC, "Choice").or_else(|| element.child(MC, "Fallback")) {
                for child in choice.elements() {
                    self.block(child)?;
                }
            }
        }
        Ok(())
    }

    fn paragraph(&mut self, p: &Element) -> Result<(), CommandError> {
        let ppr_element = p.child(W, "pPr");
        let direct = ppr_element.map(parse_ppr).unwrap_or_default();
        let style_id = ppr_element
            .and_then(|e| e.child(W, "pStyle"))
            .and_then(|s| s.attr(W, "val"))
            .map(str::to_owned)
            .or_else(|| self.default_paragraph.clone());
        let (from_style, run_from_style) = style_id.as_deref().map(|id| self.style_chain(id)).unwrap_or_default();

        let mut ppr = self.doc_ppr.clone();
        ppr.over(&from_style);
        let mut rpr = self.doc_rpr.clone();
        rpr.over(&run_from_style);

        // List numbering: the level's indents sit between the style and direct formatting.
        let numbering = direct.numbering.clone().or_else(|| ppr.numbering.clone()).filter(|(id, _)| id != "0");
        let mut label = None;
        if let Some((num, level)) = &numbering {
            if let Some(lvl) = self.numbering.level(num, *level).cloned() {
                ppr.over(&lvl.ppr);
                if let Some(text) = self.numbering.next_label(num, *level) {
                    let mut label_rpr = rpr.clone();
                    let mut level_rpr = lvl.rpr.clone();
                    // The bullet was mapped to Unicode; its symbol font would only lack it.
                    if level_rpr.family.as_deref().is_some_and(|f| matches!(f.to_lowercase().as_str(), "symbol" | "wingdings" | "wingdings 2" | "wingdings 3")) {
                        level_rpr.family = None;
                    }
                    label_rpr.over(&level_rpr);
                    label = Some(Run { text, style: label_rpr.run_style() });
                }
            }
        }
        ppr.over(&direct);

        let mut mark = rpr.clone();
        if let Some(mark_rpr) = ppr_element.and_then(|e| e.child(W, "rPr")) {
            mark.over(&parse_rpr(mark_rpr, &self.theme));
        }
        let template = Paragraph {
            runs: Vec::new(),
            align: ppr.align.unwrap_or_default(),
            indent_left: ppr.indent_left.unwrap_or(0.0),
            indent_right: ppr.indent_right.unwrap_or(0.0),
            first_line: ppr.first_line.unwrap_or(0.0),
            space_before: ppr.space_before.unwrap_or(0.0),
            space_after: ppr.space_after.unwrap_or(0.0),
            line: ppr.line.unwrap_or(LineSpacing::Multiple(1.0)),
            label,
            mark: mark.run_style(),
        };

        if ppr.page_break_before == Some(true) {
            self.blocks.push(FlowBlock::PageBreak);
        }
        let mut building = Building { template, runs: Vec::new(), emitted: false };
        let mut fields = Vec::new();
        for child in p.elements() {
            self.inline(child, &rpr, &mut building, &mut fields)?;
        }
        self.finish(&mut building, true);

        // A section break inside the paragraph starts the next section on a new page.
        if let Some(section) = ppr_element.and_then(|e| e.child(W, "sectPr")) {
            if !matches!(section.child(W, "type").and_then(|t| t.attr(W, "val")), Some("continuous")) {
                self.blocks.push(FlowBlock::PageBreak);
            }
        }
        for text_box in std::mem::take(&mut self.pending) {
            self.block(&text_box)?;
        }
        Ok(())
    }

    /// Emit the paragraph's runs so far. `last` is the paragraph's end: an
    /// empty paragraph is still a blank line, but an empty tail after an
    /// image or page break is nothing.
    fn finish(&mut self, building: &mut Building, last: bool) {
        let has_text = building.runs.iter().any(|r| !r.text.is_empty());
        if !has_text && (building.emitted || !last) {
            building.runs.clear();
            return;
        }
        let mut paragraph = building.template.clone();
        paragraph.runs = std::mem::take(&mut building.runs);
        if building.emitted {
            paragraph.space_before = 0.0;
            paragraph.label = None;
        }
        if !last {
            paragraph.space_after = 0.0;
        }
        self.blocks.push(FlowBlock::Paragraph(paragraph));
        building.emitted = true;
    }

    fn inline(&mut self, element: &Element, base: &RProps, building: &mut Building, fields: &mut Vec<bool>) -> Result<(), CommandError> {
        if element.is(MC, "AlternateContent") {
            if let Some(choice) = element.child(MC, "Choice").or_else(|| element.child(MC, "Fallback")) {
                for child in choice.elements() {
                    self.inline(child, base, building, fields)?;
                }
            }
            return Ok(());
        }
        if element.ns == MATH && matches!(element.name.as_str(), "oMath" | "oMathPara") {
            self.warn("Equations are shown as plain text.");
            let text: String = element.descendants(MATH, "t").iter().map(|t| t.text()).collect();
            building.runs.push(Run { text, style: base.run_style() });
            return Ok(());
        }
        if element.ns != W {
            return Ok(());
        }
        match element.name.as_str() {
            "r" => self.run(element, base, building, fields)?,
            "hyperlink" | "smartTag" | "fldSimple" | "customXml" | "dir" | "bdo" => {
                for child in element.elements() {
                    self.inline(child, base, building, fields)?;
                }
            }
            "ins" | "moveTo" => {
                self.warn("Tracked changes are shown as if they had been accepted.");
                for child in element.elements() {
                    self.inline(child, base, building, fields)?;
                }
            }
            "del" | "moveFrom" => self.warn("Tracked changes are shown as if they had been accepted."),
            "sdt" => {
                if let Some(content) = element.child(W, "sdtContent") {
                    for child in content.elements() {
                        self.inline(child, base, building, fields)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn run(&mut self, r: &Element, base: &RProps, building: &mut Building, fields: &mut Vec<bool>) -> Result<(), CommandError> {
        let mut rpr = base.clone();
        if let Some(rpr_element) = r.child(W, "rPr") {
            if let Some(id) = rpr_element.child(W, "rStyle").and_then(|s| s.attr(W, "val")) {
                let (_, style_rpr) = self.style_chain(id);
                rpr.over(&style_rpr);
            }
            rpr.over(&parse_rpr(rpr_element, &self.theme));
        }
        if rpr.vanish == Some(true) {
            return Ok(()); // hidden text: not shown when printed either
        }
        let style = rpr.run_style();
        let caps = rpr.caps == Some(true);
        let in_code = |fields: &Vec<bool>| fields.last().copied().unwrap_or(false);
        let push = |building: &mut Building, text: &str| {
            let text = if caps { text.to_uppercase() } else { text.to_owned() };
            match building.runs.last_mut() {
                Some(last) if last.style == style => last.text.push_str(&text),
                _ => building.runs.push(Run { text, style: style.clone() }),
            }
        };
        for child in r.elements() {
            if child.is(MC, "AlternateContent") {
                if let Some(choice) = child.child(MC, "Choice").or_else(|| child.child(MC, "Fallback")) {
                    for inner in choice.elements() {
                        self.drawing_or_text_box(inner, building)?;
                    }
                }
                continue;
            }
            if child.ns != W {
                continue;
            }
            match child.name.as_str() {
                "t" if !in_code(fields) => push(building, &child.text()),
                "tab" | "ptab" if !in_code(fields) => push(building, "\t"),
                "br" => match child.attr(W, "type") {
                    Some("page") => {
                        self.finish(building, false);
                        self.blocks.push(FlowBlock::PageBreak);
                    }
                    _ => push(building, "\n"),
                },
                "cr" => push(building, "\n"),
                "noBreakHyphen" => push(building, "\u{2011}"),
                "sym" => {
                    if let Some(c) = child.attr(W, "char").and_then(|v| u32::from_str_radix(v, 16).ok()).and_then(char::from_u32) {
                        push(building, &symbol(c).to_string());
                    }
                }
                "fldChar" => match child.attr(W, "fldCharType") {
                    Some("begin") => fields.push(true),
                    Some("separate") => {
                        if let Some(top) = fields.last_mut() {
                            *top = false;
                        }
                    }
                    Some("end") => {
                        fields.pop();
                    }
                    _ => {}
                },
                "drawing" | "pict" | "object" => self.drawing_or_text_box(child, building)?,
                "footnoteReference" | "endnoteReference" => self.warn("Footnotes and endnotes were left out."),
                _ => {}
            }
        }
        Ok(())
    }

    /// An image (drawn in place) or a text box (its paragraphs kept for later).
    fn drawing_or_text_box(&mut self, element: &Element, building: &mut Building) -> Result<(), CommandError> {
        let boxes = element.descendants(W, "txbxContent");
        if !boxes.is_empty() {
            self.warn("Text boxes are shown as ordinary paragraphs after the paragraph they're anchored to.");
            for content in boxes {
                self.pending.extend(content.elements().cloned());
            }
            return Ok(());
        }
        if element.descendant(WP, "anchor").is_some() {
            self.warn("Floating images are placed in line with the text.");
        }
        let extent = element.descendant(WP, "extent");
        let size = extent.and_then(|e| {
            let cx = e.attr("", "cx")?.parse::<f32>().ok()?;
            let cy = e.attr("", "cy")?.parse::<f32>().ok()?;
            Some((cx / EMU_PER_POINT, cy / EMU_PER_POINT))
        });
        let blip = element.descendant(A, "blip");
        let rel = blip
            .and_then(|b| b.attr(R, "embed"))
            .or_else(|| element.descendant(V, "imagedata").and_then(|i| i.attr(R, "id")));
        let Some(rel) = rel else {
            if blip.and_then(|b| b.attr(R, "link")).is_some() {
                self.warn("Images linked from outside the document were left out.");
            }
            return Ok(());
        };
        let Some((path, external)) = self.rels.get(rel).cloned() else { return Ok(()) };
        if external {
            self.warn("Images linked from outside the document were left out.");
            return Ok(());
        }
        let bytes = self.zip.read(&path)?;
        let supported = matches!(
            image::guess_format(&bytes),
            Ok(image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::Tiff | image::ImageFormat::WebP)
        );
        if !supported {
            self.warn("Images in formats other than JPEG, PNG, TIFF and WebP (such as EMF or SVG) were left out.");
            return Ok(());
        }
        let Some((width, height)) = size.filter(|(w, h)| *w > 0.0 && *h > 0.0) else { return Ok(()) };
        self.finish(building, false);
        self.images += 1;
        self.blocks.push(FlowBlock::Image(FlowImage {
            bytes,
            name: format!("image {} in {}", self.images, self.name),
            width,
            height,
            align: building.template.align,
        }));
        Ok(())
    }
}

/// SPEC: P7-OCR-009 — the flow model of the Word document `bytes`, plus
/// warnings for everything that was simplified.
///
/// # Errors
/// When the file isn't a readable `.docx` (not a ZIP, no main document, or
/// damaged XML).
pub fn read_docx(name: &str, bytes: &[u8]) -> Result<(FlowDocument, Vec<String>), CommandError> {
    let not_word = || CommandError::InvalidInput(format!("{name} isn't a Word document that can be read."));
    let zip = ZipArchive::parse(bytes).map_err(|_| not_word())?;

    // The main part is wherever the package relationships say it is.
    let package_rels = if zip.contains("_rels/.rels") { parse(&String::from_utf8_lossy(&zip.read("_rels/.rels")?))? } else { return Err(not_word()) };
    let main = package_rels
        .elements()
        .filter(|e| e.is(PKG_REL, "Relationship"))
        .find(|e| e.attr("", "Type").is_some_and(|t| t.ends_with("/officeDocument")))
        .and_then(|e| e.attr("", "Target"))
        .map(|t| resolve_target("", t))
        .ok_or_else(not_word)?;
    let main_dir = main.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
    let main_file = main.rsplit_once('/').map_or(main.as_str(), |(_, f)| f).to_owned();

    let mut reader = Reader {
        zip: &zip,
        name: name.to_owned(),
        styles: HashMap::new(),
        default_paragraph: None,
        doc_ppr: PProps::default(),
        doc_rpr: RProps::default(),
        theme: Theme::default(),
        numbering: Numbering::default(),
        rels: HashMap::new(),
        blocks: Vec::new(),
        warnings: BTreeSet::new(),
        notes: Vec::new(),
        pending: Vec::new(),
        images: 0,
    };

    let rels_path = format!("{}{}_rels/{main_file}.rels", main_dir, if main_dir.is_empty() { "" } else { "/" });
    let mut part_of_type: HashMap<String, String> = HashMap::new();
    if let Some(rels) = reader.xml(&rels_path)? {
        for rel in rels.elements().filter(|e| e.is(PKG_REL, "Relationship")) {
            let (Some(id), Some(target)) = (rel.attr("", "Id"), rel.attr("", "Target")) else { continue };
            let external = rel.attr("", "TargetMode") == Some("External");
            let path = if external { target.to_owned() } else { resolve_target(&main_dir, target) };
            if let Some(kind) = rel.attr("", "Type").and_then(|t| t.rsplit('/').next()) {
                part_of_type.entry(kind.to_owned()).or_insert_with(|| path.clone());
            }
            reader.rels.insert(id.to_owned(), (path, external));
        }
    }
    if let Some(theme) = part_of_type.get("theme").cloned() {
        reader.load_theme(&theme)?;
    }
    if let Some(styles) = part_of_type.get("styles").cloned() {
        reader.load_styles(&styles)?;
    }
    if let Some(numbering) = part_of_type.get("numbering").cloned() {
        reader.load_numbering(&numbering)?;
    }

    let document = reader.xml(&main)?.ok_or_else(not_word)?;
    let body = document.child(W, "body").ok_or_else(not_word)?;
    for element in body.elements() {
        reader.block(element)?;
    }

    // Page setup: the last section's, which governs the end of the document.
    let mut page = PageSetup::default();
    if let Some(section) = body.child(W, "sectPr") {
        if let Some(size) = section.child(W, "pgSz") {
            page.width = size.attr(W, "w").and_then(twips).unwrap_or(page.width);
            page.height = size.attr(W, "h").and_then(twips).unwrap_or(page.height);
        }
        if let Some(margins) = section.child(W, "pgMar") {
            let margin = |name: &str, default: f32| margins.attr(W, name).and_then(twips).map_or(default, f32::abs);
            page.top = margin("top", page.top);
            page.right = margin("right", page.right);
            page.bottom = margin("bottom", page.bottom);
            page.left = margin("left", page.left);
        }
        if section.child(W, "headerReference").is_some() || section.child(W, "footerReference").is_some() {
            reader.warn("Headers and footers were left out.");
        }
        if section.child(W, "cols").and_then(|c| c.attr(W, "num")).and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n > 1) {
            reader.warn("Text set in several columns is shown in one.");
        }
    }
    if body.descendant(W, "commentReference").is_some() {
        reader.warn("Comments were left out.");
    }

    let mut warnings: Vec<String> = reader.warnings.iter().map(|w| format!("{name}: {w}")).collect();
    warnings.extend(reader.notes);
    Ok((FlowDocument { page, blocks: reader.blocks }, warnings))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn list_numbers_format_as_word_counts_them() {
        assert_eq!(number(3, "decimal"), "3");
        assert_eq!(number(3, "lowerLetter"), "c");
        assert_eq!(number(28, "upperLetter"), "BB");
        assert_eq!(number(14, "lowerRoman"), "xiv");
        assert_eq!(number(1994, "upperRoman"), "MCMXCIV");
        assert_eq!(number(7, "decimalZero"), "07");
    }

    #[test]
    fn symbol_font_bullets_become_unicode() {
        assert_eq!(symbol('\u{F0B7}'), '•');
        assert_eq!(symbol('\u{F0A7}'), '▪');
        assert_eq!(symbol('o'), 'o', "Courier's 'o' bullet is a real character");
    }

    #[test]
    fn relationship_targets_resolve_against_the_part() {
        assert_eq!(resolve_target("word", "media/image1.png"), "word/media/image1.png");
        assert_eq!(resolve_target("word", "../customXml/item1.xml"), "customXml/item1.xml");
        assert_eq!(resolve_target("word", "/word/media/x.jpeg"), "word/media/x.jpeg");
    }

    #[test]
    fn properties_layer_and_hex_colours_parse() {
        let mut base = RProps { family: Some("Calibri".into()), size: Some(11.0), ..RProps::default() };
        base.over(&RProps { bold: Some(true), size: Some(14.0), ..RProps::default() });
        assert_eq!((base.family.as_deref(), base.bold, base.size), (Some("Calibri"), Some(true), Some(14.0)));
        assert_eq!(hex_colour("1F4E79"), Some([0x1F, 0x4E, 0x79]));
        assert_eq!(hex_colour("auto"), None);
    }
}
