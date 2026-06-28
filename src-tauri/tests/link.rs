//! Integration tests for hyperlinks (P4.C3).
//!
//! SPEC: P4-EDIT-007 — add a `/Link` annotation over a region, targeting an
//! external URL, internal page (Go-To), a named destination, or an email
//! (`mailto:`). The exact dict shapes are asserted at the COS level (no PDFium
//! re-normalization); undo is exercised through the actor against `hello.pdf`.

use std::path::PathBuf;

use lopdf::{Dictionary, Document, Object, ObjectId};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::add_link;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

/// Every `/Link` annotation in `bytes`, across all pages.
fn link_dicts(bytes: &[u8]) -> Vec<Dictionary> {
    let doc = Document::load_mem(bytes).expect("load");
    let mut out = Vec::new();
    for &page_id in doc.get_pages().values() {
        let annots = doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
        let arr = match annots {
            Some(Object::Array(a)) => a,
            Some(Object::Reference(id)) => {
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
            }
            _ => continue,
        };
        for o in arr {
            let Ok(id) = o.as_reference() else { continue };
            if let Ok(d) = doc.get_dictionary(id) {
                if d.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Link"[..]) {
                    out.push(d.clone());
                }
            }
        }
    }
    out
}

/// A link's `/Rect` as a `[f32; 4]` (defaulting any unreadable entry to 0).
fn rect_of(link: &Dictionary) -> [f32; 4] {
    let mut out = [0.0_f32; 4];
    if let Ok(arr) = link.get(b"Rect").and_then(Object::as_array) {
        for (slot, o) in out.iter_mut().zip(arr) {
            *slot = o.as_float().unwrap_or(0.0);
        }
    }
    out
}

/// The `/A << /S /URI /URI (..) >>` target string of a link dict, if any.
fn uri_of(link: &Dictionary) -> Option<String> {
    let action = link.get(b"A").and_then(Object::as_dict).ok()?;
    let uri = action.get(b"URI").and_then(Object::as_str).ok()?;
    Some(String::from_utf8_lossy(uri).into_owned())
}

#[test]
fn url_link_roundtrips() {
    let out = add_link(&bytes("hello.pdf"), 0, [72.0, 700.0, 272.0, 720.0], "url", "https://example.com")
        .expect("add url link");
    let links = link_dicts(&out);
    assert_eq!(links.len(), 1, "exactly one /Link");
    let link = &links[0];
    assert_eq!(uri_of(link).as_deref(), Some("https://example.com"));
    // Invisible hot-zone: no /AP.
    assert!(link.get(b"AP").is_err(), "a link carries no appearance stream");
    assert_eq!(rect_of(link), [72.0, 700.0, 272.0, 720.0]);
}

#[test]
fn mailto_link_prefixes_scheme() {
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "email", "ada@example.com")
        .expect("add email link");
    let links = link_dicts(&out);
    assert_eq!(uri_of(&links[0]).as_deref(), Some("mailto:ada@example.com"));
}

#[test]
fn internal_page_link_targets_page() {
    // links.pdf has 3 pages (and its own pre-existing links); target the second
    // (0-based "1") via a uniquely-positioned rect so we can find ours back.
    let src = bytes("links.pdf");
    let our_rect = [50.0, 50.0, 150.0, 70.0];
    let out = add_link(&src, 0, our_rect, "page", "1").expect("add page link");

    let doc = Document::load_mem(&out).expect("load");
    let page2 = *doc.get_pages().get(&2).expect("page 2 exists");
    let ours = link_dicts(&out)
        .into_iter()
        .find(|l| rect_of(l) == our_rect)
        .expect("our link, found by its rect");
    let dest_target: Option<ObjectId> = ours
        .get(b"Dest")
        .and_then(Object::as_array)
        .ok()
        .and_then(|arr| arr.first().and_then(|o| o.as_reference().ok()));
    assert_eq!(dest_target, Some(page2), "/Dest must point at page 2's object");
}

#[test]
fn named_dest_link_kept() {
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "named", "chapter-2")
        .expect("add named link");
    let link = &link_dicts(&out)[0];
    let dest = link.get(b"Dest").and_then(Object::as_str).expect("named dest is a string");
    assert_eq!(String::from_utf8_lossy(dest), "chapter-2");
}

#[test]
fn url_with_parens_is_escaped() {
    // Unbalanced/parenthesized URLs must not corrupt the literal-string syntax.
    let tricky = "https://example.com/path_(1)?q=(a)&r=)";
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", tricky)
        .expect("add tricky url");
    assert_eq!(uri_of(&link_dicts(&out)[0]).as_deref(), Some(tricky));
}

#[test]
fn target_page_out_of_range_errors() {
    // hello.pdf has 1 page; page index 5 is out of range.
    let err = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "page", "5");
    assert!(err.is_err(), "out-of-range target page must error, not silently corrupt");
}

#[test]
fn unknown_kind_errors() {
    let err = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "telnet", "x");
    assert!(err.is_err(), "an unknown link kind must error");
}

#[tokio::test]
async fn actor_add_link_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-link-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("link.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_link(0, [72.0, 700.0, 272.0, 720.0], "url".into(), "https://example.com".into())
        .await
        .expect("add link");
    assert!(state.can_undo, "a link must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(link_dicts(&std::fs::read(&out).expect("read")).len(), 1, "link present after save");

    handle.undo().await.expect("undo");
    handle.save(Some(out.clone())).await.expect("save after undo");
    assert!(link_dicts(&std::fs::read(&out).expect("read")).is_empty(), "undo removes the link");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a link-bearing PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual (Acrobat / Preview / Okular). Ignored; run on demand:
///   cargo test --test link link_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn link_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-link.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");
    handle
        .add_link(0, [72.0, 700.0, 320.0, 724.0], "url".into(), "https://example.com".into())
        .await
        .expect("add url link");
    handle
        .add_link(0, [72.0, 660.0, 320.0, 684.0], "page".into(), "2".into())
        .await
        .expect("add page link");
    handle
        .add_link(0, [72.0, 620.0, 320.0, 644.0], "email".into(), "user@example.com".into())
        .await
        .expect("add email link");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote link verification artifact to {}", out.display());

    drop(handle);
}
