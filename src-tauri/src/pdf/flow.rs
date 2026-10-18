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
use crate::pdf::create::{embed_image, num, placement, Builder};
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

#[derive(Clone, Debug, PartialEq)]
pub enum FlowBlock {
    Paragraph(Paragraph),
    Image(FlowImage),
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
}

impl<'i> Faces<'i> {
    fn new(index: &'i FontIndex) -> Self {
        Self {
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
enum Mark {
    /// `width` is the advance of `text`, so pieces that touch can be joined.
    Text { face: usize, text: String, x: f32, y: f32, width: f32, size: f32, color: [u8; 3] },
    Rule { x: f32, y: f32, width: f32, thickness: f32, color: [u8; 3] },
    Image { xobject: ObjectId, matrix: [f32; 6] },
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

    /// Room for `height` more, starting a page if not (unless this one is empty).
    fn make_room(&mut self, height: f32) {
        if self.y - height < self.setup.bottom - 0.01 && !self.at_top() {
            self.new_page();
        }
    }

    fn draw(&mut self, mark: Mark) {
        if let Some(page) = self.sheets.last_mut() {
            page.push(mark);
        }
    }
}

fn lay_out_paragraph(paragraph: &Paragraph, pages: &mut Pages, faces: &mut Faces, warnings: &mut Vec<String>) -> Result<(), CommandError> {
    let setup = pages.setup;
    let text_left = setup.left + paragraph.indent_left;
    let right = setup.width - setup.right - paragraph.indent_right;

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
    let origins = |i: usize| if i == 0 { first_start - setup.left } else { paragraph.indent_left };
    let lines = break_lines(measured, &widths, &origins, faces);

    if !pages.at_top() {
        pages.y -= paragraph.space_before;
    }
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
        pages.make_room(height);
        let baseline = pages.y - if height >= ascent + descent { ascent } else { height * ascent / (ascent + descent).max(0.01) };

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
                    pages.draw(Mark::Text {
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
            pages.draw(Mark::Text {
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
                pages.draw(Mark::Rule { x, y: baseline - piece.style.size * 0.12, width, thickness, color: piece.style.color });
            }
            if piece.style.strike {
                pages.draw(Mark::Rule { x, y: baseline + piece.style.size * 0.28, width, thickness, color: piece.style.color });
            }
        }
        pages.y -= height;
    }
    pages.y -= paragraph.space_after;
    Ok(())
}

fn lay_out_image(image: &FlowImage, builder: &mut Builder, pages: &mut Pages) -> Result<(), CommandError> {
    let setup = pages.setup;
    let embedded = embed_image(&mut builder.doc, &image.name, &image.bytes)?;
    let (room_w, room_h) = (setup.width - setup.left - setup.right, setup.height - setup.top - setup.bottom);
    let scale = (room_w / image.width).min(room_h / image.height).min(1.0);
    let (w, h) = (image.width * scale, image.height * scale);
    pages.make_room(h);
    let x = match image.align {
        Align::Center => setup.left + (room_w - w) / 2.0,
        Align::Right => setup.width - setup.right - w,
        Align::Left | Align::Justify => setup.left,
    };
    let y = pages.y - h;
    pages.draw(Mark::Image { xobject: embedded.xobject, matrix: placement(embedded.orientation, x, y, w, h) });
    pages.y = y;
    Ok(())
}

/// SPEC: P7-OCR-009 — lay `document` out and add its pages to `builder`.
/// Returns how many pages it added.
///
/// # Errors
/// When no font can be found at all, or an image can't be read.
pub(crate) fn render_flow(builder: &mut Builder, document: &FlowDocument, index: &FontIndex, warnings: &mut Vec<String>) -> Result<usize, CommandError> {
    let mut faces = Faces::new(index);
    let mut pages = Pages::new(document.page);
    for block in &document.blocks {
        match block {
            FlowBlock::Paragraph(p) => lay_out_paragraph(p, &mut pages, &mut faces, warnings)?,
            FlowBlock::Image(image) => lay_out_image(image, builder, &mut pages)?,
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
                Mark::Rule { x, y, width, thickness, color } => {
                    let _ = writeln!(content, "{} rg {} {} {} {} re f", rgb(color), num(x), num(y - thickness / 2.0), num(width), num(thickness));
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
