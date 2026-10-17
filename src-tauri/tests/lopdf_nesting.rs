//! GHSA-6fj4-rcr3-r7rr — lopdf before 0.42 parsed nested arrays and
//! dictionaries by unbounded recursion, so a small file nested deeply enough
//! overflowed the stack. That is an abort, not a panic: nothing can catch it,
//! and VibePDF parses every file it opens to look for signatures, so opening
//! one closed the app. lopdf 0.42 limits the depth.
//!
//! The property is only that these calls *return*. Whether they return a
//! parse error or a document with the offending object dropped is lopdf's
//! business; the process going away is ours. A regression here fails as a
//! crashed test binary rather than as an assertion.

use std::path::PathBuf;
use std::time::SystemTime;

use vibepdf_lib::pdf::merge::merge_documents;
use vibepdf_lib::security::verify::verify_signatures;

/// Far deeper than any real document, and deep enough to have overflowed a
/// test thread's stack under the old recursive parser.
const DEPTH: usize = 100_000;

/// A one-page PDF whose catalog carries an array nested `DEPTH` levels deep,
/// with a correct cross-reference table so every parser reaches it.
fn deeply_nested_pdf() -> Vec<u8> {
    let nested = format!("{}{}", "[".repeat(DEPTH), "]".repeat(DEPTH));
    let objects = [
        format!("<< /Type /Catalog /Pages 2 0 R /Nested {nested} >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_string(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
    );
    out
}

// The open path: every opened document is checked for signatures.
#[test]
fn checking_signatures_on_a_deeply_nested_file_returns() {
    let _ = verify_signatures(&deeply_nested_pdf(), SystemTime::now());
}

// Merge reads its inputs straight from disk into lopdf.
#[test]
fn merging_a_deeply_nested_file_returns() {
    let dir = std::env::temp_dir().join(format!("vibepdf-nesting-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let nested = dir.join("nested.pdf");
    std::fs::write(&nested, deeply_nested_pdf()).expect("write");
    let hello = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/basic/hello.pdf");

    let _ = merge_documents(&[hello, nested], &dir.join("merged.pdf"));
    let _ = std::fs::remove_dir_all(&dir);
}
