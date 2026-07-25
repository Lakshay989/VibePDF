//! Text-box (P4.B2) verification artifact for the non-WinAnsi embed path (P4.HF8).
//!
//! SPEC: P4-EDIT-005 — a text box is page content, not an annotation. The
//! correctness/wrap/underline assertions live in `cos.rs`'s inline
//! `text_box_embed_tests`; this file just writes a human-checkable artifact.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

/// A multi-line, underlined Cyrillic text box takes the PDFium font-embed path
/// (subsetted). Ignored; run on demand:
///   cargo test --test text_box text_box_embedded_unicode_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn text_box_embedded_unicode_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-text-box-unicode.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_text_box(
            0,
            [72.0, 500.0, 320.0, 700.0],
            "Съешь же ещё этих мягких французских булок да выпей чаю".into(),
            "Helvetica".into(),
            16.0,
            "#102080".into(),
            false,
            false,
            true, // underline
        )
        .await
        .expect("embedded text box");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote embedded-Unicode text-box artifact to {}", out.display());

    drop(handle);
}

/// SPEC: P4-EDIT-003b — a multi-line ASCII (base-14) box, now wrapped in the
/// `/VibePDF` re-edit tag. Confirms the WinAnsi path — newly BDC-wrapped this
/// phase — still renders across readers. Ignored; run on demand:
///   cargo test --test text_box text_box_ascii_reedit_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn text_box_ascii_reedit_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-text-box-ascii.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_text_box(
            0,
            [72.0, 500.0, 360.0, 680.0],
            "The quick brown fox\njumps over the lazy dog\nand then re-reads this box.".into(),
            "Times".into(),
            15.0,
            "#183028".into(),
            true,  // bold
            false, // italic
            true,  // underline
        )
        .await
        .expect("ascii text box");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote ASCII re-edit text-box artifact to {}", out.display());

    drop(handle);
}

/// SPEC: P4-EDIT-003b — the full actor round-trip: add a box, read it back, re-edit
/// it in place (preserving its rect), then undo back to the original text. Exercises
/// UpdateTextBoxEdit + the read path + the undo snapshot.
#[tokio::test]
async fn text_box_reedit_roundtrip_through_actor() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let rect = [80.0, 500.0, 380.0, 560.0];
    handle
        .add_text_box(
            0,
            rect,
            "before".into(),
            "Helvetica".into(),
            14.0,
            "#000000".into(),
            false,
            false,
            false,
        )
        .await
        .expect("add");

    let boxes = handle.read_text_boxes(0).await.expect("read");
    assert_eq!(boxes.len(), 1, "one box after add");
    assert_eq!(boxes[0].text, "before");
    let box_id = boxes[0].id.clone();

    handle
        .update_text_box(
            0,
            box_id,
            "after the edit".into(),
            "Times".into(),
            18.0,
            "#204080".into(),
            true,
            false,
            true,
        )
        .await
        .expect("update");

    let edited = handle.read_text_boxes(0).await.expect("read edited");
    assert_eq!(edited.len(), 1, "still one box after update");
    assert_eq!(edited[0].text, "after the edit");
    assert_eq!(edited[0].font_family, "Times");
    for (got, want) in edited[0].rect.iter().zip(rect.iter()) {
        assert!((got - want).abs() < 0.01, "rect preserved through re-edit");
    }

    handle.undo().await.expect("undo");
    let reverted = handle.read_text_boxes(0).await.expect("read reverted");
    assert_eq!(reverted.len(), 1, "one box after undo");
    assert_eq!(reverted[0].text, "before", "undo restores the original text");

    drop(handle);
}

/// SPEC: P4-EDIT-003b / P4-EDIT-004 — delete an added text box through the actor,
/// and undo brings it back. Exercises RemoveTextBoxEdit + the delete command path.
#[tokio::test]
async fn text_box_delete_roundtrip_through_actor() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_text_box(
            0,
            [80.0, 500.0, 380.0, 560.0],
            "delete me".into(),
            "Helvetica".into(),
            14.0,
            "#000000".into(),
            false,
            false,
            false,
        )
        .await
        .expect("add");
    let box_id = handle.read_text_boxes(0).await.expect("read")[0].id.clone();

    handle.delete_text_box(0, box_id).await.expect("delete");
    assert!(
        handle.read_text_boxes(0).await.expect("read after delete").is_empty(),
        "box gone after delete",
    );

    handle.undo().await.expect("undo");
    let restored = handle.read_text_boxes(0).await.expect("read after undo");
    assert_eq!(restored.len(), 1, "undo restores the deleted box");
    assert_eq!(restored[0].text, "delete me");

    drop(handle);
}
