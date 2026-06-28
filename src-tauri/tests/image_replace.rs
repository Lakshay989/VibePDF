//! Integration tests for replacing an image's pixel data (P4.C2b).
//!
//! SPEC: P4-EDIT-006 — "…allow … replace … The original image data SHALL be
//! preserved unless the user explicitly replaces it." Replace swaps the XObject
//! the image references, keeping its placement (bbox unchanged).

use std::path::PathBuf;

use lopdf::Document;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::add_image;
use vibepdf_lib::pdf::image_edit::replace_image;
use vibepdf_lib::pdf::image_extract::extract_images_from_bytes;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn make_png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let (color, comps) = if alpha {
        (png::ColorType::Rgba, 4)
    } else {
        (png::ColorType::Rgb, 3)
    };
    let data = vec![160u8; (w * h) as usize * comps];
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
    }
    out
}

const RECT: [f32; 4] = [100.0, 400.0, 300.0, 600.0];

fn hello_with_image(w: u32, h: u32) -> Vec<u8> {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    add_image(&original, 0, RECT, &make_png(w, h, false)).expect("add image")
}

/// The first image XObject's `[/Width, /Height, has /SMask]` on page 1.
fn first_image_dims(bytes: &[u8]) -> (i64, i64, bool) {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page");
    let resources = doc.get_dictionary(page_id).unwrap().get(b"Resources").unwrap().as_dict().unwrap();
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    let (_n, obj) = xobjects.iter().next().expect("an xobject");
    let stream = doc.get_object(obj.as_reference().unwrap()).unwrap().as_stream().unwrap();
    (
        stream.dict.get(b"Width").unwrap().as_i64().unwrap(),
        stream.dict.get(b"Height").unwrap().as_i64().unwrap(),
        stream.dict.get(b"SMask").is_ok(),
    )
}

#[test]
fn replace_swaps_pixels_preserving_placement() {
    let with_image = hello_with_image(4, 3);
    let before_bbox = extract_images_from_bytes(&with_image, 0).expect("extract")[0].bbox;
    assert_eq!(first_image_dims(&with_image).0, 4, "original is 4 wide");

    // Replace the 4×3 image with an 8×8 one.
    let replaced = replace_image(&with_image, 0, 0, &make_png(8, 8, false)).expect("replace");

    // The XObject now holds the new pixels…
    let (w, h, _) = first_image_dims(&replaced);
    assert_eq!((w, h), (8, 8), "pixels swapped to the new image");
    // …but the placement is unchanged.
    let after_bbox = extract_images_from_bytes(&replaced, 0).expect("extract")[0].bbox;
    assert!(
        before_bbox.iter().zip(&after_bbox).all(|(a, b)| (a - b).abs() < 1.0),
        "placement preserved: {before_bbox:?} vs {after_bbox:?}"
    );
}

#[test]
fn replace_keeps_alpha() {
    let with_image = hello_with_image(4, 3);
    assert!(!first_image_dims(&with_image).2, "the RGB original has no SMask");

    let replaced = replace_image(&with_image, 0, 0, &make_png(6, 6, true)).expect("replace");
    assert!(first_image_dims(&replaced).2, "the RGBA replacement carries an SMask");
}

#[test]
fn replace_preserves_other_images() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read");
    let one = add_image(&original, 0, [50.0, 50.0, 150.0, 150.0], &make_png(2, 2, false)).expect("add 0");
    let two = add_image(&one, 0, [400.0, 600.0, 500.0, 700.0], &make_png(2, 2, false)).expect("add 1");
    let before = extract_images_from_bytes(&two, 0).expect("extract");

    let replaced = replace_image(&two, 0, 1, &make_png(16, 16, false)).expect("replace #1");
    let after = extract_images_from_bytes(&replaced, 0).expect("extract");
    assert_eq!(after.len(), 2, "still two images");
    // Image #0's placement is untouched.
    assert!(
        before[0].bbox.iter().zip(&after[0].bbox).all(|(a, b)| (a - b).abs() < 1.0),
        "image #0 untouched: {:?} vs {:?}",
        before[0].bbox,
        after[0].bbox
    );
}

#[test]
fn out_of_range_errors() {
    let with_image = hello_with_image(4, 3);
    assert!(replace_image(&with_image, 0, 9, &make_png(2, 2, false)).is_err(), "bad index");
    assert!(replace_image(&with_image, 9, 0, &make_png(2, 2, false)).is_err(), "bad page");
    assert!(replace_image(&with_image, 0, 0, b"not an image").is_err(), "bad image bytes");
}

#[tokio::test]
async fn actor_replace_then_undo() {
    let path = std::env::temp_dir().join(format!("vibepdf-imgrepl-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, hello_with_image(4, 3)).expect("write");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, None).expect("spawn");

    assert_eq!(first_image_dims(&handle.get_bytes().await.expect("bytes")).0, 4, "starts at 4");
    let state = handle.replace_image(0, 0, make_png(8, 8, false)).await.expect("replace");
    assert!(state.can_undo);
    assert_eq!(first_image_dims(&handle.get_bytes().await.expect("bytes")).0, 8, "replaced to 8");

    handle.undo().await.expect("undo");
    assert_eq!(first_image_dims(&handle.get_bytes().await.expect("bytes")).0, 4, "undo restores 4");
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let with_image = hello_with_image(4, 3);
    let replaced = replace_image(&with_image, 0, 0, &make_png(64, 32, false)).expect("replace");
    std::fs::write("/tmp/vibepdf-verify.pdf", &replaced).expect("write artifact");
}
