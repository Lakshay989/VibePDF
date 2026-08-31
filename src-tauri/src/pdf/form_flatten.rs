//! Form flatten (P5.C2) — bake the fields into the page, drop the interactivity.
//!
//! SPEC: P5-FORM-010 — "WHEN the user flattens a form, THE system SHALL render
//! each field's current appearance into the page content and remove the
//! interactive field definitions."
//!
//! ## Why this isn't just [`crate::pdf::flatten`] with a filter
//!
//! Field widgets *are* annotations, so the P3-ANN-011 machinery (bake `/AP /N`
//! into `/Contents`, drop from `/Annots`) is exactly the second half of this job.
//! The first half exists because of how filling works: [`crate::pdf::form`]'s
//! text and choice writers set `/V`, **delete the stale `/AP`**, and flip
//! `/NeedAppearances true` — deliberately, so the viewer regenerates the look
//! from the value (PDF 32000-1 §12.7.2). That is the right call for an
//! interactive form and the wrong one for flatten: there is no appearance stream
//! left to bake, so a naive widget-flatten would silently drop every value the
//! user typed.
//!
//! So flatten runs an appearance pass first: for each text/choice field with a
//! value but no usable `/AP` (or a stale one, under `/NeedAppearances`), build a
//! self-contained form `XObject` from `/V` + `/DA` and hang it off the widget.
//! Buttons need none of this — their `/AP /N` is pre-baked per state and selected
//! by `/AS`, which is why `set_button_field` never touches `/NeedAppearances`.
//!
//! Everything after that is bookkeeping: bake, drop every remaining widget
//! (hidden and appearance-less ones included — they were never visible, so they
//! leave no content behind), remove the catalog's `/AcroForm` (which takes
//! `/XFA` with it), and prune.
//!
//! Undo is in-session only, the same byte-snapshot inverse P3.E2's flatten uses.

use lopdf::{Dictionary, Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::free_text_appearance;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::flatten::flatten_annots_where;
use crate::pdf::form::{
    dict_rect, field_kind, field_widget_ids, inherited, node_dict, parse_opt, terminal_field_ids,
    FF_MULTILINE, FF_MULTISELECT,
};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

#[allow(clippy::needless_pass_by_value)]
fn lop(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("form flatten: {e}"))
}

/// `/F` bit 2 (1-indexed) — the annotation is hidden, so it renders nothing.
const F_HIDDEN: i64 = 1 << 1;

/// SPEC: P5-FORM-010 — flatten every interactive form field in `bytes` into the
/// page content and remove the `AcroForm`. Non-widget annotations (markup, links)
/// survive as live annotations; only the form becomes static.
pub fn flatten_form(bytes: &[u8]) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(lop)?;

    synthesize_field_appearances(&mut doc)?;
    // Bake the widgets; keep everything that isn't one, and skip hidden widgets
    // (nothing was visible to bake — they're removed by the sweep below).
    flatten_annots_where(&mut doc, &|d| !is_widget(d) || is_hidden(d))?;
    remove_widget_annots(&mut doc);
    remove_acroform(&mut doc)?;
    doc.prune_objects();

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

fn is_widget(d: &Dictionary) -> bool {
    d.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Widget")
}

fn is_hidden(d: &Dictionary) -> bool {
    d.get(b"F").and_then(Object::as_i64).is_ok_and(|f| f & F_HIDDEN != 0)
}

// ── appearance pass ─────────────────────────────────────────────────────────

/// One widget that needs an appearance built, with everything the builder needs
/// resolved *before* `doc` is borrowed mutably.
struct Pending {
    widget: ObjectId,
    rect: [f32; 4],
    text: String,
    base: String,
    size: f32,
    color: (f32, f32, f32),
}

