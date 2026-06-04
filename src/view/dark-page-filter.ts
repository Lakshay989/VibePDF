/**
 * CSS filter that inverts a rendered PDF page for dark mode.
 *
 * Applied as a compositor filter (crisp at native resolution, unlike a
 * per-pixel getImageData invert) on BOTH the main page canvases
 * (`PageVirtualizer`) and the sidebar thumbnails (`ThumbnailPanel`), so
 * the two always agree. `invert(1)` flips luminance; `hue-rotate(180deg)`
 * undoes the hue flip invert introduces, so colours stay recognisable.
 */
export const DARK_PAGE_FILTER = "invert(1) hue-rotate(180deg)";
