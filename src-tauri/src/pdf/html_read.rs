//! SPEC: P7-OCR-009 (P7.C1c) — an HTML page as the flow model, so it is laid
//! out, given real fonts and broken into pages by the same code as a Word
//! document.
//!
//! **Parsed as a browser parses.** html5ever (through `dom_query`) builds the
//! tree and mends broken markup the way browsers do. Scripts are never run.
//!
//! **Styled by a cascade.** A browser's default stylesheet, then the page's
//! own `<style>` and local stylesheets, then `style` attributes — by
//! importance, origin, specificity and order — with inheritance. [`css`]
//! parses; `dom_query` matches selectors.
//!
//! **Offline.** Nothing is fetched from the web. Images and stylesheets load
//! only from the page's own folder and below (where a browser's "save page"
//! puts them) or from `data:` URIs.
//!
//! **What is simplified, and said.** Page layout from CSS (floats,
//! positioning, flexbox, grid, columns) is not applied, so content follows
//! source order; drawings, video, frames and form fields are left out; images
//! in formats other than JPEG, PNG, TIFF and WebP are left out. Each adds one
//! warning.

// Pixel and character counts become point sizes.
#![allow(clippy::cast_precision_loss)]

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use dom_query::{Document, Matcher, NodeId, NodeRef};

use crate::error::CommandError;
use crate::pdf::create::{decode_text, windows_1252};
use crate::pdf::css::{self, Declaration, Paint, Specificity, PX};
use crate::pdf::docx_read::number;
use crate::pdf::flow::{
    Align, Border, Borders, FlowBlock, FlowCell, FlowDocument, FlowImage, FlowRow, FlowTable, LineSpacing, Merge, PageSetup, Paragraph, Run,
    RunStyle, TableWidth, VAlign,
};
use crate::pdf::font_index::{fallback_families, Class, FontIndex};

/// Deeper than this, an element's content is kept as plain text: the walk is
/// recursive, and a hostile page can nest without limit.
const MAX_DEPTH: usize = 200;
const IMAGE_LIMIT: u64 = 64 << 20;
const STYLESHEET_LIMIT: u64 = 4 << 20;
/// Browsers' default text: 16 px.
const ROOT_SIZE: f32 = 12.0;
/// A border's width when its style is set but its width isn't: `medium`.
const MEDIUM: f32 = 3.0 * PX;

const LAYOUT: &str = "Page layout from the page's CSS (floats, positioning, flexbox, grid, columns) isn't applied; its content follows in source order.";
const REMOTE: &str = "Images and stylesheets on the web were left out: VibePDF doesn't go online.";
const OUTSIDE: &str = "Images or stylesheets outside the page's own folder were left out.";
const EMBEDDED: &str = "Drawings, video, audio, frames and other embedded content (SVG, canvas, iframes) were left out.";
const FORMS: &str = "Form fields were left out.";
const IMAGE_FORMAT: &str = "Images in formats other than JPEG, PNG, TIFF and WebP (such as GIF or SVG) were left out.";
const DEPTH: &str = "Very deeply nested HTML (over 200 levels) was reduced to its text.";

