//! SPEC: P7-OCR-003 — prepare a page image before OCR: deskew, denoise, and
//! upscale anything below 300 DPI. Every stage is switchable, which is the
//! spec's "configurable".
//!
//! Written against an 8-bit grayscale buffer we own rather than pulling in an
//! image-processing crate: the three operations are a few dozen lines each,
//! and `docs/03_TECH_STACK.md` already argues for keeping this dependency
//! surface small (the `png` crate is there for the same reason). Tesseract
//! binarises internally, so nothing here thresholds.

/// 8-bit grayscale, row-major, one byte per pixel. 0 is black, 255 is white.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrayImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl GrayImage {
    /// # Errors
    /// When `pixels` is not exactly `width * height` bytes.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, PreprocessError> {
        let expected = width as usize * height as usize;
        if pixels.len() == expected {
            Ok(Self {
                width,
                height,
                pixels,
            })
        } else {
            Err(PreprocessError::Size {
                expected,
                got: pixels.len(),
            })
        }
    }

    /// Convert a `PDFium` page render (`ImageFormat::Rgba8`) to grayscale.
    ///
    /// Rec. 601 luma, the weighting scanners and Tesseract's own conversion
    /// use. Alpha is ignored: page renders are opaque, and a transparent
    /// pixel is paper anyway.
    ///
    /// # Errors
    /// When `rgba` is not exactly `width * height * 4` bytes.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Self, PreprocessError> {
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(PreprocessError::Size {
                expected,
                got: rgba.len(),
            });
        }
        let pixels = rgba
            .chunks_exact(4)
            .map(|p| {
                (0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]))
                    .round()
                    .clamp(0.0, 255.0) as u8
            })
            .collect();
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            WHITE
        } else {
            self.pixels[y as usize * self.width as usize + x as usize]
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PreprocessError {
    #[error("image buffer is {got} bytes, expected {expected}")]
    Size { expected: usize, got: usize },
    #[error("image has no pixels")]
    Empty,
}

const WHITE: u8 = 255;

/// Which stages run. Defaults are the spec's defaults: all three on, 300 DPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreprocessOptions {
    pub deskew: bool,
    pub denoise: bool,
    /// Upscale when the source is below this. 0 switches upscaling off.
    pub min_dpi: u32,
}

impl Default for PreprocessOptions {
    fn default() -> Self {
        Self {
            deskew: true,
            denoise: true,
            min_dpi: 300,
        }
    }
}

/// The largest tilt worth looking for, in degrees. Beyond this a page is not
/// skewed, it is rotated, and guessing would do more harm than good.
const MAX_SKEW_DEGREES: f32 = 8.0;
/// Estimation runs on a copy no wider than this: skew is a property of the
/// layout, not of the detail, and a full-size page costs seconds.
const SKEW_ESTIMATE_WIDTH: u32 = 600;

/// Run the enabled stages, in the order that makes each cheaper: denoise
/// first (so speckles don't pull the skew estimate), then deskew, then
/// upscale (so rotation runs on the smaller image).
///
/// `source_dpi` is what the page image was rendered at.
///
/// # Errors
/// When the image is empty.
pub fn preprocess(
    image: &GrayImage,
    source_dpi: u32,
    options: PreprocessOptions,
) -> Result<GrayImage, PreprocessError> {
    Ok(preprocess_reported(image, source_dpi, options)?.0)
}

/// What the pipeline did to the image, so a caller can map coordinates in the
/// processed image back to the original.
///
/// P7-OCR-001 needs exactly this: Tesseract's word boxes are in the *processed*
/// image, and the invisible text has to land on the ink in the *page*.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreprocessReport {
    /// How much the image was enlarged (1.0 when it was not).
    pub scale: f64,
    /// The tilt that was straightened out, in degrees, positive meaning the
    /// text ran down to the right. 0 when deskewing was off or found nothing.
    pub skew_degrees: f32,
}

