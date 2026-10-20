//! SPEC: P7-OCR-009 (P7.C1b1) — flowed text: paragraphs of styled runs laid out
//! onto pages, for documents that arrive as structure rather than as pages
//! (Word now, HTML next).
//!
//! **Two passes.** Layout measures every piece of text with the face that will
//! draw it and decides where it goes; only then are the fonts built and
//! embedded, each subset to exactly the characters it drew. Widths come from
//! the same `hmtx` advances the embedded font's `/W` array is written from,
//! so a line measured here is the line a reader draws.
//!
//! **Faces are real.** A bold run is drawn with the family's bold face, found
//! by name in [`FontIndex`]; a missing family is replaced by an installed one
//! of the same class, and the substitution is reported. A character the chosen
//! face lacks is drawn with the broad-coverage face; one no face has is
//! reported by name — never left as an empty box without a word.
//!
//! **Not Word's layout.** Line breaks follow from real glyph widths but not
//! Word's kerning, hyphenation or exact leading, so they can fall a word
//! earlier or later. Text, order, fonts and formatting are what is kept.

// Font units and point sizes are f32 layout values; the casts from glyph
// counts and font metrics are bounded far below f32's exact-integer range.
#![allow(clippy::cast_precision_loss)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use lopdf::{Dictionary, Object, ObjectId};

use crate::error::CommandError;
use crate::pdf::create::{converted_note, embed_image, num, placement, Builder};
use crate::pdf::font_embed_cid::{build_cid_font_from, CidFont};
use crate::pdf::font_index::{class_of, fallback_families, FontIndex};
use crate::pdf::font_resolver::broad_coverage_font_paths;

/// Page size and margins, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Default for PageSetup {
    /// US Letter with one-inch margins — Word's own default.
    fn default() -> Self {
        Self { width: 612.0, height: 792.0, top: 72.0, right: 72.0, bottom: 72.0, left: 72.0 }
    }
}

// Four independent formatting flags, as Word stores them; folding them into
// an enum or bitset would only obscure that.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// Points.
    pub size: f32,
    pub color: [u8; 3],
    /// Baseline shift in points: up for superscript, down for subscript.
    pub rise: f32,
}

