//! Integration tests for the `Message::RenderPage` actor message and
//! the `pdf::render` module behind it (B3).
//!
//! SPEC: P1-VIEW-001, P1-VIEW-008, NFR-PERF-003. Render fidelity
//! (P1-VIEW-004) is tested by the visual-diff harness in E2; here we
//! only assert *that* bytes come out and that they're shaped right.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::render::ImageFormat;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

fn hello_pdf() -> PathBuf {
    // Test runs with CWD = src-tauri/.
    let p = PathBuf::from("../tests/fixtures/basic/hello.pdf");
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

async fn spawn() -> DocumentActorHandle {
    let id = uuid::Uuid::new_v4();
    DocumentActorHandle::spawn(None, id, hello_pdf(), None)
        .expect("spawn should succeed on hello.pdf")
}

#[tokio::test]
async fn renders_hello_pdf_page_0_as_png_at_72_dpi() {
    // SPEC: P1-VIEW-001 — first page renders. PNG path end-to-end.
    let h = spawn().await;
    let rp = h
        .render_page(0, 72.0, ImageFormat::Png)
        .await
        .expect("render_page should succeed");

    assert_eq!(rp.format, ImageFormat::Png);
    assert!(rp.width >= 1 && rp.height >= 1, "non-zero dimensions");
    assert!(rp.bytes.len() >= PNG_MAGIC.len(), "PNG too small to have a header");
    assert_eq!(
        &rp.bytes[..PNG_MAGIC.len()],
        PNG_MAGIC,
        "PNG magic bytes missing"
    );

    // Sanity: hello.pdf is mostly white with one line of text. We
    // don't decode the PNG here (no decoder in dev-deps), but the
    // encoded size for an all-white 612×792 RGBA PNG should be
    // small — a few kilobytes once deflate gets through it.
    assert!(
        rp.bytes.len() < 200_000,
        "PNG suspiciously large ({} bytes) — encoder may be misconfigured",
        rp.bytes.len()
    );
}

#[tokio::test]
async fn renders_rgba8_with_correct_buffer_size() {
    // Locks the "no stride padding" invariant in `pdf::render`. If a
    // future PDFium / pdfium-render version row-pads the bitmap, this
    // assertion fails and the fix is to stride-strip in render.rs.
    let h = spawn().await;
    let rp = h
        .render_page(0, 72.0, ImageFormat::Rgba8)
        .await
        .expect("rgba8 render should succeed");

    assert_eq!(rp.format, ImageFormat::Rgba8);
    let expected = (rp.width as usize) * (rp.height as usize) * 4;
    assert_eq!(
        rp.bytes.len(),
        expected,
        "RGBA8 buffer size mismatch (got {}, expected w*h*4 = {})",
        rp.bytes.len(),
        expected
    );

    // At least one fully-opaque pixel — proves the alpha channel
    // isn't being silently dropped.
    let has_opaque = rp.bytes.chunks_exact(4).any(|px| px[3] == 0xFF);
    assert!(has_opaque, "no fully-opaque pixel — alpha channel broken");
}

#[tokio::test]
async fn dpi_scaling_doubles_dimensions() {
    // Locks the DPI math in `target_width_from_dpi`. 144 DPI should
    // produce ~2× the dimensions of 72 DPI (±1 px for rounding).
    let h = spawn().await;
    let a = h.render_page(0, 72.0, ImageFormat::Rgba8).await.unwrap();
    let b = h.render_page(0, 144.0, ImageFormat::Rgba8).await.unwrap();

    let dw = (i64::from(b.width) - i64::from(a.width) * 2).abs();
    let dh = (i64::from(b.height) - i64::from(a.height) * 2).abs();
    assert!(
        dw <= 1,
        "width didn't scale 2× (72dpi={}, 144dpi={}, expected ~{}, diff {})",
        a.width,
        b.width,
        a.width * 2,
        dw
    );
    assert!(
        dh <= 1,
        "height didn't scale 2× (72dpi={}, 144dpi={}, expected ~{}, diff {})",
        a.height,
        b.height,
        a.height * 2,
        dh
    );
}

#[tokio::test]
async fn page_out_of_range_returns_typed_error() {
    // SPEC: P1-VIEW-002 — the render path must not panic on bad
    // input. hello.pdf has 1 page (index 0); page 999 is out of
    // range. PDFium surfaces a typed error; we just assert it's
    // *some* CommandError, not a panic.
    let h = spawn().await;
    let err = h
        .render_page(999, 72.0, ImageFormat::Png)
        .await
        .expect_err("page 999 should fail");
    let msg = format!("{err}");
    assert!(!msg.is_empty(), "error message should be non-empty");
}

#[tokio::test]
async fn rgba8_skips_png_encoding_and_is_smaller_in_memory_only_when_uncompressed() {
    // Sanity check: RGBA8 buffer is *larger* than the PNG (it's
    // uncompressed). This catches an accidental swap of the two
    // branches in `render::render_page`.
    let h = spawn().await;
    let png = h.render_page(0, 72.0, ImageFormat::Png).await.unwrap();
    let rgba = h.render_page(0, 72.0, ImageFormat::Rgba8).await.unwrap();
    assert!(
        rgba.bytes.len() > png.bytes.len(),
        "RGBA8 ({}) should be larger than PNG ({}); branches probably swapped",
        rgba.bytes.len(),
        png.bytes.len()
    );
}

/// Performance sentinel for the step's "≤ 50 ms on hello.pdf at 72
/// DPI" budget. Marked `#[ignore]` because:
///
/// 1. PDFium debug builds are several × slower than release; this
///    test only makes sense under `--release`.
/// 2. CI hardware varies; a loose envelope (≤ 200 ms median) catches
///    catastrophic regressions without flaking on slow runners.
///
/// Run with `cargo test --release -- --include-ignored`.
#[tokio::test]
#[ignore = "release-only performance sentinel"]
async fn renders_within_budget() {
    let h = spawn().await;
    // Warm-up: first render pays PDFium's lazy-init costs.
    let _ = h.render_page(0, 72.0, ImageFormat::Png).await.unwrap();

    let mut times = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let _ = h.render_page(0, 72.0, ImageFormat::Png).await.unwrap();
        times.push(t.elapsed());
    }
    times.sort();
    let median = times[1];
    assert!(
        median <= Duration::from_millis(200),
        "median render time {median:?} > 200 ms loose envelope (step budget: 50 ms)"
    );
}
