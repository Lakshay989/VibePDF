//! System fonts indexed by family and style (P7.C1b1), from the names inside
//! each font file.
//!
//! [`crate::pdf::font_resolver`] matches file *stems* on purpose: it is an
//! edit-time honesty gate and errs towards warning. Laying out a Word document
//! needs more — "Arial, bold, italic" has to become one exact face, a file and
//! a collection index, because a bold run drawn with the regular face is a
//! formatting change the person never made.
//!
//! **Cheap to build.** Only each file's table directory and its `name` and
//! `OS/2` tables are read — a few kilobytes — never whole fonts, so a few
//! hundred system fonts index in milliseconds even though some are hundreds of
//! megabytes. Faces without TrueType outlines (`glyf`) are left out: the CID
//! embedding writes `FontFile2`, which can only carry those.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One face in one font file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceRef {
    pub path: PathBuf,
    /// Index within a `.ttc` collection; 0 for a single font.
    pub index: u32,
    pub family: String,
    /// Usually `Regular`, `Bold`, `Italic`, `Bold Italic`.
    pub style: String,
    pub weight: u16,
    pub italic: bool,
}

impl FaceRef {
    #[must_use]
    pub fn bold(&self) -> bool {
        self.weight >= 600
    }
}

/// Every usable face, by lower-cased family name.
#[derive(Debug, Default)]
pub struct FontIndex {
    by_family: HashMap<String, Vec<FaceRef>>,
}

/// The broad class a family belongs to, for substituting one that is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Sans,
    Serif,
    Mono,
}

/// Installed families tried, in order, when a document's family is missing.
/// Metric-compatible substitutes come first where they exist (Carlito for
/// Calibri, Liberation for the Microsoft core fonts).
const FALLBACKS: [(Class, &[&str]); 3] = [
    (Class::Sans, &["Carlito", "Arial", "Liberation Sans", "Helvetica Neue", "Helvetica", "DejaVu Sans", "Noto Sans"]),
    (Class::Serif, &["Times New Roman", "Liberation Serif", "Georgia", "Times", "DejaVu Serif", "Noto Serif"]),
    (Class::Mono, &["Courier New", "Liberation Mono", "Menlo", "Consolas", "DejaVu Sans Mono", "Noto Sans Mono"]),
];

/// The installed-family fallbacks for `class`, in order.
#[must_use]
pub fn fallback_families(class: Class) -> &'static [&'static str] {
    FALLBACKS.iter().find(|(c, _)| *c == class).map_or(&[], |(_, names)| names)
}

/// Which class a family name most likely is. Unknown families are sans, the
/// default of every modern word processor.
#[must_use]
pub fn class_of(family: &str) -> Class {
    const MONO: [&str; 9] = ["mono", "courier", "consolas", "menlo", "monaco", "lucida console", "code", "typewriter", "fixed"];
    const SERIF: [&str; 20] = [
        "times", "georgia", "cambria", "garamond", "palatino", "book antiqua", "baskerville", "constantia",
        "serif", "minion", "caslon", "bookman", "didot", "bodoni", "hoefler", "century", "goudy", "charter",
        "cochin", "new york",
    ];
    let name = family.to_lowercase();
    if MONO.iter().any(|m| name.contains(m)) {
        Class::Mono
    } else if SERIF.iter().any(|s| name.contains(s)) && !name.contains("sans") {
        Class::Serif
    } else {
        Class::Sans
    }
}

impl FontIndex {
    /// Index every font file under `dirs`, recursively.
    #[must_use]
    pub fn scan(dirs: &[PathBuf]) -> Self {
        let mut index = Self::default();
        let mut pending: Vec<PathBuf> = dirs.to_vec();
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path
                    .extension()
                    .is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "ttf" | "ttc" | "otf"))
                {
                    for face in read_faces(&path) {
                        index.by_family.entry(face.family.to_lowercase()).or_default().push(face);
                    }
                }
            }
        }
        for faces in index.by_family.values_mut() {
            faces.sort_by(|a, b| a.path.cmp(&b.path).then(a.index.cmp(&b.index)));
        }
        index
    }

    /// The system's fonts, scanned once per process.
    pub fn system() -> &'static Self {
        static SYSTEM: OnceLock<FontIndex> = OnceLock::new();
        SYSTEM.get_or_init(|| Self::scan(&system_font_dirs()))
    }

    #[must_use]
    pub fn has_family(&self, family: &str) -> bool {
        self.by_family.contains_key(&family.to_lowercase())
    }

    /// The face of `family` closest to `bold` / `italic`: an exact match, then
    /// the right slant at any weight, then the right weight, then anything.
    #[must_use]
    pub fn face(&self, family: &str, bold: bool, italic: bool) -> Option<&FaceRef> {
        let faces = self.by_family.get(&family.to_lowercase())?;
        // Lower is better: slant matters more than weight, and among faces of
        // the right weight class the one nearest 400 / 700 wins.
        faces.iter().min_by_key(|f| {
            let target = if bold { 700 } else { 400 };
            let slant = u32::from(f.italic != italic) * 10_000;
            let class = u32::from(f.bold() != bold) * 1_000;
            slant + class + u32::from(f.weight.abs_diff(target))
        })
    }

    /// `family` if installed, otherwise the first installed fallback of its
    /// class. The second value names the family actually used when it differs.
    #[must_use]
    pub fn resolve(&self, family: &str, bold: bool, italic: bool) -> Option<(&FaceRef, Option<String>)> {
        if let Some(face) = self.face(family, bold, italic) {
            return Some((face, None));
        }
        let class = class_of(family);
        let order = FALLBACKS
            .iter()
            .filter(|(c, _)| *c == class)
            .chain(FALLBACKS.iter().filter(|(c, _)| *c != class))
            .flat_map(|(_, names)| names.iter());
        for name in order {
            if let Some(face) = self.face(name, bold, italic) {
                return Some((face, Some(face.family.clone())));
            }
        }
        None
    }
}

