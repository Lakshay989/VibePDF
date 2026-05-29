//! Bitmap-render utilities used by the document actor.
//!
//! This module is the single home for "`PDFium` page → bytes" so the
//! actor's `RenderPage` and `RenderThumbnail` paths share DPI math,
//! page-index validation, and (for `RenderPage`) PNG encoding. The
//! actor only knows about messages; the rasterisation lives here.

use std::io::Cursor;
use std::sync::Mutex;

use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::CommandError;

/// Process-wide lock around the `PDFium` render call.
///
/// `PDFium` is documented as per-document-safe but NOT process-safe
/// for the rendering subsystem: two concurrent `render_with_config`
/// calls from different documents still race on `FX_GE`'s global
/// state and crash with SIGTRAP (and `pages().get(idx)` is enough to
/// reproduce on its own under our `cargo test` runner). The
/// per-document actor pattern serialises reads *within* one document;
/// this lock extends that guarantee across documents.
///
/// Performance cost: rendering is single-threaded across the whole
/// process. For Phase 1 (≤ one viewer + one thumbnail panel per
/// document, modest scroll rates) this is comfortably under the
/// NFR-PERF-003 budget. If a future profiler shows it bottlenecking
/// scrolling, the right fix is to make the render call non-blocking
/// in `PDFium` itself (`FPDF_FFLDraw` with an interrupt callback),
/// not to remove the lock.
static RENDER_LOCK: Mutex<()> = Mutex::new(());

/// Wire-format selector for `pdf_render_page` / `Message::RenderPage`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// PNG bytes (8-bit RGBA, deflate-compressed). Ready to drop into
    /// an `<img>` tag or a Blob URL on the frontend.
    Png,
    /// Raw 8-bit RGBA pixels, row-major, no padding (`len == w*h*4`).
    /// The frontend wraps this in an `ImageData` and `putImageData`s
    /// it onto a canvas. Cheaper than PNG when the consumer is a
    /// canvas (no decode step).
    Rgba8,
}

/// IPC reply payload for `RenderPage`. `bytes` is whichever the
/// caller asked for in `format`.
///
/// Wire format note: serde's default `Vec<u8>` serializer emits a
/// JSON array-of-numbers, so a 1 MB PNG round-trips as ~5 MB of
/// JSON. Acceptable for thumbnails; if full-page renders show IPC
/// dominating in a profile, the upgrade path is either
/// `#[serde(with = "serde_bytes")]` (base64, ~1.3× overhead) or
/// `tauri::ipc::Response` (raw bytes, ~1× overhead). Both are a
/// one-line swap — see B3 plan, Risks #3.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub bytes: Vec<u8>,
}

/// Convert a DPI value to `PDFium`'s pixel-target-width for a given
/// page. `PdfRenderConfig::set_target_width` takes pixel width; "DPI"
/// in PDF terms means `pixels = (page_width_in_points / 72.0) * dpi`.
///
/// `dpi` is clamped defensively to `[1.0, 2000.0]` and the result is
/// clamped to a safe display-bound (`200_000` px); callers that trust
/// their inputs see no behavioral change.
fn target_width_from_dpi(page_width_points: f32, dpi: f32) -> i32 {
    // Conservative upper bound: at 2000 DPI on a 36-inch poster
    // (~2600 pt wide) you get ~72_000 px. 200_000 leaves plenty of
    // headroom while keeping the cast lossless on every real input.
    const MAX_PX: f32 = 200_000.0;
    let dpi = dpi.clamp(1.0, 2000.0);
    let px = ((page_width_points / 72.0) * dpi).round();
    if !px.is_finite() || px < 1.0 {
        1
    } else if px > MAX_PX {
        // PERFORMANCE: a 200k-px-wide render is absurd; we cap rather
        // than panic. PDFium will still allocate ~600 GB if asked, so
        // this guard matters.
        #[allow(clippy::cast_possible_truncation)]
        let v = MAX_PX as i32;
        v
    } else {
        // Safe: bounded above by `MAX_PX` (< i32::MAX) and below by 1.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = px as i32;
        v
    }
}

