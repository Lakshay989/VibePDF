//! Integration tests for headers / footers (P4.D3).
//!
//! SPEC: P4-EDIT-010 — draw left/center/right text in the top/bottom margin with
//! `{n}` / `{total}` / `{date}` placeholders substituted per page. Content-stream
//! assertions are at the lopdf level; undo runs through the actor.

use std::path::PathBuf;

use lopdf::Document;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::header_footer::{add_header_footer, substitute};

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
/// `x y Td`. (The fixtures draw their own text, so tests target their own
/// distinctive strings rather than "any `Tj`".)
fn show_pos(content: &str, needle: &str) -> Option<(f32, f32)> {
    let marker = format!("({needle}) Tj");
    let at = content.find(&marker)?;
    let td = content[..at].rfind(" Td")?;
    let mut it = content[..td].split_whitespace().rev();
    let y: f32 = it.next()?.parse().ok()?;
    let x: f32 = it.next()?.parse().ok()?;
    Some((x, y))
}

/// Convenience: a footer/header with a single centre template on `hello.pdf`.
fn one(position: &str, center: &str, date: &str) -> Vec<u8> {
    add_header_footer(&bytes("hello.pdf"), &[0], position, "", center, "", "Helvetica", 10.0, "#000000", 36.0, date)
        .expect("header/footer")
}

#[test]
fn substitute_replaces_all_placeholders() {
    assert_eq!(substitute("Page {n} of {total} — {date}", 3, 50, "2026-07-01"), "Page 3 of 50 — 2026-07-01");
    assert_eq!(substitute("no placeholders", 1, 9, "x"), "no placeholders");
    assert_eq!(substitute("{n}/{n}", 7, 9, "x"), "7/7");
}

#[test]
fn footer_center_shows_page_of_total() {
    let out = add_header_footer(
        &bytes("many-pages.pdf"),
        &[0, 2],
        "footer",
        "",
        "Page {n} of {total}",
        "",
        "Helvetica",
        10.0,
        "#000000",
        36.0,
        "2026-07-01",
    )
    .expect("footer");
    assert!(page_content(&out, 1).contains("(Page 1 of 50) Tj"), "page 1 footer");
    assert!(page_content(&out, 3).contains("(Page 3 of 50) Tj"), "page 3 footer");
    assert!(!page_content(&out, 2).contains("of 50"), "page 2 has no footer");
}

#[test]
fn header_sits_high_footer_sits_low() {
    // US-Letter is 792 tall; margin 36, size 10.
    let header = one("header", "HEADER", "d");
    let footer = one("footer", "FOOTER", "d");
    let (_, hy) = show_pos(&page_content(&header, 1), "HEADER").expect("header show");
    let (_, fy) = show_pos(&page_content(&footer, 1), "FOOTER").expect("footer show");
    assert!(hy > 700.0, "header baseline {hy} near the top");
    assert!(fy < 60.0, "footer baseline {fy} near the bottom");
    assert!(hy > fy, "header is above footer");
}

#[test]
fn left_center_right_get_distinct_x() {
    let out = add_header_footer(
        &bytes("hello.pdf"),
        &[0],
        "header",
        "LEFT",
        "CTR",
        "RIGHT",
        "Helvetica",
        10.0,
        "#000000",
        36.0,
        "d",
    )
    .expect("hf");
    let c = page_content(&out, 1);
    let (xl, _) = show_pos(&c, "LEFT").expect("left");
    let (xc, _) = show_pos(&c, "CTR").expect("center");
    let (xr, _) = show_pos(&c, "RIGHT").expect("right");
    assert!(xl < xc && xc < xr, "left < center < right x: {xl}/{xc}/{xr}");
}

#[test]
fn only_nonempty_positions_are_drawn() {
    // left + right, empty centre → exactly two font-select ops in the fragment.
    let out = add_header_footer(&bytes("hello.pdf"), &[0], "footer", "LT", "", "RT", "Helvetica", 10.0, "#000000", 36.0, "d")
        .expect("hf");
    let c = page_content(&out, 1);
    assert!(c.contains("(LT) Tj") && c.contains("(RT) Tj"), "both non-empty positions drawn");
    assert_eq!(c.matches("/Fhf").count(), 2, "only two positions selected the header/footer font");
}

#[test]
fn all_empty_errors() {
    let err = add_header_footer(&bytes("hello.pdf"), &[0], "header", "", "  ", "", "Helvetica", 10.0, "#000000", 36.0, "d");
    assert!(err.is_err(), "no text in any position must error");
}

#[test]
fn unknown_position_errors() {
    let err = add_header_footer(&bytes("hello.pdf"), &[0], "middle", "", "x", "", "Helvetica", 10.0, "#000000", 36.0, "d");
    assert!(err.is_err(), "position must be header or footer");
}