/// Where each platform keeps fonts, system-wide and per user.
#[must_use]
pub fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = &home {
            dirs.push(home.join("Library/Fonts"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(windir) = std::env::var_os("SystemRoot") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = &home {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    let _ = &home;
    dirs
}

/// Read `length` bytes at `offset`, refusing anything implausibly large for
/// the small tables read here.
fn read_at(file: &mut File, offset: u64, length: usize) -> Option<Vec<u8>> {
    if length > 1 << 20 {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buffer = vec![0; length];
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Every TrueType-outline face in the file at `path`.
fn read_faces(path: &Path) -> Vec<FaceRef> {
    let Ok(mut file) = File::open(path) else { return Vec::new() };
    let Some(header) = read_at(&mut file, 0, 12) else { return Vec::new() };
    let offsets: Vec<u64> = if &header[..4] == b"ttcf" {
        let count = be32(&header, 8).unwrap_or(0).min(256);
        let Some(table) = read_at(&mut file, 12, 4 * count as usize) else { return Vec::new() };
        (0..count as usize).filter_map(|i| be32(&table, i * 4)).map(u64::from).collect()
    } else {
        vec![0]
    };
    offsets
        .into_iter()
        .zip(0u32..)
        .filter_map(|(offset, index)| read_face(&mut file, path, offset, index))
        .collect()
}

fn read_face(file: &mut File, path: &Path, offset: u64, index: u32) -> Option<FaceRef> {
    let header = read_at(file, offset, 12)?;
    // 0x00010000 or 'true' is TrueType; 'OTTO' is CFF, which `FontFile2` can't carry.
    if !(header[..4] == [0, 1, 0, 0] || &header[..4] == b"true") {
        return None;
    }
    let tables = be16(&header, 4)?;
    let directory = read_at(file, offset + 12, 16 * usize::from(tables))?;
    let mut name = None;
    let mut os2 = None;
    let mut has_glyf = false;
    for record in directory.chunks_exact(16) {
        let location = (u64::from(be32(record, 8)?), be32(record, 12)? as usize);
        match &record[..4] {
            b"name" => name = Some(location),
            b"OS/2" => os2 = Some(location),
            b"glyf" => has_glyf = true,
            _ => {}
        }
    }
    if !has_glyf {
        return None;
    }
    let (name_at, name_len) = name?;
    let name_bytes = read_at(file, name_at, name_len)?;
    let names = ttf_parser::name::Table::parse(&name_bytes)?;

    // Prefer the typographic names (16/17) a modern font carries for families
    // with more than four styles; fall back to the classic ones (1/2).
    let find = |ids: [u16; 2]| ids.iter().find_map(|&id| english_name(names.names, id));
    let family = find([16, 1])?;
    let style = find([17, 2]).unwrap_or_else(|| "Regular".into());

    let (weight, italic) = match os2.and_then(|(at, len)| read_at(file, at, len)) {
        Some(bytes) => match ttf_parser::os2::Table::parse(&bytes) {
            Some(table) => (table.weight().to_number(), table.style() != ttf_parser::Style::Normal),
            None => style_from_name(&style),
        },
        None => style_from_name(&style),
    };
    Some(FaceRef { path: path.to_path_buf(), index, family, style, weight, italic })
}

/// Mac OS Roman, bytes 0x80–0xFF.
const MAC_ROMAN_HIGH: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{a0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{f8ff}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

/// Name `id` in English, from whichever record has it.
///
/// `ttf-parser` decodes only Unicode records, and some fonts keep their
/// English names nowhere else but in the old Macintosh records: macOS's own
/// `Times.ttc` names its family "Times" only there, with a Chinese style name
/// as its sole Windows record. So: Windows English, then Unicode, then
/// Macintosh English (Mac Roman), then any Windows record.
pub(crate) fn english_name(names: ttf_parser::name::Names<'_>, id: u16) -> Option<String> {
    use ttf_parser::PlatformId;
    let records: Vec<ttf_parser::name::Name<'_>> = names.into_iter().filter(|n| n.name_id == id && !n.name.is_empty()).collect();
    let pick = |test: &dyn Fn(&ttf_parser::name::Name<'_>) -> bool| records.iter().find(|n| test(n));
    let decoded = |n: &ttf_parser::name::Name<'_>| -> Option<String> {
        if n.platform_id == PlatformId::Macintosh && n.encoding_id == 0 {
            let high: Vec<char> = MAC_ROMAN_HIGH.chars().collect();
            return Some(n.name.iter().map(|&b| if b < 0x80 { char::from(b) } else { high.get(usize::from(b - 0x80)).copied().unwrap_or('?') }).collect());
        }
        n.to_string()
    };
    let found = [
        pick(&|n| n.platform_id == PlatformId::Windows && n.language_id == 0x0409),
        pick(&|n| n.platform_id == PlatformId::Unicode),
        pick(&|n| n.platform_id == PlatformId::Macintosh && n.encoding_id == 0 && n.language_id == 0),
        pick(&|n| n.platform_id == PlatformId::Windows),
    ]
    .into_iter()
    .flatten()
    .find_map(decoded)
    .filter(|s| !s.trim().is_empty());
    found
}

/// Weight and slant guessed from a style name, for a font without `OS/2`.
fn style_from_name(style: &str) -> (u16, bool) {
    let lower = style.to_lowercase();
    let weight = if lower.contains("bold") || lower.contains("black") || lower.contains("heavy") { 700 } else { 400 };
    (weight, lower.contains("italic") || lower.contains("oblique"))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn face(family: &str, weight: u16, italic: bool) -> FaceRef {
        FaceRef {
            path: PathBuf::from(format!("/{family}-{weight}-{italic}.ttf")),
            index: 0,
            family: family.into(),
            style: String::new(),
            weight,
            italic,
        }
    }

    fn index(faces: Vec<FaceRef>) -> FontIndex {
        let mut index = FontIndex::default();
        for f in faces {
            index.by_family.entry(f.family.to_lowercase()).or_default().push(f);
        }
        index
    }

    #[test]
    fn the_closest_style_wins_and_slant_matters_most() {
        let idx = index(vec![face("Arial", 400, false), face("Arial", 700, false), face("Arial", 400, true)]);
        assert_eq!(idx.face("arial", true, false).map(|f| (f.weight, f.italic)), Some((700, false)));
        assert_eq!(idx.face("Arial", false, true).map(|f| (f.weight, f.italic)), Some((400, true)));
        // No bold italic: the italic is closer than the upright bold.
        assert_eq!(idx.face("Arial", true, true).map(|f| (f.weight, f.italic)), Some((400, true)));
    }

    #[test]
    fn a_missing_family_falls_back_within_its_class_and_says_so() {
        let idx = index(vec![face("Arial", 400, false), face("Times New Roman", 400, false), face("Courier New", 400, false)]);
        let used = |family: &str| idx.resolve(family, false, false).map(|(f, sub)| (f.family.clone(), sub));
        assert_eq!(used("Calibri"), Some(("Arial".into(), Some("Arial".into()))));
        assert_eq!(used("Cambria"), Some(("Times New Roman".into(), Some("Times New Roman".into()))));
        assert_eq!(used("Consolas"), Some(("Courier New".into(), Some("Courier New".into()))));
        assert_eq!(used("Arial"), Some(("Arial".into(), None)), "an installed family is not a substitution");
    }

    #[test]
    fn classes_are_guessed_from_names() {
        assert_eq!(class_of("Times New Roman"), Class::Serif);
        assert_eq!(class_of("Source Code Pro"), Class::Mono);
        assert_eq!(class_of("Noto Serif CJK"), Class::Serif);
        assert_eq!(class_of("PT Sans Caption"), Class::Sans);
        assert_eq!(class_of("Aptos"), Class::Sans);
        assert_eq!(class_of("Didot"), Class::Serif);
    }

    // Against the real system: the families the tests and CI rely on are found
    // with their four styles, from their own name tables.
    // Times names itself only in Macintosh records (see `english_name`).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_family_named_only_in_mac_records_is_found() {
        let face = FontIndex::system().face("Times", true, false).expect("Times is installed on macOS");
        assert!(face.bold() && !face.italic, "{face:?}");
    }

    #[test]
    fn mac_roman_is_exactly_the_upper_half() {
        assert_eq!(MAC_ROMAN_HIGH.chars().count(), 128);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_index_finds_arial_in_four_styles() {
        let idx = FontIndex::system();
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let f = idx.face("Arial", bold, italic).expect("Arial is installed on macOS");
            assert_eq!((f.bold(), f.italic), (bold, italic), "{f:?}");
        }
    }
}
