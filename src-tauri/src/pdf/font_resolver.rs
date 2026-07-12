//! Font fallback resolution (P4.A2) — the honesty gate for text editing.
//!
//! SPEC: P4-EDIT-002 — "IF the original font is not embedded and not installed
//! on the system, THEN THE system SHALL substitute the closest match from a
//! fallback stack (Helvetica → Arial → sans-serif), warn the user once per
//! document, and offer to re-flow the affected text run."
//!
//! This module is **pure logic**: given the `(font name, embedded?)` pairs a
//! document uses (collected by [`crate::pdf::text_extract::collect_document_fonts`]),
//! it decides per font whether editing it is safe (the glyphs are present) or
//! lossy (we'll substitute a base-14 face). The only side effect is a one-time
//! directory scan of the OS font folders in [`load_system_fonts`]; everything
//! else — and all the tests — runs against an injected index.
//!
//! **Why a heuristic, not a font parser.** Deciding "installed on the system"
//! precisely means parsing every system font's `name` table for its family.
//! That needs a font-parsing dependency we deliberately don't take (see
//! `docs/03_TECH_STACK.md`). Instead we match on the normalized file *stem*.
//! It's approximate, so the bias is deliberate: **when unsure, warn** — never
//! silently substitute and pretend the result is identical (the roadmap's hard
//! rule). The base-14 faces every viewer ships are always treated as safe.

use std::collections::HashSet;
use std::sync::OnceLock;

/// A set of normalized font keys (see [`normalize_font_key`]) for the faces
/// installed on this machine. Built once by [`load_system_fonts`].
pub type SystemFontIndex = HashSet<String>;

/// How a document font maps onto what we can actually render when editing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FontStatus {
    /// Glyphs are embedded in the file — edits are lossless.
    Embedded,
    /// A PDF base-14 face (Helvetica/Times/Courier/Symbol/ZapfDingbats and the
    /// Arial/Times-New-Roman/Courier-New aliases every viewer ships). Safe.
    Standard,
    /// Not embedded, but a same-named face is installed locally. Safe *here*,
    /// though a different machine might still substitute.
    SystemAvailable,
    /// Not embedded and not found — editing this run substitutes [`substitute`]
    /// and is therefore lossy. Triggers the once-per-document warning.
    ///
    /// [`substitute`]: FontResolution::substitute
    Fallback,
}

/// One document font's resolution, as the frontend banner consumes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontResolution {
    /// The font name as it appears in the file (subset tag already stripped).
    pub font_name: String,
    /// Whether the file embeds this font's glyphs.
    pub embedded: bool,
    /// The resolution outcome.
    pub status: FontStatus,
    /// The base-14 face we'd substitute, present only when `status == Fallback`.
    pub substitute: Option<String>,
}

/// The document-wide font report: one entry per distinct font, plus a rolled-up
/// flag the UI uses to decide whether to raise the once-per-document banner.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontReport {
    pub fonts: Vec<FontResolution>,
    /// True iff any font resolved to [`FontStatus::Fallback`].
    pub needs_fallback: bool,
}

/// SPEC: P4-EDIT-002 — resolve a single font against the system index. Pure;
/// the index is injected so this is fully testable without touching the disk.
#[must_use]
pub fn resolve_font(name: &str, embedded: bool, system: &SystemFontIndex) -> FontResolution {
    let status = if embedded {
        FontStatus::Embedded
    } else if is_standard_14(name) {
        FontStatus::Standard
    } else if system.contains(&normalize_font_key(name)) {
        FontStatus::SystemAvailable
    } else {
        FontStatus::Fallback
    };
    let substitute = match status {
        FontStatus::Fallback => Some(fallback_substitute(name)),
        _ => None,
    };
    FontResolution {
        font_name: name.to_owned(),
        embedded,
        status,
        substitute,
    }
}

/// SPEC: P4-EDIT-002 — build the document report from collected fonts, scanning
/// the OS font folders once. Dedups by name (first occurrence wins).
#[must_use]
pub fn build_font_report(fonts: Vec<(String, bool)>) -> FontReport {
    build_font_report_with(fonts, load_system_fonts())
}

/// The pure core of [`build_font_report`], with the system index injected so
/// tests don't depend on what's installed on the CI machine.
#[must_use]
pub fn build_font_report_with(fonts: Vec<(String, bool)>, system: &SystemFontIndex) -> FontReport {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    for (name, embedded) in fonts {
        if !seen.insert(name.clone()) {
            continue; // same font referenced on multiple pages — report once
        }
        resolved.push(resolve_font(&name, embedded, system));
    }
    let needs_fallback = resolved.iter().any(|r| r.status == FontStatus::Fallback);
    FontReport {
        fonts: resolved,
        needs_fallback,
    }
}

