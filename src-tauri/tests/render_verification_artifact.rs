//! Writes a sample render to `/tmp/vibepdf-verify-72dpi.png` and
//! `/tmp/vibepdf-verify-144dpi.png` so a human can eyeball the
//! result. Not part of CI gates; marked `#[ignore]` and run on
//! demand via `cargo test -- --include-ignored render_verify`.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::render::ImageFormat;

#[tokio::test]
#[ignore = "produces verification artifacts; run on demand"]
async fn render_verify_writes_pngs_to_tmp() {
    let fixture = PathBuf::from("../tests/fixtures/basic/hello.pdf");
    assert!(fixture.is_file());

    let id = uuid::Uuid::new_v4();
    let h = DocumentActorHandle::spawn(None, id, fixture, None).unwrap();

    for dpi in [72.0_f32, 144.0_f32] {
        let rp = h.render_page(0, dpi, ImageFormat::Png).await.unwrap();
        let out = format!("/tmp/vibepdf-verify-{}dpi.png", dpi as i32);
        std::fs::write(&out, &rp.bytes).unwrap();
        eprintln!(
            "wrote {} ({}x{}, {} bytes)",
            out,
            rp.width,
            rp.height,
            rp.bytes.len()
        );
    }
}
