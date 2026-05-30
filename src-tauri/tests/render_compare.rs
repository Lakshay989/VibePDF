//! SPEC: P1-VIEW-004 — render-fidelity scaffold (P1.E2).
//!
//! For each fixture in `CASES`, render the page to RGBA8 (via the
//! document actor → PDFium) and compare it pixel-for-pixel, within a
//! tolerance, against a committed golden PNG. Any divergence is written
//! as a row into `tests/render-failures.md` and fails the gate test.
//!
//! ## Phase-1 interpretation of P1-VIEW-004
//!
//! The spec's bar is "same pixel-fidelity as Adobe Acrobat for the W3C
//! conformance suite." We have neither Acrobat output nor that suite
//! checked in, so this *scaffold* compares against a golden produced by
//! our **own** pipeline — a regression baseline, not an Acrobat
//! reference. The real conformance comparison is future work; this is
//! the machinery it will plug into. (Step title: "scaffold".)
//!
//! ## Conventions
//!
//! - Test CWD is `src-tauri/`, so repo paths are `../`-prefixed.
//! - The golden is regenerated on demand by the `#[ignore]`
//!   `bless_goldens` test after an intentional renderer change.
//! - `render-failures.md` is rewritten every run; on all-match the
//!   content is byte-identical, so a passing run leaves git clean.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::render::ImageFormat;

/// A fixture to check: PDF path (repo-relative), page (0-based), DPI,
/// and the golden PNG path (repo-relative).
struct Case {
    label: &'static str,
    pdf: &'static str,
    page: u32,
    dpi: f32,
    golden: &'static str,
}

const CASES: &[Case] = &[Case {
    label: "basic/hello.pdf",
    pdf: "../tests/fixtures/basic/hello.pdf",
    page: 0,
    dpi: 72.0,
    golden: "../tests/fixtures/golden/hello-p0-72dpi.png",
}];

/// Per-channel absolute delta a pixel may differ by before it counts as
/// "mismatched". Tolerates minor cross-platform anti-aliasing.
const CHANNEL_TOLERANCE: u8 = 16;
/// Maximum fraction of mismatched pixels before a case is a divergence.
const MAX_MISMATCH_FRACTION: f64 = 0.02;

const FAILURES_MD: &str = "../tests/render-failures.md";

async fn render_rgba(case: &Case) -> (u32, u32, Vec<u8>) {
    let pdf = PathBuf::from(case.pdf);
    assert!(pdf.is_file(), "fixture missing at {}", pdf.display());
    let id = uuid::Uuid::new_v4();
    let h = DocumentActorHandle::spawn(None, id, pdf, None)
        .expect("spawn should succeed on the fixture");
    let rp = h
        .render_page(case.page, case.dpi, ImageFormat::Rgba8)
        .await
        .expect("render_page (rgba8) should succeed");
    (rp.width, rp.height, rp.bytes)
}

/// Decode a golden PNG to (width, height, RGBA8 bytes). Panics with a
/// regenerate hint if the file is missing or not 8-bit RGBA.
fn decode_golden(path: &str) -> (u32, u32, Vec<u8>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "golden missing at {path}: {e}\n  regenerate with: \
             cargo test --manifest-path src-tauri/Cargo.toml -- --ignored bless_goldens",
        )
    });
    let decoder = png::Decoder::new(bytes.as_slice());
    let mut reader = decoder.read_info().expect("golden: read PNG header");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("golden: decode frame");
    assert_eq!(
        info.color_type,
        png::ColorType::Rgba,
        "golden must be RGBA (our encoder writes color type 6)",
    );
    assert_eq!(info.bit_depth, png::BitDepth::Eight, "golden must be 8-bit");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// Compare a fresh render against its golden. Returns `Some(detail)`
/// describing the divergence, or `None` on a match within tolerance.
fn compare(
    render: (u32, u32, &[u8]),
    golden: (u32, u32, &[u8]),
) -> Option<String> {
    let (rw, rh, rpx) = render;
    let (gw, gh, gpx) = golden;
    if (rw, rh) != (gw, gh) {
        return Some(format!(
            "dimension mismatch: rendered {rw}×{rh}, golden {gw}×{gh}"
        ));
    }
    if rpx.len() != gpx.len() {
        return Some(format!(
            "buffer length mismatch: rendered {}, golden {}",
            rpx.len(),
            gpx.len()
        ));
    }
    let total_pixels = (rw as usize) * (rh as usize);
    let mut mismatched = 0usize;
    for (a, b) in rpx.chunks_exact(4).zip(gpx.chunks_exact(4)) {
        let differs = a
            .iter()
            .zip(b.iter())
            .any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE);
        if differs {
            mismatched += 1;
        }
    }
    let fraction = mismatched as f64 / total_pixels.max(1) as f64;
    if fraction > MAX_MISMATCH_FRACTION {
        return Some(format!(
            "{mismatched}/{total_pixels} pixels differ ({:.2}% > {:.2}% threshold, |Δ|>{CHANNEL_TOLERANCE})",
            fraction * 100.0,
            MAX_MISMATCH_FRACTION * 100.0,
        ));
    }
    None
}

