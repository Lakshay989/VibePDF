//! Integration tests for Bates numbering (P4.D5).
//!
//! SPEC: P4-EDIT-012 — stamp a sequential `{prefix}{padded seq}{suffix}` id on
//! every page, from a starting number. Content-stream assertions are at the lopdf
//! level; undo runs through the actor.

use std::path::PathBuf;

use lopdf::Document;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::bates::add_bates;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

fn page_content(bytes: &[u8], page_no: u32) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&page_no).expect("page exists");
    String::from_utf8_lossy(&doc.get_page_content(page_id).expect("content")).into_owned()
}

/// `(needle) Tj` position from the preceding `x y Td`.
fn show_pos(content: &str, needle: &str) -> Option<(f32, f32)> {
    let marker = format!("({needle}) Tj");
    let at = content.find(&marker)?;
    let td = content[..at].rfind(" Td")?;
    let mut it = content[..td].split_whitespace().rev();
    let y: f32 = it.next()?.parse().ok()?;
    let x: f32 = it.next()?.parse().ok()?;
    Some((x, y))
}

fn reloads_with_pages(pdf: &[u8], expected: usize) {
    let doc = Document::load_mem(pdf).expect("output re-parses");
    assert_eq!(doc.get_pages().len(), expected, "page count preserved");
}

#[test]
fn stamps_consecutive_ids() {
    let out = add_bates(
        &bytes("many-pages.pdf"), "footer", "right", "ABC", "", 6, 1, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("bates");
    assert!(page_content(&out, 1).contains("(ABC000001) Tj"), "page 1");
    assert!(page_content(&out, 2).contains("(ABC000002) Tj"), "page 2");
    assert!(page_content(&out, 50).contains("(ABC000050) Tj"), "page 50");
    reloads_with_pages(&out, 50);
}

#[test]
fn start_offset() {
    let out = add_bates(
        &bytes("many-pages.pdf"), "footer", "right", "ABC", "", 6, 1000, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("bates");
    assert!(page_content(&out, 1).contains("(ABC001000) Tj"), "page 1 starts at 1000");
    assert!(page_content(&out, 2).contains("(ABC001001) Tj"), "page 2");
}

#[test]
fn suffix_and_padding() {
    let out = add_bates(
        &bytes("hello.pdf"), "footer", "right", "EX-", "-END", 3, 7, "Helvetica", 10.0,
        "#000000", 36.0,
    )
    .expect("bates");
    assert!(page_content(&out, 1).contains("(EX-007-END) Tj"), "prefix+pad+suffix");
}

#[test]
fn footer_right_default_placement() {
    let out = add_bates(
        &bytes("hello.pdf"), "footer", "right", "ABC", "", 6, 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("bates");
    let (x, y) = show_pos(&page_content(&out, 1), "ABC000001").expect("show");
    // US-Letter 612 wide; right-aligned id sits well past centre, footer near the bottom.
    assert!(x > 400.0, "right-aligned id x {x} is toward the right edge");
    assert!(y < 60.0, "footer baseline {y} near the bottom");
}

#[test]
fn compensates_for_page_rotation() {
    let out = add_bates(
        &bytes("rotated.pdf"), "footer", "right", "R", "", 3, 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("bates");
    for (page_no, id) in [(1, "R001"), (2, "R002"), (3, "R003"), (4, "R004")] {
        let c = page_content(&out, page_no);
        let (_, y) = show_pos(&c, id).expect("show");
        assert!((y - 36.0).abs() < 0.01, "page {page_no} footer baseline is the margin, got {y}");
    }
}

#[test]
fn is_tagged() {
    let out = add_bates(
        &bytes("hello.pdf"), "footer", "right", "ABC", "", 6, 1, "Helvetica", 10.0, "#000000",
        36.0,
    )
    .expect("bates");
    let c = page_content(&out, 1);
    assert!(c.contains("/VibePDF") && c.contains("/Kind (bates)"), "tag + kind; got:\n{c}");
    assert_eq!(c.matches("BDC").count(), c.matches("EMC").count(), "balanced BDC/EMC");
}

#[test]
fn negative_start_errors() {
    let err = add_bates(
        &bytes("hello.pdf"), "footer", "right", "ABC", "", 6, -1, "Helvetica", 10.0, "#000000",
        36.0,
    );
    assert!(err.is_err(), "negative start must error");
}

#[test]
fn non_winansi_prefix_errors() {
    // A prefix outside WinAnsi is rejected honestly (base-14 fonts can't draw it).
    let err = add_bates(
        &bytes("hello.pdf"), "footer", "right", "\u{2C81}", "", 6, 1, "Helvetica", 10.0,
        "#000000", 36.0,
    );
    assert!(err.is_err(), "non-WinAnsi prefix must error");
}

#[test]
fn unknown_position_align_error() {
    assert!(add_bates(&bytes("hello.pdf"), "middle", "right", "A", "", 6, 1, "Helvetica", 10.0, "#000000", 36.0).is_err(), "bad position");
    assert!(add_bates(&bytes("hello.pdf"), "footer", "middle", "A", "", 6, 1, "Helvetica", 10.0, "#000000", 36.0).is_err(), "bad align");
}

#[tokio::test]
async fn actor_bates_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-bates-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("bates.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_bates(
            "footer".into(),
            "right".into(),
            "ABC".into(),
            String::new(),
            6,
            1,
            "Helvetica".into(),
            10.0,
            "#000000".into(),
            36.0,
        )
        .await
        .expect("bates");
    assert!(state.can_undo, "Bates numbering must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert!(page_content(&std::fs::read(&out).unwrap(), 1).contains("(ABC000001) Tj"), "after save");

    handle.undo().await.expect("undo");
    handle.save(Some(out.clone())).await.expect("save after undo");
    assert!(!page_content(&std::fs::read(&out).unwrap(), 1).contains("ABC000001"), "undo removes it");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a Bates-numbered PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test bates bates_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn bates_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-bates.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("many-pages.pdf"), None).expect("spawn");
    handle
        .add_bates(
            "footer".into(),
            "right".into(),
            "ABC".into(),
            String::new(),
            6,
            1,
            "Helvetica".into(),
            10.0,
            "#333333".into(),
            36.0,
        )
        .await
        .expect("bates");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote Bates verification artifact to {}", out.display());

    drop(handle);
}