/// [`preprocess`], reporting what it did.
///
/// # Errors
/// When the image is empty.
pub fn preprocess_reported(
    image: &GrayImage,
    source_dpi: u32,
    options: PreprocessOptions,
) -> Result<(GrayImage, PreprocessReport), PreprocessError> {
    if image.width == 0 || image.height == 0 {
        return Err(PreprocessError::Empty);
    }
    let mut out = image.clone();
    let mut report = PreprocessReport {
        scale: 1.0,
        skew_degrees: 0.0,
    };
    if options.denoise {
        out = despeckle(&out);
    }
    if options.deskew {
        let angle = estimate_skew_degrees(&out);
        if angle.abs() >= 0.1 {
            out = rotate(&out, -angle);
            report.skew_degrees = angle;
        }
    }
    if options.min_dpi > 0 && source_dpi > 0 && source_dpi < options.min_dpi {
        let factor = f64::from(options.min_dpi) / f64::from(source_dpi);
        out = upscale(&out, factor);
        // What the resize actually achieved, not what was asked for: `upscale`
        // rounds to whole pixels, and a caller mapping boxes back needs the
        // ratio that moved them.
        report.scale = f64::from(out.width) / f64::from(image.width);
    }
    Ok((out, report))
}

/// Replace isolated specks with the median of their neighbours, and leave
/// every other pixel exactly as it was.
///
/// A plain 3×3 median was the first implementation and was measurably worse:
/// on 9 pt text rendered at 150 DPI it cost 3 of 10 words (`brown` → `broval`,
/// `VibePDF` → `Vibe POF`), because at that size a stroke is 2–3 pixels wide
/// and a median rounds its edges away. Measured 2026-09-21; the same page with
/// denoising off read perfectly.
///
/// So this only fires where the evidence for a speck is strong: the pixel is
/// the darkest or lightest in its window, it differs from its neighbours'
/// median by more than [`SPECK_CONTRAST`], and at least seven of its eight
/// neighbours sit on the other side of that median. A stroke pixel has
/// neighbours that agree with it, so it survives; a lone dust mote does not.
#[must_use]
pub fn despeckle(image: &GrayImage) -> GrayImage {
    let mut pixels = image.pixels.clone();
    if image.width < 3 || image.height < 3 {
        return image.clone();
    }
    for y in 1..image.height - 1 {
        for x in 1..image.width - 1 {
            let centre = image.pixel(x, y);
            let mut neighbours = [0u8; 8];
            let mut i = 0;
            for dy in 0..3u32 {
                for dx in 0..3u32 {
                    if dx == 1 && dy == 1 {
                        continue;
                    }
                    neighbours[i] = image.pixel(x + dx - 1, y + dy - 1);
                    i += 1;
                }
            }
            neighbours.sort_unstable();
            // (a + b) / 2 without midpoint(), which needs Rust 1.85 and our MSRV is 1.80.
            let median = (u16::from(neighbours[3]) + u16::from(neighbours[4])) / 2;
            let centre16 = u16::from(centre);
            let contrast = centre16.abs_diff(median);
            if contrast <= SPECK_CONTRAST {
                continue;
            }
            let agreeing = neighbours
                .iter()
                .filter(|&&n| {
                    if centre16 > median {
                        u16::from(n) < centre16 - contrast / 2
                    } else {
                        u16::from(n) > centre16 + contrast / 2
                    }
                })
                .count();
            if agreeing >= 7 {
                #[allow(clippy::cast_possible_truncation)]
                let replacement = median as u8;
                pixels[y as usize * image.width as usize + x as usize] = replacement;
            }
        }
    }
    GrayImage {
        width: image.width,
        height: image.height,
        pixels,
    }
}

/// How far a pixel must stand out from its neighbours before it counts as a
/// speck rather than part of a stroke. 64 of 255 is roughly "a quarter of the
/// way across the greyscale", well beyond antialiasing.
const SPECK_CONTRAST: u16 = 64;

/// Estimate page tilt in degrees; positive means the text runs down to the
/// right, so `rotate(-angle)` straightens it.
///
/// Projection profiles: rotate a candidate angle, sum the ink in each row,
/// and score by how much those sums vary. Straight text puts every line's ink
/// in a few rows (spiky profile, high variance); tilted text smears it across
/// many (flat profile). The best-scoring candidate is the tilt.
#[must_use]
pub fn estimate_skew_degrees(image: &GrayImage) -> f32 {
    let small = if image.width > SKEW_ESTIMATE_WIDTH {
        downscale_to_width(image, SKEW_ESTIMATE_WIDTH)
    } else {
        image.clone()
    };
    if small.height < 3 {
        return 0.0;
    }

    let mut best = (0.0f32, f64::MIN);
    let mut angle = -MAX_SKEW_DEGREES;
    while angle <= MAX_SKEW_DEGREES {
        let score = row_profile_variance(&rotate(&small, -angle));
        if score > best.1 {
            best = (angle, score);
        }
        angle += 0.25;
    }
    best.0
}

