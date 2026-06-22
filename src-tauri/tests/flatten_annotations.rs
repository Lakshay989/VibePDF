//! Integration tests for flattening annotations (P3.E2).
//!
//! SPEC: P3-ANN-011 — bake every `/AP`-bearing annotation into the page content
//! streams (so the markup becomes page content, not a separate annotation),
//! undoable in-session only. Structural assertions use lopdf directly on the
//! flatten output; behavioural ones go through the actor against `hello.pdf`.

use std::path::PathBuf;

use lopdf::{Document, Object, ObjectId};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::read_annotations;
use vibepdf_lib::pdf::flatten::flatten_annotations;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn spawn() -> DocumentActorHandle {
    let id = uuid::Uuid::new_v4();
    DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn")
}

/// A highlight + a filled rectangle (both `/AP`-bearing) and a sticky note
/// (`/AP`-less). Returns the actor handle.
async fn build_markup(handle: &DocumentActorHandle) {
    handle
        .add_text_markup(0, "highlight".into(), vec![[100.0, 700.0, 200.0, 700.0, 100.0, 680.0, 200.0, 680.0]], "#ffff00".into(), 1.0)
        .await
        .expect("highlight");
    handle
        .add_shape(0, "rectangle".into(), [120.0, 500.0, 260.0, 580.0], "#ff0000".into(), Some("#00ff00".into()), 0.8, 2.0)
        .await
        .expect("rectangle");
    handle
        .add_note("note1".into(), 0, 130.0, 650.0, "a comment".into(), "Ada".into())
        .await
        .expect("note");
}

fn first_page(doc: &Document) -> ObjectId {
    *doc.get_pages().values().next().expect("at least one page")
}

fn annot_array(doc: &Document, page_id: ObjectId) -> Vec<Object> {
    match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn xobject_names(doc: &Document, page_id: ObjectId) -> Vec<String> {
    let page = doc.get_dictionary(page_id).expect("page");
    let res = match page.get(b"Resources") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_object(*id).and_then(Object::as_dict).cloned().unwrap_or_default(),
        _ => return Vec::new(),
    };
    res.get(b"XObject")
        .and_then(Object::as_dict)
        .map(|x| x.iter().map(|(k, _)| String::from_utf8_lossy(k).into_owned()).collect())
        .unwrap_or_default()
}

fn content_refs(doc: &Document, page_id: ObjectId) -> usize {
    match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Contents").ok().cloned()) {
        Some(Object::Array(a)) => a.len(),
        Some(Object::Reference(_)) => 1,
        _ => 0,
    }
}

#[tokio::test]
async fn flatten_bakes_into_content_and_drops_ap_annotations() {
    let handle = spawn();
    build_markup(&handle).await;
    let before = handle.get_bytes().await.expect("bytes");

    let before_doc = Document::load_mem(&before).expect("load before");
    let before_page = first_page(&before_doc);
    let before_contents = content_refs(&before_doc, before_page);
    assert_eq!(annot_array(&before_doc, before_page).len(), 3, "3 annotations before");

    // Flatten the pure COS transform (deterministic, no PDFium reflow).
    let after = flatten_annotations(&before).expect("flatten");
    let doc = Document::load_mem(&after).expect("load after");
    let page = first_page(&doc);

    // The two /AP-bearing annotations are gone; the /AP-less note survives.
    assert_eq!(annot_array(&doc, page).len(), 1, "only the note remains in /Annots");

    // Each baked annotation registered its appearance form on the page...
    let xobjects = xobject_names(&doc, page);
    let baked: Vec<_> = xobjects.iter().filter(|n| n.starts_with("VPFlat")).collect();
    assert_eq!(baked.len(), 2, "both appearance forms registered: {xobjects:?}");

    // ...and the page gained a content stream that paints them.
    assert!(content_refs(&doc, page) > before_contents, "a content fragment was appended");
    assert!(after.windows(6).any(|w| w == b"VPFlat"), "a `Do` fragment references a form");

    drop(handle);
}

#[tokio::test]
async fn flatten_is_undoable_in_session() {
    let handle = spawn();
    build_markup(&handle).await;

    let state = handle.flatten_annotations().await.expect("flatten");
    assert!(state.can_undo, "flatten is undoable");
    let flat = read_annotations(&handle.get_bytes().await.expect("bytes")).expect("read");
    assert_eq!(flat.len(), 1, "only the /AP-less note remains after flatten");
    assert_eq!(flat[0].kind, "note");

    handle.undo().await.expect("undo");
    let restored = read_annotations(&handle.get_bytes().await.expect("bytes")).expect("read");
    assert_eq!(restored.len(), 3, "undo brings every annotation back");

    drop(handle);
}

#[tokio::test]
async fn flatten_keeps_ap_less_notes() {
    let handle = spawn();
    handle
        .add_note("only".into(), 0, 130.0, 650.0, "comment".into(), "Ada".into())
        .await
        .expect("note");

    handle.flatten_annotations().await.expect("flatten");
    let after = read_annotations(&handle.get_bytes().await.expect("bytes")).expect("read");
    assert_eq!(after.len(), 1, "a note has no /AP to bake, so it stays live");
    assert_eq!(after[0].contents, "comment");

    drop(handle);
}

#[tokio::test]
async fn flatten_empty_document_is_safe() {
    let handle = spawn();
    handle.flatten_annotations().await.expect("flatten on a doc with no annotations");
    let after = read_annotations(&handle.get_bytes().await.expect("bytes")).expect("read");
    assert_eq!(after.len(), 0);
    drop(handle);
}

/// Writes a flattened PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test flatten_annotations flatten_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn flatten_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-flatten.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let handle = spawn();
    build_markup(&handle).await;
    handle
        .add_ink(0, vec![[100.0, 300.0, 0.5], [140.0, 330.0, 0.5], [180.0, 300.0, 0.5]], "#0000ff".into(), 1.0, 2.0)
        .await
        .expect("ink");
    handle.flatten_annotations().await.expect("flatten");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote flatten verification artifact to {}", out.display());
    drop(handle);
}