#[test]
fn page_out_of_range_errors() {
    let err = add_header_footer(&bytes("hello.pdf"), &[5], "header", "", "x", "", "Helvetica", 10.0, "#000000", 36.0, "d");
    assert!(err.is_err(), "out-of-range page must error");
}

#[tokio::test]
async fn actor_header_footer_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-hf-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("hf.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_header_footer(
            vec![0],
            "footer".into(),
            String::new(),
            "Page {n} of {total}".into(),
            String::new(),
            "Helvetica".into(),
            10.0,
            "#000000".into(),
            36.0,
            "2026-07-01".into(),
        )
        .await
        .expect("footer");
    assert!(state.can_undo, "a header/footer must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert!(page_content(&std::fs::read(&out).unwrap(), 1).contains("(Page 1 of 1) Tj"), "footer after save");

    handle.undo().await.expect("undo");
    handle.save(Some(out.clone())).await.expect("save after undo");
    assert!(!page_content(&std::fs::read(&out).unwrap(), 1).contains("Page 1 of 1"), "undo removes it");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- P4.HF: rotation + CropBox compensation ------------------------------------

/// SPEC: P4-EDIT-010 — a footer must land at the *visual* bottom of a rotated
/// page: the content carries the compensating visual→page `cm` per /Rotate.
#[test]
fn footer_compensates_for_page_rotation() {
    let out = add_header_footer(
        &bytes("rotated.pdf"),
        &[0, 1, 2, 3],
        "footer",
        "",
        "F{n}",
        "",
        "Helvetica",
        10.0,
        "#000000",
        36.0,
        "d",
    )
    .expect("footer on rotated pages");
    // Per-angle compensating matrices (visual space → page space).
    let expect = [
        (1, "1.00000 0.00000 0.00000 1.00000 0.00 0.00 cm"), // /Rotate 0
        (2, "0.00000 1.00000 -1.00000 0.00000 612.00 0.00 cm"), // /Rotate 90
        (3, "-1.00000 0.00000 0.00000 -1.00000 612.00 792.00 cm"), // /Rotate 180
        (4, "0.00000 -1.00000 1.00000 0.00000 0.00 792.00 cm"), // /Rotate 270
    ];
    for (page_no, cm) in expect {
        let c = page_content(&out, page_no);
        assert!(c.contains(cm), "page {page_no} carries its compensating cm ({cm}); got:\n{c}");
        // The footer text itself is drawn at the visual bottom (y = margin).
        let (_, y) = show_pos(&c, &format!("F{page_no}")).expect("footer show");
        assert!((y - 36.0).abs() < 0.01, "visual footer baseline is the margin, got {y}");
    }
}

/// SPEC: P4-EDIT-010 — placement targets the visible CropBox, not the MediaBox:
/// the visual origin shifts to the crop corner and margins are crop-relative.
#[test]
fn footer_respects_cropbox() {
    let out = add_header_footer(
        &bytes("cropped.pdf"),
        &[0],
        "footer",
        "LC",
        "",
        "",
        "Helvetica",
        10.0,
        "#000000",
        36.0,
        "d",
    )
    .expect("footer on cropped page");
    let c = page_content(&out, 1);
    // CropBox [100 100 512 692] → visual origin (100, 100).
    assert!(
        c.contains("1.00000 0.00000 0.00000 1.00000 100.00 100.00 cm"),
        "visual cm shifts to the crop origin; got:\n{c}"
    );
    let (x, y) = show_pos(&c, "LC").expect("left show");
    assert!((x - 36.0).abs() < 0.01 && (y - 36.0).abs() < 0.01, "crop-relative margins, got ({x}, {y})");
}

/// FABLE_REVIEW 3.13 — the fragment is wrapped in a `/VibePDF` marked-content
/// block carrying its kind.
#[test]
fn header_footer_is_tagged() {
    let out = one("footer", "tagged", "d");
    let c = page_content(&out, 1);
    assert!(c.contains("/VibePDF") && c.contains("/Kind (header-footer)"), "tag + kind; got:\n{c}");
    assert_eq!(c.matches("BDC").count(), c.matches("EMC").count(), "balanced BDC/EMC");
}

/// Writes a header/footer PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test header_footer hf_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn hf_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-header-footer.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("many-pages.pdf"), None).expect("spawn");
    let pages: Vec<i32> = (0..50).collect();
    handle
        .add_header_footer(
            pages,
            "footer".into(),
            String::new(),
            "Page {n} of {total}".into(),
            "{date}".into(),
            "Helvetica".into(),
            10.0,
            "#333333".into(),
            36.0,
            "2026-07-01".into(),
        )
        .await
        .expect("footer");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote header/footer verification artifact to {}", out.display());

    drop(handle);
}