/// How unevenly ink is spread across rows. Ink is `255 - value`, so a blank
/// page scores 0 and a page of ruled lines scores high.
#[allow(clippy::cast_precision_loss)] // row counts are thousands, not 2^53
fn row_profile_variance(image: &GrayImage) -> f64 {
    let width = image.width as usize;
    let sums: Vec<f64> = (0..image.height as usize)
        .map(|y| {
            image.pixels[y * width..(y + 1) * width]
                .iter()
                .map(|&p| f64::from(WHITE - p))
                .sum()
        })
        .collect();
    let mean = sums.iter().sum::<f64>() / sums.len() as f64;
    sums.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / sums.len() as f64
}

/// Rotate about the centre by `degrees` (positive = clockwise), sampling
/// bilinearly and filling anything outside the source with white — paper, not
/// black borders, because black borders would themselves look like ink.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn rotate(image: &GrayImage, degrees: f32) -> GrayImage {
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let (w, h) = (image.width as f32, image.height as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let mut pixels = vec![WHITE; image.pixels.len()];

    for y in 0..image.height {
        for x in 0..image.width {
            // Sample the source at the inverse rotation of this destination
            // pixel, which avoids the holes a forward mapping leaves.
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let sx = dx * cos + dy * sin + cx;
            let sy = -dx * sin + dy * cos + cy;
            if sx < -0.5 || sy < -0.5 || sx > w - 0.5 || sy > h - 0.5 {
                continue;
            }
            pixels[y as usize * image.width as usize + x as usize] = sample_bilinear(image, sx, sy);
        }
    }
    GrayImage {
        width: image.width,
        height: image.height,
        pixels,
    }
}

/// Bicubic-quality upscale is overkill here: Tesseract wants edges that stay
/// put, and bilinear keeps stroke centres where they were. `factor` above 1
/// only; the caller decides whether upscaling applies.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn upscale(image: &GrayImage, factor: f64) -> GrayImage {
    if factor <= 1.0 {
        return image.clone();
    }
    let width = ((f64::from(image.width) * factor).round() as u32).max(1);
    let height = ((f64::from(image.height) * factor).round() as u32).max(1);
    let mut pixels = vec![WHITE; width as usize * height as usize];
    let x_scale = f64::from(image.width) / f64::from(width);
    let y_scale = f64::from(image.height) / f64::from(height);
    for y in 0..height {
        for x in 0..width {
            let sx = ((f64::from(x) + 0.5) * x_scale - 0.5) as f32;
            let sy = ((f64::from(y) + 0.5) * y_scale - 0.5) as f32;
            pixels[y as usize * width as usize + x as usize] =
                sample_bilinear(image, sx.max(0.0), sy.max(0.0));
        }
    }
    GrayImage {
        width,
        height,
        pixels,
    }
}

