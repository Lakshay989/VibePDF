//! Integration tests for page numbers (P4.D4).
//!
//! SPEC: P4-EDIT-011 — stamp a page number in the header/footer margin, in a
//! chosen format, from a starting number, skipping excluded pages. Content-stream
//! assertions are at the lopdf level; undo runs through the actor.

use std::path::PathBuf;

use lopdf::Document;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::page_numbers::add_page_numbers;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

/// The decoded, concatenated content of page `page_no` (1-based).
fn page_content(bytes: &[u8], page_no: u32) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&page_no).expect("page exists");
    String::from_utf8_lossy(&doc.get_page_content(page_id).expect("content")).into_owned()
}

/// Locate `(needle) Tj` in `content` and return its `(x, y)` from the preceding
/// `x y Td`.
fn show_pos(content: &str, needle: &str) -> Option<(f32, f32)> {
    let marker = format!("({needle}) Tj");
    let at = content.find(&marker)?;
    let td = content[..at].rfind(" Td")?;
    let mut it = content[..td].split_whitespace().rev();
    let y: f32 = it.next()?.parse().ok()?;
    let x: f32 = it.next()?.parse().ok()?;
    Some((x, y))
}

/// The output re-parses cleanly and keeps its page count. (The *PDFium* round-trip
/// — the "no silent breakage" constraint — is exercised by `actor_page_numbers_*`,
/// which reloads through PDFium on apply; the crate's PDFium lock is private to
/// the library, so byte-level tests verify via lopdf like the sibling suites.)
fn reloads_with_pages(pdf: &[u8], expected: usize) {
    let doc = Document::load_mem(pdf).expect("output re-parses");
    assert_eq!(doc.get_pages().len(), expected, "page count preserved");
}