impl Default for RunStyle {
    /// Word's built-in default when a document declares none.
    fn default() -> Self {
        Self {
            family: "Times New Roman".into(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            size: 10.0,
            color: [0, 0, 0],
            rise: 0.0,
        }
    }
}

/// Text in one style. `\t` is a tab and `\n` a line break within the paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineSpacing {
    /// A multiple of the font's own line height (1.0 = Word's "single").
    Multiple(f32),
    Exactly(f32),
    AtLeast(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    pub align: Align,
    /// From the left margin, points.
    pub indent_left: f32,
    pub indent_right: f32,
    /// The first line's offset from `indent_left`; negative is a hanging indent.
    pub first_line: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub line: LineSpacing,
    /// A list bullet or number, drawn at the first line's start.
    pub label: Option<Run>,
    /// The style of the paragraph mark: what sizes an empty paragraph's line.
    pub mark: RunStyle,
}

impl Paragraph {
    #[must_use]
    pub fn new(mark: RunStyle) -> Self {
        Self {
            runs: Vec::new(),
            align: Align::Left,
            indent_left: 0.0,
            indent_right: 0.0,
            first_line: 0.0,
            space_before: 0.0,
            space_after: 0.0,
            line: LineSpacing::Multiple(1.0),
            label: None,
            mark,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowImage {
    pub bytes: Vec<u8>,
    /// For messages: which image, from where.
    pub name: String,
    /// Display size, points.
    pub width: f32,
    pub height: f32,
    pub align: Align,
}

/// One side of a cell's border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    /// Points.
    pub width: f32,
    pub color: [u8; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Borders {
    pub top: Option<Border>,
    pub right: Option<Border>,
    pub bottom: Option<Border>,
    pub left: Option<Border>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

/// A cell's part in a vertical merge: the cell that holds the content, or one
/// of the cells below it that it covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Merge {
    #[default]
    None,
    Start,
    Continue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowCell {
    pub blocks: Vec<FlowBlock>,
    /// Grid columns this cell covers.
    pub span: usize,
    pub merge: Merge,
    pub fill: Option<[u8; 3]>,
    pub borders: Borders,
    pub valign: VAlign,
    /// Inner margins, points: top, right, bottom, left.
    pub margins: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowRow {
    pub cells: Vec<FlowCell>,
    /// Points; with `exact`, the row is exactly this tall.
    pub min_height: f32,
    pub exact: bool,
    /// Repeated at the top of each page the table continues onto.
    pub header: bool,
}

/// A table's declared width, which its grid is scaled to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TableWidth {
    /// Whatever the grid adds up to (scaled down only if it won't fit).
    Grid,
    Points(f32),
    /// A fraction of the space between the margins.
    Fraction(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowTable {
    /// Grid column widths, points. Scaled to `width`, and down if the table is
    /// wider than the page. For an `auto` table, each is a declared width (0
    /// where none was), which its content can widen.
    pub columns: Vec<f32>,
    /// Columns sized from their content, as a browser sizes an HTML table,
    /// rather than taken from a saved grid as Word's are.
    pub auto: bool,
    pub width: TableWidth,
    pub rows: Vec<FlowRow>,
    /// From the left margin, points.
    pub indent: f32,
    pub align: Align,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FlowBlock {
    Paragraph(Paragraph),
    Image(FlowImage),
    Table(FlowTable),
    /// A horizontal line across the text width, less the indents (points).
    Rule { thickness: f32, color: [u8; 3], indent_left: f32, indent_right: f32 },
    /// Vertical space, points; dropped at the top of a page like paragraph spacing.
    Gap(f32),
    PageBreak,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlowDocument {
    pub page: PageSetup,
    pub blocks: Vec<FlowBlock>,
}

/// Word's default tab stops: every half inch from the margin.
const TAB_INTERVAL: f32 = 36.0;

// ---------------------------------------------------------------------------
// Faces

struct Loaded {
    bytes: Vec<u8>,
    index: u32,
    units_per_em: f32,
    ascender: f32,
    descender: f32,
    line_gap: f32,
    /// Advance in font units, or `None` when the face has no glyph.
    advances: HashMap<char, Option<u16>>,
    notdef: u16,
    used: BTreeSet<char>,
}

impl Loaded {
    fn load(bytes: Vec<u8>, index: u32) -> Option<Self> {
        let face = ttf_parser::Face::parse(&bytes, index).ok()?;
        let units_per_em = f32::from(face.units_per_em().max(1));
        let (ascender, descender, line_gap) = (
            f32::from(face.ascender()),
            f32::from(face.descender()),
            f32::from(face.line_gap()),
        );
        let notdef = face.glyph_hor_advance(ttf_parser::GlyphId(0)).unwrap_or(0);
        Some(Self { bytes, index, units_per_em, ascender, descender, line_gap, advances: HashMap::new(), notdef, used: BTreeSet::new() })
    }

    fn advance(&mut self, c: char) -> Option<u16> {
        if let Some(cached) = self.advances.get(&c) {
            return *cached;
        }
        let found = ttf_parser::Face::parse(&self.bytes, self.index)
            .ok()
            .and_then(|face| face.glyph_index(c).map(|g| face.glyph_hor_advance(g).unwrap_or(0)));
        self.advances.insert(c, found);
        found
    }

    fn covers(&mut self, c: char) -> bool {
        self.advance(c).is_some()
    }

    fn width(&mut self, text: &str, size: f32) -> f32 {
        let units: u32 = text.chars().map(|c| u32::from(self.advance(c).unwrap_or(self.notdef))).sum();
        size * units as f32 / self.units_per_em
    }

    /// Ascent, descent (positive) and natural line height at `size`.
    fn metrics(&self, size: f32) -> (f32, f32, f32) {
        let scale = size / self.units_per_em;
        let ascent = self.ascender * scale;
        let descent = -self.descender * scale;
        (ascent, descent, ascent + descent + self.line_gap.max(0.0) * scale)
    }
}

struct Faces<'i> {
    index: &'i FontIndex,
    loaded: Vec<Loaded>,
    by_style: HashMap<(String, bool, bool), usize>,
    /// The same face, asked for under two names (Calibri → Arial, and Arial),
    /// is loaded and embedded once.
    by_file: HashMap<(std::path::PathBuf, u32), usize>,
    fallback: Option<usize>,
    fallback_tried: bool,
    reported: BTreeSet<String>,
    /// PDF/A: a character no face has is refused rather than drawn as `.notdef`.
    archival: bool,
}

impl<'i> Faces<'i> {
    fn new(index: &'i FontIndex, archival: bool) -> Self {
        Self {
            archival,
            index,
            loaded: Vec::new(),
            by_style: HashMap::new(),
            by_file: HashMap::new(),
            fallback: None,
            fallback_tried: false,
            reported: BTreeSet::new(),
        }
    }

    fn face_for(&mut self, style: &RunStyle, warnings: &mut Vec<String>) -> Result<usize, CommandError> {
        let key = (style.family.to_lowercase(), style.bold, style.italic);
        if let Some(&at) = self.by_style.get(&key) {
            return Ok(at);
        }
        let (face, substitute) = self.index.resolve(&style.family, style.bold, style.italic).ok_or_else(|| {
            CommandError::InvalidInput("No usable fonts were found on this computer.".into())
        })?;
        if let Some(used) = substitute {
            if self.reported.insert(style.family.to_lowercase()) {
                warnings.push(format!("{} isn't installed here, so {used} was used instead.", style.family));
            }
        }
        let file = (face.path.clone(), face.index);
        let at = if let Some(&at) = self.by_file.get(&file) {
            at
        } else {
            let bytes = std::fs::read(&face.path)
                .map_err(|e| CommandError::Internal(format!("could not read {}: {e}", face.path.display())))?;
            let loaded = Loaded::load(bytes, face.index)
                .ok_or_else(|| CommandError::Internal(format!("could not parse {}", face.path.display())))?;
            self.loaded.push(loaded);
            self.by_file.insert(file, self.loaded.len() - 1);
            self.loaded.len() - 1
        };
        self.by_style.insert(key, at);
        Ok(at)
    }

    /// A face for `c` that `style`'s own face lacks: first a family of the
    /// same class and style (a serif for a serif — Times has no Cyrillic, Times
    /// New Roman does), then the broad-coverage face.
    fn fallback_for(&mut self, style: &RunStyle, c: char) -> Option<usize> {
        for family in fallback_families(class_of(&style.family)) {
            let Some(face) = self.index.face(family, style.bold, style.italic) else { continue };
            let file = (face.path.clone(), face.index);
            let at = if let Some(&at) = self.by_file.get(&file) {
                at
            } else {
                let Some(loaded) = std::fs::read(&face.path).ok().and_then(|bytes| Loaded::load(bytes, face.index)) else { continue };
                self.loaded.push(loaded);
                self.by_file.insert(file, self.loaded.len() - 1);
                self.loaded.len() - 1
            };
            if self.loaded[at].covers(c) {
                return Some(at);
            }
        }
        self.fallback().filter(|&f| self.loaded[f].covers(c))
    }

    fn fallback(&mut self) -> Option<usize> {
        if self.fallback_tried {
            return self.fallback;
        }
        let found = broad_coverage_font_paths().into_iter().find_map(|path| {
            let bytes = std::fs::read(path).ok()?;
            Loaded::load(bytes, 0)
        });
        let at = found.map(|loaded| {
            self.loaded.push(loaded);
            self.loaded.len() - 1
        });
        self.fallback = at;
        self.fallback_tried = true;
        at
    }
}

// ---------------------------------------------------------------------------
// Layout

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Tab,
    Break,
}

#[derive(Clone, Debug)]
struct Piece {
    text: String,
    face: usize,
    style: RunStyle,
    width: f32,
    kind: Kind,
    /// A line may end before this piece (spaces, CJK, the start of a word).
    break_before: bool,
}

/// Chinese, Japanese and Korean: a line may break between any two characters.
fn breaks_anywhere(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x31FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
        | 0x20000..=0x2FA1F)
}

/// Split runs into measured pieces, each in one face and one style.
fn pieces(runs: &[Run], faces: &mut Faces, warnings: &mut Vec<String>) -> Result<Vec<Piece>, CommandError> {
    let mut out: Vec<Piece> = Vec::new();
    for run in runs {
        let primary = faces.face_for(&run.style, warnings)?;
        for c in run.text.chars() {
            let kind = match c {
                '\n' => Kind::Break,
                '\t' => Kind::Tab,
                ' ' => Kind::Space,
                _ => Kind::Word,
            };
            let face = if kind == Kind::Word && !faces.loaded[primary].covers(c) {
                if let Some(f) = faces.fallback_for(&run.style, c) {
                    f
                } else if faces.archival {
                    // PDF/A forbids drawing `.notdef`, the box a missing character becomes.
                    return Err(CommandError::InvalidInput(format!(
                        "{c} (U+{:04X}) has no font on this computer, and a PDF/A file can't show it as a box.",
                        u32::from(c)
                    )));
                } else {
                    let note = format!("{c} (U+{:04X}) has no font on this computer and is shown as a box.", u32::from(c));
                    if !warnings.contains(&note) {
                        warnings.push(note);
                    }
                    primary
                }
            } else {
                primary
            };
            let cjk = breaks_anywhere(c);
            let joins = out.last().is_some_and(|p| {
                p.kind == kind
                    && matches!(kind, Kind::Word | Kind::Space)
                    && p.face == face
                    && p.style == run.style
                    && !cjk
                    && !p.text.chars().last().is_some_and(breaks_anywhere)
            });
            if joins {
                if let Some(last) = out.last_mut() {
                    last.text.push(c);
                }
            } else {
                let break_before = match (out.last(), kind) {
                    (None, _) => false,
                    (Some(_), Kind::Space | Kind::Tab | Kind::Break) => true,
                    (Some(prev), Kind::Word) => {
                        prev.kind != Kind::Word || cjk || prev.text.chars().last().is_some_and(breaks_anywhere)
                    }
                };
                out.push(Piece { text: c.to_string(), face, style: run.style.clone(), width: 0.0, kind, break_before });
            }
        }
    }
    for piece in &mut out {
        if matches!(piece.kind, Kind::Word | Kind::Space) {
            let loaded = &mut faces.loaded[piece.face];
            piece.width = loaded.width(&piece.text, piece.style.size);
            loaded.used.extend(piece.text.chars());
        }
    }
    Ok(out)
}

/// One laid-out line: its pieces and where each starts, relative to the
/// line's left edge.
#[derive(Default)]
struct Line {
    placed: Vec<(Piece, f32)>,
    width: f32,
    /// The line ends at a `\n` or at the paragraph's end: never justified.
    last: bool,
}

/// Break `pieces` into lines. `widths(i)` is line *i*'s available width and
/// `origins(i)` its left edge measured from the margin (for tab stops).
fn break_lines(pieces: Vec<Piece>, widths: &dyn Fn(usize) -> f32, origins: &dyn Fn(usize) -> f32, faces: &mut Faces) -> Vec<Line> {
    let mut lines = vec![Line::default()];
    let mut x = 0.0f32;
    let mut items = pieces.into_iter().peekable();
    while let Some(mut piece) = items.next() {
        let at = lines.len() - 1;
        let available = widths(at);
        match piece.kind {
            Kind::Break => {
                if let Some(line) = lines.last_mut() {
                    line.last = true;
                }
                lines.push(Line::default());
                x = 0.0;
                continue;
            }
            Kind::Tab => {
                let from_margin = origins(at) + x;
                let next = ((from_margin / TAB_INTERVAL).floor() + 1.0) * TAB_INTERVAL;
                piece.width = next - from_margin;
            }
            Kind::Space | Kind::Word => {}
        }
        // A word continues until the next break opportunity: measure all of it.
        let mut group = vec![piece];
        while items.peek().is_some_and(|p| p.kind == Kind::Word && !p.break_before) {
            if let Some(next) = items.next() {
                group.push(next);
            }
        }
        let group_width: f32 = group.iter().map(|p| p.width).sum();
        let is_space = group[0].kind == Kind::Space;
        let line_has_words = lines.last().is_some_and(|l| l.placed.iter().any(|(p, _)| p.kind == Kind::Word));

        if !is_space && line_has_words && x + group_width > available + 0.01 {
            // Wrap: trailing spaces belong to no line.
            if let Some(line) = lines.last_mut() {
                while line.placed.last().is_some_and(|(p, _)| p.kind == Kind::Space) {
                    line.placed.pop();
                }
                line.width = line.placed.last().map_or(0.0, |(p, at)| at + p.width);
            }
            lines.push(Line::default());
            x = 0.0;
        }
        if is_space && x == 0.0 && lines.len() > 1 && !lines[lines.len() - 2].last {
            continue; // a wrapped line does not start with the space it broke at
        }
        let available = widths(lines.len() - 1);
        for mut p in group {
            // A word wider than a whole line is broken between characters.
            if p.kind == Kind::Word && p.width > available && x == 0.0 {
                let mut chunk = String::new();
                for c in p.text.chars() {
                    let candidate = format!("{chunk}{c}");
                    let w = faces.loaded[p.face].width(&candidate, p.style.size);
                    if !chunk.is_empty() && w > available {
                        let width = faces.loaded[p.face].width(&chunk, p.style.size);
                        let piece = Piece { text: std::mem::take(&mut chunk), width, ..p.clone() };
                        if let Some(line) = lines.last_mut() {
                            line.placed.push((piece, 0.0));
                            line.width = width;
                        }
                        lines.push(Line::default());
                    }
                    chunk.push(c);
                }
                p.width = faces.loaded[p.face].width(&chunk, p.style.size);
                p.text = chunk;
                x = 0.0;
            }
            let width = p.width;
            if let Some(line) = lines.last_mut() {
                line.placed.push((p, x));
                x += width;
                line.width = x;
            }
        }
    }
    if let Some(line) = lines.last_mut() {
        while line.placed.last().is_some_and(|(p, _)| p.kind == Kind::Space) {
            line.placed.pop();
        }
        line.width = line.placed.last().map_or(0.0, |(p, at)| at + p.width);
        line.last = true;
    }
    lines
}

/// Something to draw, decided by layout, emitted once the fonts exist.
#[derive(Clone, Debug)]
enum Mark {
    /// `width` is the advance of `text`, so pieces that touch can be joined.
    Text { face: usize, text: String, x: f32, y: f32, width: f32, size: f32, color: [u8; 3] },
    /// A filled rectangle: an underline, a border, a cell's shading.
    Rect { x: f32, y: f32, width: f32, height: f32, color: [u8; 3] },
    Image { xobject: ObjectId, matrix: [f32; 6] },
}

impl Mark {
    /// The same mark, `dy` points higher up the page.
    fn shifted(mut self, dy: f32) -> Self {
        match &mut self {
            Self::Text { y, .. } | Self::Rect { y, .. } => *y += dy,
            Self::Image { matrix, .. } => matrix[5] += dy,
        }
        self
    }
}

/// Layout's output: what goes down the page, in order. Marks in a `Line` are
/// placed relative to the line's top (so their `y` is zero or negative); the
/// placer decides which page the line is on.
#[derive(Clone, Debug)]
enum Item {
    /// Paragraph spacing. Dropped at the top of a page, as Word drops it.
    Gap(f32),
    Line { height: f32, marks: Vec<Mark> },
}

fn item_height(item: &Item) -> f32 {
    match item {
        Item::Gap(g) => *g,
        Item::Line { height, .. } => *height,
    }
}

struct Pages {
    setup: PageSetup,
    sheets: Vec<Vec<Mark>>,
    /// The next line's top, in PDF coordinates (from the bottom).
    y: f32,
}

impl Pages {
    fn new(setup: PageSetup) -> Self {
        Self { setup, sheets: vec![Vec::new()], y: setup.height - setup.top }
    }

    fn at_top(&self) -> bool {
        (self.y - (self.setup.height - self.setup.top)).abs() < 0.01
    }

    fn new_page(&mut self) {
        self.sheets.push(Vec::new());
        self.y = self.setup.height - self.setup.top;
    }

    /// The height a page holds between its margins.
    fn body(&self) -> f32 {
        self.setup.height - self.setup.top - self.setup.bottom
    }

    /// Whether `height` more fits on this page.
    fn fits(&self, height: f32) -> bool {
        self.y - height >= self.setup.bottom - 0.01
    }

    /// Room for `height` more, starting a page if not (unless this one is empty).
    fn make_room(&mut self, height: f32) {
        if !self.fits(height) && !self.at_top() {
            self.new_page();
        }
    }

    fn draw(&mut self, mark: Mark) {
        if let Some(page) = self.sheets.last_mut() {
            page.push(mark);
        }
    }

    /// Put `items` on the pages, breaking pages between lines.
    fn place(&mut self, items: Vec<Item>) {
        for item in items {
            match item {
                Item::Gap(g) => {
                    if !self.at_top() {
                        self.y -= g;
                    }
                }
                Item::Line { height, marks } => {
                    self.make_room(height);
                    let top = self.y;
                    for mark in marks {
                        self.draw(mark.shifted(top));
                    }
                    self.y -= height;
                }
            }
        }
    }
}

/// Lay `paragraph` out between `left` and `right` (page x), as items.
fn lay_out_paragraph(paragraph: &Paragraph, left: f32, right: f32, faces: &mut Faces, warnings: &mut Vec<String>) -> Result<Vec<Item>, CommandError> {
    let text_left = left + paragraph.indent_left;
    let right = right - paragraph.indent_right;

    // The label hangs to the left of the text; the first line's text starts
    // at the indent when the label fits in the hanging space.
    let mut label_mark = None;
    let first_start = if let Some(label) = &paragraph.label {
        let label_pieces = pieces(std::slice::from_ref(label), faces, warnings)?;
        let label_x = text_left + paragraph.first_line;
        let label_width: f32 = label_pieces.iter().map(|p| p.width).sum();
        label_mark = Some((label_pieces, label_x));
        if label_x + label_width + 2.0 <= text_left { text_left } else { label_x + label_width + 4.0 }
    } else {
        text_left + paragraph.first_line
    };

    let measured = pieces(&paragraph.runs, faces, warnings)?;
    let widths = |i: usize| if i == 0 { right - first_start } else { right - text_left };
    let origins = |i: usize| if i == 0 { first_start - left } else { paragraph.indent_left };
    let lines = break_lines(measured, &widths, &origins, faces);

    let mut items = vec![Item::Gap(paragraph.space_before)];
    let mark_face = faces.face_for(&paragraph.mark, warnings)?;

    for (index, line) in lines.iter().enumerate() {
        // Metrics from the tallest piece, or from the paragraph mark for an empty line.
        let (mut ascent, mut descent, mut natural) = faces.loaded[mark_face].metrics(paragraph.mark.size);
        if line.placed.iter().any(|(p, _)| p.kind == Kind::Word) {
            (ascent, descent, natural) = (0.0, 0.0, 0.0);
            for (piece, _) in line.placed.iter().filter(|(p, _)| p.kind == Kind::Word) {
                let (a, d, n) = faces.loaded[piece.face].metrics(piece.style.size);
                ascent = ascent.max(a);
                descent = descent.max(d);
                natural = natural.max(n);
            }
        }
        let height = match paragraph.line {
            LineSpacing::Multiple(m) => natural * m,
            LineSpacing::Exactly(h) => h,
            LineSpacing::AtLeast(h) => natural.max(h),
        };
        // Relative to the line's top.
        let baseline = -if height >= ascent + descent { ascent } else { height * ascent / (ascent + descent).max(0.01) };
        let mut marks = Vec::new();

        let start = if index == 0 { first_start } else { text_left };
        let available = right - start;
        let words: usize = line.placed.iter().filter(|(p, _)| p.kind == Kind::Space).count();
        let (offset, stretch) = match paragraph.align {
            Align::Center => ((available - line.width) / 2.0, 0.0),
            Align::Right => (available - line.width, 0.0),
            Align::Justify if !line.last && words > 0 => (0.0, (available - line.width) / words as f32),
            Align::Left | Align::Justify => (0.0, 0.0),
        };

        if index == 0 {
            if let Some((label_pieces, label_x)) = label_mark.take() {
                let mut lx = label_x;
                for piece in label_pieces.into_iter().filter(|p| p.kind == Kind::Word || p.kind == Kind::Space) {
                    marks.push(Mark::Text {
                        face: piece.face,
                        text: piece.text,
                        x: lx,
                        y: baseline + piece.style.rise,
                        width: piece.width,
                        size: piece.style.size,
                        color: piece.style.color,
                    });
                    lx += piece.width;
                }
            }
        }

        let mut extra = 0.0;
        for (piece, at) in &line.placed {
            let x = start + offset + at + extra;
            if piece.kind == Kind::Space {
                extra += stretch;
            }
            if !matches!(piece.kind, Kind::Word | Kind::Space) {
                continue;
            }
            let width = piece.width + if piece.kind == Kind::Space { stretch } else { 0.0 };
            marks.push(Mark::Text {
                face: piece.face,
                text: piece.text.clone(),
                x,
                y: baseline + piece.style.rise,
                width: piece.width,
                size: piece.style.size,
                color: piece.style.color,
            });
            let thickness = (piece.style.size * 0.06).max(0.5);
            if piece.style.underline {
                let y = baseline - piece.style.size * 0.12 - thickness / 2.0;
                marks.push(Mark::Rect { x, y, width, height: thickness, color: piece.style.color });
            }
            if piece.style.strike {
                let y = baseline + piece.style.size * 0.28 - thickness / 2.0;
                marks.push(Mark::Rect { x, y, width, height: thickness, color: piece.style.color });
            }
        }
        items.push(Item::Line { height, marks });
    }
    items.push(Item::Gap(paragraph.space_after));
    Ok(items)
}

/// An image as one line, scaled down to fit `left..right` and `max_height`.
fn lay_out_image(image: &FlowImage, left: f32, right: f32, max_height: f32, builder: &mut Builder, warnings: &mut Vec<String>) -> Result<Item, CommandError> {
    let embedded = embed_image(&mut builder.doc, &image.name, &image.bytes, builder.archival)?;
    if embedded.converted {
        warnings.push(converted_note(&image.name));
    }
    let room = right - left;
    let scale = (room / image.width).min(max_height / image.height).min(1.0);
    let (w, h) = (image.width * scale, image.height * scale);
    let x = match image.align {
        Align::Center => left + (room - w) / 2.0,
        Align::Right => right - w,
        Align::Left | Align::Justify => left,
    };
    Ok(Item::Line { height: h, marks: vec![Mark::Image { xobject: embedded.xobject, matrix: placement(embedded.orientation, x, -h, w, h) }] })
}

/// A rule as one line `thickness` tall.
fn rule_item(left: f32, right: f32, thickness: f32, color: [u8; 3], (indent_left, indent_right): (f32, f32)) -> Item {
    let (x, width) = (left + indent_left, (right - left - indent_left - indent_right).max(0.0));
    Item::Line { height: thickness, marks: vec![Mark::Rect { x, y: -thickness, width, height: thickness, color }] }
}

// ---------------------------------------------------------------------------
// Tables

/// The narrowest and widest `blocks` can be laid out: the widest word that
/// can't be broken, and every line unbroken (CSS's min- and max-content).
fn content_widths(blocks: &[FlowBlock], faces: &mut Faces, warnings: &mut Vec<String>) -> Result<(f32, f32), CommandError> {
    let (mut narrowest, mut widest) = (0.0f32, 0.0f32);
    for block in blocks {
        let (low, high) = match block {
            FlowBlock::Paragraph(p) => {
                let (mut word, mut longest_word, mut line, mut longest_line) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                for piece in pieces(&p.runs, faces, warnings)? {
                    if piece.break_before {
                        longest_word = longest_word.max(word);
                        word = 0.0;
                    }
                    match piece.kind {
                        Kind::Word => {
                            word += piece.width;
                            line += piece.width;
                        }
                        Kind::Space => line += piece.width,
                        // A tab's width depends on where it lands; one stop is the usual.
                        Kind::Tab => line += TAB_INTERVAL,
                        Kind::Break => {
                            longest_line = longest_line.max(line);
                            line = 0.0;
                        }
                    }
                }
                let extra = p.indent_left.max(0.0) + p.indent_right.max(0.0) + p.first_line.max(0.0);
                (longest_word.max(word) + extra, longest_line.max(line) + extra)
            }
            FlowBlock::Image(image) => (image.width, image.width),
            FlowBlock::Table(table) => {
                let ranges = column_ranges(table, faces, warnings)?;
                (ranges.iter().map(|r| r.0).sum::<f32>() + table.indent, ranges.iter().map(|r| r.1).sum::<f32>() + table.indent)
            }
            FlowBlock::Rule { .. } | FlowBlock::Gap(_) | FlowBlock::PageBreak => (0.0, 0.0),
        };
        narrowest = narrowest.max(low);
        widest = widest.max(high);
    }
    Ok((narrowest, widest.max(narrowest)))
}

/// How many grid columns `table` has.
fn grid_count(table: &FlowTable) -> usize {
    table.rows.iter().map(|r| r.cells.iter().map(|c| c.span.max(1)).sum::<usize>()).max().unwrap_or(1).max(table.columns.len()).max(1)
}

/// Each grid column's narrowest and widest, cell margins included. A
/// declared width is both, unless the content needs more; a cell spanning
/// columns spreads what they lack evenly over them.
fn column_ranges(table: &FlowTable, faces: &mut Faces, warnings: &mut Vec<String>) -> Result<Vec<(f32, f32)>, CommandError> {
    let count = grid_count(table);
    let mut ranges = vec![(0.0f32, 0.0f32); count];
    let mut spanning = Vec::new();
    for row in &table.rows {
        let mut column = 0;
        for cell in &row.cells {
            let span = cell.span.max(1);
            if cell.merge != Merge::Continue && column < count {
                let (low, high) = content_widths(&cell.blocks, faces, warnings)?;
                let padding = cell.margins[1] + cell.margins[3];
                if span == 1 {
                    ranges[column].0 = ranges[column].0.max(low + padding);
                    ranges[column].1 = ranges[column].1.max(high + padding);
                } else {
                    spanning.push((column, span, low + padding, high + padding));
                }
            }
            column += span;
        }
    }
    for (range, declared) in ranges.iter_mut().zip(&table.columns) {
        if *declared > 0.0 {
            // A declared width holds the content to it, as browsers do — but
            // one a hair short of the content is the writer's own measurement,
            // rounded, and shouldn't push the last word onto a new line.
            let fits = range.1 <= declared * 1.03 + 0.5;
            let width = if fits { range.1.max(*declared) } else { range.0.max(*declared) };
            *range = (width, width);
        }
    }
    for (column, span, low, high) in spanning {
        let covered = column..(column + span).min(count);
        let n = covered.len() as f32;
        let (have_low, have_high) = ranges[covered.clone()].iter().fold((0.0, 0.0), |(l, h), r| (l + r.0, h + r.1));
        for range in &mut ranges[covered] {
            range.0 += (low - have_low).max(0.0) / n;
            range.1 += (high - have_high).max(0.0) / n;
        }
    }
    for range in &mut ranges {
        range.1 = range.1.max(range.0);
    }
    Ok(ranges)
}

/// An `auto` table's column widths: as wide as their content wants when that
/// fits, and between their narrowest and widest in proportion when it
/// doesn't (CSS 2.1's automatic table layout, simplified).
fn auto_widths(table: &FlowTable, (left, right): (f32, f32), faces: &mut Faces, warnings: &mut Vec<String>) -> Result<Vec<f32>, CommandError> {
    let ranges = column_ranges(table, faces, warnings)?;
    let room = (right - left - table.indent).max(1.0);
    let (low, high): (f32, f32) = ranges.iter().fold((0.0, 0.0), |(l, h), r| (l + r.0, h + r.1));
    if high <= 0.0 {
        // Nothing in it to measure: share the width.
        return Ok(vec![room / ranges.len() as f32; ranges.len()]);
    }
    let target = match table.width {
        TableWidth::Grid => high,
        TableWidth::Points(points) => points,
        TableWidth::Fraction(fraction) => (right - left) * fraction,
    }
    .min(room)
    .max(low.min(room));
    let widths = if target >= high {
        // A table wider than its content gives the extra to the columns
        // without a declared width, by their content; all, if all have one.
        let open: Vec<bool> = (0..ranges.len()).map(|i| table.columns.get(i).is_none_or(|w| *w <= 0.0)).collect();
        let open_high: f32 = ranges.iter().zip(&open).filter(|(_, o)| **o).map(|(r, _)| r.1).sum();
        let open_count = open.iter().filter(|o| **o).count();
        let extra = target - high;
        ranges
            .iter()
            .zip(&open)
            .map(|(r, &o)| match (open_count, o) {
                (0, _) => r.1 * target / high,
                (_, false) => r.1,
                _ if open_high > 0.0 => r.1 + extra * r.1 / open_high,
                _ => r.1 + extra / open_count as f32,
            })
            .collect()
    } else if target >= low && high > low {
        ranges.iter().map(|r| r.0 + (target - low) * (r.1 - r.0) / (high - low)).collect()
    } else {
        ranges.iter().map(|r| r.0 * target / low.max(0.01)).collect()
    };
    Ok(widths)
}

/// Blocks laid out between `left` and `right` with no page breaks — a table
/// cell's content. A nested table becomes one line per row.
fn lay_out_blocks(
    blocks: &[FlowBlock],
    (left, right): (f32, f32),
    max_height: f32,
    faces: &mut Faces,
    builder: &mut Builder,
    warnings: &mut Vec<String>,
) -> Result<Vec<Item>, CommandError> {
    let mut items = Vec::new();
    for block in blocks {
        match block {
            FlowBlock::Paragraph(p) => items.extend(lay_out_paragraph(p, left, right, faces, warnings)?),
            FlowBlock::Image(image) => items.push(lay_out_image(image, left, right, max_height, builder, warnings)?),
            FlowBlock::Table(table) => {
                let rows = lay_out_rows(table, (left, right), max_height, faces, builder, warnings)?;
                for index in 0..rows.len() {
                    items.push(Item::Line { height: rows[index].height, marks: row_marks(&rows, index) });
                }
            }
            FlowBlock::Rule { thickness, color, indent_left, indent_right } => {
                items.push(rule_item(left, right, *thickness, *color, (*indent_left, *indent_right)));
            }
            FlowBlock::Gap(gap) => items.push(Item::Gap(*gap)),
            FlowBlock::PageBreak => {}
        }
    }
    Ok(items)
}

/// A cell, laid out.
struct CellBox {
    x: f32,
    width: f32,
    items: Vec<Item>,
    /// The content's height, without margins.
    content: f32,
    fill: Option<[u8; 3]>,
    borders: Borders,
    valign: VAlign,
    margins: [f32; 4],
    merge: Merge,
    /// Rows this cell covers (more than one for the start of a vertical merge).
    rows: usize,
}

struct RowBox {
    cells: Vec<CellBox>,
    height: f32,
    header: bool,
}

/// Grid column boundaries in page x, from the left edge of the first column.
/// `measured` are an `auto` table's widths, used as they are.
fn column_edges(table: &FlowTable, (left, right): (f32, f32), measured: Option<Vec<f32>>) -> Vec<f32> {
    let count = grid_count(table);
    let room = (right - left - table.indent).max(1.0);
    let (widths, scale) = if let Some(widths) = measured {
        (widths, 1.0)
    } else {
        let mut widths = table.columns.clone();
        if widths.len() < count || widths.iter().any(|w| *w <= 0.0) {
            // No grid to go by: share the width evenly.
            widths = vec![room / count as f32; count];
        }
        let total: f32 = widths.iter().sum();
        let wanted = match table.width {
            TableWidth::Grid => total,
            TableWidth::Points(points) => points,
            TableWidth::Fraction(fraction) => (right - left) * fraction,
        };
        let scale = (wanted / total).min(room / total);
        (widths, scale)
    };
    let total: f32 = widths.iter().sum();
    let table_width = total * scale;
    let start = match table.align {
        Align::Center => left + (right - left - table_width) / 2.0,
        Align::Right => right - table_width,
        Align::Left | Align::Justify => left + table.indent,
    };
    let mut edges = vec![start];
    for w in widths {
        let last = *edges.last().unwrap_or(&start);
        edges.push(last + w * scale);
    }
    edges
}

fn lay_out_rows(
    table: &FlowTable,
    region: (f32, f32),
    max_height: f32,
    faces: &mut Faces,
    builder: &mut Builder,
    warnings: &mut Vec<String>,
) -> Result<Vec<RowBox>, CommandError> {
    let measured = if table.auto { Some(auto_widths(table, region, faces, warnings)?) } else { None };
    let edges = column_edges(table, region, measured);
    let last_edge = edges.len() - 1;
    let mut rows = Vec::with_capacity(table.rows.len());
    // The grid column each cell starts at, row by row, to find merges.
    let mut starts: Vec<Vec<usize>> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut column = 0;
        let mut cells = Vec::with_capacity(row.cells.len());
        let mut row_starts = Vec::with_capacity(row.cells.len());
        for cell in &row.cells {
            let (from, to) = (column.min(last_edge), (column + cell.span.max(1)).min(last_edge));
            row_starts.push(column);
            column += cell.span.max(1);
            let (x, width) = (edges[from], (edges[to] - edges[from]).max(0.0));
            let [top, right_margin, bottom, left_margin] = cell.margins;
            let inner = (x + left_margin, (x + width - right_margin).max(x + left_margin + 1.0));
            let items = if cell.merge == Merge::Continue {
                Vec::new()
            } else {
                // A cell's own spacing at its very top and bottom is kept, as Word keeps it.
                lay_out_blocks(&cell.blocks, inner, max_height - top - bottom, faces, builder, warnings)?
            };
            let content = items.iter().map(item_height).sum();
            cells.push(CellBox {
                x,
                width,
                items,
                content,
                fill: cell.fill,
                borders: cell.borders,
                valign: cell.valign,
                margins: cell.margins,
                merge: cell.merge,
                rows: 1,
            });
        }
        let natural = cells
            .iter()
            .filter(|c| c.merge == Merge::None)
            .map(|c| c.content + c.margins[0] + c.margins[2])
            .fold(0.0, f32::max);
        let height = if row.exact { row.min_height } else { natural.max(row.min_height) };
        rows.push(RowBox { cells, height, header: row.header });
        starts.push(row_starts);
    }

    // Vertical merges: a start cell covers the cells below it that continue
    // it; if its content needs more than those rows give, the last one grows.
    for r in 0..rows.len() {
        for c in 0..rows[r].cells.len() {
            if rows[r].cells[c].merge != Merge::Start {
                continue;
            }
            let column = starts[r][c];
            let mut span = 1;
            while r + span < rows.len()
                && starts[r + span].iter().zip(&rows[r + span].cells).any(|(&s, cell)| s == column && cell.merge == Merge::Continue)
            {
                span += 1;
            }
            rows[r].cells[c].rows = span;
            if span > 1 {
                // The merged cell's bottom edge is the last covered row's.
                let last = r + span - 1;
                let bottom = starts[last].iter().zip(&rows[last].cells).find(|(&s, _)| s == column).and_then(|(_, cell)| cell.borders.bottom);
                rows[r].cells[c].borders.bottom = bottom;
            }
            let cell = &rows[r].cells[c];
            let needed = cell.content + cell.margins[0] + cell.margins[2];
            let have: f32 = rows[r..r + span].iter().map(|row| row.height).sum();
            if needed > have {
                rows[r + span - 1].height += needed - have;
            }
        }
    }
    Ok(rows)
}

/// Row `index`'s marks, relative to its top: shading, then content, then borders.
fn row_marks(rows: &[RowBox], index: usize) -> Vec<Mark> {
    let row = &rows[index];
    let mut fills = Vec::new();
    let mut content = Vec::new();
    let mut lines = Vec::new();
    for cell in row.cells.iter().filter(|c| c.merge != Merge::Continue) {
        let span = rows[index..(index + cell.rows).min(rows.len())].iter().map(|r| r.height).sum::<f32>();
        if let Some(color) = cell.fill {
            fills.push(Mark::Rect { x: cell.x, y: -span, width: cell.width, height: span, color });
        }
        let [top, _, bottom, _] = cell.margins;
        let slack = (span - cell.content - top - bottom).max(0.0);
        let mut y = -(top + match cell.valign {
            VAlign::Top => 0.0,
            VAlign::Center => slack / 2.0,
            VAlign::Bottom => slack,
        });
        for item in &cell.items {
            match item {
                Item::Gap(g) => y -= g,
                Item::Line { height, marks } => {
                    content.extend(marks.iter().cloned().map(|m| m.shifted(y)));
                    y -= height;
                }
            }
        }
        lines.extend(border_marks(cell.x, cell.width, span, &cell.borders));
    }
    fills.extend(content);
    fills.extend(lines);
    fills
}

/// A cell's four borders, centred on its edges, for a cell `height` tall
/// whose top is at y = 0.
fn border_marks(x: f32, width: f32, height: f32, borders: &Borders) -> Vec<Mark> {
    let mut marks = Vec::new();
    if let Some(b) = borders.top {
        marks.push(Mark::Rect { x: x - b.width / 2.0, y: -b.width / 2.0, width: width + b.width, height: b.width, color: b.color });
    }
    if let Some(b) = borders.bottom {
        marks.push(Mark::Rect { x: x - b.width / 2.0, y: -height - b.width / 2.0, width: width + b.width, height: b.width, color: b.color });
    }
    if let Some(b) = borders.left {
        marks.push(Mark::Rect { x: x - b.width / 2.0, y: -height, width: b.width, height, color: b.color });
    }
    if let Some(b) = borders.right {
        marks.push(Mark::Rect { x: x + width - b.width / 2.0, y: -height, width: b.width, height, color: b.color });
    }
    marks
}

type Header = (f32, Vec<Mark>);

/// Start a page and draw `headers` at its top.
fn new_page_with(pages: &mut Pages, headers: &[Header]) {
    pages.new_page();
    for (height, marks) in headers {
        let top = pages.y;
        for mark in marks {
            pages.draw(mark.clone().shifted(top));
        }
        pages.y -= height;
    }
}

/// Put a table on the pages: a row that doesn't fit moves to the next page,
/// where the header rows are drawn again; a row taller than a whole page is
/// split between lines.
fn place_table(table: &FlowTable, pages: &mut Pages, faces: &mut Faces, builder: &mut Builder, warnings: &mut Vec<String>) -> Result<(), CommandError> {
    let region = (pages.setup.left, pages.setup.width - pages.setup.right);
    let mut rows = lay_out_rows(table, region, pages.body(), faces, builder, warnings)?;
    let headers = rows.iter().take_while(|r| r.header).count();
    let header_marks: Vec<Header> = (0..headers).map(|i| (rows[i].height, row_marks(&rows, i))).collect();
    // Headers that would take more than half of every page aren't repeated:
    // the rows they head would get almost no room.
    let header_height: f32 = header_marks.iter().map(|(h, _)| *h).sum();
    let repeated: &[Header] = if header_height <= pages.body() / 2.0 { &header_marks } else { &[] };
    let repeated_height: f32 = repeated.iter().map(|(h, _)| *h).sum();

    for index in 0..rows.len() {
        // The header rows themselves start a page plainly.
        let heads = if index < headers { &[] } else { repeated };
        // A vertical merge keeps its rows together when they fit on a page.
        let group: f32 = {
            let end = rows[index].cells.iter().map(|c| index + c.rows).max().unwrap_or(index + 1).min(rows.len());
            rows[index..end].iter().map(|r| r.height).sum()
        };
        let height = rows[index].height;
        if height <= pages.body() - repeated_height || height <= pages.body() && index < headers {
            if !pages.fits(group.min(pages.body() - repeated_height)) && !pages.at_top() {
                new_page_with(pages, heads);
            }
            let marks = row_marks(&rows, index);
            let top = pages.y;
            for mark in marks {
                pages.draw(mark.shifted(top));
            }
            pages.y -= height;
        } else {
            split_row(&mut rows[index], pages, heads, warnings);
        }
    }
    Ok(())
}

/// A row taller than a page: each cell's lines dealt out page by page, the
/// row's shading and borders drawn around each page's part.
fn split_row(row: &mut RowBox, pages: &mut Pages, heads: &[Header], warnings: &mut Vec<String>) {
    let mut queues: Vec<std::collections::VecDeque<Item>> = row.cells.iter_mut().map(|c| std::mem::take(&mut c.items).into()).collect();
    // Whether this page has had nothing but headers drawn on it yet: on such a
    // page a line is placed even when it doesn't fit, or nothing would ever be.
    let mut fresh = pages.at_top();
    loop {
        let room = pages.y - pages.setup.bottom;
        // Measured through each cell's next line, not just its next item: a
        // paragraph starts with its (often zero) space before, and a part of
        // nothing but gaps would draw an empty slice of the row here.
        let first_fits = row.cells.iter().zip(&queues).all(|(cell, queue)| {
            let mut through_line = 0.0;
            for item in queue {
                through_line += item_height(item);
                if matches!(item, Item::Line { .. }) {
                    break;
                }
            }
            through_line + cell.margins[0] + cell.margins[2] <= room
        });
        if !first_fits && !fresh {
            new_page_with(pages, heads);
            fresh = true;
            continue;
        }
        let mut taken: Vec<Vec<Item>> = Vec::with_capacity(queues.len());
        for (cell, queue) in row.cells.iter().zip(queues.iter_mut()) {
            let budget = room - cell.margins[0] - cell.margins[2];
            let (mut used, mut part) = (0.0, Vec::new());
            while let Some(item) = queue.front() {
                let h = item_height(item);
                if used + h > budget && !part.is_empty() {
                    break;
                }
                // At least one item a page, or the row would never end. One
                // taller than the page can only be a nested table's row.
                if h > budget && fresh && matches!(item, Item::Line { .. }) {
                    let message = "A table inside a table cell has a row taller than a page; what runs past the bottom of the page is cut off.";
                    if !warnings.iter().any(|w| w == message) {
                        warnings.push(message.to_owned());
                    }
                }
                used += h;
                part.extend(queue.pop_front());
            }
            taken.push(part);
        }
        let fragment = row
            .cells
            .iter()
            .zip(&taken)
            .map(|(c, part)| part.iter().map(item_height).sum::<f32>() + c.margins[0] + c.margins[2])
            .fold(0.0, f32::max);
        let top = pages.y;
        for (cell, part) in row.cells.iter().zip(taken) {
            if let Some(color) = cell.fill {
                pages.draw(Mark::Rect { x: cell.x, y: top - fragment, width: cell.width, height: fragment, color });
            }
            let mut y = top - cell.margins[0];
            for item in part {
                match item {
                    Item::Gap(g) => y -= g,
                    Item::Line { height, marks } => {
                        for mark in marks {
                            pages.draw(mark.shifted(y));
                        }
                        y -= height;
                    }
                }
            }
            for mark in border_marks(cell.x, cell.width, fragment, &cell.borders) {
                pages.draw(mark.shifted(top));
            }
        }
        pages.y -= fragment;
        if queues.iter().all(std::collections::VecDeque::is_empty) {
            break;
        }
        new_page_with(pages, heads);
        fresh = true;
    }
}

/// SPEC: P7-OCR-009 — lay `document` out and add its pages to `builder`.
/// Returns how many pages it added.
///
/// # Errors
/// When no font can be found at all, or an image can't be read.
pub(crate) fn render_flow(builder: &mut Builder, document: &FlowDocument, index: &FontIndex, warnings: &mut Vec<String>) -> Result<usize, CommandError> {
    let mut faces = Faces::new(index, builder.archival);
    let mut pages = Pages::new(document.page);
    let (left, right) = (document.page.left, document.page.width - document.page.right);
    for block in &document.blocks {
        match block {
            FlowBlock::Paragraph(p) => {
                let items = lay_out_paragraph(p, left, right, &mut faces, warnings)?;
                pages.place(items);
            }
            FlowBlock::Image(image) => {
                let item = lay_out_image(image, left, right, pages.body(), builder, warnings)?;
                pages.place(vec![item]);
            }
            FlowBlock::Table(table) => place_table(table, &mut pages, &mut faces, builder, warnings)?,
            FlowBlock::Rule { thickness, color, indent_left, indent_right } => {
                pages.place(vec![rule_item(left, right, *thickness, *color, (*indent_left, *indent_right))]);
            }
            FlowBlock::Gap(gap) => pages.place(vec![Item::Gap(*gap)]),
            FlowBlock::PageBreak => pages.new_page(),
        }
    }
    while pages.sheets.len() > 1 && pages.sheets.last().is_some_and(Vec::is_empty) {
        pages.sheets.pop();
    }
    if pages.sheets.iter().all(Vec::is_empty) {
        return Ok(0);
    }

    // Fonts, now that every character each face draws is known.
    let mut fonts: BTreeMap<usize, (CidFont, ObjectId)> = BTreeMap::new();
    for (at, loaded) in faces.loaded.iter().enumerate() {
        if loaded.used.is_empty() {
            continue;
        }
        let chars: String = loaded.used.iter().collect();
        let font = build_cid_font_from(&mut builder.doc, &loaded.bytes, loaded.index, &chars)?;
        let reference = builder.doc.add_object(font.font_dict.clone());
        fonts.insert(at, (font, reference));
    }

    let count = pages.sheets.len();
    for marks in pages.sheets {
        let marks = join_touching(marks);
        let mut content = String::new();
        let mut font_refs = Dictionary::new();
        let mut images = Dictionary::new();
        for mark in marks {
            match mark {
                Mark::Text { face, text, x, y, size, color, .. } => {
                    let Some((font, reference)) = fonts.get(&face) else { continue };
                    let name = format!("F{face}");
                    font_refs.set(name.clone(), *reference);
                    let _ = writeln!(
                        content,
                        "BT /{name} {} Tf {} rg 1 0 0 1 {} {} Tm <{}> Tj ET",
                        num(size),
                        rgb(color),
                        num(x),
                        num(y),
                        font.encode_hex(&text)
                    );
                }
                Mark::Rect { x, y, width, height, color } => {
                    let _ = writeln!(content, "{} rg {} {} {} {} re f", rgb(color), num(x), num(y), num(width), num(height));
                }
                Mark::Image { xobject, matrix } => {
                    let name = format!("Im{}", images.len());
                    images.set(name.clone(), xobject);
                    let m: Vec<String> = matrix.iter().map(|v| num(*v)).collect();
                    let _ = writeln!(content, "q {} cm /{name} Do Q", m.join(" "));
                }
            }
        }
        let mut resources = Dictionary::new();
        if !font_refs.is_empty() {
            resources.set("Font", Object::Dictionary(font_refs));
        }
        if !images.is_empty() {
            resources.set("XObject", Object::Dictionary(images));
        }
        builder.add_page((document.page.width, document.page.height), &content, resources);
    }
    Ok(count)
}

/// Join text pieces that touch — same face, size, colour and baseline, each
/// starting where the last one ended — into one text-showing operation.
///
/// Layout places every word and space separately, which is right for
/// justification but leaves a reader's text extraction to guess where words
/// meet: `PDFium` read "漢字" as "漢 字". Joined, the gaps that exist are exactly
/// the ones the document has. A justified line keeps its separate pieces,
/// because its stretched spaces don't touch the next word.
fn join_touching(marks: Vec<Mark>) -> Vec<Mark> {
    let mut out: Vec<Mark> = Vec::with_capacity(marks.len());
    for mark in marks {
        if let (Some(Mark::Text { face: f0, text: t0, x: x0, y: y0, width: w0, size: s0, color: c0 }), Mark::Text { face, text, x, y, width, size, color }) =
            (out.last_mut(), &mark)
        {
            if *f0 == *face && (*s0 - size).abs() < 0.001 && *c0 == *color && (*y0 - y).abs() < 0.001 && (*x0 + *w0 - x).abs() < 0.01 {
                t0.push_str(text);
                *w0 += width;
                continue;
            }
        }
        out.push(mark);
    }
    out
}

fn rgb([r, g, b]: [u8; 3]) -> String {
    format!("{} {} {}", num(f32::from(r) / 255.0), num(f32::from(g) / 255.0), num(f32::from(b) / 255.0))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_written_as_fractions() {
        assert_eq!(rgb([255, 0, 128]), "1 0 0.502");
    }

    #[test]
    fn cjk_is_breakable_and_latin_is_not() {
        assert!(breaks_anywhere('日') && breaks_anywhere('か'));
        assert!(!breaks_anywhere('a') && !breaks_anywhere('é'));
    }
}
