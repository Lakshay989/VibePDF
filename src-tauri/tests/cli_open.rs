//! SPEC: P1-VIEW-001 — CLI-arg parser. Pure function, no IO, no Tauri.
//!
//! The existence filter (`Path::is_file()`) runs at the call site in
//! `lib.rs::setup`, so these tests cover only the extension + argv0
//! discipline that the parser is responsible for.

use vibepdf_lib::commands::cli::pdf_paths_from_args;

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn filters_pdf_args_case_insensitively() {
    // .pdf, .PDF, .PdF all kept; .txt dropped.
    let got = pdf_paths_from_args(args(&["vibepdf", "/a.pdf", "/b.txt", "/c.PDF", "/d.PdF"]));
    assert_eq!(got, vec!["/a.pdf", "/c.PDF", "/d.PdF"]);
}

#[test]
fn drops_non_pdf_and_argv0() {
    // argv[0] is the binary; it must never be opened, even if it
    // happens to end in .pdf (unlikely but defensive).
    let got = pdf_paths_from_args(args(&["target/debug/vibepdf.pdf", "/x.txt", "/y.pdf"]));
    assert_eq!(got, vec!["/y.pdf"]);
}

#[test]
fn preserves_arg_order() {
    let got = pdf_paths_from_args(args(&[
        "vibepdf", "/c.pdf", "/a.pdf", "/b.pdf",
    ]));
    // Tab order at launch follows command-line order, not sort order.
    assert_eq!(got, vec!["/c.pdf", "/a.pdf", "/b.pdf"]);
}

#[test]
fn empty_args_yield_empty() {
    // Nothing at all (corner case — `std::env::args` always has argv0
    // in practice, but the parser shouldn't assume).
    let empty: Vec<String> = Vec::new();
    assert!(pdf_paths_from_args(empty).is_empty());
}

#[test]
fn only_argv0_yields_empty() {
    // Normal launch with no PDF args.
    let got = pdf_paths_from_args(args(&["vibepdf"]));
    assert!(got.is_empty());
}

#[test]
fn bare_dot_pdf_filename_kept() {
    // ".pdf" alone is exactly 4 chars and case-insensitively matches.
    // Edge case: shouldn't crash on the boundary.
    let got = pdf_paths_from_args(args(&["vibepdf", ".pdf", "x"]));
    assert_eq!(got, vec![".pdf"]);
}
