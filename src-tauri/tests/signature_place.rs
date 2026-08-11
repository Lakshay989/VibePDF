//! Integration tests for placing a saved signature (P6.A5a).
//!
//! SPEC: P6-SEC-004 — "THE system SHALL embed it as a stamp annotation". The
//! other clause (a PKCS#7 signature into a `/Sig` field) needs P6.B1 and is not
//! implemented; the frontend declines that case rather than stamping over a
//! signature field, and `src/tools/signature/place.ts` tests that decision.
//!
//! `pdf_place_signature` itself needs a Tauri `AppHandle` to locate the library,
//! so what is exercised here is everything on either side of that: the library
//! round-trip that produces the bytes, and the placement that consumes them.
//! The stamp primitive is covered in `stamp.rs`; what is new here is the
//! composition, and the fact that a **real signature** is a shape those tests
//! never used — very wide, mostly transparent, with a varying alpha channel
//! rather than a uniform one.

use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::add_image_stamp;
use vibepdf_lib::settings::signatures::{self, SignatureKind};

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

/// A fresh temp directory, removed when the guard drops.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-place-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).expect("mkdir");
        Self(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An RGBA PNG shaped like a real signature: dark ink, and an alpha channel that
/// **varies** — mostly clear with a band of ink. The stamp tests use a uniform
/// fill, which cannot tell a carried alpha channel from a constant one.
fn signature_png(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            // A diagonal stroke of ink across an otherwise empty image.
            let on = (x + y) % 7 < 2;
            data.extend_from_slice(&[24, 26, 48, if on { 255 } else { 0 }]);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("header");
        w.write_image_data(&data).expect("data");
    }
    out
}

/// The first Image XObject that carries an `/SMask` reference, plus the mask.
fn image_and_mask(doc: &Document) -> Option<(Dictionary, Dictionary)> {
    for o in doc.objects.values() {
        let Ok(s) = o.as_stream() else { continue };
        if s.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(&b"Image"[..]) {
            continue;
        }
        let Ok(id) = s.dict.get(b"SMask").and_then(Object::as_reference) else { continue };
        let mask = doc.get_object(id).ok()?.as_stream().ok()?;
        return Some((s.dict.clone(), mask.dict.clone()));
    }
    None
}

/// The first `/Stamp` annotation's `/Rect` on page 1.
fn first_stamp_rect(doc: &Document) -> Option<[f32; 4]> {
    let page_id = *doc.get_pages().get(&1)?;
    let arr = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => {
            doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
        }
        _ => return None,
    };
    let dict = arr
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .filter_map(|id| doc.get_dictionary(id).ok())
        .find(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Stamp"[..]))?;
    let r = dict.get(b"Rect").and_then(Object::as_array).ok()?;
    let n = |i: usize| match r.get(i) {
        Some(Object::Real(v)) => *v,
        Some(Object::Integer(v)) => *v as f32,
        _ => 0.0,
    };
    Some([n(0), n(1), n(2), n(3)])
}

/// Page 1's `/MediaBox`, so clamping is checked against the real page rather
/// than an assumption about letter size.
fn media_box(doc: &Document) -> [f32; 4] {
    let fallback = [0.0, 0.0, 612.0, 792.0];
    let Some(&page_id) = doc.get_pages().get(&1) else { return fallback };
    let Ok(dict) = doc.get_dictionary(page_id) else { return fallback };
    let Ok(arr) = dict.get(b"MediaBox").and_then(Object::as_array) else { return fallback };
    let n = |i: usize| match arr.get(i) {
        Some(Object::Real(v)) => *v,
        Some(Object::Integer(v)) => *v as f32,
        _ => 0.0,
    };
    [n(0), n(1), n(2), n(3)]
}

/// The height the UI places at (`SIGNATURE_HEIGHT` in `tools/signature/place.ts`).
const PLACE_HEIGHT: f32 = 40.0;

// SPEC: P6-SEC-004 — a signature stored in the library places as a stamp.
#[test]
fn a_library_signature_places_as_a_stamp() {
    let dir = TempDir::new();
    let png = signature_png(620, 230);
    let entry = signatures::add(dir.path(), SignatureKind::Draw, &png, 1_700_000_000_000)
        .expect("save to library");

    // This is what `pdf_place_signature` does between the library and the actor.
    let bytes = signatures::bytes(dir.path(), &entry.id).expect("read back");
    assert_eq!(bytes, png, "the bytes placed are the bytes stored");

    let out = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 300.0, 400.0, PLACE_HEIGHT, &bytes, None, 1.0)
        .expect("place");
    let doc = Document::load_mem(&out).expect("load");
    assert!(first_stamp_rect(&doc).is_some(), "a /Stamp annotation is written");
}