#[test]
fn decimal_footer_numbers_every_page() {
    let out = add_page_numbers(
        &bytes("many-pages.pdf"), &[], "footer", "center", "decimal", 1, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("page numbers");
    assert!(page_content(&out, 1).contains("(1) Tj"), "page 1 shows 1");
    assert!(page_content(&out, 2).contains("(2) Tj"), "page 2 shows 2");
    assert!(page_content(&out, 50).contains("(50) Tj"), "page 50 shows 50");
    reloads_with_pages(&out, 50);
}

#[test]
fn start_number_offsets_the_sequence() {
    let out = add_page_numbers(
        &bytes("many-pages.pdf"), &[], "footer", "center", "decimal", 5, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("page numbers");
    assert!(page_content(&out, 1).contains("(5) Tj"), "page 1 shows the start value 5");
    assert!(page_content(&out, 2).contains("(6) Tj"), "page 2 shows 6");
}

#[test]
fn excluded_pages_are_not_stamped_and_sequence_holds() {
    // Skip pages 1 and 3 (0-based 0 and 2); page 2 must still read "2".
    let out = add_page_numbers(
        &bytes("many-pages.pdf"), &[0, 2], "footer", "center", "decimal", 1, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("page numbers");
    // The fixture draws its own page text, so key off our /VibePDF tag, not any Tj.
    assert!(
        !page_content(&out, 1).contains("/Kind (page-number)"),
        "page 1 (excluded) has no page-number stamp"
    );
    assert!(page_content(&out, 2).contains("(2) Tj"), "page 2 keeps its number");
    assert!(
        !page_content(&out, 3).contains("/Kind (page-number)"),
        "page 3 (excluded) has no page-number stamp"
    );
    assert!(page_content(&out, 4).contains("(4) Tj"), "page 4 keeps its number (no shift)");
}

#[test]
fn page_x_of_n_uses_the_total() {
    let out = add_page_numbers(
        &bytes("many-pages.pdf"), &[], "footer", "center", "page-x-of-n", 1, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("page numbers");
    assert!(page_content(&out, 1).contains("(Page 1 of 50) Tj"), "page 1");
    assert!(page_content(&out, 50).contains("(Page 50 of 50) Tj"), "page 50");
}

#[test]
fn roman_and_alpha_render() {
    let roman = add_page_numbers(
        &bytes("many-pages.pdf"), &[], "footer", "center", "lower-roman", 1, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("roman");
    assert!(page_content(&roman, 4).contains("(iv) Tj"), "page 4 is iv");
    let alpha = add_page_numbers(
        &bytes("many-pages.pdf"), &[], "header", "right", "upper-alpha", 1, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("alpha");
    assert!(page_content(&alpha, 27).contains("(AA) Tj"), "page 27 is AA");
}

#[test]
fn header_sits_high_footer_sits_low() {
    let header = add_page_numbers(
        &bytes("hello.pdf"), &[], "header", "center", "decimal", 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("header");
    let footer = add_page_numbers(
        &bytes("hello.pdf"), &[], "footer", "center", "decimal", 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("footer");
    let (_, hy) = show_pos(&page_content(&header, 1), "1").expect("header show");
    let (_, fy) = show_pos(&page_content(&footer, 1), "1").expect("footer show");
    assert!(hy > 700.0, "header baseline {hy} near the top");
    assert!(fy < 60.0, "footer baseline {fy} near the bottom");
}

#[test]
fn alignment_changes_x() {
    let left = add_page_numbers(
        &bytes("hello.pdf"), &[], "footer", "left", "decimal", 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("left");
    let right = add_page_numbers(
        &bytes("hello.pdf"), &[], "footer", "right", "decimal", 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("right");
    let (xl, _) = show_pos(&page_content(&left, 1), "1").expect("left show");
    let (xr, _) = show_pos(&page_content(&right, 1), "1").expect("right show");
    assert!(xl < xr, "left x {xl} < right x {xr}");
}

#[test]
fn footer_compensates_for_page_rotation() {
    // The 4-page rotated fixture: each footer lands at the visual bottom (y=margin).
    let out = add_page_numbers(
        &bytes("rotated.pdf"), &[], "footer", "center", "decimal", 1, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("rotated");
    for (page_no, shown) in [(1, "1"), (2, "2"), (3, "3"), (4, "4")] {
        let c = page_content(&out, page_no);
        let (_, y) = show_pos(&c, shown).expect("show");
        assert!((y - 36.0).abs() < 0.01, "page {page_no} footer baseline is the margin, got {y}");
    }
}

#[test]
fn is_tagged() {
    let out = add_page_numbers(
        &bytes("hello.pdf"), &[], "footer", "center", "decimal", 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("page number");
    let c = page_content(&out, 1);
    assert!(c.contains("/VibePDF") && c.contains("/Kind (page-number)"), "tag + kind; got:\n{c}");
    assert_eq!(c.matches("BDC").count(), c.matches("EMC").count(), "balanced BDC/EMC");
}

#[test]
fn start_below_one_errors() {
    let err = add_page_numbers(
        &bytes("hello.pdf"), &[], "footer", "center", "decimal", 0, "Helvetica", 10.0, "#000000",
        36.0,
    );
    assert!(err.is_err(), "start below 1 must error");
}

#[test]
fn unknown_format_position_align_error() {
    assert!(add_page_numbers(&bytes("hello.pdf"), &[], "middle", "center", "decimal", 1, "Helvetica", 10.0, "#000000", 36.0).is_err(), "bad position");
    assert!(add_page_numbers(&bytes("hello.pdf"), &[], "footer", "middle", "decimal", 1, "Helvetica", 10.0, "#000000", 36.0).is_err(), "bad align");
    assert!(add_page_numbers(&bytes("hello.pdf"), &[], "footer", "center", "bogus", 1, "Helvetica", 10.0, "#000000", 36.0).is_err(), "bad format");
}

#[tokio::test]
async fn actor_page_numbers_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-pn-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("pn.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_page_numbers(
            vec![],
            "footer".into(),
            "center".into(),
            "page-x-of-n".into(),
            1,
            "Helvetica".into(),
            10.0,
            "#000000".into(),
            36.0,
        )
        .await
        .expect("page numbers");
    assert!(state.can_undo, "page numbers must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert!(page_content(&std::fs::read(&out).unwrap(), 1).contains("(Page 1 of 1) Tj"), "after save");

    handle.undo().await.expect("undo");
    handle.save(Some(out.clone())).await.expect("save after undo");
    assert!(!page_content(&std::fs::read(&out).unwrap(), 1).contains("Page 1 of 1"), "undo removes it");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a page-numbered PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test page_numbers pn_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn pn_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-page-numbers.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("many-pages.pdf"), None).expect("spawn");
    handle
        .add_page_numbers(
            vec![0], // skip the "cover"
            "footer".into(),
            "center".into(),
            "page-x-of-n".into(),
            1,
            "Helvetica".into(),
            10.0,
            "#333333".into(),
            36.0,
        )
        .await
        .expect("page numbers");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote page-numbers verification artifact to {}", out.display());

    drop(handle);
}