/// Style tokens we peel off a normalized name so a weight/slant variant matches
/// its family (`arialboldmt` → `arial`, `timesnewromanpsmt` → `timesnewroman`).
// "roman"/"book" are deliberately absent: they're part of family names
// ("Times New Roman") as often as they're weights, and stripping them would
// break the family match (`timesnewroman` → `timesnew`).
const STYLE_SUFFIXES: &[&str] = &[
    "psmt", "mt", "ps", "bolditalic", "boldoblique", "bold", "italic", "oblique", "regular",
    "semibold", "demibold", "medium", "light", "black", "heavy", "thin", "condensed", "narrow",
];

/// Normalize a font name (or a system font's file stem) to a comparison key:
/// lowercase, alphanumerics only, with trailing style words stripped. Applied
/// identically to both sides of a match so `Arial-BoldMT` and a `Arial Bold.ttf`
/// file collapse to the same `arial` key.
#[must_use]
pub fn normalize_font_key(name: &str) -> String {
    // Drop a subset tag if one survived (`ABCDEF+Calibri` → `Calibri`).
    let base = name.rsplit('+').next().unwrap_or(name);
    let mut key: String = base
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();
    // Peel known style suffixes off the end until none remain.
    loop {
        let trimmed = STYLE_SUFFIXES
            .iter()
            .find_map(|suf| key.strip_suffix(suf).map(str::to_owned));
        match trimmed {
            // Don't strip the whole string away (e.g. a face literally named
            // "Bold") — keep the last token so the key stays non-empty.
            Some(rest) if !rest.is_empty() => key = rest,
            _ => break,
        }
    }
    key
}

/// The base-14 family keys plus the aliases every PDF viewer is guaranteed to
/// have. A non-embedded run in one of these is safe to edit anywhere.
fn is_standard_14(name: &str) -> bool {
    const STANDARD: &[&str] = &[
        "helvetica",
        "arial",
        "times",
        "timesroman", // base-14 "Times-Roman" normalizes here (no "roman" strip)
        "timesnewroman",
        "courier",
        "couriernew",
        "symbol",
        "zapfdingbats",
    ];
    let key = normalize_font_key(name);
    STANDARD.contains(&key.as_str())
}

/// SPEC: P4-EDIT-002 — pick the closest base-14 substitute for a missing font,
/// preserving weight/slant. Sans (and unknown) → Helvetica, serif → Times,
/// monospace → Courier; the spec's "Helvetica → Arial → sans-serif" chain is
/// all the same base-14 Helvetica metrics, so Helvetica is the representative.
#[must_use]
pub fn fallback_substitute(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let bold = lower.contains("bold") || lower.contains("black") || lower.contains("heavy");
    let italic = lower.contains("italic") || lower.contains("oblique");

    let serif = ["times", "georgia", "garamond", "serif", "roman", "minion", "book", "century"]
        .iter()
        .any(|s| lower.contains(s));
    let mono = ["courier", "mono", "consol", "menlo", "code"]
        .iter()
        .any(|s| lower.contains(s));

    if mono {
        match (bold, italic) {
            (true, true) => "Courier-BoldOblique",
            (true, false) => "Courier-Bold",
            (false, true) => "Courier-Oblique",
            (false, false) => "Courier",
        }
    } else if serif {
        match (bold, italic) {
            (true, true) => "Times-BoldItalic",
            (true, false) => "Times-Bold",
            (false, true) => "Times-Italic",
            (false, false) => "Times-Roman",
        }
    } else {
        match (bold, italic) {
            (true, true) => "Helvetica-BoldOblique",
            (true, false) => "Helvetica-Bold",
            (false, true) => "Helvetica-Oblique",
            (false, false) => "Helvetica",
        }
    }
    .to_owned()
}

/// Scan the OS font directories once and cache the normalized key set for the
/// life of the process. Offline-first: pure local filesystem, no network.
#[must_use]
pub fn load_system_fonts() -> &'static SystemFontIndex {
    static CACHE: OnceLock<SystemFontIndex> = OnceLock::new();
    CACHE.get_or_init(scan_system_fonts)
}

/// Best-effort: locate a system TrueType font likely to cover `text`'s scripts
/// and return its bytes, for embedding via [`crate::pdf::font_embed`]
/// (`FABLE_REVIEW` 3.2 stage-2). Tracer-grade — it tries a short ordered list of
/// known broad-coverage faces by path per OS and returns the first present.
///
/// It does **not** yet verify per-glyph coverage (that needs the font-parsing
/// dependency `docs/03_TECH_STACK.md` avoids), so a chosen face missing an exotic
/// script would render `.notdef` boxes; precise per-script selection — and
/// bundling a guaranteed-coverage face — is deliberate follow-up. The `text`
/// argument is threaded now so that follow-up needs no signature change.
#[must_use]
pub fn covering_font_bytes(text: &str) -> Option<Vec<u8>> {
    let _ = text; // reserved for per-script selection (see doc comment)
    broad_coverage_font_paths()
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
}