// SPEC: P6-SEC-004 — the signature's transparency survives into the PDF, so a
// placed signature does not arrive inside a white box.
#[test]
fn placement_carries_the_whole_alpha_channel() {
    let dir = TempDir::new();
    let (w, h) = (64u32, 32u32);
    let entry = signatures::add(dir.path(), SignatureKind::Draw, &signature_png(w, h), 1)
        .expect("save");
    let bytes = signatures::bytes(dir.path(), &entry.id).expect("read back");

    let out = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 200.0, 200.0, PLACE_HEIGHT, &bytes, None, 1.0)
        .expect("place");
    let doc = Document::load_mem(&out).expect("load");

    let (img, mask) = image_and_mask(&doc).expect("an Image XObject with an /SMask");
    assert_eq!(img.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceRGB");
    assert_eq!(mask.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceGray");
    // Full-resolution mask, not a token one: a smaller mask would still satisfy
    // "an /SMask exists" while quietly throwing the shape of the ink away.
    assert_eq!(mask.get(b"Width").unwrap().as_i64().unwrap(), i64::from(w));
    assert_eq!(mask.get(b"Height").unwrap().as_i64().unwrap(), i64::from(h));
}

// SPEC: P6-SEC-004 — a signature is wide; placing it must not stretch it.
#[test]
fn a_signature_is_placed_at_its_own_proportions() {
    // 620×230 — the shape a real signature raster comes out at (P6.A2–A4 fit
    // the long edge to 600 and pad, so this is not a hypothetical).
    let png = signature_png(620, 230);
    let out = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 300.0, 400.0, PLACE_HEIGHT, &png, None, 1.0)
        .expect("place");
    let doc = Document::load_mem(&out).expect("load");

    let [x0, y0, x1, y1] = first_stamp_rect(&doc).expect("a /Stamp rect");
    let (w, h) = (x1 - x0, y1 - y0);
    assert!((h - PLACE_HEIGHT).abs() < 0.5, "height honoured: {h}");
    let want = 620.0 / 230.0;
    assert!((w / h - want).abs() < 0.05, "aspect {want:.2} preserved, got {:.2}", w / h);
}

// SPEC: P6-SEC-004 — an extreme aspect ratio must not push the stamp off the
// page. A long flourish is a plausible signature, and 40pt tall at 30:1 is
// wider than any page.
//
// This pins **existing** `add_image_stamp` behaviour (P3.C3b) rather than
// asserting it is right, because it is not entirely: staying on the page wins,
// and the aspect ratio is what pays for it. A 1200×40 image comes back as
// 612×40 — squashed to roughly half its proper width — which contradicts that
// function's own "never stretched". It is only reachable when the natural width
// would exceed the page: a real signature is ~2.7:1, so at 40pt tall it is
// ~108pt wide and nowhere near the limit. Recorded here so the behaviour is
// visible instead of surprising, and so a later fix has something to change.
#[test]
fn an_extremely_wide_signature_is_clamped_to_the_page_at_the_cost_of_its_aspect() {
    let png = signature_png(1200, 40);
    let out = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 300.0, 400.0, PLACE_HEIGHT, &png, None, 1.0)
        .expect("place");
    let doc = Document::load_mem(&out).expect("load");

    let [x0, y0, x1, y1] = first_stamp_rect(&doc).expect("a /Stamp rect");
    let [mx0, my0, mx1, my1] = media_box(&doc);
    assert!(x1 - x0 > 0.0 && y1 - y0 > 0.0, "a real rect, not a degenerate one");
    assert!(
        x0 >= mx0 - 0.5 && x1 <= mx1 + 0.5,
        "stays within the page horizontally: {x0}..{x1} in {mx0}..{mx1}"
    );
    assert!(
        y0 >= my0 - 0.5 && y1 <= my1 + 0.5,
        "stays within the page vertically: {y0}..{y1} in {my0}..{my1}"
    );
    // The distortion, stated rather than glossed: 30:1 in, ~15:1 out.
    let natural = 1200.0 / 40.0;
    assert!(
        (x1 - x0) / (y1 - y0) < natural * 0.75,
        "clamping squashes rather than shrinking — see the comment above"
    );
}

/// SPEC: P6-SEC-004 (P6.A5a) — a file to open in Acrobat / Preview / a third
/// reader. A passing test does not prove the PDF is valid to other viewers, and
/// transparency is exactly the sort of thing one renderer gets right and
/// another does not.
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn signature_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-signature.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Real signature proportions, at the height the UI places at, plus a second
    // at half opacity — the two things worth eyeballing across readers.
    handle
        .add_image_stamp(0, 200.0, 600.0, PLACE_HEIGHT, signature_png(620, 230), None, 1.0)
        .await
        .expect("place");
    handle
        .add_image_stamp(0, 200.0, 500.0, PLACE_HEIGHT, signature_png(620, 230), None, 0.5)
        .await
        .expect("place at half opacity");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote signature verification artifact to {}", out.display());

    drop(handle);
}