/// Rewrite `tests/render-failures.md`. Content is deterministic on an
/// all-match run (no timestamps), so a passing gate leaves git clean.
fn write_log(rows: &[(&Case, Option<String>)]) {
    let mut md = String::new();
    md.push_str("# Render-failure log\n\n");
    md.push_str("> SPEC: P1-VIEW-004 — render fidelity.\n>\n");
    md.push_str("> Phase-1 interpretation: each fixture's render must match its committed\n");
    md.push_str("> golden, a regression baseline produced by our own PDFium pipeline —\n");
    md.push_str("> **not** an Adobe Acrobat reference. The real Acrobat / W3C conformance\n");
    md.push_str("> comparison is future work; this scaffold is the machinery it plugs into.\n>\n");
    md.push_str("> Regenerate goldens after an intentional renderer change:\n>\n");
    md.push_str(">     cargo test --manifest-path src-tauri/Cargo.toml -- --ignored bless_goldens\n>\n");
    md.push_str(&format!(
        "> Tolerance: per-channel |Δ| ≤ {CHANNEL_TOLERANCE}, at most {:.0}% of pixels may differ.\n",
        MAX_MISMATCH_FRACTION * 100.0,
    ));
    md.push_str("> This file is rewritten by `src-tauri/tests/render_compare.rs` on every run.\n\n");

    md.push_str("## Checked fixtures\n\n");
    md.push_str("| Fixture | Page | DPI | Status |\n|---|---|---|---|\n");
    for (case, result) in rows {
        let status = if result.is_none() { "✅ match" } else { "❌ diverged" };
        md.push_str(&format!(
            "| {} | {} | {} | {status} |\n",
            case.label, case.page, case.dpi as i32,
        ));
    }

    md.push_str("\n## Divergences\n\n");
    let any = rows.iter().any(|(_, r)| r.is_some());
    if any {
        md.push_str("| Fixture | Page | DPI | Detail |\n|---|---|---|---|\n");
        for (case, result) in rows {
            if let Some(detail) = result {
                md.push_str(&format!(
                    "| {} | {} | {} | {detail} |\n",
                    case.label, case.page, case.dpi as i32,
                ));
            }
        }
    } else {
        md.push_str("_None._\n");
    }

    std::fs::write(FAILURES_MD, md).expect("write render-failures.md");
}

#[tokio::test]
async fn renders_match_goldens() {
    // SPEC: P1-VIEW-004 — the gate. Render every case, compare to its
    // golden, log the outcome, and fail if anything diverged.
    let mut rows: Vec<(&Case, Option<String>)> = Vec::new();
    for case in CASES {
        let (rw, rh, rpx) = render_rgba(case).await;
        let (gw, gh, gpx) = decode_golden(case.golden);
        let result = compare((rw, rh, &rpx), (gw, gh, &gpx));
        rows.push((case, result));
    }

    write_log(&rows);

    let diverged: Vec<String> = rows
        .iter()
        .filter_map(|(case, r)| r.as_ref().map(|d| format!("{}: {d}", case.label)))
        .collect();
    assert!(
        diverged.is_empty(),
        "render divergences (see tests/render-failures.md):\n{}",
        diverged.join("\n"),
    );
}

#[tokio::test]
#[ignore = "regenerates committed golden PNGs; run on demand after an intentional renderer change"]
async fn bless_goldens() {
    for case in CASES {
        let pdf = PathBuf::from(case.pdf);
        assert!(pdf.is_file(), "fixture missing at {}", pdf.display());
        let id = uuid::Uuid::new_v4();
        let h = DocumentActorHandle::spawn(None, id, pdf, None)
            .expect("spawn should succeed");
        let rp = h
            .render_page(case.page, case.dpi, ImageFormat::Png)
            .await
            .expect("render_page (png) should succeed");

        let path = PathBuf::from(case.golden);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create golden dir");
        }
        std::fs::write(&path, &rp.bytes).expect("write golden png");
        eprintln!(
            "blessed {} ({}×{}, {} bytes)",
            case.golden,
            rp.width,
            rp.height,
            rp.bytes.len(),
        );
    }
}
