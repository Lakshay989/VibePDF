//! SPEC: P7-OCR-010 (P7.C2c) — font subsetting, the third of Compress's targets.
//!
//! An embedded font is cut down to the glyphs the document draws with it. That
//! saves real space on a font embedded whole, as some report generators do, and
//! on a subset that still covers pages since deleted. It saves nothing on a
//! font that is already a subset, such as `VibePDF`'s own.
//!
//! **A missing glyph is silent.** Text drawn with a glyph the subset dropped is
//! simply not drawn, and nothing reports an error. So a font is subset only when
//! every use of it is accounted for, and anything this can't account for is
//! left whole:
//!
//! - **The walk** reads every content stream that can draw text: pages, form
//!   `XObject`s (which inherit the caller's font), annotation appearances,
//!   tiling patterns, soft masks and Type3 glyph procedures. It follows the font
//!   set by `Tf` and by a graphics state's `/Font`, across `q`/`Q`, and records
//!   the codes `Tj`, `TJ`, `'` and `"` show. One content stream it can't read
//!   leaves every font whole.
//! - **The completeness proof.** A font is subset only if every object that
//!   refers to it was read on the way down from content the walk read. A font
//!   also reachable from anywhere else is left whole: a form field's `/DR`,
//!   where new text gets typed; an `XObject` nothing draws; a structure this
//!   doesn't model.
//! - **The guard**, in `compress`, renders every page with and without the
//!   subsetting and throws it away if a single pixel differs.
//!
//! **What is subset.** TrueType outlines (`FontFile2`, or `FontFile3` of subtype
//! `OpenType` holding a `glyf` table) used by composite fonts with an `Identity`
//! `CMap`, where a code *is* a CID. `subsetter` keeps glyph IDs, so only the
//! font file's bytes change: encodings, widths and `ToUnicode` stay as they
//! are. CFF, Type1, Type3 and other `CMap`s are left whole.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};

/// How deep form `XObject`s may nest before the walk gives up. Real documents
/// nest a few levels; a cycle or a hostile file would otherwise recurse without
/// end.
const MAX_DEPTH: usize = 32;

/// The guard's resolution. Low, because it compares two renders of the same
/// page for *equality*: a dropped glyph at any normal size still covers pixels.
const GUARD_DPI: f32 = 50.0;

/// Why a font file was left whole. Not serialized: it names each skip decision,
/// so the tests can assert on the reason rather than on "nothing happened".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeptWhole {
    /// A content stream could not be read, so no use of any font is certain.
    UnreadableContent,
    /// Not TrueType outlines: CFF, Type1, or not a font `subsetter` can parse.
    NotTrueType,
    /// The font structure isn't the shape this models, or is used by a kind of
    /// font this step doesn't handle (a simple font, a non-`Identity` `CMap`).
    Unsupported,
    /// A form field may type new text in it (`/AcroForm /DR`).
    FormFieldFont,
    /// Something outside the walk refers to it, so not every use is known.
    OutsideTheWalk,
    /// `subsetter` failed on it.
    SubsetFailed,
    /// The subset would not be smaller once deflated.
    NotSmaller,
}

/// One font file to replace with its subset.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    /// The `FontFile2`/`FontFile3` stream.
    pub file: ObjectId,
    /// The subset font program, uncompressed. The deflation pass compresses it.
    pub bytes: Vec<u8>,
    /// The glyphs kept, `.notdef` included.
    pub glyphs: BTreeSet<u16>,
    /// `FontFile2` carries `/Length1`; `FontFile3` doesn't.
    pub is_font_file2: bool,
    /// Font dictionaries and descriptors whose name gets the subset tag.
    pub names: Vec<(ObjectId, &'static [u8])>,
}

/// What the planner decided for every embedded font file in the document.
#[derive(Debug, Default)]
pub(crate) struct Plans {
    pub subset: Vec<Plan>,
    pub kept: BTreeMap<ObjectId, KeptWhole>,
}

/// SPEC: P7-OCR-010 — decide which embedded fonts can be subset, and to what.
/// Reads the document only; [`apply`] makes the change.
pub(crate) fn plan(doc: &Document) -> Plans {
    let files = font_files(doc);
    let mut plans = Plans::default();
    let Ok(walk) = Walk::run(doc) else {
        plans.kept = files.iter().map(|id| (*id, KeptWhole::UnreadableContent)).collect();
        return plans;
    };
    let index = Index {
        doc,
        referrers: referrers(doc),
        walk: &walk,
        form_fonts: form_field_fonts(doc),
    };
    for file in files {
        match index.plan_file(file) {
            Ok(plan) => plans.subset.push(plan),
            Err(reason) => {
                plans.kept.insert(file, reason);
            }
        }
    }
    plans
}

/// Replace each planned font file with its subset, and tag the fonts' names.
pub(crate) fn apply(doc: &mut Document, plans: &[Plan]) {
    for plan in plans {
        let Ok(Object::Stream(stream)) = doc.get_object(plan.file) else {
            continue;
        };
        let mut dict = stream.dict.clone();
        for key in [&b"Filter"[..], b"DecodeParms", b"DP", b"Length"] {
            dict.remove(key);
        }
        if plan.is_font_file2 {
            dict.set("Length1", i64::try_from(plan.bytes.len()).unwrap_or(i64::MAX));
        }
        doc.objects.insert(plan.file, Object::Stream(Stream::new(dict, plan.bytes.clone())));
        tag_names(doc, &plan.names);
        tracing::debug!(file = ?plan.file, glyphs = plan.glyphs.len(), "subset a font");
    }
}

/// Every stream a font descriptor embeds, of any kind.
pub(crate) fn font_files(doc: &Document) -> BTreeSet<ObjectId> {
    doc.objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .flat_map(|d| {
            [&b"FontFile"[..], b"FontFile2", b"FontFile3"]
                .into_iter()
                .filter_map(|key| d.get(key).and_then(Object::as_reference).ok())
        })
        .collect()
}