/// Box-average downscale, used only to make skew estimation cheap.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn downscale_to_width(image: &GrayImage, target: u32) -> GrayImage {
    let factor = f64::from(target) / f64::from(image.width);
    let height = ((f64::from(image.height) * factor).round() as u32).max(1);
    let mut pixels = vec![WHITE; target as usize * height as usize];
    let (bx, by) = (
        f64::from(image.width) / f64::from(target),
        f64::from(image.height) / f64::from(height),
    );
    for y in 0..height {
        for x in 0..target {
            let x0 = (f64::from(x) * bx) as u32;
            let y0 = (f64::from(y) * by) as u32;
            let x1 = (((f64::from(x) + 1.0) * bx) as u32).min(image.width).max(x0 + 1);
            let y1 = (((f64::from(y) + 1.0) * by) as u32).min(image.height).max(y0 + 1);
            let mut total = 0u32;
            let mut count = 0u32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    total += u32::from(image.pixel(sx, sy));
                    count += 1;
                }
            }
            pixels[y as usize * target as usize + x as usize] = (total / count.max(1)) as u8;
        }
    }
    GrayImage {
        width: target,
        height,
        pixels,
    }
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn sample_bilinear(image: &GrayImage, x: f32, y: f32) -> u8 {
    let x0 = x.floor().max(0.0) as u32;
    let y0 = y.floor().max(0.0) as u32;
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (x1, y1) = (x0 + 1, y0 + 1);
    let top = f32::from(image.pixel(x0, y0)) * (1.0 - fx) + f32::from(image.pixel(x1, y0)) * fx;
    let bottom = f32::from(image.pixel(x0, y1)) * (1.0 - fx) + f32::from(image.pixel(x1, y1)) * fx;
    (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod preprocess_tests {
    use super::{
        despeckle, estimate_skew_degrees, preprocess, rotate, GrayImage, PreprocessError,
        PreprocessOptions,
    };

    /// A page of ruled "text lines": eight black bars on white. Enough
    /// structure for a projection profile, and its true skew is known.
    fn ruled_page() -> GrayImage {
        let (w, h) = (300u32, 200u32);
        let mut pixels = vec![255u8; (w * h) as usize];
        for line in 0..8u32 {
            let y0 = 12 + line * 22;
            for y in y0..y0 + 6 {
                for x in 30..w - 30 {
                    pixels[(y * w + x) as usize] = 0;
                }
            }
        }
        GrayImage::new(w, h, pixels).expect("size matches")
    }

    fn ink(image: &GrayImage) -> usize {
        image.pixels.iter().filter(|&&p| p < 128).count()
    }

    #[test]
    fn a_straight_page_reports_no_skew() {
        assert!(estimate_skew_degrees(&ruled_page()).abs() < 0.3);
    }

    #[test]
    fn deskew_straightens_a_known_tilt() {
        for tilt in [-3.0f32, 1.5, 4.0] {
            let tilted = rotate(&ruled_page(), tilt);
            let found = estimate_skew_degrees(&tilted);
            assert!(
                (found - tilt).abs() < 0.6,
                "tilt {tilt}: estimated {found}"
            );
            // And the pipeline actually applies it.
            let fixed = preprocess(
                &tilted,
                300,
                PreprocessOptions {
                    denoise: false,
                    ..PreprocessOptions::default()
                },
            )
            .expect("preprocess");
            assert!(
                estimate_skew_degrees(&fixed).abs() < 0.6,
                "tilt {tilt}: still skewed after deskew"
            );
        }
    }

    #[test]
    fn denoise_removes_speckles_without_eating_strokes() {
        let clean = ruled_page();
        let mut speckled = clean.clone();
        // Single black pixels in the left margin, 5 apart in both directions so
        // no two share a 3x3 window — that is what "speckle" means, and two
        // touching specks are a stroke as far as a median filter can tell.
        let mut added = 0;
        for y in (5..clean.height - 5).step_by(5) {
            for x in (5..25).step_by(5) {
                assert_eq!(clean.pixel(x, y), 255, "speckle at {x},{y} lands on a stroke");
                speckled.pixels[(y * clean.width + x) as usize] = 0;
                added += 1;
            }
        }
        assert_eq!(ink(&speckled), ink(&clean) + added, "fixture added no speckles");

        let filtered = despeckle(&speckled);
        assert_eq!(ink(&filtered), ink(&despeckle(&clean)), "speckles survived");
        // The bars are still there: a median filter must not erase strokes.
        assert!(ink(&filtered) > ink(&clean) * 9 / 10, "strokes were eaten");
    }

    #[test]
    fn upscale_only_below_the_target_dpi() {
        let page = ruled_page();
        let options = PreprocessOptions {
            deskew: false,
            denoise: false,
            min_dpi: 300,
        };

        let upscaled = preprocess(&page, 150, options).expect("preprocess");
        assert_eq!((upscaled.width, upscaled.height), (600, 400));

        let untouched = preprocess(&page, 400, options).expect("preprocess");
        assert_eq!((untouched.width, untouched.height), (page.width, page.height));

        let disabled = preprocess(&page, 150, PreprocessOptions { min_dpi: 0, ..options })
            .expect("preprocess");
        assert_eq!((disabled.width, disabled.height), (page.width, page.height));
    }

    #[test]
    fn every_stage_can_be_switched_off() {
        let page = ruled_page();
        let untouched = preprocess(
            &page,
            72,
            PreprocessOptions {
                deskew: false,
                denoise: false,
                min_dpi: 0,
            },
        )
        .expect("preprocess");
        assert_eq!(untouched, page, "a no-op pipeline must return the input");
    }

    #[test]
    fn a_mismatched_buffer_is_refused() {
        assert_eq!(
            GrayImage::new(4, 4, vec![0; 10]).unwrap_err(),
            PreprocessError::Size {
                expected: 16,
                got: 10
            }
        );
        let empty = GrayImage::new(0, 0, vec![]).expect("empty is a valid buffer");
        assert_eq!(
            preprocess(&empty, 300, PreprocessOptions::default()).unwrap_err(),
            PreprocessError::Empty
        );
    }
}