/// SPEC: P5-FORM-010 — give every valued text/choice widget a bakeable `/AP /N`.
/// Read pass (resolve values + style + geometry), then write pass (build the
/// appearance forms), because [`free_text_appearance`] needs `&mut Document`.
fn synthesize_field_appearances(doc: &mut Document) -> Result<(), CommandError> {
    let stale = need_appearances(doc);
    let mut pending: Vec<Pending> = Vec::new();

    for field in terminal_field_ids(doc) {
        let Ok(dict) = doc.get_dictionary(field) else { continue };
        let Some(kind) = field_kind(doc, dict) else { continue };
        if !matches!(kind, "text" | "combo" | "list") {
            continue;
        }
        let Some(text) = display_value(doc, dict, kind) else { continue };
        if text.is_empty() {
            continue;
        }
        let (base, da_size, color) = field_style(doc, dict);
        let multiline = flags(doc, dict) & FF_MULTILINE != 0;

        for widget in field_widget_ids(doc, field) {
            let Ok(wd) = doc.get_dictionary(widget) else { continue };
            // A usable, non-stale appearance from the producer is left alone.
            if !stale && has_appearance_stream(doc, wd) {
                continue;
            }
            let Some(rect) = dict_rect(wd, b"Rect").map(normalized) else { continue };
            let size = resolve_size(da_size, rect, multiline);
            pending.push(Pending {
                widget,
                rect,
                text: text.clone(),
                base: base.clone(),
                size,
                color,
            });
        }
    }

    for p in pending {
        let (ap, _da) =
            free_text_appearance(doc, p.rect, &p.text, &p.base, p.size, p.color, false)?;
        doc.get_dictionary_mut(p.widget).map_err(lop)?.set("AP", Object::Dictionary(ap));
    }
    Ok(())
}

/// Whether the `AcroForm` declares its stored appearances stale.
fn need_appearances(doc: &Document) -> bool {
    crate::pdf::cos::acroform_dict(doc)
        .ok()
        .flatten()
        .and_then(|a| a.get(b"NeedAppearances").ok().and_then(|o| o.as_bool().ok()))
        .unwrap_or(false)
}

/// Whether this widget's `/AP /N` resolves to an actual stream (not a state
/// sub-dictionary, which only buttons use).
fn has_appearance_stream(doc: &Document, widget: &Dictionary) -> bool {
    let Some(ap) = widget.get(b"AP").ok().and_then(|o| node_dict(doc, o)) else { return false };
    match ap.get(b"N") {
        Ok(Object::Reference(id)) => matches!(doc.get_object(*id), Ok(Object::Stream(_))),
        Ok(Object::Stream(_)) => true,
        _ => false,
    }
}

fn flags(doc: &Document, dict: &Dictionary) -> i64 {
    inherited(doc, dict, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0)
}

/// The text a viewer would draw for this field: the value for a text field, the
/// *labels* of the selected options for a choice field. `None` when there's no
/// value at all. Multi-select list values stack one per line.
fn display_value(doc: &Document, dict: &Dictionary, kind: &str) -> Option<String> {
    let v = inherited(doc, dict, b"V")?;
    let raw = value_strings(v);
    if kind == "text" {
        return Some(raw.join("\n"));
    }
    let options = parse_opt(doc, inherited(doc, dict, b"Opt"));
    let labels: Vec<String> = raw
        .iter()
        .map(|export| {
            options
                .iter()
                .find(|o| o.export == *export)
                .map_or_else(|| export.clone(), |o| o.label.clone())
        })
        .collect();
    let sep = if flags(doc, dict) & FF_MULTISELECT != 0 { "\n" } else { ", " };
    Some(labels.join(sep))
}

/// A `/V` as a list of strings — a text string, a Name, or an array of either.
fn value_strings(v: &Object) -> Vec<String> {
    match v {
        Object::String(s, _) => vec![crate::pdf::form::decode_pdf_text_string(s)],
        Object::Name(n) => vec![crate::pdf::form::decode_pdf_text_string(n)],
        Object::Array(a) => a.iter().flat_map(value_strings).collect(),
        _ => Vec::new(),
    }
}

/// The field's `(base font, /DA size, colour)`. Falls back to the `AcroForm`'s
/// default `/DA`, then to 12pt black Helvetica. A size of `0.0` means "auto",
/// resolved against the widget box by [`resolve_size`].
fn field_style(doc: &Document, dict: &Dictionary) -> (String, f32, (f32, f32, f32)) {
    let da = inherited(doc, dict, b"DA")
        .and_then(|o| o.as_str().ok().map(<[u8]>::to_vec))
        .or_else(|| {
            crate::pdf::cos::acroform_dict(doc)
                .ok()
                .flatten()
                .and_then(|a| a.get(b"DA").ok().and_then(|o| o.as_str().ok().map(<[u8]>::to_vec)))
        })
        .unwrap_or_default();
    let (res_name, size, color) = parse_da(&String::from_utf8_lossy(&da));
    (resolve_base_font(doc, res_name.as_deref()), size, color)
}

