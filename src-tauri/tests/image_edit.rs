//! Integration tests for image editing (P4.C2).
//!
//! SPEC: P4-EDIT-006 — locate, move/resize/rotate (matrix), and delete an image.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::add_image;
use vibepdf_lib::pdf::image_edit::{delete_image, transform_image};
use vibepdf_lib::pdf::image_extract::{extract_images_from_bytes, ImageInfo};

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn make_png(w: u32, h: u32) -> Vec<u8> {
    let data = vec![140u8; (w * h) as usize * 3];
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
    }
    out
}

fn images_of(bytes: &[u8]) -> Vec<ImageInfo> {
    extract_images_from_bytes(bytes, 0).expect("extract")
}

/// A hello.pdf with one image added (via C1), aspect-fit into the given box.
fn hello_with_image() -> Vec<u8> {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    add_image(&original, 0, [100.0, 400.0, 300.0, 600.0], &make_png(4, 3)).expect("add image")
}

/// RISK #1 VALIDATION: does PDFium `reset_matrix` + regenerate + save + drop
/// SIGSEGV like `remove_object`, or work like `set_text`? If this test runs to a
/// normal pass/fail (not signal 11), the transform approach is viable.
#[test]
fn transform_moves_image_without_crashing() {
    let with_image = hello_with_image();
    let before = images_of(&with_image);
    assert_eq!(before.len(), 1, "the added image is located: {before:?}");

    // Override the placement matrix to a 120×90 box translated to (250, 300).
    let moved = transform_image(&with_image, 0, 0, [120.0, 0.0, 0.0, 90.0, 250.0, 300.0])
        .expect("transform");
    let after = images_of(&moved);
    assert_eq!(after.len(), 1, "image survives the transform");
    // The new bbox reflects the new matrix (origin ~250,300; size ~120×90).
    let [x0, y0, x1, y1] = after[0].bbox;
    assert!((x0 - 250.0).abs() < 2.0, "x0 moved to ~250: {x0}");
    assert!((y0 - 300.0).abs() < 2.0, "y0 moved to ~300: {y0}");
    assert!((x1 - x0 - 120.0).abs() < 2.0, "width ~120: {}", x1 - x0);
    assert!((y1 - y0 - 90.0).abs() < 2.0, "height ~90: {}", y1 - y0);
}

#[test]
fn extract_locates_the_added_image() {
    let imgs = images_of(&hello_with_image());
    assert_eq!(imgs.len(), 1);
    assert_eq!(imgs[0].index, 0);
    // Aspect-fit of 4×3 into [100,400,300,600] → centred, ~200×150 → bbox near box.
    let [x0, y0, x1, y1] = imgs[0].bbox;
    assert!(x0 >= 99.0 && x1 <= 301.0 && y0 >= 399.0 && y1 <= 601.0, "within the box: {:?}", imgs[0].bbox);
}

#[test]
fn delete_removes_the_image() {
    let with_image = hello_with_image();
    assert_eq!(images_of(&with_image).len(), 1);

    let deleted = delete_image(&with_image, 0, 0).expect("delete");
    assert!(images_of(&deleted).is_empty(), "the image is gone after delete");
}

#[test]
fn delete_preserves_other_images_ordinal_correctness() {
    // Two images at different boxes; deleting #0 must leave #1 where it was.
    let original = std::fs::read(fixture("hello.pdf")).expect("read");
    let one = add_image(&original, 0, [50.0, 50.0, 150.0, 150.0], &make_png(2, 2)).expect("add 0");
    let two = add_image(&one, 0, [400.0, 600.0, 500.0, 700.0], &make_png(2, 2)).expect("add 1");
    let before = images_of(&two);
    assert_eq!(before.len(), 2);

    let deleted = delete_image(&two, 0, 0).expect("delete image 0");
    let after = images_of(&deleted);
    assert_eq!(after.len(), 1, "exactly one image removed");
    // The survivor is the second image (its box near 400..500 / 600..700).
    let [x0, _, _, y1] = after[0].bbox;
    assert!(x0 >= 399.0 && y1 <= 701.0, "survivor is image #1: {:?}", after[0].bbox);
}

#[test]
fn out_of_range_errors() {
    let with_image = hello_with_image();
    assert!(transform_image(&with_image, 0, 9, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]).is_err());
    assert!(delete_image(&with_image, 0, 9).is_err());
    assert!(delete_image(&with_image, 9, 0).is_err());
}

#[tokio::test]
async fn actor_transform_delete_undo() {
    let path = std::env::temp_dir().join(format!("vibepdf-imgedit-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, hello_with_image()).expect("write temp");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, None).expect("spawn");

    assert_eq!(handle.read_images(0).await.expect("read").len(), 1);

    // Move/resize via the actor → bbox reflects the new matrix; undo restores.
    let state = handle
        .transform_image(0, 0, [120.0, 0.0, 0.0, 90.0, 250.0, 300.0])
        .await
        .expect("transform");
    assert!(state.can_undo);
    assert!((handle.read_images(0).await.expect("read")[0].bbox[0] - 250.0).abs() < 2.0, "moved");
    handle.undo().await.expect("undo");
    assert!((handle.read_images(0).await.expect("read")[0].bbox[0] - 100.0).abs() < 2.0, "undo restores");

    // Delete via the actor → gone; undo restores.
    handle.delete_image(0, 0).await.expect("delete");
    assert!(handle.read_images(0).await.expect("read").is_empty(), "deleted");
    handle.undo().await.expect("undo");
    assert_eq!(handle.read_images(0).await.expect("read").len(), 1, "delete undone");
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let with_image = hello_with_image();
    // Move + resize the image to a new box.
    let edited = transform_image(&with_image, 0, 0, [180.0, 0.0, 0.0, 240.0, 200.0, 350.0]).expect("transform");
    std::fs::write("/tmp/vibepdf-verify.pdf", &edited).expect("write artifact");
}

#[test]
fn rotate_90_swaps_aspect() {
    // A 90° rotation matrix about a point: [0, s, -s, 0, e, f]. Width/height swap.
    let with_image = hello_with_image();
    // Rotate the unit image 90°: [0 200 -150 0 300 400] → a 150-wide, 200-tall box.
    let rotated = transform_image(&with_image, 0, 0, [0.0, 200.0, -150.0, 0.0, 300.0, 400.0])
        .expect("rotate");
    let after = images_of(&rotated);
    assert_eq!(after.len(), 1);
    let [x0, y0, x1, y1] = after[0].bbox;
    assert!((x1 - x0 - 150.0).abs() < 2.0, "rotated width ~150: {}", x1 - x0);
    assert!((y1 - y0 - 200.0).abs() < 2.0, "rotated height ~200: {}", y1 - y0);
}