/// SPEC: P7-OCR-010 — whether every page of `after` renders exactly as `before`
/// does, annotations included. The guard: `Ok(false)` discards the subsetting.
///
/// # Errors
/// When `before` itself can't be opened, which is not this pass's to decide.
pub(crate) fn renders_identically(before: &[u8], after: &[u8]) -> Result<bool, CommandError> {
    use pdfium_render::prelude::PdfRenderConfig;

    let _guard = pdfium_lock()?;
    let engine = pdfium()?;
    let a = engine.load_pdf_from_byte_slice(before, None).map_err(CommandError::from)?;
    let Ok(b) = engine.load_pdf_from_byte_slice(after, None) else {
        return Ok(false);
    };
    if a.pages().len() != b.pages().len() {
        return Ok(false);
    }
    for index in 0..a.pages().len() {
        let (Ok(pa), Ok(pb)) = (a.pages().get(index), b.pages().get(index)) else {
            return Ok(false);
        };
        #[allow(clippy::cast_possible_truncation)] // a page is far narrower than i32::MAX pixels
        let width = ((pa.width().value / 72.0 * GUARD_DPI).ceil() as i32).max(1);
        let config = PdfRenderConfig::new().set_target_width(width).render_annotations(true);
        let (Ok(ra), Ok(rb)) = (pa.render_with_config(&config), pb.render_with_config(&config)) else {
            return Ok(false);
        };
        if ra.width() != rb.width() || ra.height() != rb.height() || ra.as_rgba_bytes() != rb.as_rgba_bytes() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A content stream the walk couldn't read.
#[derive(Debug)]
struct Unreadable;

/// A resource dictionary and the top-level object it lives in: the object a
/// reference written inside it belongs to.
#[derive(Clone, Copy)]
struct Scope<'a> {
    dict: &'a Dictionary,
    owner: ObjectId,
}

/// What the walk learned.
#[derive(Default)]
struct Walk {
    /// The strings shown in each font, by the font dictionary's object id.
    shown: HashMap<ObjectId, HashSet<Vec<u8>>>,
    /// Where the walk starts reading: pages, form `XObject`s, appearance
    /// streams, tiling patterns, Type3 fonts, and page-tree nodes whose
    /// resources pages inherit (only the pages below can use those).
    content: HashSet<ObjectId>,
    /// Objects read on the way from that content to a font: resource
    /// dictionaries, their `/Font`, `/ExtGState` and `/XObject` entries, and the
    /// page-tree nodes resources were inherited from.
    resources: HashSet<ObjectId>,
    /// Form `XObject`s already walked, with the font they inherited.
    forms_seen: HashSet<(ObjectId, Option<ObjectId>)>,
    /// Resource dictionaries already searched for Type3 fonts, patterns and
    /// soft masks, by the object they live in: one per object.
    scopes_seen: HashSet<ObjectId>,
}

impl Walk {
    fn run(doc: &Document) -> Result<Self, Unreadable> {
        let mut walk = Self::default();
        let mut walker = Walker { doc, walk: &mut walk };
        for page in doc.get_pages().into_values() {
            walker.page(page)?;
        }
        Ok(walk)
    }
}

struct Walker<'a, 'w> {
    doc: &'a Document,
    walk: &'w mut Walk,
}

impl<'a> Walker<'a, '_> {
    fn page(&mut self, page: ObjectId) -> Result<(), Unreadable> {
        let doc = self.doc;
        let dict = doc.get_dictionary(page).map_err(|_| Unreadable)?;
        self.walk.content.insert(page);
        let scope = self.page_scope(page, dict)?;

        let mut bytes = Vec::new();
        for id in doc.get_page_contents(page) {
            let stream = doc.get_object(id).and_then(Object::as_stream).map_err(|_| Unreadable)?;
            bytes.extend(stream.decompressed_content().map_err(|_| Unreadable)?);
            bytes.push(b'\n');
        }
        self.content(&bytes, scope, None, 0)?;

        let annots = match dict.get(b"Annots") {
            Ok(Object::Reference(id)) => doc.get_object(*id).and_then(Object::as_array).ok(),
            Ok(Object::Array(items)) => Some(items),
            _ => None,
        };
        for annot in annots.into_iter().flatten() {
            let annot = match annot {
                Object::Reference(id) => doc.get_dictionary(*id).ok(),
                Object::Dictionary(d) => Some(d),
                _ => None,
            };
            let Some(appearances) = annot.and_then(|a| a.get(b"AP").ok()) else {
                continue;
            };
            let appearances = match appearances {
                Object::Reference(id) => doc.get_dictionary(*id).ok(),
                Object::Dictionary(d) => Some(d),
                _ => None,
            };
            for (_, entry) in appearances.into_iter().flat_map(Dictionary::iter) {
                self.appearance(entry)?;
            }
        }
        Ok(())
    }

    /// An appearance entry: a stream, or a dictionary of streams by state.
    fn appearance(&mut self, entry: &Object) -> Result<(), Unreadable> {
        let doc = self.doc;
        let Object::Reference(id) = entry else {
            if let Object::Dictionary(states) = entry {
                for (_, state) in states {
                    self.appearance(state)?;
                }
            }
            return Ok(());
        };
        match doc.get_object(*id) {
            Ok(Object::Stream(stream)) => self.form(*id, stream, None, None, 1),
            Ok(Object::Dictionary(states)) => {
                for (_, state) in states {
                    self.appearance(state)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The resources a page draws with: its own, or the nearest ancestor's.
    fn page_scope(&mut self, page: ObjectId, dict: &'a Dictionary) -> Result<Option<Scope<'a>>, Unreadable> {
        let doc = self.doc;
        let (mut node, mut node_id) = (dict, page);
        for _ in 0..MAX_DEPTH {
            if let Ok(resources) = node.get(b"Resources") {
                self.walk.content.insert(node_id);
                return self.scope(resources, node_id, 0);
            }
            let Ok(parent) = node.get(b"Parent").and_then(Object::as_reference) else {
                return Ok(None);
            };
            node = doc.get_dictionary(parent).map_err(|_| Unreadable)?;
            node_id = parent;
        }
        Err(Unreadable)
    }

    /// Enter a resource dictionary written at `entry` inside `owner`. Searches
    /// it, once, for the content that draws without being called by name:
    /// Type3 glyphs, tiling patterns and soft masks.
    fn scope(&mut self, entry: &'a Object, owner: ObjectId, depth: usize) -> Result<Option<Scope<'a>>, Unreadable> {
        let Some(scope) = self.resolve(entry, owner) else {
            return Ok(None);
        };
        if !self.walk.scopes_seen.insert(scope.owner) {
            return Ok(Some(scope));
        }
        let doc = self.doc;

        if let Some(fonts) = self.entries(scope, b"Font") {
            for (_, font) in fonts.dict {
                let Ok(id) = font.as_reference() else { continue };
                let Ok(font) = doc.get_dictionary(id) else { continue };
                if font.get(b"Subtype").and_then(Object::as_name).is_ok_and(|s| s == b"Type3") {
                    self.walk.content.insert(id);
                    let inner = match font.get(b"Resources") {
                        Ok(resources) => self.scope(resources, id, depth + 1)?,
                        Err(_) => Some(scope),
                    };
                    let procs = match font.get(b"CharProcs") {
                        Ok(Object::Reference(p)) => doc.get_dictionary(*p).ok(),
                        Ok(Object::Dictionary(d)) => Some(d),
                        _ => None,
                    };
                    for (_, glyph) in procs.into_iter().flat_map(Dictionary::iter) {
                        let Ok(glyph_id) = glyph.as_reference() else { continue };
                        let stream = doc.get_object(glyph_id).and_then(Object::as_stream).map_err(|_| Unreadable)?;
                        let bytes = stream.decompressed_content().map_err(|_| Unreadable)?;
                        self.content(&bytes, inner, None, depth + 1)?;
                    }
                }
            }
        }
        if let Some(patterns) = self.entries(scope, b"Pattern") {
            for (_, pattern) in patterns.dict {
                let Ok(id) = pattern.as_reference() else { continue };
                if let Ok(Object::Stream(stream)) = doc.get_object(id) {
                    self.form(id, stream, None, None, depth + 1)?;
                }
            }
        }
        if let Some(states) = self.entries(scope, b"ExtGState") {
            for (_, state) in states.dict {
                let Some(state) = self.resolve(state, states.owner) else { continue };
                let Ok(mask) = state.dict.get(b"SMask") else { continue };
                let Some(mask) = self.resolve(mask, state.owner) else { continue };
                if let Ok(group) = mask.dict.get(b"G").and_then(Object::as_reference) {
                    if let Ok(Object::Stream(stream)) = doc.get_object(group) {
                        self.form(group, stream, None, None, depth + 1)?;
                    }
                }
            }
        }
        Ok(Some(scope))
    }

    /// `scope`'s dictionary of `key` resources, marked as read.
    fn entries(&mut self, scope: Scope<'a>, key: &[u8]) -> Option<Scope<'a>> {
        let entry = scope.dict.get(key).ok()?;
        self.resolve(entry, scope.owner)
    }

    /// A dictionary written at `entry` inside `owner`, and the object it lives
    /// in, marked as read.
    fn resolve(&mut self, entry: &'a Object, owner: ObjectId) -> Option<Scope<'a>> {
        let doc = self.doc;
        let scope = match entry {
            Object::Reference(id) => Scope { dict: doc.get_dictionary(*id).ok()?, owner: *id },
            Object::Dictionary(dict) => Scope { dict, owner },
            _ => return None,
        };
        self.walk.resources.insert(scope.owner);
        Some(scope)
    }

    /// A form `XObject` (or pattern, or soft-mask group): its content, with its
    /// own resources or, failing those, the caller's.
    fn form(
        &mut self,
        id: ObjectId,
        stream: &'a Stream,
        inherited: Option<Scope<'a>>,
        font: Option<ObjectId>,
        depth: usize,
    ) -> Result<(), Unreadable> {
        if depth > MAX_DEPTH {
            return Err(Unreadable);
        }
        if !self.walk.forms_seen.insert((id, font)) {
            return Ok(());
        }
        self.walk.content.insert(id);
        let scope = match stream.dict.get(b"Resources") {
            Ok(resources) => self.scope(resources, id, depth)?,
            Err(_) => inherited,
        };
        let bytes = stream.decompressed_content().map_err(|_| Unreadable)?;
        self.content(&bytes, scope, font, depth)
    }

    /// Read one content stream. Strict: a stream that doesn't parse to the end
    /// could hide a text operator after the point where parsing stopped.
    fn content(
        &mut self,
        bytes: &[u8],
        scope: Option<Scope<'a>>,
        mut font: Option<ObjectId>,
        depth: usize,
    ) -> Result<(), Unreadable> {
        let content = Content::decode_strict(bytes).map_err(|_| Unreadable)?;
        let mut saved = Vec::new();
        for op in &content.operations {
            match op.operator.as_str() {
                "q" => saved.push(font),
                "Q" => {
                    if let Some(previous) = saved.pop() {
                        font = previous;
                    }
                }
                "Tf" => font = self.named_font(scope, op.operands.first()),
                "gs" => {
                    if let Some(set) = self.state_font(scope, op.operands.first()) {
                        font = Some(set);
                    }
                }
                "Tj" | "'" => self.show(font, op.operands.first()),
                "\"" => self.show(font, op.operands.get(2)),
                "TJ" => {
                    if let Some(Object::Array(items)) = op.operands.first() {
                        for item in items {
                            self.show(font, Some(item));
                        }
                    }
                }
                "Do" => self.draw(scope, op.operands.first(), font, depth)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn show(&mut self, font: Option<ObjectId>, operand: Option<&Object>) {
        if let (Some(font), Some(Object::String(bytes, _))) = (font, operand) {
            self.walk.shown.entry(font).or_default().insert(bytes.clone());
        }
    }

    /// The font a `Tf` names. A font written directly in the resources has no
    /// object id, so its uses can't be recorded; it is never subset, because
    /// its font file is then referred to by something that isn't a font.
    fn named_font(&mut self, scope: Option<Scope<'a>>, name: Option<&Object>) -> Option<ObjectId> {
        let name = name?.as_name().ok()?;
        let fonts = self.entries(scope?, b"Font")?;
        fonts.dict.get(name).and_then(Object::as_reference).ok()
    }

    /// The font a `gs` sets, from the graphics state's `/Font [font size]`.
    fn state_font(&mut self, scope: Option<Scope<'a>>, name: Option<&Object>) -> Option<ObjectId> {
        let name = name?.as_name().ok()?;
        let states = self.entries(scope?, b"ExtGState")?;
        let state = self.resolve(states.dict.get(name).ok()?, states.owner)?;
        state.dict.get(b"Font").and_then(Object::as_array).ok()?.first()?.as_reference().ok()
    }

    fn draw(
        &mut self,
        scope: Option<Scope<'a>>,
        name: Option<&Object>,
        font: Option<ObjectId>,
        depth: usize,
    ) -> Result<(), Unreadable> {
        let doc = self.doc;
        let (Some(scope), Some(Ok(name))) = (scope, name.map(Object::as_name)) else {
            return Ok(());
        };
        let Some(xobjects) = self.entries(scope, b"XObject") else {
            return Ok(());
        };
        let Ok(id) = xobjects.dict.get(name).and_then(Object::as_reference) else {
            return Ok(());
        };
        let Ok(Object::Stream(stream)) = doc.get_object(id) else {
            return Ok(());
        };
        if stream.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|s| s == b"Form") {
            self.form(id, stream, Some(scope), font, depth + 1)?;
        }
        Ok(())
    }
}

/// For each object, the top-level objects whose value refers to it.
fn referrers(doc: &Document) -> HashMap<ObjectId, BTreeSet<ObjectId>> {
    fn collect(object: &Object, from: ObjectId, into: &mut HashMap<ObjectId, BTreeSet<ObjectId>>, depth: usize) {
        if depth > 64 {
            return;
        }
        match object {
            Object::Reference(id) => {
                into.entry(*id).or_default().insert(from);
            }
            Object::Array(items) => items.iter().for_each(|o| collect(o, from, into, depth + 1)),
            Object::Dictionary(dict) => dict.iter().for_each(|(_, o)| collect(o, from, into, depth + 1)),
            Object::Stream(stream) => stream.dict.iter().for_each(|(_, o)| collect(o, from, into, depth + 1)),
            _ => {}
        }
    }
    let mut into = HashMap::new();
    for (id, object) in &doc.objects {
        collect(object, *id, &mut into, 0);
    }
    into
}

/// The fonts a form field can type new text in: `/AcroForm /DR /Font`.
fn form_field_fonts(doc: &Document) -> HashSet<ObjectId> {
    fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
        match object {
            Object::Reference(id) => doc.get_dictionary(*id).ok(),
            Object::Dictionary(d) => Some(d),
            _ => None,
        }
    }
    let form = doc.catalog().ok().and_then(|catalog| catalog.get(b"AcroForm").ok());
    let dr = form.and_then(|f| resolve(doc, f)).and_then(|f| f.get(b"DR").ok());
    let fonts = dr.and_then(|d| resolve(doc, d)).and_then(|d| d.get(b"Font").ok());
    fonts
        .and_then(|f| resolve(doc, f))
        .map(|fonts| fonts.iter().filter_map(|(_, f)| f.as_reference().ok()).collect())
        .unwrap_or_default()
}

struct Index<'a> {
    doc: &'a Document,
    referrers: HashMap<ObjectId, BTreeSet<ObjectId>>,
    walk: &'a Walk,
    form_fonts: HashSet<ObjectId>,
}

impl Index<'_> {
    fn referrers_of(&self, id: ObjectId) -> Result<&BTreeSet<ObjectId>, KeptWhole> {
        self.referrers.get(&id).filter(|r| !r.is_empty()).ok_or(KeptWhole::Unsupported)
    }

    /// Plan one font file: find every font that uses it, prove each one's uses
    /// are all known, and subset to the union of their glyphs.
    fn plan_file(&self, file: ObjectId) -> Result<Plan, KeptWhole> {
        let doc = self.doc;
        let stream = doc.get_object(file).and_then(Object::as_stream).map_err(|_| KeptWhole::Unsupported)?;
        let mut is_font_file2 = None;
        let mut glyphs = BTreeSet::from([0u16]);
        let mut names = Vec::new();

        for &descriptor_id in self.referrers_of(file)? {
            let descriptor = doc.get_dictionary(descriptor_id).map_err(|_| KeptWhole::Unsupported)?;
            let refers = |key: &[u8]| descriptor.get(key).and_then(Object::as_reference).is_ok_and(|r| r == file);
            let font_file2 = if refers(b"FontFile2") {
                true
            } else if refers(b"FontFile3")
                && stream.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|s| s == b"OpenType")
            {
                false
            } else {
                return Err(KeptWhole::NotTrueType);
            };
            if *is_font_file2.get_or_insert(font_file2) != font_file2 {
                return Err(KeptWhole::Unsupported);
            }
            names.push((descriptor_id, &b"FontName"[..]));

            for &cid_font_id in self.referrers_of(descriptor_id)? {
                let cid_font = doc.get_dictionary(cid_font_id).map_err(|_| KeptWhole::Unsupported)?;
                let points_here = cid_font
                    .get(b"FontDescriptor")
                    .and_then(Object::as_reference)
                    .is_ok_and(|r| r == descriptor_id);
                let is_cid_type2 =
                    cid_font.get(b"Subtype").and_then(Object::as_name).is_ok_and(|s| s == b"CIDFontType2");
                if !points_here || !is_cid_type2 {
                    return Err(KeptWhole::Unsupported);
                }
                let to_gid = cid_to_gid(doc, cid_font)?;
                names.push((cid_font_id, &b"BaseFont"[..]));

                for &type0_id in self.referrers_of(cid_font_id)? {
                    let type0 = doc.get_dictionary(type0_id).map_err(|_| KeptWhole::Unsupported)?;
                    if !is_identity_type0(type0, cid_font_id) {
                        return Err(KeptWhole::Unsupported);
                    }
                    if self.form_fonts.contains(&type0_id) {
                        return Err(KeptWhole::FormFieldFont);
                    }
                    if !self.only_from_the_walk(type0_id) {
                        return Err(KeptWhole::OutsideTheWalk);
                    }
                    for shown in self.walk.shown.get(&type0_id).into_iter().flatten() {
                        glyphs.extend(cids(shown).map(|cid| to_gid.glyph(cid)));
                    }
                    names.push((type0_id, &b"BaseFont"[..]));
                }
            }
        }

        let program = stream.decompressed_content().map_err(|_| KeptWhole::NotTrueType)?;
        let face = ttf_parser::Face::parse(&program, 0).map_err(|_| KeptWhole::NotTrueType)?;
        if face.tables().glyf.is_none() {
            return Err(KeptWhole::NotTrueType);
        }
        let list: Vec<u16> = glyphs.iter().copied().collect();
        let bytes = subsetter::subset(&program, 0, subsetter::Profile::pdf(&list)).map_err(|_| KeptWhole::SubsetFailed)?;

        // As images are: kept only if smaller, here once deflated, since that
        // is how both end up in the file.
        let now = if stream.dict.get(b"Filter").is_ok() { stream.content.len() } else { deflated_len(&program) };
        if deflated_len(&bytes) >= now {
            return Err(KeptWhole::NotSmaller);
        }
        Ok(Plan { file, bytes, glyphs, is_font_file2: is_font_file2.unwrap_or(true), names })
    }

    /// Whether every object referring to `id` is one the walk read, or leads
    /// up, only through objects the walk read, to content it read.
    fn only_from_the_walk(&self, id: ObjectId) -> bool {
        let mut pending = vec![id];
        let mut checked = HashSet::new();
        while let Some(next) = pending.pop() {
            if !checked.insert(next) {
                continue;
            }
            let Some(referrers) = self.referrers.get(&next).filter(|r| !r.is_empty()) else {
                return false;
            };
            for &referrer in referrers {
                if self.walk.content.contains(&referrer) {
                    continue;
                }
                if !self.walk.resources.contains(&referrer) || checked.len() > 4096 {
                    return false;
                }
                pending.push(referrer);
            }
        }
        true
    }
}

/// A Type0 font over `cid_font` whose codes are CIDs: an `Identity` `CMap`.
fn is_identity_type0(type0: &Dictionary, cid_font: ObjectId) -> bool {
    let is_type0 = type0.get(b"Subtype").and_then(Object::as_name).is_ok_and(|s| s == b"Type0");
    let identity = type0
        .get(b"Encoding")
        .and_then(Object::as_name)
        .is_ok_and(|e| e == b"Identity-H" || e == b"Identity-V");
    let descends = matches!(
        type0.get(b"DescendantFonts"),
        Ok(Object::Array(fonts)) if fonts.len() == 1 && fonts[0].as_reference().is_ok_and(|r| r == cid_font)
    );
    is_type0 && identity && descends
}

/// A composite font's CID → glyph map.
enum CidToGid {
    Identity,
    Table(Vec<u8>),
}

impl CidToGid {
    fn glyph(&self, cid: u16) -> u16 {
        match self {
            Self::Identity => cid,
            Self::Table(table) => {
                let at = usize::from(cid) * 2;
                table.get(at..at + 2).map_or(0, |pair| u16::from_be_bytes([pair[0], pair[1]]))
            }
        }
    }
}

/// `/CIDToGIDMap`: `Identity` when absent (the default for `CIDFontType2`), or
/// a stream of big-endian glyph IDs indexed by CID.
fn cid_to_gid(doc: &Document, cid_font: &Dictionary) -> Result<CidToGid, KeptWhole> {
    match cid_font.get(b"CIDToGIDMap") {
        Err(_) => Ok(CidToGid::Identity),
        Ok(Object::Name(name)) if name == b"Identity" => Ok(CidToGid::Identity),
        Ok(Object::Reference(id)) => doc
            .get_object(*id)
            .and_then(Object::as_stream)
            .and_then(Stream::decompressed_content)
            .map(CidToGid::Table)
            .map_err(|_| KeptWhole::Unsupported),
        Ok(_) => Err(KeptWhole::Unsupported),
    }
}

/// The CIDs in a string shown with an `Identity` `CMap`: two bytes each. A
/// stray final byte is kept as the CID it would start, since keeping a glyph
/// too many costs only bytes.
fn cids(shown: &[u8]) -> impl Iterator<Item = u16> + '_ {
    shown.chunks(2).map(|pair| u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]))
}

/// The length `bytes` would have after the deflation pass.
fn deflated_len(bytes: &[u8]) -> usize {
    let mut stream = Stream::new(Dictionary::new(), bytes.to_vec());
    let _ = stream.compress();
    stream.content.len()
}

/// ISO 32000-1 §9.6.4: a subset font's name *shall* begin with a tag of six
/// capital letters and a plus sign. One tag for the whole font; a name that is
/// already tagged keeps its own.
fn tag_names(doc: &mut Document, names: &[(ObjectId, &[u8])]) {
    let tagged = |name: &[u8]| name.len() > 7 && name[..6].iter().all(u8::is_ascii_uppercase) && name[6] == b'+';
    let existing = names.iter().find_map(|(id, key)| {
        let name = doc.get_dictionary(*id).ok()?.get(key).and_then(Object::as_name).ok()?;
        tagged(name).then(|| name[..6].to_vec())
    });
    let tag = existing.unwrap_or_else(|| crate::pdf::font_embed_cid::subset_tag().into_bytes());
    for (id, key) in names {
        let Ok(Object::Dictionary(dict)) = doc.get_object_mut(*id) else { continue };
        let Ok(name) = dict.get(key).and_then(Object::as_name).map(<[u8]>::to_vec) else { continue };
        if !tagged(&name) {
            dict.set(*key, Object::Name([tag.as_slice(), b"+", name.as_slice()].concat()));
        }
    }
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    use std::fmt::Write;

    fn coptic() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf"))
            .expect("the committed Coptic font")
    }

    /// The glyph ID the Coptic font gives `ch`.
    fn gid(ch: char) -> u16 {
        let font = coptic();
        ttf_parser::Face::parse(&font, 0).unwrap().glyph_index(ch).expect("a Coptic glyph").0
    }

    /// `chars` as an `Identity-H` hex string of glyph IDs.
    fn shown(chars: &str) -> String {
        let hex = chars.chars().fold(String::new(), |mut hex, c| {
            let _ = write!(hex, "{:04X}", gid(c));
            hex
        });
        format!("<{hex}>")
    }

    /// A composite font embedding the whole Coptic font: (`Type0`, `CIDFont`, file).
    fn composite(doc: &mut Document, encoding: &str) -> (ObjectId, ObjectId, ObjectId) {
        let font = coptic();
        let file = doc.add_object(Stream::new(dictionary! { "Length1" => i64::try_from(font.len()).unwrap() }, font));
        let descriptor = doc.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => "NotoSansCoptic", "Flags" => 4,
            "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()], "ItalicAngle" => 0,
            "Ascent" => 1000, "Descent" => 0, "CapHeight" => 700, "StemV" => 80, "FontFile2" => file,
        });
        let cid = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "NotoSansCoptic",
            "CIDSystemInfo" => dictionary! {
                "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"), "Supplement" => 0,
            },
            "FontDescriptor" => descriptor, "CIDToGIDMap" => "Identity",
        });
        let type0 = Type0(doc, cid, encoding);
        (type0, cid, file)
    }

    #[allow(non_snake_case)]
    fn Type0(doc: &mut Document, cid: ObjectId, encoding: &str) -> ObjectId {
        doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "NotoSansCoptic",
            "Encoding" => Object::Name(encoding.as_bytes().to_vec()), "DescendantFonts" => vec![cid.into()],
        })
    }

    /// One page drawing `content` with `resources`, plus whatever `extra` adds.
    fn one_page(doc: &mut Document, content: &str, resources: Dictionary) -> ObjectId {
        let pages = doc.new_object_id();
        let contents = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => contents, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
        });
        doc.objects.insert(pages, Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }));
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        page
    }

    fn glyphs_of(plans: &Plans, file: ObjectId) -> BTreeSet<u16> {
        plans.subset.iter().find(|p| p.file == file).map_or_else(
            || panic!("{file:?} was not subset: {:?}", plans.kept.get(&file)),
            |p| p.glyphs.clone(),
        )
    }

    fn with_notdef(chars: &str) -> BTreeSet<u16> {
        std::iter::once(0).chain(chars.chars().map(gid)).collect()
    }

    #[test]
    fn every_text_operator_shows_glyphs_that_are_kept() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        let content = format!(
            "BT /F1 12 Tf 10 10 Td {} Tj [{} 50 {}] TJ {} ' 1 2 {} \" ET",
            shown("ⲁ"), shown("ⲃ"), shown("ⲅ"), shown("ⲇ"), shown("ⲉ")
        );
        one_page(&mut doc, &content, dictionary! { "Font" => dictionary! { "F1" => type0 } });
        assert_eq!(glyphs_of(&plan(&doc), file), with_notdef("ⲁⲃⲅⲇⲉ"));
    }

    // `Q` restores the font `q` saved, and a graphics state's /Font sets one.
    #[test]
    fn the_font_follows_q_q_and_gs() {
        let mut doc = Document::with_version("1.7");
        let (one, _, one_file) = composite(&mut doc, "Identity-H");
        let (two, _, two_file) = composite(&mut doc, "Identity-H");
        let state = doc.add_object(dictionary! { "Type" => "ExtGState", "Font" => vec![two.into(), 12.into()] });
        let content = format!(
            "BT /F1 12 Tf q /F2 12 Tf {} Tj Q {} Tj /GS1 gs {} Tj ET",
            shown("ⲁ"), shown("ⲃ"), shown("ⲅ")
        );
        let resources = dictionary! {
            "Font" => dictionary! { "F1" => one, "F2" => two },
            "ExtGState" => dictionary! { "GS1" => state },
        };
        one_page(&mut doc, &content, resources);
        let plans = plan(&doc);
        assert_eq!(glyphs_of(&plans, one_file), with_notdef("ⲃ"));
        assert_eq!(glyphs_of(&plans, two_file), with_notdef("ⲁⲅ"));
    }

    // A form XObject starts with the caller's graphics state, font included.
    #[test]
    fn a_form_xobject_draws_with_the_callers_font() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        let form = doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()], "Resources" => Dictionary::new() },
            format!("BT {} Tj ET", shown("ⲍ")).into_bytes(),
        ));
        let resources = dictionary! { "Font" => dictionary! { "F1" => type0 }, "XObject" => dictionary! { "X1" => form } };
        one_page(&mut doc, "BT /F1 12 Tf ET /X1 Do", resources);
        assert_eq!(glyphs_of(&plan(&doc), file), with_notdef("ⲍ"));
    }

    #[test]
    fn annotation_appearances_are_read() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        let appearance = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 100.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => type0 } },
            },
            format!("BT /F1 12 Tf {} Tj ET", shown("ⲏ")).into_bytes(),
        ));
        let annot = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "FreeText", "Rect" => vec![0.into(), 0.into(), 100.into(), 20.into()],
            "AP" => dictionary! { "N" => appearance },
        });
        let page = one_page(&mut doc, "", Dictionary::new());
        doc.get_dictionary_mut(page).unwrap().set("Annots", vec![annot.into()]);
        assert_eq!(glyphs_of(&plan(&doc), file), with_notdef("ⲏ"));
    }

    // Resources inherited from the page tree, as older producers write them.
    #[test]
    fn inherited_resources_are_read() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        let page = one_page(&mut doc, &format!("BT /F1 12 Tf {} Tj ET", shown("ⲑ")), Dictionary::new());
        let pages = doc.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
        doc.get_dictionary_mut(page).unwrap().remove(b"Resources");
        doc.get_dictionary_mut(pages).unwrap().set("Resources", dictionary! { "Font" => dictionary! { "F1" => type0 } });
        assert_eq!(glyphs_of(&plan(&doc), file), with_notdef("ⲑ"));
    }

    // Content that draws without being called by name: a tiling pattern, a
    // soft mask's group, and a Type3 font's glyph procedures. Each draws text
    // in a font of its own, and each glyph has to survive.
    #[test]
    fn patterns_soft_masks_and_type3_glyphs_are_read() {
        let mut doc = Document::with_version("1.7");
        let (in_pattern, _, pattern_file) = composite(&mut doc, "Identity-H");
        let (in_mask, _, mask_file) = composite(&mut doc, "Identity-H");
        let (in_glyph, _, glyph_file) = composite(&mut doc, "Identity-H");
        let form = |font: ObjectId, text: &str| {
            Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 50.into(), 50.into()],
                    "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
                },
                format!("BT /F1 12 Tf {} Tj ET", shown(text)).into_bytes(),
            )
        };
        let mut pattern = form(in_pattern, "ⲁ");
        pattern.dict.set("PatternType", 1);
        let pattern = doc.add_object(pattern);
        let group = doc.add_object(form(in_mask, "ⲃ"));
        let state = doc.add_object(dictionary! { "Type" => "ExtGState", "SMask" => dictionary! { "S" => "Luminosity", "G" => group } });
        let glyph = doc.add_object(Stream::new(Dictionary::new(), format!("0 0 d0 BT /F1 12 Tf {} Tj ET", shown("ⲅ")).into_bytes()));
        let type3 = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type3", "FontBBox" => vec![0.into(), 0.into(), 1.into(), 1.into()],
            "FontMatrix" => vec![0.001.into(), 0.into(), 0.into(), 0.001.into(), 0.into(), 0.into()],
            "CharProcs" => dictionary! { "a" => glyph }, "FirstChar" => 97, "LastChar" => 97, "Widths" => vec![1000.into()],
            "Encoding" => dictionary! { "Differences" => vec![97.into(), Object::Name(b"a".to_vec())] },
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => in_glyph } },
        });
        let resources = dictionary! {
            "Font" => dictionary! { "T3" => type3 },
            "Pattern" => dictionary! { "P1" => pattern },
            "ExtGState" => dictionary! { "GS1" => state },
        };
        one_page(&mut doc, "", resources);
        let plans = plan(&doc);
        assert_eq!(glyphs_of(&plans, pattern_file), with_notdef("ⲁ"));
        assert_eq!(glyphs_of(&plans, mask_file), with_notdef("ⲃ"));
        assert_eq!(glyphs_of(&plans, glyph_file), with_notdef("ⲅ"));
    }

    // Kept only if smaller: a font that is already the subset it would become
    // is left as it is, so compressing twice changes nothing the second time.
    #[test]
    fn a_subset_is_not_subset_again() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        one_page(&mut doc, &format!("BT /F1 12 Tf {} Tj ET", shown("ⲁⲃ")), dictionary! { "Font" => dictionary! { "F1" => type0 } });
        let plans = plan(&doc);
        apply(&mut doc, &plans.subset);
        assert_eq!(plan(&doc).kept.get(&file), Some(&KeptWhole::NotSmaller));
    }

    // Two Type0 fonts over one CIDFont share its file: the subset keeps both    // Two Type0 fonts over one CIDFont share its file: the subset keeps both
    // fonts' glyphs.
    #[test]
    fn a_shared_font_file_keeps_every_fonts_glyphs() {
        let mut doc = Document::with_version("1.7");
        let (horizontal, cid, file) = composite(&mut doc, "Identity-H");
        let vertical = Type0(&mut doc, cid, "Identity-V");
        let content = format!("BT /F1 12 Tf {} Tj /F2 12 Tf {} Tj ET", shown("ⲁ"), shown("ⲃ"));
        one_page(&mut doc, &content, dictionary! { "Font" => dictionary! { "F1" => horizontal, "F2" => vertical } });
        assert_eq!(glyphs_of(&plan(&doc), file), with_notdef("ⲁⲃ"));
    }

    // A form field types new text in its /DR font, with glyphs no page shows.
    #[test]
    fn a_form_field_font_is_left_whole() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        one_page(&mut doc, &format!("BT /F1 12 Tf {} Tj ET", shown("ⲁ")), dictionary! { "Font" => dictionary! { "F1" => type0 } });
        let form = doc.add_object(dictionary! { "Fields" => Vec::<Object>::new(), "DR" => dictionary! { "Font" => dictionary! { "F1" => type0 } } });
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        doc.get_dictionary_mut(root).unwrap().set("AcroForm", form);
        assert_eq!(plan(&doc).kept.get(&file), Some(&KeptWhole::FormFieldFont));
    }

    // An XObject no page draws still refers to the font: its uses aren't all
    // known, so the font stays whole.
    #[test]
    fn a_font_also_used_from_outside_the_walk_is_left_whole() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        one_page(&mut doc, &format!("BT /F1 12 Tf {} Tj ET", shown("ⲁ")), dictionary! { "Font" => dictionary! { "F1" => type0 } });
        doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form", "Resources" => dictionary! { "Font" => dictionary! { "F1" => type0 } } },
            format!("BT /F1 12 Tf {} Tj ET", shown("ⲃ")).into_bytes(),
        ));
        assert_eq!(plan(&doc).kept.get(&file), Some(&KeptWhole::OutsideTheWalk));
    }

    #[test]
    fn an_unreadable_content_stream_leaves_every_font_whole() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "Identity-H");
        one_page(&mut doc, "BT /F1 12 Tf (never closed", dictionary! { "Font" => dictionary! { "F1" => type0 } });
        let plans = plan(&doc);
        assert!(plans.subset.is_empty());
        assert_eq!(plans.kept.get(&file), Some(&KeptWhole::UnreadableContent));
    }

    #[test]
    fn other_cmaps_cff_and_type1_are_left_whole() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, file) = composite(&mut doc, "UniGB-UCS2-H");
        one_page(&mut doc, "BT /F1 12 Tf <0041> Tj ET", dictionary! { "Font" => dictionary! { "F1" => type0 } });
        assert_eq!(plan(&doc).kept.get(&file), Some(&KeptWhole::Unsupported));

        for (key, subtype) in [("FontFile", None), ("FontFile3", Some("Type1C"))] {
            let mut doc = Document::with_version("1.7");
            let mut dict = Dictionary::new();
            if let Some(subtype) = subtype {
                dict.set("Subtype", Object::Name(subtype.as_bytes().to_vec()));
            }
            let file = doc.add_object(Stream::new(dict, b"not a TrueType font".to_vec()));
            doc.add_object(dictionary! { "Type" => "FontDescriptor", "FontName" => "X", key => file });
            one_page(&mut doc, "", Dictionary::new());
            assert_eq!(plan(&doc).kept.get(&file), Some(&KeptWhole::NotTrueType), "{key}");
        }
    }

    #[test]
    fn codes_are_two_byte_cids_mapped_through_cid_to_gid() {
        assert_eq!(cids(&[0x00, 0x41, 0x01, 0x02]).collect::<Vec<_>>(), vec![0x0041, 0x0102]);
        // A stray final byte keeps the CID it would start.
        assert_eq!(cids(&[0x00, 0x41, 0x07]).collect::<Vec<_>>(), vec![0x0041, 0x0700]);
        let table = CidToGid::Table(vec![0, 0, 0, 9, 0x01, 0x02]);
        assert_eq!((table.glyph(1), table.glyph(2), table.glyph(3)), (9, 0x0102, 0));
        assert_eq!(CidToGid::Identity.glyph(77), 77);
    }

    // ISO 32000-1 §9.6.4: one tag across the font, its CIDFont and descriptor,
    // and a name already tagged keeps its own.
    #[test]
    fn the_subset_is_tagged_once() {
        let mut doc = Document::with_version("1.7");
        let (type0, cid, _) = composite(&mut doc, "Identity-H");
        one_page(&mut doc, &format!("BT /F1 12 Tf {} Tj ET", shown("ⲁ")), dictionary! { "Font" => dictionary! { "F1" => type0 } });
        let plans = plan(&doc);
        apply(&mut doc, &plans.subset);
        let name = |id: ObjectId, key: &[u8]| doc.get_dictionary(id).unwrap().get(key).unwrap().as_name().unwrap().to_vec();
        let base = name(type0, b"BaseFont");
        assert_eq!(&base[6..], b"+NotoSansCoptic", "{}", String::from_utf8_lossy(&base));
        assert!(base[..6].iter().all(u8::is_ascii_uppercase));
        assert_eq!(name(cid, b"BaseFont"), base);

        let mut doc = Document::with_version("1.7");
        let (type0, _, _) = composite(&mut doc, "Identity-H");
        doc.get_dictionary_mut(type0).unwrap().set("BaseFont", Object::Name(b"ABCDEF+NotoSansCoptic".to_vec()));
        one_page(&mut doc, &format!("BT /F1 12 Tf {} Tj ET", shown("ⲁ")), dictionary! { "Font" => dictionary! { "F1" => type0 } });
        let plans = plan(&doc);
        apply(&mut doc, &plans.subset);
        let descriptor = plans.subset[0].names.iter().find(|(_, k)| *k == b"FontName").unwrap().0;
        let font_name = doc.get_dictionary(descriptor).unwrap().get(b"FontName").unwrap().as_name().unwrap().to_vec();
        assert_eq!(font_name, b"ABCDEF+NotoSansCoptic");
    }

    fn saved(doc: &mut Document) -> Vec<u8> {
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    // The guard has to tell a page that lost a glyph from one that didn't.
    #[test]
    fn the_guard_sees_a_dropped_glyph() {
        let mut doc = Document::with_version("1.7");
        let (type0, _, _) = composite(&mut doc, "Identity-H");
        one_page(
            &mut doc,
            &format!("BT /F1 48 Tf 20 80 Td {} Tj ET", shown("ⲁⲃⲅ")),
            dictionary! { "Font" => dictionary! { "F1" => type0 } },
        );
        let before = saved(&mut doc.clone());

        let mut subset = doc.clone();
        let mut plans = plan(&subset).subset;
        assert_eq!(plans.len(), 1);
        apply(&mut subset, &plans);
        assert!(renders_identically(&before, &saved(&mut subset)).unwrap(), "a correct subset changed the page");

        plans[0].bytes = subsetter::subset(&coptic(), 0, subsetter::Profile::pdf(&[0])).unwrap();
        let mut broken = doc.clone();
        apply(&mut broken, &plans);
        assert!(!renders_identically(&before, &saved(&mut broken)).unwrap(), "the dropped glyphs went unnoticed");
    }
}