/// Parse a `/DA` fragment (`"/Helv 10 Tf 0 g"`) into its font resource name, size
/// and colour. Operands are read backwards from each operator, which is all a
/// `/DA` ever is — a handful of `Tf` / `g` / `rg` / `k` calls, no state.
fn parse_da(da: &str) -> (Option<String>, f32, (f32, f32, f32)) {
    let toks: Vec<&str> = da.split_whitespace().collect();
    let num = |i: usize| -> f32 { toks.get(i).and_then(|t| t.parse::<f32>().ok()).unwrap_or(0.0) };

    let mut font = None;
    let mut size = 0.0;
    let mut color = (0.0, 0.0, 0.0);
    for (i, tok) in toks.iter().enumerate() {
        match *tok {
            "Tf" if i >= 2 => {
                font = Some(toks[i - 2].trim_start_matches('/').to_owned());
                size = num(i - 1);
            }
            "g" if i >= 1 => {
                let v = num(i - 1);
                color = (v, v, v);
            }
            "rg" if i >= 3 => color = (num(i - 3), num(i - 2), num(i - 1)),
            "k" if i >= 4 => {
                let (c, m, y, k) = (num(i - 4), num(i - 3), num(i - 2), num(i - 1));
                color = ((1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k));
            }
            _ => {}
        }
    }
    (font, size, color)
}

/// Map a `/DA` font resource name to a base-14 family: through the `AcroForm`
/// `/DR /Font /<name> /BaseFont` when it resolves, else the conventional short
/// names a mainstream reader writes, else Helvetica. The appearance carries its own font
/// dictionary, so it never depends on `/DR` surviving the flatten.
fn resolve_base_font(doc: &Document, res_name: Option<&str>) -> String {
    let Some(name) = res_name else { return "Helvetica".to_owned() };
    let from_dr = crate::pdf::cos::acroform_dict(doc)
        .ok()
        .flatten()
        .and_then(|a| a.get(b"DR").ok().and_then(|o| node_dict(doc, o)).cloned())
        .and_then(|dr| dr.get(b"Font").ok().and_then(|o| node_dict(doc, o)).cloned())
        .and_then(|fonts| fonts.get(name.as_bytes()).ok().and_then(|o| node_dict(doc, o)).cloned())
        .and_then(|f| f.get(b"BaseFont").and_then(Object::as_name).ok().map(<[u8]>::to_vec))
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    if let Some(base) = from_dr {
        // Subset prefixes ("ABCDEF+Helvetica") aren't base-14 names.
        return base.split_once('+').map_or(base.clone(), |(_, rest)| rest.to_owned());
    }
    match name {
        "HeBo" => "Helvetica-Bold",
        "HeOb" => "Helvetica-Oblique",
        "TiRo" => "Times-Roman",
        "TiBo" => "Times-Bold",
        "TiIt" => "Times-Italic",
        "Cour" => "Courier",
        "CoBo" => "Courier-Bold",
        _ => "Helvetica",
    }
    .to_owned()
}

/// Resolve the `/DA` size against the widget box. `0` means auto-size: fit the
/// box height for a single line (capped at 12pt, the size a mainstream reader converges on
/// for ordinary field heights), a fixed 10pt for a multi-line box where the line
/// count — not the height — sets the scale.
fn resolve_size(da_size: f32, rect: [f32; 4], multiline: bool) -> f32 {
    if da_size > 0.0 {
        return da_size;
    }
    if multiline {
        return 10.0;
    }
    let h = rect[3] - rect[1];
    (h * 0.66).clamp(4.0, 12.0)
}

fn normalized(r: [f32; 4]) -> [f32; 4] {
    [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])]
}

// ── teardown ────────────────────────────────────────────────────────────────

