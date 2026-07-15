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
