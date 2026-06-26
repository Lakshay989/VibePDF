//! Integration tests for text-run extraction (P4.A1).
//!
//! SPEC: P4-EDIT-001 (infra) — extract every text run on a page (text, bbox,
//! font, size, colour, transform) so the frontend can hit-test a click to a run.
//! Read-only; through the actor against `hello.pdf`.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn spawn(name: &str) -> DocumentActorHandle {
    let id = uuid::Uuid::new_v4();
    DocumentActorHandle::spawn(None, id, fixture(name), None).expect("spawn")
}

#[tokio::test]
async fn extracts_runs_from_hello() {
    let handle = spawn("hello.pdf");
    let runs = handle.read_text_runs(0).await.expect("read text runs");

    assert!(!runs.is_empty(), "hello.pdf has visible text → at least one run");
    let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert!(!joined.trim().is_empty(), "the runs carry text: {joined:?}");

    for run in &runs {
        // Geometry is sane page-space: ordered, finite, on a Letter-ish page.
        let [x0, y0, x1, y1] = run.bbox;
        assert!(x0 <= x1 && y0 <= y1, "ordered bbox: {:?}", run.bbox);
        assert!(run.bbox.iter().all(|v| v.is_finite()), "finite bbox: {:?}", run.bbox);
        assert!((0.0..=2000.0).contains(&x1) && (0.0..=2000.0).contains(&y1), "on-page: {:?}", run.bbox);
        // Style is populated.
        assert!(run.font_size > 0.0, "positive font size: {}", run.font_size);
        assert!(!run.font_name.is_empty(), "a font name");
        assert!(
            run.color.len() == 7 && run.color.starts_with('#'),
            "colour is #rrggbb: {}",
            run.color
        );
        assert_eq!(run.transform.len(), 6);
    }

    drop(handle);
}

#[tokio::test]
async fn bad_page_index_errors() {
    let handle = spawn("hello.pdf");
    assert!(handle.read_text_runs(99).await.is_err(), "out-of-range page is rejected");
    drop(handle);
}

/// A page whose only content is non-text (a bookmarks fixture's pages still have
/// text, so this just asserts extraction never panics on a different document).
#[tokio::test]
async fn extraction_is_stable_across_documents() {
    for name in ["hello.pdf", "links.pdf", "forms.pdf"] {
        let handle = spawn(name);
        let runs = handle.read_text_runs(0).await.expect("read");
        // Every run, whatever the doc, is well-formed (no panic, ordered bbox).
        for run in &runs {
            assert!(run.bbox[0] <= run.bbox[2] && run.bbox[1] <= run.bbox[3], "{name}: {:?}", run.bbox);
        }
        drop(handle);
    }
}
