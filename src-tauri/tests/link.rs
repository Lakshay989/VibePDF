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

/// True when the link carries a `/AP << /N <Form XObject> >>` appearance.
fn has_form_ap(doc: &Document, link: &Dictionary) -> bool {
    let Ok(ap) = link.get(b"AP").and_then(Object::as_dict) else { return false };
    let Ok(n) = ap.get(b"N").and_then(Object::as_reference) else { return false };
    doc.get_object(n)
        .ok()
        .and_then(|o| o.as_stream().ok())
        .and_then(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok())
        == Some(&b"Form"[..])
}

/// The link's `/BS /S` border-style name (`S` = box, `U` = underline), if any.
fn bs_style(link: &Dictionary) -> Option<String> {
    let bs = link.get(b"BS").and_then(Object::as_dict).ok()?;
    let s = bs.get(b"S").and_then(Object::as_name).ok()?;
    Some(String::from_utf8_lossy(s).into_owned())
}

/// The link's `/C` colour as `[r, g, b]`, if present.
fn color_of(link: &Dictionary) -> Option<Vec<f32>> {
    let arr = link.get(b"C").and_then(Object::as_array).ok()?;
    Some(arr.iter().map(|o| o.as_float().unwrap_or(-1.0)).collect())
}

#[test]
fn url_link_roundtrips() {
    let out = add_link(
        &bytes("hello.pdf"),
        0,
        [72.0, 700.0, 272.0, 720.0],
        "url",
        "https://example.com",
        "invisible",
        "#000000",
    )
    .expect("add url link");
    let links = link_dicts(&out);
    assert_eq!(links.len(), 1, "exactly one /Link");
    let link = &links[0];
    assert_eq!(uri_of(link).as_deref(), Some("https://example.com"));
    // Invisible hot-zone: no /AP.
    assert!(link.get(b"AP").is_err(), "an invisible link carries no appearance stream");
    assert_eq!(rect_of(link), [72.0, 700.0, 272.0, 720.0]);
}

#[test]
fn mailto_link_prefixes_scheme() {
    let out = add_link(
        &bytes("hello.pdf"),
        0,
        [10.0, 10.0, 110.0, 30.0],
        "email",
        "ada@example.com",
        "invisible",
        "#000000",
    )
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
    let out = add_link(&src, 0, our_rect, "page", "1", "invisible", "#000000").expect("add page link");

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
    let out = add_link(
        &bytes("hello.pdf"),
        0,
        [10.0, 10.0, 110.0, 30.0],
        "named",
        "chapter-2",
        "invisible",
        "#000000",
    )
    .expect("add named link");
    let link = &link_dicts(&out)[0];
    let dest = link.get(b"Dest").and_then(Object::as_str).expect("named dest is a string");
    assert_eq!(String::from_utf8_lossy(dest), "chapter-2");
}

#[test]
fn url_with_parens_is_escaped() {
    // Unbalanced/parenthesized URLs must not corrupt the literal-string syntax.
    let tricky = "https://example.com/path_(1)?q=(a)&r=)";
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", tricky, "invisible", "#000000")
        .expect("add tricky url");
    assert_eq!(uri_of(&link_dicts(&out)[0]).as_deref(), Some(tricky));
}

#[test]
fn target_page_out_of_range_errors() {
    // hello.pdf has 1 page; page index 5 is out of range.
    let err = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "page", "5", "invisible", "#000000");
    assert!(err.is_err(), "out-of-range target page must error, not silently corrupt");
}

#[test]
fn unknown_kind_errors() {
    let err = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "telnet", "x", "invisible", "#000000");
    assert!(err.is_err(), "an unknown link kind must error");
}

// --- P4-EDIT-007b: appearance -------------------------------------------------

#[test]
fn invisible_style_has_no_ap() {
    // SPEC: P4-EDIT-007b — the invisible style stays a borderless hot-zone.
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", "https://x.com", "invisible", "#0000ff")
        .expect("add");
    let doc = Document::load_mem(&out).expect("load");
    let link = &link_dicts(&out)[0];
    assert!(!has_form_ap(&doc, link), "invisible → no /AP");
    assert!(link.get(b"C").is_err(), "invisible → no /C");
    let border: Vec<i64> = link
        .get(b"Border")
        .and_then(Object::as_array)
        .expect("/Border")
        .iter()
        .map(|o| o.as_i64().unwrap_or(-1))
        .collect();
    assert_eq!(border, vec![0, 0, 0], "invisible → zero border");
}

#[test]
fn box_link_has_ap_and_color() {
    // SPEC: P4-EDIT-007b — a box carries a Form /AP, /C, and /BS /S = S.
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", "https://x.com", "box", "#ff0000")
        .expect("add");
    let doc = Document::load_mem(&out).expect("load");
    let link = &link_dicts(&out)[0];
    assert!(has_form_ap(&doc, link), "box → a Form /AP");
    assert_eq!(bs_style(link).as_deref(), Some("S"), "box → /BS /S = S");
    assert_eq!(color_of(link), Some(vec![1.0, 0.0, 0.0]), "/C parsed from #ff0000");
}

#[test]
fn underline_link_has_ap_and_u_style() {
    // SPEC: P4-EDIT-007b — an underline carries a Form /AP and /BS /S = U.
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", "https://x.com", "underline", "#00ff00")
        .expect("add");
    let doc = Document::load_mem(&out).expect("load");
    let link = &link_dicts(&out)[0];
    assert!(has_form_ap(&doc, link), "underline → a Form /AP");
    assert_eq!(bs_style(link).as_deref(), Some("U"), "underline → /BS /S = U");
}

#[test]
fn styled_link_still_navigates() {
    // SPEC: P4-EDIT-007b — appearance must not clobber the target.
    let out = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", "https://x.com", "box", "#0000ff")
        .expect("add");
    assert_eq!(uri_of(&link_dicts(&out)[0]).as_deref(), Some("https://x.com"));
}

#[test]
fn unknown_style_errors() {
    let err = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", "https://x.com", "glow", "#0000ff");
    assert!(err.is_err(), "an unknown style must error");
}

#[test]
fn bad_color_errors() {
    let err = add_link(&bytes("hello.pdf"), 0, [10.0, 10.0, 110.0, 30.0], "url", "https://x.com", "box", "blue");
    assert!(err.is_err(), "a non-hex colour must error");
}

#[tokio::test]
async fn actor_add_link_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-link-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("link.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_link(
            0,
            [72.0, 700.0, 272.0, 720.0],
            "url".into(),
            "https://example.com".into(),
            "box".into(),
            "#0000ff".into(),
        )
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
/// cross-reader ritual (several independent readers). Ignored; run on demand:
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
    // One of each visible style, so the artifact demonstrates P4-EDIT-007b too.
    handle
        .add_link(
            0,
            [72.0, 700.0, 320.0, 724.0],
            "url".into(),
            "https://example.com".into(),
            "box".into(),
            "#0000ff".into(),
        )
        .await
        .expect("add url link");
    handle
        // 0-based "1" → page 2.
        .add_link(
            0,
            [72.0, 660.0, 320.0, 684.0],
            "page".into(),
            "1".into(),
            "underline".into(),
            "#cc0000".into(),
        )
        .await
        .expect("add page link");
    handle
        .add_link(
            0,
            [72.0, 620.0, 320.0, 644.0],
            "email".into(),
            "user@example.com".into(),
            "invisible".into(),
            "#000000".into(),
        )
        .await
        .expect("add email link");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote link verification artifact to {}", out.display());

    drop(handle);
}