/// Ordered candidate paths for a broad-Unicode system face, most-covering first.
fn broad_coverage_font_paths() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut paths = Vec::new();
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"));
        paths.push(PathBuf::from("/Library/Fonts/Arial Unicode.ttf"));
        paths.push(PathBuf::from("/System/Library/Fonts/Supplemental/NotoSans-Regular.ttf"));
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf"));
        paths.push(PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"));
        paths.push(PathBuf::from("/usr/share/fonts/noto/NotoSans-Regular.ttf"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("SystemRoot") {
            paths.push(PathBuf::from(&windir).join("Fonts/arialuni.ttf"));
            paths.push(PathBuf::from(&windir).join("Fonts/seguisym.ttf"));
            paths.push(PathBuf::from(windir).join("Fonts/arial.ttf"));
        }
    }
    paths
}

fn font_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut dirs = Vec::new();
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(&home).join(".fonts"));
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("SystemRoot") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
        }
    }
    dirs
}

/// Walk the font dirs (bounded depth + file count, so a pathological directory
/// can't stall document open) and collect normalized stems.
fn scan_system_fonts() -> SystemFontIndex {
    const FONT_EXTS: &[&str] = &["ttf", "otf", "ttc", "otc", "dfont"];
    const MAX_FILES: usize = 20_000;

    let mut index = SystemFontIndex::new();
    let mut stack: Vec<(std::path::PathBuf, u32)> = font_dirs().into_iter().map(|d| (d, 0)).collect();
    let mut visited = 0usize;

    while let Some((dir, depth)) = stack.pop() {
        if depth > 6 || visited >= MAX_FILES {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue; // missing/unreadable font dir — just skip it
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited >= MAX_FILES {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            let is_font = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| FONT_EXTS.contains(&e.to_ascii_lowercase().as_str()));
            if !is_font {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let key = normalize_font_key(stem);
                if !key.is_empty() {
                    index.insert(key);
                }
            }
        }
    }
    index
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{
        build_font_report_with, fallback_substitute, normalize_font_key, resolve_font, FontStatus,
        SystemFontIndex,
    };

    fn empty_index() -> SystemFontIndex {
        SystemFontIndex::new()
    }

    #[test]
    fn embedded_font_is_lossless() {
        let r = resolve_font("ACMECo+Calibri", true, &empty_index());
        assert_eq!(r.status, FontStatus::Embedded);
        assert!(r.substitute.is_none());
    }

    #[test]
    fn base_14_is_standard_even_when_not_embedded() {
        for name in ["Helvetica", "Helvetica-Bold", "Arial-BoldMT", "TimesNewRomanPSMT", "Courier"]
        {
            let r = resolve_font(name, false, &empty_index());
            assert_eq!(r.status, FontStatus::Standard, "{name} should be standard");
            assert!(r.substitute.is_none());
        }
    }

    #[test]
    fn unknown_non_embedded_falls_back() {
        let r = resolve_font("Calibri", false, &empty_index());
        assert_eq!(r.status, FontStatus::Fallback);
        assert_eq!(r.substitute.as_deref(), Some("Helvetica"));
    }

    #[test]
    fn locally_installed_font_is_system_available() {
        let mut idx = empty_index();
        idx.insert(normalize_font_key("Calibri"));
        let r = resolve_font("Calibri-Bold", false, &idx);
        assert_eq!(r.status, FontStatus::SystemAvailable);
        assert!(r.substitute.is_none());
    }

    #[test]
    fn substitute_matches_family_and_style() {
        assert_eq!(fallback_substitute("Calibri"), "Helvetica");
        assert_eq!(fallback_substitute("Calibri-Bold"), "Helvetica-Bold");
        assert_eq!(fallback_substitute("Georgia-Italic"), "Times-Italic");
        assert_eq!(fallback_substitute("Garamond-BoldItalic"), "Times-BoldItalic");
        assert_eq!(fallback_substitute("Consolas"), "Courier");
        assert_eq!(fallback_substitute("Menlo-BoldItalic"), "Courier-BoldOblique");
    }

    #[test]
    fn normalize_collapses_style_variants_to_family() {
        assert_eq!(normalize_font_key("Arial-BoldMT"), "arial");
        assert_eq!(normalize_font_key("TimesNewRomanPSMT"), "timesnewroman");
        assert_eq!(normalize_font_key("Calibri"), "calibri");
        // A face literally named "Bold" keeps a non-empty key.
        assert_eq!(normalize_font_key("Bold"), "bold");
    }

    #[test]
    fn report_dedups_and_rolls_up_flag() {
        let fonts = vec![
            ("Helvetica".to_owned(), false), // standard
            ("Calibri".to_owned(), false),   // fallback
            ("Calibri".to_owned(), false),   // dup — dropped
            ("ACME+Embedded".to_owned(), true),
        ];
        let report = build_font_report_with(fonts, &empty_index());
        assert_eq!(report.fonts.len(), 3, "duplicate Calibri collapsed");
        assert!(report.needs_fallback);
    }

    #[test]
    fn report_with_no_missing_fonts_needs_no_fallback() {
        let fonts = vec![("Helvetica".to_owned(), false), ("ACME+X".to_owned(), true)];
        let report = build_font_report_with(fonts, &empty_index());
        assert!(!report.needs_fallback);
    }
}