/// Drop every remaining `/Widget` from every page's `/Annots` — the hidden and
/// appearance-less ones the bake pass left live. Nothing was rendered for them,
/// so removing them loses no visible content.
fn remove_widget_annots(doc: &mut Document) {
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page_id in page_ids {
        let annots = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
            Some(Object::Array(a)) => a,
            Some(Object::Reference(id)) => {
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
            }
            _ => continue,
        };
        let kept: Vec<Object> = annots
            .into_iter()
            .filter(|o| {
                let d = o.as_reference().ok().and_then(|id| doc.get_dictionary(id).ok());
                !d.is_some_and(is_widget)
            })
            .collect();
        let Ok(pd) = doc.get_dictionary_mut(page_id) else { continue };
        if kept.is_empty() {
            pd.remove(b"Annots");
        } else {
            pd.set("Annots", Object::Array(kept));
        }
    }
}

/// SPEC: P5-FORM-010 — "remove the interactive field definitions": drop the
/// catalog's `/AcroForm` (stored either as a reference or inline). The field
/// dictionaries themselves become unreachable and are pruned by the caller;
/// `/XFA`, which hangs off `/AcroForm`, goes with it.
fn remove_acroform(doc: &mut Document) -> Result<(), CommandError> {
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(lop)?;
    doc.get_dictionary_mut(root).map_err(lop)?.remove(b"AcroForm");
    Ok(())
}

// ── undoable edit ───────────────────────────────────────────────────────────

/// SPEC: P5-FORM-010 — flatten the form as one undoable edit. The inverse is a
/// pre-flatten byte snapshot, so it's undoable *in-session* but permanent once
/// the file is saved and reopened — the same contract as P3.E2's flatten.
pub struct FlattenFormEdit;

impl<'a> Edit<PdfDocument<'a>> for FlattenFormEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = flatten_form(&pre_bytes)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?.load_pdf_from_byte_vec(new_bytes, None).map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "flatten-form"
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::{normalized, parse_da, resolve_size};

    #[test]
    fn parses_font_size_and_gray_da() {
        let (font, size, color) = parse_da("/Helv 10 Tf 0 g");
        assert_eq!(font.as_deref(), Some("Helv"));
        assert_eq!(size, 10.0);
        assert_eq!(color, (0.0, 0.0, 0.0));
    }

    #[test]
    fn parses_rgb_da_in_any_order() {
        // Colour before the font selector — legal, and a mainstream reader writes both orders.
        let (font, size, color) = parse_da("0 0 1 rg /TiRo 12 Tf");
        assert_eq!(font.as_deref(), Some("TiRo"));
        assert_eq!(size, 12.0);
        assert_eq!(color, (0.0, 0.0, 1.0));
    }

    #[test]
    fn parses_cmyk_da() {
        // Pure cyan → (0, 1, 1) in RGB.
        let (_, _, color) = parse_da("/Helv 8 Tf 1 0 0 0 k");
        assert_eq!(color, (0.0, 1.0, 1.0));
    }

    #[test]
    fn empty_da_falls_back_to_auto_black() {
        let (font, size, color) = parse_da("");
        assert!(font.is_none());
        assert_eq!(size, 0.0);
        assert_eq!(color, (0.0, 0.0, 0.0));
    }

    #[test]
    fn auto_size_fits_the_box_and_caps_at_twelve() {
        // 18pt-tall field → ~11.9pt; a tall field is still capped at 12.
        assert!((resolve_size(0.0, [0.0, 0.0, 100.0, 18.0], false) - 11.88).abs() < 0.01);
        assert_eq!(resolve_size(0.0, [0.0, 0.0, 100.0, 90.0], false), 12.0);
        assert_eq!(resolve_size(0.0, [0.0, 0.0, 100.0, 2.0], false), 4.0);
    }

    #[test]
    fn explicit_size_wins_over_auto() {
        assert_eq!(resolve_size(7.5, [0.0, 0.0, 100.0, 90.0], false), 7.5);
    }

    #[test]
    fn multiline_auto_size_is_fixed() {
        assert_eq!(resolve_size(0.0, [0.0, 0.0, 100.0, 200.0], true), 10.0);
    }

    #[test]
    fn rect_corners_are_ordered() {
        assert_eq!(normalized([110.0, 70.0, 10.0, 20.0]), [10.0, 20.0, 110.0, 70.0]);
    }
}