/// Render `page` at `dpi` and return the bytes in the requested
/// `format`. The actor calls this from its worker thread.
///
/// SPEC: P1-VIEW-001, P1-VIEW-004, P1-VIEW-008 — all three eventually
/// flow through this entry point on the Rust side.
///
/// Note on the inline page lookup: `PdfDocument<'a>` is invariant in
/// `'a`, so factoring `pages().get(idx)` into a `lookup_page` helper
/// requires lifetime gymnastics that don't pay for the ~4 lines of
/// duplication with `render_thumbnail` below. The body is short
/// enough to read both as-is.
pub fn render_page(
    doc: &PdfDocument<'_>,
    page: u32,
    dpi: f32,
    format: ImageFormat,
) -> Result<RenderedPage, CommandError> {
    let page_idx = i32::try_from(page)
        .map_err(|_| CommandError::InvalidInput(format!("page {page} out of range")))?;

    // Everything that calls into PDFium (page lookup, metadata read,
    // render) is guarded by RENDER_LOCK. PDFium's rendering and
    // page-state subsystems share process-global mutable state; even
    // `pages().get(idx)` from two threads is enough to SIGABRT.
    let (width, height, rgba) = {
        let _guard = RENDER_LOCK
            .lock()
            .map_err(|_| CommandError::Internal("render lock poisoned".into()))?;
        let pdf_page = doc.pages().get(page_idx).map_err(CommandError::from)?;
        let page_width_points = pdf_page.width().value;
        let target_w = target_width_from_dpi(page_width_points, dpi);
        let config = PdfRenderConfig::new().set_target_width(target_w);
        let bitmap = pdf_page
            .render_with_config(&config)
            .map_err(CommandError::from)?;
        let w = u32::try_from(bitmap.width()).unwrap_or(0);
        let h = u32::try_from(bitmap.height()).unwrap_or(0);
        let bytes = bitmap.as_rgba_bytes();
        (w, h, bytes)
    };

    // PDFium's `as_rgba_bytes` returns the bitmap row-major, RGBA,
    // no stride padding. If a future PDFium build adds padding we'll
    // see it here as `rgba.len() > w*h*4` and the test below
    // (`renders_rgba8_with_correct_buffer_size`) will fail loudly.
    debug_assert_eq!(
        rgba.len(),
        (width as usize) * (height as usize) * 4,
        "PDFium returned a padded bitmap; render.rs needs a stride-stripping pass"
    );

    let bytes = match format {
        ImageFormat::Rgba8 => rgba,
        ImageFormat::Png => encode_png(width, height, &rgba)?,
    };

    Ok(RenderedPage {
        width,
        height,
        format,
        bytes,
    })
}

/// Render a thumbnail at a target *pixel width* rather than a DPI.
/// Used by `Message::RenderThumbnail`. Always returns `Rgba8` — the
/// thumbnail consumer (D1) puts the bytes on a canvas; no PNG decode
/// step needed.
pub fn render_thumbnail(
    doc: &PdfDocument<'_>,
    page: u32,
    max_width: u32,
) -> Result<RenderedPage, CommandError> {
    let page_idx = i32::try_from(page)
        .map_err(|_| CommandError::InvalidInput(format!("page {page} out of range")))?;
    let target_w = i32::try_from(max_width.max(1)).unwrap_or(96);
    let config = PdfRenderConfig::new().set_target_width(target_w);

    // Same RENDER_LOCK rationale as `render_page`.
    let (width, height, rgba) = {
        let _guard = RENDER_LOCK
            .lock()
            .map_err(|_| CommandError::Internal("render lock poisoned".into()))?;
        let pdf_page = doc.pages().get(page_idx).map_err(CommandError::from)?;
        let bitmap = pdf_page
            .render_with_config(&config)
            .map_err(CommandError::from)?;
        let w = u32::try_from(bitmap.width()).unwrap_or(0);
        let h = u32::try_from(bitmap.height()).unwrap_or(0);
        let bytes = bitmap.as_rgba_bytes();
        (w, h, bytes)
    };

    Ok(RenderedPage {
        width,
        height,
        format: ImageFormat::Rgba8,
        bytes: rgba,
    })
}

/// Encode 8-bit RGBA pixels as PNG via the `png` crate. Compression
/// is left at the crate default (`Compression::Default` ≈ zlib level
/// 6) — a good balance for thumbnail and viewer use; export-to-image
/// can tune later.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, CommandError> {
    if width == 0 || height == 0 {
        return Err(CommandError::Internal(
            "cannot encode zero-dimension bitmap".into(),
        ));
    }
    let mut out = Vec::with_capacity(rgba.len() / 4);
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut out), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| CommandError::Internal(format!("png header: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| CommandError::Internal(format!("png data: {e}")))?;
        // `writer` is dropped here, finalising the PNG stream.
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_target_width_math() {
        // US-letter page (612 pt wide) at 72 DPI → 612 px.
        assert_eq!(target_width_from_dpi(612.0, 72.0), 612);
        // Same page at 144 DPI → 1224 px.
        assert_eq!(target_width_from_dpi(612.0, 144.0), 1224);
        // Defensive clamp on a nonsense DPI: 0.0 → 1.0 floor → 612 / 72 = 8.5 → 9.
        assert_eq!(target_width_from_dpi(612.0, 0.0), 9);
        // High-DPI clamp: DPI is clamped to 2000 *first* (see the
        // function's doc comment), so 99_999 DPI on a 612 pt letter page
        // is 612 / 72 * 2000 = 17_000 px — well under the 200_000 px
        // output ceiling, which never fires for letter-sized pages.
        assert_eq!(target_width_from_dpi(612.0, 99_999.0), 17_000);
        // Output ceiling (MAX_PX = 200_000): the only way to reach it is
        // an enormous page width, not a high DPI. A 10_000 pt page
        // (~139 inches) at the 2000 DPI ceiling is 10_000 / 72 * 2000 ≈
        // 277_778 px, which clamps down to 200_000.
        assert_eq!(target_width_from_dpi(10_000.0, 99_999.0), 200_000);
        // NaN / inf go to the safe floor.
        assert_eq!(target_width_from_dpi(612.0, f32::NAN), 1);
    }

    #[test]
    #[allow(clippy::unwrap_used)] // test code; the harness panics on Ok anyway
    fn png_encode_rejects_zero_dimensions() {
        let err = encode_png(0, 10, &[0; 40]).unwrap_err();
        assert!(matches!(err, CommandError::Internal(_)));
    }
}