/// What a browser applies before any page's own CSS: the parts of the HTML
/// standard's default stylesheet the flow model can show. `body` has no
/// margin: the page's margins stand in for it.
const DEFAULT_CSS: &str = r"
html, body, div, p, h1, h2, h3, h4, h5, h6, ul, ol, dl, dt, dd, blockquote, pre, address, article, aside,
footer, header, main, nav, section, figure, figcaption, form, fieldset, legend, hr, center, details,
summary, hgroup, menu, dir, caption, noscript, search, listing, xmp, plaintext { display: block }
li { display: list-item }
table { display: table }
head, script, style, template, title, meta, link, base, [hidden] { display: none }
p, dl, ul, ol, menu, dir, pre, listing, xmp { margin-top: 1em; margin-bottom: 1em }
blockquote, figure { margin: 1em 40px }
ul, ol, menu, dir { padding-left: 40px }
ul, menu, dir { list-style-type: disc }
ol { list-style-type: decimal }
ul ul, ol ul, menu ul, ul menu { list-style-type: circle }
ul ul ul, ul ol ul, ol ul ul, ol ol ul { list-style-type: square }
ul ul, ul ol, ol ul, ol ol { margin-top: 0; margin-bottom: 0 }
dd { margin-left: 40px }
h1 { font-size: 2em; margin-top: 0.67em; margin-bottom: 0.67em }
h2 { font-size: 1.5em; margin-top: 0.83em; margin-bottom: 0.83em }
h3 { font-size: 1.17em; margin-top: 1em; margin-bottom: 1em }
h4 { margin-top: 1.33em; margin-bottom: 1.33em }
h5 { font-size: 0.83em; margin-top: 1.67em; margin-bottom: 1.67em }
h6 { font-size: 0.67em; margin-top: 2.33em; margin-bottom: 2.33em }
h1, h2, h3, h4, h5, h6, b, strong, th { font-weight: bold }
i, em, cite, var, dfn, address { font-style: italic }
u, ins { text-decoration: underline }
s, strike, del { text-decoration: line-through }
pre, code, kbd, samp, tt, listing, xmp, plaintext { font-family: monospace }
pre, code, kbd, samp, tt { font-size: 0.8125em }
pre code, pre kbd, pre samp, pre tt { font-size: 1em }
pre, listing, xmp, plaintext { white-space: pre }
small { font-size: smaller }
big { font-size: larger }
sub { vertical-align: sub; font-size: smaller }
sup { vertical-align: super; font-size: smaller }
a[href] { color: #0000EE; text-decoration: underline }
center { text-align: center }
th { text-align: center }
caption { text-align: center }
td, th { padding: 1px; vertical-align: middle }
hr { border: 1px solid #808080; margin: 0.5em 0 }
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Display {
    Inline,
    Block,
    ListItem,
    Table,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LineHeight {
    Normal,
    /// Of the font size; inherited as a factor.
    Factor(f32),
    Points(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WhiteSpace {
    Normal,
    /// Spaces and line breaks kept.
    Pre,
    /// Spaces collapse, line breaks kept.
    PreLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transform {
    None,
    Upper,
    Lower,
    Capitalize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Size {
    Points(f32),
    Percent(f32),
}

/// An element's computed style: the inherited part first.
#[derive(Clone, Debug)]
struct Style {
    font: RunStyle,
    align: Align,
    line_height: LineHeight,
    white_space: WhiteSpace,
    list_type: String,
    text_indent: f32,
    transform: Transform,

    display: Display,
    margin: [f32; 4],
    margin_auto: [bool; 4],
    padding: [f32; 4],
    border_width: [f32; 4],
    border_shown: [bool; 4],
    /// `None` is `currentColor`.
    border_colour: [Option<[u8; 3]>; 4],
    background: Option<[u8; 3]>,
    width: Option<Size>,
    height: Option<f32>,
    valign: Option<VAlign>,
    break_before: bool,
    break_after: bool,
}

impl Style {
    fn root(family: String) -> Self {
        Self {
            font: RunStyle { family, size: ROOT_SIZE, ..RunStyle::default() },
            align: Align::Left,
            line_height: LineHeight::Normal,
            white_space: WhiteSpace::Normal,
            list_type: "disc".into(),
            text_indent: 0.0,
            transform: Transform::None,
            display: Display::Block,
            margin: [0.0; 4],
            margin_auto: [false; 4],
            padding: [0.0; 4],
            border_width: [MEDIUM; 4],
            border_shown: [false; 4],
            border_colour: [None; 4],
            background: None,
            width: None,
            height: None,
            valign: None,
            break_before: false,
            break_after: false,
        }
    }

    /// A child's starting point: what CSS inherits, and initial values for
    /// the rest. Text decoration is carried down too, as it is drawn.
    fn child(&self) -> Self {
        Self { display: Display::Inline, ..Self::root(String::new()) }.inheriting(self)
    }

    fn inheriting(mut self, parent: &Self) -> Self {
        self.font = parent.font.clone();
        self.align = parent.align;
        self.line_height = parent.line_height;
        self.white_space = parent.white_space;
        self.list_type.clone_from(&parent.list_type);
        self.text_indent = parent.text_indent;
        self.transform = parent.transform;
        self
    }

    fn border(&self, side: usize) -> Option<Border> {
        (self.border_shown[side] && self.border_width[side] > 0.0)
            .then(|| Border { width: self.border_width[side], color: self.border_colour[side].unwrap_or(self.font.color) })
    }

    fn line(&self) -> LineSpacing {
        match self.line_height {
            LineHeight::Normal => LineSpacing::Multiple(1.0),
            LineHeight::Factor(f) => LineSpacing::Exactly(f * self.font.size),
            LineHeight::Points(p) => LineSpacing::Exactly(p),
        }
    }
}

/// The block that inline content is collected for: its style, and how far
/// its content is indented from the text area's edges.
#[derive(Clone, Debug)]
struct Context {
    style: Style,
    left: f32,
    right: f32,
}

/// `(important, origin, specificity, order)`: later wins. Origins: 0 the
/// browser's defaults, 1 presentational attributes, 2 the page's CSS, 3
/// `style` attributes.
type Key = (bool, u8, Specificity, usize);

fn tag(node: &NodeRef) -> String {
    node.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default()
}

fn attr(node: &NodeRef, name: &str) -> Option<String> {
    node.attr(name).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// A presentational attribute's size (`width="200"`) as CSS.
fn dimension(value: &str) -> String {
    if value.parse::<f32>().is_ok() { format!("{value}px") } else { value.to_owned() }
}

/// The declarations an element's presentational attributes stand for
/// (`align`, `bgcolor`, `<font color>` …), below every stylesheet rule.
fn hints(node: &NodeRef, element: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut add = |name: &str, value: String| out.push(Declaration { name: name.into(), value, important: false });
    if let Some(align) = attr(node, "align").map(|a| a.to_ascii_lowercase()) {
        match (element, align.as_str()) {
            ("table", "center") => {
                add("margin-left", "auto".into());
                add("margin-right", "auto".into());
            }
            ("table", "right") => add("margin-left", "auto".into()),
            // On images and tables, `align` floats them.
            ("img" | "table" | "iframe" | "object", _) => {}
            _ => add("text-align", align),
        }
    }
    if let Some(colour) = attr(node, "bgcolor") {
        add("background-color", colour);
    }
    if matches!(element, "table" | "td" | "th" | "col" | "colgroup" | "img" | "hr") {
        if let Some(width) = attr(node, "width") {
            add("width", dimension(&width));
        }
    }
    if matches!(element, "td" | "th" | "tr" | "img") {
        if let Some(height) = attr(node, "height") {
            add("height", dimension(&height));
        }
    }
    if let Some(valign) = attr(node, "valign") {
        add("vertical-align", valign);
    }
    match element {
        "font" => {
            if let Some(colour) = attr(node, "color") {
                add("color", colour);
            }
            if let Some(face) = attr(node, "face") {
                add("font-family", face);
            }
            if let Some(size) = attr(node, "size") {
                let base: i32 = 3;
                let n = match size.chars().next() {
                    Some('+' | '-') => size.parse::<i32>().map(|d| base + d),
                    _ => size.parse::<i32>(),
                };
                if let Ok(n) = n {
                    const PIXELS: [u16; 7] = [10, 13, 16, 18, 24, 32, 48];
                    let at = usize::try_from(n.clamp(1, 7) - 1).unwrap_or(2);
                    add("font-size", format!("{}px", PIXELS[at]));
                }
            }
        }
        "body" => {
            if let Some(colour) = attr(node, "text") {
                add("color", colour);
            }
        }
        "table" => {
            if let Some(width) = node.attr("border").map(|b| b.trim().parse::<f32>().unwrap_or(1.0)) {
                if width > 0.0 {
                    add("border", format!("{width}px solid #808080"));
                }
            }
        }
        "td" | "th" => {
            if let Some(table) = node.ancestors_it(None).find(|a| tag(a) == "table") {
                if table.attr("border").is_some_and(|b| b.trim().parse::<f32>().unwrap_or(1.0) > 0.0) {
                    add("border", "1px solid #808080".into());
                }
                if let Some(padding) = attr(&table, "cellpadding") {
                    add("padding", dimension(&padding));
                }
            }
        }
        "ol" | "ul" | "li" => {
            if let Some(kind) = node.attr("type") {
                let list_type = match kind.trim() {
                    "1" => "decimal",
                    "a" => "lower-alpha",
                    "A" => "upper-alpha",
                    "i" => "lower-roman",
                    "I" => "upper-roman",
                    other => other,
                };
                add("list-style-type", list_type.to_ascii_lowercase());
            }
        }
        "hr" => {
            if let Some(size) = attr(node, "size") {
                add("height", dimension(&size));
            }
            if let Some(colour) = attr(node, "color") {
                add("background-color", colour);
            }
        }
        _ => {}
    }
    out
}

/// Which elements each stylesheet rule reaches, as declarations keyed for
/// the cascade.
fn cascade(doc: &Document, sheets: &[(u8, Vec<css::Rule>)]) -> HashMap<NodeId, Vec<(Key, Declaration)>> {
    let mut matched: HashMap<NodeId, Vec<(Key, Declaration)>> = HashMap::new();
    let mut order = 0;
    for (origin, rules) in sheets {
        for rule in rules {
            let first = order;
            order += rule.declarations.len();
            for selector in &rule.selectors {
                let (specificity, pseudo_element) = css::specificity(selector);
                // Generated content (::before …) isn't drawn, so its rules
                // must not land on the element itself.
                if pseudo_element {
                    continue;
                }
                let Ok(compiled) = Matcher::new(selector) else { continue };
                for node in doc.select_matcher(&compiled).nodes() {
                    let entry = matched.entry(node.id).or_default();
                    for (i, d) in rule.declarations.iter().enumerate() {
                        entry.push(((d.important, *origin, specificity, first + i), d.clone()));
                    }
                }
            }
        }
    }
    matched
}

/// Elements that never hold others or that close themselves when the next
/// one starts: a pre-scan can't count them as nesting.
const NOT_NESTING: [&str; 33] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr", "p", "li", "dt", "dd", "tr",
    "td", "th", "thead", "tbody", "tfoot", "option", "optgroup", "colgroup", "caption", "rb", "rt", "rp", "html", "head",
];
/// Elements whose content is text, not markup.
const RAW_TEXT: [&str; 7] = ["script", "style", "textarea", "title", "xmp", "noembed", "noframes"];
/// As Chrome's parser: past this depth, elements stop nesting.
const PARSE_DEPTH: usize = 512;

/// `html` with start tags past `max` levels of nesting (and their end tags)
/// removed, their text kept; `None` when nothing is that deep. The HTML
/// parser scans its stack of open elements for each start tag, so without
/// this a page of 100,000 nested elements takes minutes to parse.
fn limit_nesting(html: &str, max: usize) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let (mut depth, mut dropped) = (0usize, 0usize);
    let mut removed: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while let Some(offset) = lower[i..].find('<') {
        let at = i + offset;
        if lower[at..].starts_with("<!--") {
            i = lower[at..].find("-->").map_or(bytes.len(), |e| at + e + 3);
            continue;
        }
        let closing = bytes.get(at + 1) == Some(&b'/');
        let name_start = at + 1 + usize::from(closing);
        let name_end = lower[name_start..].find(|c: char| !c.is_ascii_alphanumeric() && c != '-').map_or(bytes.len(), |e| name_start + e);
        if name_end == name_start || !bytes[name_start].is_ascii_alphabetic() {
            // A `<` in text, a doctype or a processing instruction.
            i = if matches!(bytes.get(at + 1), Some(b'!' | b'?')) { lower[at..].find('>').map_or(bytes.len(), |e| at + e + 1) } else { at + 1 };
            continue;
        }
        let name = &lower[name_start..name_end];
        let tag_end = lower[name_end..].find('>').map_or(bytes.len(), |e| name_end + e + 1);
        let self_closed = lower[..tag_end].ends_with("/>");
        let nests = !NOT_NESTING.contains(&name) && !self_closed;
        if closing {
            if nests {
                if dropped > 0 {
                    dropped -= 1;
                    removed.push((at, tag_end));
                } else {
                    depth = depth.saturating_sub(1);
                }
            }
        } else if RAW_TEXT.contains(&name) {
            i = lower[tag_end..].find(&format!("</{name}")).map_or(bytes.len(), |e| tag_end + e);
            continue;
        } else if nests {
            if depth >= max {
                dropped += 1;
                removed.push((at, tag_end));
            } else {
                depth += 1;
            }
        }
        i = tag_end;
    }
    if removed.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(html.len());
    let mut from = 0;
    for (start, end) in removed {
        out.push_str(&html[from..start]);
        from = end;
    }
    out.push_str(&html[from..]);
    Some(out)
}

/// An image's size in pixels. This build of `image` has no PNG decoder
/// (PNG goes through the `png` crate), so a PNG's is read from its header.
fn pixel_size(bytes: &[u8], format: image::ImageFormat) -> Option<(u32, u32)> {
    if format == image::ImageFormat::Png {
        let field = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
        return (bytes.get(12..16) == Some(b"IHDR")).then(|| field(16).zip(field(20))).flatten().filter(|&(w, h)| w > 0 && h > 0);
    }
    image::ImageReader::with_format(std::io::Cursor::new(bytes), format).into_dimensions().ok()
}

/// The page's text: by byte-order mark, then by its `charset` declaration,
/// then as UTF-8, else Windows-1252 with a warning.
fn decode(name: &str, bytes: &[u8], warnings: &mut Vec<String>) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) || bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_text(name, bytes, warnings);
    }
    let head: String = bytes.iter().take(1024).map(|&b| char::from(b.to_ascii_lowercase())).collect();
    let declared = head.find("charset=").map(|at| {
        head[at + 8..]
            .trim_start_matches(['"', '\'', ' '])
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
            .collect::<String>()
    });
    match declared.as_deref() {
        None | Some("utf-8" | "utf8" | "unicode-1-1-utf-8") => decode_text(name, bytes, warnings),
        Some("windows-1252" | "cp1252" | "iso-8859-1" | "iso8859-1" | "iso_8859-1" | "latin1" | "l1" | "us-ascii" | "ascii" | "iso-8859-15") => {
            bytes.iter().map(|&b| windows_1252(b)).collect()
        }
        Some(other) => {
            if let Ok(text) = std::str::from_utf8(bytes) {
                return text.to_owned();
            }
            warnings.push(format!("{name} is in the {other} character set, which isn't supported, so it was read as Western (Windows-1252)."));
            bytes.iter().map(|&b| windows_1252(b)).collect()
        }
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok());
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A URL's scheme, if it has one (`C:` is a drive, not a scheme).
fn scheme(src: &str) -> Option<String> {
    let colon = src.find(':')?;
    let name = &src[..colon];
    (name.len() > 1 && name.starts_with(|c: char| c.is_ascii_alphabetic()) && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
        .then(|| name.to_ascii_lowercase())
}

/// A list marker: bullets as the browser draws them, numbers with a full stop.
fn marker(list_type: &str, value: u32) -> Option<String> {
    let numbered = |format: &str| Some(format!("{}.", number(value, format)));
    match list_type {
        "none" => None,
        "disc" => Some("\u{2022}".into()),
        "circle" => Some("\u{25E6}".into()),
        "square" => Some("\u{25AA}".into()),
        "lower-alpha" | "lower-latin" => numbered("lowerLetter"),
        "upper-alpha" | "upper-latin" => numbered("upperLetter"),
        "lower-roman" => numbered("lowerRoman"),
        "upper-roman" => numbered("upperRoman"),
        "decimal-leading-zero" => numbered("decimalZero"),
        // Every other numbering system is shown as decimal.
        _ => numbered("decimal"),
    }
}

fn is_list_type(value: &str) -> bool {
    matches!(
        value,
        "none" | "disc" | "circle" | "square" | "decimal" | "decimal-leading-zero" | "lower-alpha" | "upper-alpha" | "lower-latin" | "upper-latin"
            | "lower-roman" | "upper-roman" | "lower-greek" | "armenian" | "georgian"
    )
}

/// What the cells below a `rowspan` take from the cell that covers them.
#[derive(Clone)]
struct Cover {
    start: usize,
    span: usize,
    borders: Borders,
    margins: [f32; 4],
    fill: Option<[u8; 3]>,
}

struct Reader<'n> {
    name: &'n str,
    folder: Option<PathBuf>,
    matched: HashMap<NodeId, Vec<(Key, Declaration)>>,
    text_width: f32,
    blocks: Vec<FlowBlock>,
    /// Inline content waiting for its paragraph.
    line: Vec<Run>,
    /// Vertical margin owed before the next block: collapsed, as CSS
    /// collapses adjoining margins, by taking the larger.
    pending: f32,
    /// A list item's marker, for its first paragraph.
    label: Option<Run>,
    /// Ordered-list counters, innermost last.
    counters: Vec<u32>,
    families: HashMap<String, Option<String>>,
    depth: usize,
    warnings: BTreeSet<&'static str>,
    notes: Vec<String>,
}

impl Reader<'_> {
    fn warn(&mut self, message: &'static str) {
        self.warnings.insert(message);
    }

    /// The first installed family of a CSS list; a generic family is the
    /// first installed of its class. When none is installed, the first named
    /// one, which layout replaces and reports.
    fn family(&mut self, list: &[String]) -> Option<String> {
        let key = list.join(",");
        if let Some(found) = self.families.get(&key) {
            return found.clone();
        }
        let index = FontIndex::system();
        let generic = |name: &str| match name.to_ascii_lowercase().as_str() {
            "serif" | "cursive" | "ui-serif" => Some(Class::Serif),
            "sans-serif" | "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "fantasy" | "ui-rounded" => Some(Class::Sans),
            "monospace" | "ui-monospace" => Some(Class::Mono),
            _ => None,
        };
        let found = list
            .iter()
            .find_map(|name| match generic(name) {
                Some(class) => {
                    let candidates = fallback_families(class);
                    candidates.iter().find(|f| index.has_family(f)).or_else(|| candidates.first()).map(|f| (*f).to_owned())
                }
                None => index.has_family(name).then(|| name.clone()),
            })
            .or_else(|| list.iter().find(|name| generic(name).is_none()).cloned());
        self.families.insert(key, found.clone());
        found
    }

    /// `node`'s computed style under `parent`.
    fn style_of(&mut self, node: &NodeRef, tag: &str, parent: &Style) -> Style {
        let mut declarations = self.matched.get(&node.id).cloned().unwrap_or_default();
        for (i, d) in hints(node, tag).into_iter().enumerate() {
            declarations.push(((false, 1, (0, 0, 0), i), d));
        }
        if let Some(inline) = node.attr("style") {
            for (i, d) in css::parse_declarations(&inline).into_iter().enumerate() {
                declarations.push(((d.important, 3, (0, 0, 0), i), d));
            }
        }
        declarations.sort_by_key(|(key, _)| *key);
        let mut style = parent.child();
        // The font size first: every other length in `em` depends on it.
        for (_, d) in declarations.iter().filter(|(_, d)| d.name == "font-size" || d.name == "font") {
            self.apply_font(&mut style, parent, d);
        }
        for (_, d) in declarations.iter().filter(|(_, d)| d.name != "font-size" && d.name != "font") {
            self.apply(&mut style, parent, d);
        }
        style
    }

    fn apply_font(&mut self, style: &mut Style, parent: &Style, d: &Declaration) {
        if d.name == "font-size" {
            if let Some(size) = css::font_size(&d.value, parent.font.size, ROOT_SIZE) {
                style.font.size = size;
            }
            return;
        }
        let Some(font) = css::font_shorthand(&d.value) else { return };
        let Some(size) = css::font_size(&font.size, parent.font.size, ROOT_SIZE) else { return };
        style.font.size = size;
        style.font.bold = font.bold;
        style.font.italic = font.italic;
        style.line_height = font.line_height.as_deref().map_or(LineHeight::Normal, |lh| line_height(lh, size).unwrap_or(LineHeight::Normal));
        if let Some(family) = self.family(&font.families) {
            style.font.family = family;
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply(&mut self, style: &mut Style, parent: &Style, d: &Declaration) {
        let value = d.value.trim();
        let lower = value.to_ascii_lowercase();
        if matches!(lower.as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer") {
            return;
        }
        let em = style.font.size;
        let width = self.text_width;
        let length = |v: &str, of: Option<f32>| css::length(v, em, ROOT_SIZE, of);
        let side = |name: &str| match name.rsplit('-').next() {
            Some("top") => Some(0),
            Some("right") => Some(1),
            Some("bottom") => Some(2),
            Some("left") => Some(3),
            _ => None,
        };
        let name = d.name.as_str();
        match name {
            "font-family" => {
                if let Some(family) = self.family(&css::families(value)) {
                    style.font.family = family;
                }
            }
            "font-weight" => {
                style.font.bold = match lower.as_str() {
                    "bold" | "bolder" => true,
                    "normal" | "lighter" => false,
                    n => n.parse::<u16>().map_or(style.font.bold, |w| w >= 600),
                };
            }
            "font-style" => style.font.italic = lower.starts_with("italic") || lower.starts_with("oblique"),
            "color" => {
                if let Some(Paint::Rgb(c)) = css::colour(value) {
                    style.font.color = c;
                }
            }
            "text-decoration" | "text-decoration-line" => {
                if lower.split_whitespace().any(|t| t == "none") {
                    style.font.underline = false;
                    style.font.strike = false;
                }
                if lower.contains("underline") {
                    style.font.underline = true;
                }
                if lower.contains("line-through") {
                    style.font.strike = true;
                }
            }
            "vertical-align" => match lower.as_str() {
                "super" => style.font.rise = parent.font.rise + parent.font.size * 0.33,
                "sub" => style.font.rise = parent.font.rise - parent.font.size * 0.2,
                "top" | "text-top" => style.valign = Some(VAlign::Top),
                "middle" => style.valign = Some(VAlign::Center),
                "bottom" | "text-bottom" => style.valign = Some(VAlign::Bottom),
                other => {
                    if let Some(shift) = length(other, Some(em)) {
                        style.font.rise = parent.font.rise + shift;
                    }
                }
            },
            "text-align" => {
                style.align = match lower.as_str() {
                    "right" | "end" | "-webkit-right" => Align::Right,
                    "center" | "-webkit-center" | "-moz-center" => Align::Center,
                    "justify" => Align::Justify,
                    "left" | "start" | "-webkit-left" => Align::Left,
                    _ => style.align,
                };
            }
            "line-height" => {
                if let Some(lh) = line_height(value, em) {
                    style.line_height = lh;
                }
            }
            "white-space" => {
                style.white_space = match lower.as_str() {
                    "pre" | "pre-wrap" | "break-spaces" => WhiteSpace::Pre,
                    "pre-line" => WhiteSpace::PreLine,
                    _ => WhiteSpace::Normal,
                };
            }
            "list-style-type" | "list-style" => {
                if let Some(kind) = lower.split_whitespace().find(|t| is_list_type(t)) {
                    kind.clone_into(&mut style.list_type);
                }
            }
            "text-indent" => {
                if let Some(indent) = length(value, Some(width)) {
                    style.text_indent = indent;
                }
            }
            "text-transform" => {
                style.transform = match lower.as_str() {
                    "uppercase" => Transform::Upper,
                    "lowercase" => Transform::Lower,
                    "capitalize" => Transform::Capitalize,
                    _ => Transform::None,
                };
            }
            "display" => {
                style.display = match lower.split_whitespace().next().unwrap_or("") {
                    "none" => Display::None,
                    "list-item" => Display::ListItem,
                    "table" | "inline-table" => Display::Table,
                    "inline" | "inline-block" | "contents" | "ruby" => Display::Inline,
                    "flex" | "inline-flex" | "grid" | "inline-grid" => {
                        self.warn(LAYOUT);
                        Display::Block
                    }
                    _ => Display::Block,
                };
            }
            "visibility" if matches!(lower.as_str(), "hidden" | "collapse") => style.display = Display::None,
            "float" if matches!(lower.as_str(), "left" | "right" | "inline-start" | "inline-end") => self.warn(LAYOUT),
            "position" if matches!(lower.as_str(), "absolute" | "fixed") => self.warn(LAYOUT),
            "columns" | "column-count" if lower.split_whitespace().any(|t| t.parse::<u32>().is_ok_and(|n| n > 1)) => self.warn(LAYOUT),
            "margin" | "padding" => {
                if let Some(values) = css::sides(value) {
                    for (i, v) in values.iter().enumerate() {
                        set_box(style, name == "margin", i, v, em, width);
                    }
                }
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" | "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                if let Some(i) = side(name) {
                    set_box(style, name.starts_with("margin"), i, value, em, width);
                }
            }
            "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
                let parts = css::border(value, em, ROOT_SIZE);
                let sides: Vec<usize> = side(name).map_or_else(|| (0..4).collect(), |i| vec![i]);
                for i in sides {
                    style.border_width[i] = parts.width.unwrap_or(MEDIUM);
                    style.border_shown[i] = parts.style.as_deref().is_some_and(|s| s != "none" && s != "hidden");
                    style.border_colour[i] = None;
                    match parts.colour {
                        Some(Paint::Rgb(c)) => style.border_colour[i] = Some(c),
                        Some(Paint::Transparent) => style.border_shown[i] = false,
                        Some(Paint::Current) | None => {}
                    }
                }
            }
            "border-width" | "border-style" | "border-color" => {
                if let Some(values) = css::sides(value) {
                    for (i, v) in values.iter().enumerate() {
                        set_border_part(style, name, i, v, em);
                    }
                }
            }
            _ if name.starts_with("border-") && (name.ends_with("-width") || name.ends_with("-style") || name.ends_with("-color")) => {
                let part = &name[name.rfind('-').unwrap_or(0)..];
                if let Some(i) = side(&name[..name.len() - part.len()]) {
                    set_border_part(style, &format!("border{part}"), i, value, em);
                }
            }
            "background-color" | "background" => {
                if lower == "none" {
                    style.background = None;
                }
                for token in css::split_top(value, ' ') {
                    match css::colour(token) {
                        Some(Paint::Rgb(c)) => style.background = Some(c),
                        Some(Paint::Transparent) => style.background = None,
                        _ => {}
                    }
                }
            }
            "width" => {
                style.width = if lower == "auto" {
                    None
                } else if let Some(percent) = value.strip_suffix('%') {
                    percent.trim().parse::<f32>().ok().filter(|p| *p > 0.0).map(Size::Percent)
                } else {
                    length(value, None).filter(|w| *w > 0.0).map(Size::Points)
                };
            }
            "height" => style.height = length(value, None).filter(|h| *h > 0.0),
            "page-break-before" | "break-before" => style.break_before = matches!(lower.as_str(), "always" | "page" | "left" | "right" | "recto" | "verso"),
            "page-break-after" | "break-after" => style.break_after = matches!(lower.as_str(), "always" | "page" | "left" | "right" | "recto" | "verso"),
            _ => {}
        }
    }

    // --- resources ------------------------------------------------------------

    /// A local or `data:` resource's bytes, or `None` with the reason
    /// recorded. Never fetches from the web.
    fn resource(&mut self, src: &str, limit: u64) -> Option<(Vec<u8>, String)> {
        let src = src.trim();
        if src.is_empty() {
            return None;
        }
        if src.len() > 5 && src[..5].eq_ignore_ascii_case("data:") {
            return self.data_uri(&src[5..], limit);
        }
        let path = match scheme(src).as_deref() {
            Some("file") => {
                let rest = src[5..].trim_start_matches('/');
                let rest = rest.strip_prefix("localhost/").unwrap_or(rest);
                let decoded = percent_decode(rest);
                // `file:///C:/x` on Windows; `file:///Users/x` elsewhere.
                if cfg!(windows) { PathBuf::from(decoded) } else { PathBuf::from(format!("/{decoded}")) }
            }
            Some(_) => {
                self.warn(REMOTE);
                return None;
            }
            None if src.starts_with("//") => {
                self.warn(REMOTE);
                return None;
            }
            None => {
                let local = src.split(['#', '?']).next().unwrap_or(src);
                let decoded = PathBuf::from(percent_decode(local));
                match &self.folder {
                    _ if decoded.is_absolute() => decoded,
                    Some(folder) => folder.join(decoded),
                    None => {
                        self.warn(OUTSIDE);
                        return None;
                    }
                }
            }
        };
        let label = path.file_name().map_or_else(|| src.to_owned(), |n| n.to_string_lossy().into_owned());
        let Ok(found) = path.canonicalize() else {
            self.notes.push(format!("{}: {label} wasn't found, so it was left out.", self.name));
            return None;
        };
        if !self.folder.as_ref().is_some_and(|folder| found.starts_with(folder)) {
            self.warn(OUTSIDE);
            return None;
        }
        if !std::fs::metadata(&found).is_ok_and(|m| m.is_file() && m.len() <= limit) {
            self.notes.push(format!("{}: {label} is too large or not a file, so it was left out.", self.name));
            return None;
        }
        match std::fs::read(&found) {
            Ok(bytes) => Some((bytes, label)),
            Err(e) => {
                self.notes.push(format!("{}: {label} could not be read ({e}), so it was left out.", self.name));
                None
            }
        }
    }

    /// `data:[type][;base64],data`.
    fn data_uri(&mut self, rest: &str, limit: u64) -> Option<(Vec<u8>, String)> {
        let (header, data) = rest.split_once(',')?;
        if u64::try_from(data.len()).unwrap_or(u64::MAX) > limit.saturating_mul(4) / 3 + 4 {
            self.notes.push(format!("{}: an embedded image is too large, so it was left out.", self.name));
            return None;
        }
        let bytes = if header.to_ascii_lowercase().ends_with(";base64") {
            let compact: String = data.chars().filter(|c| !c.is_whitespace()).collect();
            let compact = percent_decode(&compact);
            let engine = &base64::engine::general_purpose::STANDARD;
            engine.decode(&compact).or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(compact.trim_end_matches('='))).ok()
        } else {
            Some(percent_decode(data).into_bytes())
        };
        if bytes.is_none() {
            self.notes.push(format!("{}: an embedded image's data is damaged, so it was left out.", self.name));
        }
        bytes.map(|b| (b, "an embedded image".to_owned()))
    }

    // --- inline content ---------------------------------------------------------

    fn push(&mut self, text: &str, style: &RunStyle) {
        if text.is_empty() {
            return;
        }
        match self.line.last_mut() {
            Some(last) if last.style == *style => last.text.push_str(text),
            _ => self.line.push(Run { text: text.to_owned(), style: style.clone() }),
        }
    }

    /// A text node's text, with whitespace treated as `white-space` says.
    fn text(&mut self, text: &str, style: &Style) {
        let text: String = text.chars().filter(|c| !matches!(c, '\u{AD}' | '\u{200B}')).collect();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let collapsed = match style.white_space {
            WhiteSpace::Pre => text,
            WhiteSpace::PreLine | WhiteSpace::Normal => {
                let keep_breaks = style.white_space == WhiteSpace::PreLine;
                let mut out = String::with_capacity(text.len());
                let mut after_space = self.line.last().is_none_or(|r| r.text.ends_with([' ', '\n']));
                for c in text.chars() {
                    if c == '\n' && keep_breaks {
                        while out.ends_with(' ') {
                            out.pop();
                        }
                        out.push('\n');
                        after_space = true;
                    } else if c.is_ascii_whitespace() {
                        if !after_space {
                            out.push(' ');
                            after_space = true;
                        }
                    } else {
                        out.push(c);
                        after_space = false;
                    }
                }
                out
            }
        };
        let transformed = match style.transform {
            Transform::None => collapsed,
            Transform::Upper => collapsed.to_uppercase(),
            Transform::Lower => collapsed.to_lowercase(),
            Transform::Capitalize => {
                let mut start = self.line.last().is_none_or(|r| r.text.ends_with(char::is_whitespace));
                collapsed
                    .chars()
                    .flat_map(|c| {
                        let up = start && c.is_alphabetic();
                        start = c.is_whitespace();
                        if up { c.to_uppercase().collect::<Vec<_>>() } else { vec![c] }
                    })
                    .collect()
            }
        };
        self.push(&transformed, &style.font);
    }

    /// Owed margin as space, before a block that isn't a paragraph.
    fn space(&mut self, margin: f32) {
        let gap = self.pending.max(margin);
        self.pending = 0.0;
        if gap > 0.0 {
            self.blocks.push(FlowBlock::Gap(gap));
        }
    }

    /// The collected inline content as a paragraph of `ctx`'s block.
    fn flush(&mut self, ctx: &Context) {
        let mut runs = std::mem::take(&mut self.line);
        if ctx.style.white_space != WhiteSpace::Pre {
            // Spaces at the start and end of a line aren't drawn.
            for run in &mut runs {
                while run.text.contains(" \n") {
                    run.text = run.text.replace(" \n", "\n");
                }
                while run.text.contains("\n ") {
                    run.text = run.text.replace("\n ", "\n");
                }
            }
            if let Some(first) = runs.first_mut() {
                first.text = first.text.trim_start_matches(' ').to_owned();
            }
            while let Some(last) = runs.last_mut() {
                last.text = last.text.trim_end_matches(' ').to_owned();
                if last.text.is_empty() {
                    runs.pop();
                } else {
                    break;
                }
            }
        }
        // A line break at the very end of a block makes no line of its own.
        let had_break = runs.iter().any(|r| r.text.contains('\n'));
        if let Some(last) = runs.last_mut() {
            if last.text.ends_with('\n') {
                last.text.pop();
            }
        }
        runs.retain(|r| !r.text.is_empty());
        if runs.is_empty() && !had_break {
            return;
        }
        let mut paragraph = Paragraph::new(ctx.style.font.clone());
        paragraph.runs = runs;
        paragraph.align = ctx.style.align;
        (paragraph.indent_left, paragraph.indent_right) = self.indents(ctx.left, ctx.right);
        paragraph.first_line = ctx.style.text_indent;
        paragraph.line = ctx.style.line();
        paragraph.space_before = std::mem::take(&mut self.pending);
        if let Some(label) = self.label.take() {
            // The marker hangs left of the text, as a browser's outside marker does.
            paragraph.first_line = -(label.text.chars().count() as f32 * 0.55 + 0.5) * label.style.size;
            paragraph.label = Some(label);
        }
        self.blocks.push(FlowBlock::Paragraph(paragraph));
    }

    /// Indents that leave at least an inch for the text. Deeply nested lists
    /// and quotes indent past the page, where a browser lets the text run off
    /// its edge; on paper it would be lost, so it is kept in.
    fn indents(&self, left: f32, right: f32) -> (f32, f32) {
        let spare = (self.text_width - 72.0).max(0.0);
        let left = left.clamp(0.0, spare);
        (left, right.clamp(0.0, spare - left))
    }

    // --- the walk ----------------------------------------------------------------

    fn node(&mut self, node: &NodeRef, parent: &Style, ctx: &Context) {
        if node.is_text() {
            self.text(&node.text(), parent);
            return;
        }
        if !node.is_element() {
            return;
        }
        let tag = tag(node);
        match tag.as_str() {
            "head" | "script" | "style" | "template" | "title" | "meta" | "link" | "base" | "noembed" | "param" | "source" | "track" | "area"
            | "map" | "datalist" | "option" | "optgroup" => return,
            "svg" | "canvas" | "video" | "audio" | "iframe" | "object" | "embed" | "math" | "frame" | "frameset" | "applet" | "picture" => {
                // A <picture> holds an <img> fallback; everything else here is drawn by the browser.
                if tag == "picture" {
                    if let Some(img) = node.children().into_iter().find(|c| self::tag(c) == "img") {
                        self.node(&img, parent, ctx);
                    }
                } else {
                    self.warn(EMBEDDED);
                }
                return;
            }
            "input" | "select" | "textarea" => {
                self.warn(FORMS);
                return;
            }
            _ => {}
        }
        let style = self.style_of(node, &tag, parent);
        if style.display == Display::None {
            return;
        }
        if self.depth >= MAX_DEPTH {
            self.warn(DEPTH);
            self.text(&flat_text(node), &style);
            return;
        }
        self.depth += 1;
        match tag.as_str() {
            "br" => self.push("\n", &style.font),
            "img" => self.image(node, &style, ctx),
            "hr" => self.rule(&style, ctx),
            _ => match style.display {
                Display::Table => self.table(node, &style, ctx),
                Display::Block | Display::ListItem => self.block(node, &tag, &style, ctx),
                Display::Inline | Display::None => {
                    for child in node.children() {
                        self.node(&child, &style, ctx);
                    }
                }
            },
        }
        self.depth -= 1;
    }

    fn block(&mut self, node: &NodeRef, tag: &str, style: &Style, ctx: &Context) {
        self.flush(ctx);
        if style.break_before {
            self.blocks.push(FlowBlock::PageBreak);
            self.pending = 0.0;
        }
        let edge = |i: usize| style.margin[i] + style.padding[i] + style.border(i).map_or(0.0, |b| b.width);
        self.pending = self.pending.max(style.margin[0]) + style.padding[0];
        let inner = Context { style: style.clone(), left: ctx.left + edge(3), right: ctx.right + edge(1) };

        let list = matches!(tag, "ol" | "ul" | "menu" | "dir");
        if list {
            let start = if tag == "ol" { attr(node, "start").and_then(|s| s.parse::<i64>().ok()).unwrap_or(1) } else { 1 };
            self.counters.push(u32::try_from(start.saturating_sub(1).max(0)).unwrap_or(0));
        }
        if style.display == Display::ListItem {
            let value = match (attr(node, "value").and_then(|v| v.parse::<u32>().ok()), self.counters.last_mut()) {
                (Some(v), Some(counter)) => {
                    *counter = v;
                    v
                }
                (None, Some(counter)) => {
                    *counter += 1;
                    *counter
                }
                (Some(v), None) => v,
                (None, None) => 1,
            };
            let mut font = style.font.clone();
            font.underline = false;
            font.strike = false;
            self.label = marker(&style.list_type, value).map(|text| Run { text, style: font });
        }
        for child in node.children() {
            self.node(&child, style, &inner);
        }
        self.flush(&inner);
        // A marker with nothing after it stays with the item it belongs to.
        if style.display == Display::ListItem {
            self.label = None;
        }
        if list {
            self.counters.pop();
        }
        self.pending = self.pending.max(style.margin[2]) + style.padding[2];
        if style.break_after {
            self.blocks.push(FlowBlock::PageBreak);
            self.pending = 0.0;
        }
    }

    fn rule(&mut self, style: &Style, ctx: &Context) {
        self.flush(ctx);
        self.space(style.margin[0]);
        let borders = style.border(0).map_or(0.0, |b| b.width) + style.border(2).map_or(0.0, |b| b.width);
        let thickness = style.height.unwrap_or(borders).max(0.5);
        let color = style.background.or_else(|| style.border(0).map(|b| b.color)).unwrap_or(style.font.color);
        let (left, right) = self.indents(ctx.left + style.margin[3], ctx.right + style.margin[1]);
        let available = (self.text_width - left - right).max(0.0);
        // A rule narrower than the line is centred, as browsers centre <hr>.
        let width = match style.width {
            Some(Size::Points(w)) => w.min(available),
            Some(Size::Percent(p)) => available * p.min(100.0) / 100.0,
            None => available,
        };
        let inset = (available - width) / 2.0;
        self.blocks.push(FlowBlock::Rule { thickness, color, indent_left: left + inset, indent_right: right + inset });
        self.pending = style.margin[2];
    }

    fn image(&mut self, node: &NodeRef, style: &Style, ctx: &Context) {
        let alt = attr(node, "alt");
        let fetched = attr(node, "src").and_then(|src| self.resource(&src, IMAGE_LIMIT));
        let usable = fetched.and_then(|(bytes, label)| {
            let format = image::guess_format(&bytes).ok();
            if !matches!(format, Some(image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::Tiff | image::ImageFormat::WebP)) {
                self.warn(IMAGE_FORMAT);
                return None;
            }
            let size = pixel_size(&bytes, format?);
            if size.is_none() {
                self.notes.push(format!("{}: {label} couldn't be read, so it was left out.", self.name));
            }
            size.map(|(w, h)| (bytes, label, w, h))
        });
        let Some((bytes, label, px_w, px_h)) = usable else {
            // As a browser does, show what the image was of.
            if let Some(alt) = alt {
                self.text(&alt, style);
            }
            return;
        };
        let (natural_w, natural_h) = (px_w as f32 * PX, px_h as f32 * PX);
        let available = (self.text_width - ctx.left - ctx.right).max(1.0);
        let width = match style.width {
            Some(Size::Points(w)) => Some(w),
            Some(Size::Percent(p)) => Some(available * p / 100.0),
            None => None,
        };
        let (width, height) = match (width, style.height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * natural_h / natural_w.max(0.01)),
            (None, Some(h)) => (h * natural_w / natural_h.max(0.01), h),
            (None, None) => (natural_w, natural_h),
        };
        self.flush(ctx);
        self.space(0.0);
        self.blocks.push(FlowBlock::Image(FlowImage { bytes, name: label, width, height, align: ctx.style.align }));
    }

    // --- tables ------------------------------------------------------------------

    /// A cell's content as blocks of its own.
    fn cell_blocks(&mut self, cell: &NodeRef, style: &Style) -> Vec<FlowBlock> {
        let blocks = std::mem::take(&mut self.blocks);
        let line = std::mem::take(&mut self.line);
        let pending = std::mem::take(&mut self.pending);
        let label = self.label.take();
        let ctx = Context { style: style.clone(), left: 0.0, right: 0.0 };
        for child in cell.children() {
            self.node(&child, style, &ctx);
        }
        self.flush(&ctx);
        let content = std::mem::replace(&mut self.blocks, blocks);
        self.line = line;
        self.pending = pending;
        self.label = label;
        content.into_iter().filter(|b| !matches!(b, FlowBlock::PageBreak)).collect()
    }

    #[allow(clippy::too_many_lines)]
    fn table(&mut self, node: &NodeRef, style: &Style, ctx: &Context) {
        self.flush(ctx);
        if style.break_before {
            self.blocks.push(FlowBlock::PageBreak);
            self.pending = 0.0;
        }
        let children: Vec<NodeRef> = node.children().into_iter().filter(NodeRef::is_element).collect();
        for caption in children.iter().filter(|c| tag(c) == "caption") {
            self.node(caption, style, ctx);
        }
        self.space(style.margin[0]);
        let available = (self.text_width - ctx.left - ctx.right).max(1.0);

        // Rows in display order: the head, the bodies, then the foot.
        let mut head = Vec::new();
        let mut body = Vec::new();
        let mut foot = Vec::new();
        let mut declared: Vec<f32> = Vec::new();
        for child in &children {
            let child_tag = tag(child);
            match child_tag.as_str() {
                "thead" | "tbody" | "tfoot" => {
                    let group = self.style_of(child, &child_tag, style);
                    if group.display == Display::None {
                        continue;
                    }
                    let rows = child.children().into_iter().filter(|r| tag(r) == "tr").map(|r| (r, group.clone()));
                    match child_tag.as_str() {
                        "thead" => head.extend(rows),
                        "tfoot" => foot.extend(rows),
                        _ => body.extend(rows),
                    }
                }
                "tr" => body.push((*child, style.clone())),
                "colgroup" | "col" => {
                    let cols: Vec<NodeRef> = if child_tag == "col" { vec![*child] } else { child.children().into_iter().filter(|c| tag(c) == "col").collect() };
                    let cols = if cols.is_empty() { vec![*child] } else { cols };
                    for col in cols {
                        let col_style = self.style_of(&col, "col", style);
                        let width = match col_style.width {
                            Some(Size::Points(w)) => w,
                            Some(Size::Percent(p)) => available * p / 100.0,
                            None => 0.0,
                        };
                        let span = attr(&col, "span").and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).clamp(1, 1000);
                        declared.extend(std::iter::repeat_n(width, span));
                    }
                }
                _ => {}
            }
        }
        let headers = head.len();
        let rows: Vec<(NodeRef, Style)> = head.into_iter().chain(body).chain(foot).collect();
        let row_count = rows.len();

        let mut covered: Vec<Vec<Option<Cover>>> = vec![Vec::new(); row_count];
        let mut flow_rows = Vec::with_capacity(row_count);
        for (r, (row, group)) in rows.iter().enumerate() {
            let row_style = self.style_of(row, "tr", group);
            if row_style.display == Display::None {
                continue;
            }
            let mut cells = Vec::new();
            let mut column = 0;
            let mut min_height = row_style.height.unwrap_or(0.0);
            let skip_covered = |cells: &mut Vec<FlowCell>, column: &mut usize, covered: &[Vec<Option<Cover>>]| {
                while let Some(Some(cover)) = covered[r].get(*column) {
                    if cover.start == *column {
                        cells.push(FlowCell {
                            blocks: Vec::new(),
                            span: cover.span,
                            merge: Merge::Continue,
                            fill: cover.fill,
                            borders: cover.borders,
                            valign: VAlign::Top,
                            margins: cover.margins,
                        });
                        *column += cover.span;
                    } else {
                        *column += 1;
                    }
                }
            };
            for cell in row.children().into_iter().filter(|c| matches!(tag(c).as_str(), "td" | "th")) {
                skip_covered(&mut cells, &mut column, &covered);
                let cell_tag = tag(&cell);
                let cell_style = self.style_of(&cell, &cell_tag, &row_style);
                if cell_style.display == Display::None {
                    continue;
                }
                let colspan = attr(&cell, "colspan").and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).clamp(1, 1000);
                let rowspan = match attr(&cell, "rowspan").and_then(|s| s.parse::<usize>().ok()) {
                    // 0: to the end of the table.
                    Some(0) => row_count - r,
                    Some(n) => n.clamp(1, row_count - r),
                    None => 1,
                };
                let blocks = self.cell_blocks(&cell, &cell_style);
                let borders = Borders { top: cell_style.border(0), right: cell_style.border(1), bottom: cell_style.border(2), left: cell_style.border(3) };
                let fill = cell_style.background.or(row_style.background).or(group.background).or(style.background);
                // CSS puts the content inside the border, so the border's
                // width is part of the margin around it.
                let margins: [f32; 4] = std::array::from_fn(|i| cell_style.padding[i] + cell_style.border(i).map_or(0.0, |b| b.width));
                if colspan == 1 {
                    // A length is the content box's (CSS's default); padding
                    // and borders come on top. A percentage is the column's.
                    let sides = |a: usize, b: usize| {
                        cell_style.padding[a] + cell_style.padding[b] + cell_style.border(a).map_or(0.0, |x| x.width) + cell_style.border(b).map_or(0.0, |x| x.width)
                    };
                    let width = match cell_style.width {
                        Some(Size::Points(w)) => w + sides(1, 3),
                        Some(Size::Percent(p)) => available * p / 100.0,
                        None => 0.0,
                    };
                    if declared.len() <= column {
                        declared.resize(column + 1, 0.0);
                    }
                    declared[column] = declared[column].max(width);
                }
                if rowspan == 1 {
                    min_height = min_height.max(cell_style.height.map_or(0.0, |h| h + cell_style.padding[0] + cell_style.padding[2]));
                }
                for below in covered.iter_mut().take(r + rowspan).skip(r + 1) {
                    if below.len() < column + colspan {
                        below.resize(column + colspan, None);
                    }
                    for slot in &mut below[column..column + colspan] {
                        *slot = Some(Cover { start: column, span: colspan, borders, margins, fill });
                    }
                }
                cells.push(FlowCell {
                    blocks,
                    span: colspan,
                    merge: if rowspan > 1 { Merge::Start } else { Merge::None },
                    fill,
                    borders,
                    valign: cell_style.valign.or(row_style.valign).or(group.valign).unwrap_or(VAlign::Center),
                    margins,
                });
                column += colspan;
            }
            skip_covered(&mut cells, &mut column, &covered);
            flow_rows.push(FlowRow { cells, min_height, exact: false, header: r < headers });
        }

        // The table's own border, on its outer cells' outer edges where they have none.
        let grid = flow_rows.iter().map(|row| row.cells.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(0);
        let last_row = flow_rows.len().saturating_sub(1);
        for (r, row) in flow_rows.iter_mut().enumerate() {
            let mut column = 0;
            for cell in &mut row.cells {
                let borders = &mut cell.borders;
                if r == 0 && borders.top.is_none() {
                    borders.top = style.border(0);
                }
                if column + cell.span >= grid && borders.right.is_none() {
                    borders.right = style.border(1);
                }
                if r == last_row && borders.bottom.is_none() {
                    borders.bottom = style.border(2);
                }
                if column == 0 && borders.left.is_none() {
                    borders.left = style.border(3);
                }
                column += cell.span;
            }
        }
        if declared.len() > grid {
            declared.truncate(grid.max(1));
        }

        let width = match style.width {
            Some(Size::Points(w)) => TableWidth::Points(w),
            Some(Size::Percent(p)) => TableWidth::Fraction(p / 100.0),
            None => TableWidth::Grid,
        };
        let align = match (style.margin_auto[3], style.margin_auto[1]) {
            (true, true) => Align::Center,
            (true, false) => Align::Right,
            _ => Align::Left,
        };
        if !flow_rows.is_empty() {
            self.blocks.push(FlowBlock::Table(FlowTable {
                columns: declared,
                auto: true,
                width,
                rows: flow_rows,
                indent: self.indents(ctx.left + style.margin[3], ctx.right).0,
                align,
            }));
        }
        self.pending = style.margin[2];
        if style.break_after {
            self.blocks.push(FlowBlock::PageBreak);
            self.pending = 0.0;
        }
    }

    /// A linked stylesheet's text, when it's local.
    fn stylesheet(&mut self, href: &str) -> Option<String> {
        self.resource(href, STYLESHEET_LIMIT).map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// All the text under `node`, with a space where a block or a line break
/// starts so its words don't run together. Iterative, as the tree under it
/// may be arbitrarily deep.
fn flat_text(node: &NodeRef) -> String {
    const BREAKS: [&str; 20] =
        ["p", "div", "br", "li", "tr", "td", "th", "table", "ul", "ol", "dl", "dt", "dd", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote"];
    let mut out = String::new();
    for descendant in node.descendants_it() {
        if descendant.is_text() {
            // A script's or a stylesheet's text is its element's only child.
            if !descendant.parent().is_some_and(|p| matches!(tag(&p).as_str(), "script" | "style" | "title")) {
                out.push_str(&descendant.text());
            }
        } else if descendant.is_element() && BREAKS.contains(&tag(&descendant).as_str()) {
            out.push(' ');
        }
    }
    out
}

/// `line-height`: `normal`, a factor, or a length.
fn line_height(value: &str, em: f32) -> Option<LineHeight> {
    if value.trim().eq_ignore_ascii_case("normal") {
        return Some(LineHeight::Normal);
    }
    if let Some(factor) = css::plain_number(value) {
        return (factor > 0.0).then_some(LineHeight::Factor(factor));
    }
    css::length(value, em, ROOT_SIZE, Some(em)).filter(|h| *h > 0.0).map(LineHeight::Points)
}

/// One side of `margin` (with `auto`) or `padding` (never negative).
fn set_box(style: &mut Style, margin: bool, side: usize, value: &str, em: f32, width: f32) {
    if margin && value.trim().eq_ignore_ascii_case("auto") {
        style.margin_auto[side] = true;
        style.margin[side] = 0.0;
        return;
    }
    let Some(points) = css::length(value, em, ROOT_SIZE, Some(width)) else { return };
    if margin {
        style.margin_auto[side] = false;
        style.margin[side] = points;
    } else {
        style.padding[side] = points.max(0.0);
    }
}

/// `border-width` / `-style` / `-color` for one side.
fn set_border_part(style: &mut Style, name: &str, side: usize, value: &str, em: f32) {
    match name {
        "border-width" => {
            if let Some(width) = css::border_width(value, em, ROOT_SIZE) {
                style.border_width[side] = width;
            }
        }
        "border-style" if css::is_border_style(value) => {
            let lower = value.trim().to_ascii_lowercase();
            style.border_shown[side] = lower != "none" && lower != "hidden";
        }
        "border-color" => match css::colour(value) {
            Some(Paint::Rgb(c)) => style.border_colour[side] = Some(c),
            Some(Paint::Current) => style.border_colour[side] = None,
            Some(Paint::Transparent) => style.border_shown[side] = false,
            None => {}
        },
        _ => {}
    }
}

/// SPEC: P7-OCR-009 — read an HTML page into the flow model. `folder` is the
/// page's own folder, the only place local images and stylesheets are read
/// from. Returns the document and what was simplified, each warning once.
///
/// # Errors
/// When the file is binary rather than text.
pub fn read_html(name: &str, bytes: &[u8], folder: Option<&Path>, page: PageSetup) -> Result<(FlowDocument, Vec<String>), CommandError> {
    let mut warnings = Vec::new();
    let text = decode(name, bytes, &mut warnings);
    if text.chars().take(4096).any(|c| c == '\0') {
        return Err(CommandError::InvalidInput(format!("{name} isn't an HTML page that can be read.")));
    }
    let limited = limit_nesting(&text, PARSE_DEPTH);
    let doc = Document::from(limited.as_deref().unwrap_or(&text));
    let mut reader = Reader {
        name,
        folder: folder.and_then(|f| f.canonicalize().ok()),
        matched: HashMap::new(),
        text_width: (page.width - page.left - page.right).max(1.0),
        blocks: Vec::new(),
        line: Vec::new(),
        pending: 0.0,
        label: None,
        counters: Vec::new(),
        families: HashMap::new(),
        depth: 0,
        warnings: BTreeSet::new(),
        notes: Vec::new(),
    };

    // The browser's defaults, then the page's stylesheets in document order.
    let mut sheets = vec![(0u8, css::parse_stylesheet(DEFAULT_CSS))];
    for node in doc.select("style, link").nodes() {
        if tag(node) == "style" {
            sheets.push((2, css::parse_stylesheet(&node.text())));
        } else if attr(node, "rel").is_some_and(|rel| {
            let rel = rel.to_ascii_lowercase();
            rel.split_whitespace().any(|r| r == "stylesheet") && !rel.split_whitespace().any(|r| r == "alternate")
        }) {
            if let Some(text) = attr(node, "href").and_then(|href| reader.stylesheet(&href)) {
                sheets.push((2, css::parse_stylesheet(&text)));
            }
        }
    }
    reader.matched = cascade(&doc, &sheets);
    if limited.is_some() {
        reader.warn(DEPTH);
    }

    let family = reader.family(&["serif".to_owned()]).unwrap_or_else(|| "Times New Roman".to_owned());
    let root = Style::root(family);
    let ctx = Context { style: root.clone(), left: 0.0, right: 0.0 };
    for child in doc.root().children() {
        reader.node(&child, &root, &ctx);
    }
    reader.flush(&ctx);

    warnings.extend(reader.warnings.iter().map(|w| format!("{name}: {w}")));
    warnings.extend(reader.notes);
    Ok((FlowDocument { page, blocks: reader.blocks }, warnings))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_told_apart_from_paths() {
        assert_eq!(scheme("https://example.com/a.png").as_deref(), Some("https"));
        assert_eq!(scheme("file:///tmp/a.png").as_deref(), Some("file"));
        assert_eq!(scheme("C:\\pics\\a.png"), None);
        assert_eq!(scheme("pics/a:b.png"), None);
        assert_eq!(scheme("a.png"), None);
        assert_eq!(percent_decode("My%20report_files/x%2Fy.png"), "My report_files/x/y.png");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn nesting_past_the_limit_is_cut_and_text_kept() {
        let deep = format!("{}Deep{}", "<div class=\"x\">".repeat(30), "</div>".repeat(30));
        let cut = limit_nesting(&deep, 10).expect("cut");
        assert_eq!(cut.matches("<div").count(), 10);
        assert_eq!(cut.matches("</div>").count(), 10);
        assert!(cut.contains("Deep"));
        // Paragraphs and list items close themselves; scripts hold text; none of it is nesting.
        let flat = format!("{}<script>{}</script><!-- <div> -->a < b <br/>", "<p>x<li>y".repeat(50), "<div>".repeat(50));
        assert_eq!(limit_nesting(&flat, 10), None);
        assert_eq!(limit_nesting("<DIV><Div>t</div></DIV>", 1).as_deref(), Some("<DIV>t</DIV>"));
    }

    #[test]
    fn a_png_size_comes_from_its_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&40u32.to_be_bytes());
        png.extend_from_slice(&20u32.to_be_bytes());
        assert_eq!(pixel_size(&png, image::ImageFormat::Png), Some((40, 20)));
        assert_eq!(pixel_size(&png[..20], image::ImageFormat::Png), None);
    }

    #[test]
    fn markers_follow_the_list_type() {
        assert_eq!(marker("disc", 3).as_deref(), Some("\u{2022}"));
        assert_eq!(marker("decimal", 3).as_deref(), Some("3."));
        assert_eq!(marker("lower-alpha", 28).as_deref(), Some("bb."));
        assert_eq!(marker("upper-roman", 14).as_deref(), Some("XIV."));
        assert_eq!(marker("lower-greek", 2).as_deref(), Some("2."));
        assert_eq!(marker("none", 1), None);
    }

    #[test]
    fn a_declared_charset_is_honoured() {
        let mut warnings = Vec::new();
        let latin = b"<meta charset=\"windows-1252\"><p>caf\xE9 \x80</p>";
        assert!(decode("t.html", latin, &mut warnings).contains("caf\u{e9} \u{20ac}"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let unknown = b"<meta http-equiv=Content-Type content='text/html; charset=shift_jis'><p>\x82\xA0</p>";
        decode("t.html", unknown, &mut warnings);
        assert!(warnings.iter().any(|w| w.contains("shift_jis")), "{warnings:?}");
    }
}

