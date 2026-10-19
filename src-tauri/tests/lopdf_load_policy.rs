//! Production code reaches lopdf only through `pdf::document::load_cos`.
//!
//! A document that needs a password does not make lopdf fail. 0.36 returns its
//! objects still encrypted; 0.42 logs a warning and returns `Ok` with no
//! objects at all. Either way the caller carries on: a panel reads empty, an
//! edit fails on a page that "doesn't exist", a signature check finds no
//! signatures. `load_cos` turns that into an error that says what happened, and
//! this test keeps every new call site going through it.
//!
//! A source scan rather than a clippy `disallowed-methods` rule because test
//! code — 40-odd files of it — builds raw fixtures with lopdf on purpose, and
//! that lint has no test exemption. Each file is checked up to its first
//! `#[cfg(test)]`, the layout every module here follows.

use std::path::{Path, PathBuf};

/// Calls that parse bytes or files into a `lopdf::Document`.
const LOADERS: &[&str] = &[
    "load_mem(",
    "load_mem_with_options(",
    "load_mem_with_password(",
    "Document::load(",
    "load_with_password(",
    "load_with_options(",
    "load_from(",
];

/// The files allowed to call lopdf's loaders directly, and why.
const ALLOWED: &[(&str, &str)] = &[
    ("src/pdf/document.rs", "defines load_cos"),
    (
        "src/security/decrypt.rs",
        "Unlock needs the encryption dictionary of a file it cannot decrypt yet, which is exactly what load_cos refuses",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `(line number, line)` for every loader call outside comments, before the
/// file's tests.
fn direct_loads(source: &str) -> Vec<(usize, String)> {
    source
        .lines()
        .enumerate()
        .take_while(|(_, line)| line.trim() != "#[cfg(test)]")
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .filter(|(_, line)| LOADERS.iter().any(|l| line.contains(l)))
        .map(|(i, line)| (i + 1, line.trim().to_string()))
        .collect()
}

#[test]
fn production_code_loads_pdfs_only_through_load_cos() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    assert!(files.len() > 50, "found only {} source files — is the scan looking in the right place?", files.len());

    let mut offenders = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(root).expect("under the crate").to_string_lossy().replace('\\', "/");
        if ALLOWED.iter().any(|(allowed, _)| *allowed == rel) {
            continue;
        }
        let source = std::fs::read_to_string(file).expect("read source");
        for (line, text) in direct_loads(&source) {
            offenders.push(format!("{rel}:{line}: {text}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "load PDFs through `crate::pdf::document::load_cos`, not lopdf directly:\n  {}",
        offenders.join("\n  ")
    );
}

// The scan itself, on inputs with a known answer — a scan that matches nothing
// would pass the test above forever.
#[test]
fn the_scan_finds_calls_and_skips_comments_and_tests() {
    let source = "\
use lopdf::Document;
// Document::load_mem(bytes) in a comment is fine
fn f(b: &[u8]) { let _ = Document::load_mem(b); }
fn g(b: &[u8]) { let _ = lopdf::Document::load_mem_with_options(b, Default::default()); }
#[cfg(test)]
mod tests { fn h(b: &[u8]) { let _ = lopdf::Document::load_mem(b); } }
";
    let found: Vec<usize> = direct_loads(source).into_iter().map(|(line, _)| line).collect();
    assert_eq!(found, vec![3, 4]);
}
