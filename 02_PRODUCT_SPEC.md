# 02 — Product Specification

This is the **source of truth** for what VibePDF does. All requirements are written in EARS syntax (Easy Approach to Requirements Syntax), which structures requirements so they're unambiguous to humans and to LLMs.

### EARS quick reference

- **Ubiquitous:** `THE system SHALL ...` (always true)
- **Event-driven:** `WHEN <event>, THE system SHALL ...`
- **Conditional:** `WHILE <state>, THE system SHALL ...`
- **State-driven:** `IF <condition>, THEN THE system SHALL ...`
- **Optional:** `WHERE <feature is enabled>, THE system SHALL ...`

Every requirement has an ID. The ID is stable. Tests reference the ID. Commits should reference the ID. Do not renumber.

Features are grouped by phase (P1–P8). A feature is in the phase where it ships, not where it's planned.

---

## P1 — Viewer foundation

### P1-VIEW-001 — Open files
WHEN the user opens a PDF file via menu, drag-drop, or command-line argument, THE system SHALL load the file and render the first page within 1 second for files up to 50MB.

### P1-VIEW-002 — Unsupported files
IF the file is not a valid PDF, THEN THE system SHALL display the message "This file does not appear to be a valid PDF" and SHALL NOT crash, freeze, or corrupt application state.

### P1-VIEW-003 — Encrypted PDFs
WHEN the user opens a password-protected PDF, THE system SHALL prompt for the password, retry up to 3 times, and SHALL never store the password to disk.

### P1-VIEW-004 — Render fidelity
THE system SHALL render every page at the same pixel-fidelity as Adobe Acrobat Reader for the W3C PDF conformance test suite. Failures must be documented in `tests/render-failures.md`.

### P1-VIEW-005 — Navigation
WHEN the user scrolls, presses Page Up/Down, presses arrow keys, or clicks a thumbnail, THE system SHALL navigate to the corresponding page within 100ms for files up to 500 pages.

### P1-VIEW-006 — Zoom
THE system SHALL support zoom levels from 25% to 1600% with the standard fit modes: Actual Size, Fit Page, Fit Width, Fit Height. Zoom level SHALL persist per-document across sessions.

### P1-VIEW-007 — Search
WHEN the user invokes search (Cmd/Ctrl+F), THE system SHALL find all matches across all pages, highlight them, and allow next/previous navigation. Search SHALL support case-sensitive and whole-word options. Search SHALL handle PDFs of up to 5000 pages in under 5 seconds.

### P1-VIEW-008 — Thumbnails
THE system SHALL display a thumbnails sidebar (collapsible) for any open document. Thumbnails SHALL be generated lazily as pages enter the viewport.

### P1-VIEW-009 — Outline
WHERE the PDF contains a document outline, THE system SHALL display it as a collapsible tree in a sidebar. Clicking an entry SHALL navigate to the target page.

### P1-VIEW-010 — Dark mode
THE system SHALL support light, dark, and system-default themes. In dark mode, PDF pages SHALL render with an inverted color scheme that preserves images and figures.

### P1-VIEW-011 — Multi-document
THE system SHALL support multiple PDFs open simultaneously as tabs within a single window.

### P1-VIEW-012 — Recents
THE system SHALL remember the last 20 opened files and surface them on the start screen. The list SHALL be clearable by the user.

---

## P2 — Page operations

### P2-PAGE-001 — Rotate
WHEN the user rotates a page (90°, 180°, 270°), THE system SHALL persist the rotation in the saved PDF, not as a viewer-only transform.

### P2-PAGE-002 — Reorder
WHEN the user drags a thumbnail to a new position, THE system SHALL renumber pages and update all internal references (links, bookmarks, named destinations).

### P2-PAGE-003 — Delete
WHEN the user deletes one or more pages, THE system SHALL update the page count, renumber pages, and update internal references. Deletion SHALL be undoable.

### P2-PAGE-004 — Insert blank
WHEN the user inserts a blank page, THE system SHALL inherit the page size and orientation from the adjacent page unless overridden.

### P2-PAGE-005 — Insert from PDF
WHEN the user inserts pages from another PDF, THE system SHALL preserve their content, annotations, form fields, and page dimensions.

### P2-PAGE-006 — Extract
WHEN the user extracts pages, THE system SHALL produce a new PDF containing exactly those pages, with all required resources (fonts, images, color spaces) copied or referenced correctly.

### P2-PAGE-007 — Split
WHEN the user splits a PDF, THE system SHALL support: (a) every N pages, (b) at specific page numbers, (c) by file size target, (d) by top-level bookmarks.

### P2-PAGE-008 — Merge
WHEN the user merges multiple PDFs, THE system SHALL preserve all annotations, form fields, and bookmarks. Form field names SHALL be made unique on collision (suffix `_2`, `_3`, etc.).

### P2-PAGE-009 — Crop
WHEN the user crops a page, THE system SHALL adjust the CropBox without altering the underlying content. The crop SHALL be reversible by setting the CropBox back to the MediaBox.

### P2-PAGE-010 — Resize
WHEN the user resizes a page to a standard size (Letter, A4, Legal, A3, etc.) or a custom size, THE system SHALL scale content to fit and offer a "preserve aspect ratio" option.

---

## P3 — Annotations

### P3-ANN-001 — Highlight, underline, strikethrough
WHEN the user selects text and applies highlight/underline/strikethrough, THE system SHALL store the annotation as a standard PDF text-markup annotation that is visible in Adobe Acrobat and other major PDF readers.

### P3-ANN-002 — Sticky notes
WHEN the user places a sticky note, THE system SHALL store it as a PDF Text annotation with author, timestamp, and free-text body. Notes SHALL be re-openable, editable, and deletable.

### P3-ANN-003 — Free text
WHEN the user adds a free-text annotation, THE system SHALL support font family, size, color, bold/italic/underline, and rich text formatting per the PDF specification's rich text annotation appearance.

### P3-ANN-004 — Drawing — shapes
THE system SHALL support adding rectangles, ellipses, lines, arrows, and polygons as PDF shape annotations with configurable stroke color, fill color, opacity, and stroke width.

### P3-ANN-005 — Drawing — freehand
WHEN the user draws freehand (pen tool), THE system SHALL store the path as a PDF Ink annotation with smoothing applied. Pressure sensitivity SHALL be supported where available from the input device.

### P3-ANN-006 — Stamps
THE system SHALL provide a library of built-in stamps (Approved, Confidential, Draft, etc.) and SHALL allow the user to create custom stamps from an image, text, or combination.

### P3-ANN-007 — Measurement tools
THE system SHALL provide distance, perimeter, and area measurement tools that calibrate against a user-specified scale.

### P3-ANN-008 — Annotation list
THE system SHALL display all annotations in a sidebar list, grouped by page, with search and filter by type, author, and date.

### P3-ANN-009 — Reply threads
WHEN the user replies to an annotation, THE system SHALL store the reply as a linked annotation with `IRT` (in reply to) reference, per the PDF specification.

### P3-ANN-010 — Import/export
THE system SHALL support importing and exporting annotations as XFDF and FDF files, compatible with Adobe Acrobat.

### P3-ANN-011 — Flatten
WHEN the user chooses "flatten annotations," THE system SHALL render all annotations into the page content streams permanently. The result SHALL not be undoable from a saved file (only from session history).

---

## P4 — Content editing

### P4-EDIT-001 — Edit existing text
WHEN the user clicks on a text run in edit mode, THE system SHALL show an editable text box bounded by the text run's bounding box. Edits SHALL preserve font, size, color, and style where the font is embedded or available locally.

### P4-EDIT-002 — Font fallback
IF the original font is not embedded and not installed on the system, THEN THE system SHALL substitute the closest match from a fallback stack (Helvetica → Arial → sans-serif), warn the user once per document, and offer to re-flow the affected text run.

### P4-EDIT-003 — Add text
WHEN the user adds a new text box, THE system SHALL allow font, size, color, and style selection. New text SHALL be added as part of the page content stream, not as an annotation.

### P4-EDIT-004 — Delete text
WHEN the user deletes a text run, THE system SHALL remove it from the content stream and reflow surrounding content where possible.

### P4-EDIT-005 — Images
WHEN the user adds an image (PNG, JPG, GIF, BMP, TIFF, WebP), THE system SHALL embed it in the PDF with appropriate filters (JPEG → DCTDecode, PNG → FlateDecode), resize and position it interactively, and allow rotation.

### P4-EDIT-006 — Image edit
WHEN the user clicks an existing image, THE system SHALL allow move, resize, rotate, replace, and delete. The original image data SHALL be preserved unless the user explicitly replaces it.

### P4-EDIT-007 — Hyperlinks
WHEN the user adds a hyperlink to selected text or a region, THE system SHALL store it as a PDF Link annotation. Hyperlinks SHALL support: external URL, internal page navigation, named destination, and email (mailto:).

### P4-EDIT-008 — Background
WHEN the user adds a background (color, image, or PDF page), THE system SHALL render it behind all existing content on selected pages.

### P4-EDIT-009 — Watermark
WHEN the user adds a watermark, THE system SHALL support: text watermark (with font, size, color, opacity, rotation) and image watermark, applied to selected pages, on top of or behind content.

### P4-EDIT-010 — Header/footer
WHEN the user adds headers or footers, THE system SHALL support: left/center/right alignment, page number placeholder (`{n}`), date placeholder (`{date}`), total pages (`{total}`), and font/style configuration.

### P4-EDIT-011 — Page numbers
WHEN the user adds page numbers, THE system SHALL support: position (header/footer × L/C/R), format (1, 1/N, Page 1 of N, i/I/a/A), starting number, and exclusion ranges.

### P4-EDIT-012 — Bates numbering
WHEN the user applies Bates numbering, THE system SHALL apply sequential numbering with configurable prefix, suffix, padding, and starting number across one or more PDFs.

---

## P5 — Forms

### P5-FORM-001 — Detect existing forms
WHEN the user opens a PDF containing AcroForm fields, THE system SHALL detect them and display a "Form mode" entry point with field count.

### P5-FORM-002 — Fill text fields
WHEN the user clicks an AcroForm text field, THE system SHALL allow typing, support tab navigation, and respect maximum-length constraints declared by the field.

### P5-FORM-003 — Fill checkbox / radio
WHEN the user clicks a checkbox or radio button, THE system SHALL toggle/select the field per its declared appearance states.

### P5-FORM-004 — Fill choice fields
WHEN the user interacts with a combo box or list box, THE system SHALL display options from the field's options array and allow selection per the field's single-select/multi-select flag.

### P5-FORM-005 — XFA forms (degraded support)
WHERE a PDF contains XFA forms only (no AcroForm fallback), THE system SHALL display a warning that XFA editing is not supported and offer to convert XFA to flat content (read-only).

### P5-FORM-006 — Create text fields
WHEN the user adds a text field in form-edit mode, THE system SHALL allow configuration of name, default value, max length, multi-line, and required flag.

### P5-FORM-007 — Create other fields
THE system SHALL support creating checkbox, radio button (grouped), combo box, list box, signature, and push-button fields.

### P5-FORM-008 — Export data
WHEN the user exports form data, THE system SHALL support FDF, XFDF, JSON, and CSV formats. Export SHALL include field name, value, and type.

### P5-FORM-009 — Import data
WHEN the user imports form data, THE system SHALL fill matching fields by name. Unmatched fields SHALL be reported. Type mismatches SHALL be reported, not silently coerced.

### P5-FORM-010 — Flatten
WHEN the user flattens a form, THE system SHALL render each field's current appearance into the page content and remove the interactive field definitions.

---

## P6 — Signing & security

### P6-SEC-001 — Draw signature
WHEN the user creates a signature by drawing, THE system SHALL capture stroke data with smoothing and SHALL allow saving the signature to the local signature library.

### P6-SEC-002 — Type signature
WHEN the user creates a signature by typing, THE system SHALL render the typed text in one of several handwriting-style fonts. The user SHALL be able to choose the font.

### P6-SEC-003 — Image signature
WHEN the user creates a signature from an image, THE system SHALL accept PNG (with transparency), JPG, BMP and SHALL allow background removal via a simple threshold.

### P6-SEC-004 — Place signature
WHEN the user places a signature, THE system SHALL embed it as a stamp annotation or, when a signature field is targeted, as a PKCS#7 digital signature using the user's certificate.

### P6-SEC-005 — Certificate-based signing
WHEN the user signs with a certificate (PKCS#12 / .pfx), THE system SHALL create a PAdES-compliant signature, embed the certificate chain, and lock the signed content per the signature's permission level.

### P6-SEC-006 — Verify signatures
WHEN the user opens a signed PDF, THE system SHALL verify all signatures and display per-signature status: cryptographically valid, chain-trusted, document-modified-after-signing, expired.

### P6-SEC-007 — Password protect
WHEN the user adds password protection, THE system SHALL support both user password (open) and owner password (permissions) with 256-bit AES encryption.

### P6-SEC-008 — Remove password
WHEN the user removes password protection, THE system SHALL require the owner password and SHALL re-save the PDF without encryption.

### P6-SEC-009 — Set permissions
WHEN the user sets permissions, THE system SHALL allow restricting: print, copy, modify, fill forms, annotate, extract, assemble.

### P6-SEC-010 — True redaction
WHEN the user redacts a region, THE system SHALL: (a) remove the content (text, images) within the region from the content stream, not merely overlay a black box; (b) optionally remove or rewrite metadata; (c) verify by extracting text from the saved file that the redacted text is gone.

### P6-SEC-011 — Redaction patterns
THE system SHALL provide pattern-based redaction (regex and built-in patterns: SSN, credit-card, email, phone) that finds matches and asks the user to confirm before applying.

### P6-SEC-012 — Metadata cleaning
WHEN the user invokes "Clean document," THE system SHALL remove: metadata (author, creator, producer, custom keys), hidden text, comments, attachments, bookmarks, form data, embedded files — each toggle-able.

---

## P7 — OCR & conversion

### P7-OCR-001 — OCR a scanned PDF
WHEN the user runs OCR on a PDF containing scanned images, THE system SHALL produce a searchable PDF with an invisible text layer aligned to the visual text. Tesseract SHALL be used.

### P7-OCR-002 — Languages
THE system SHALL ship with OCR support for English, Spanish, French, German, Chinese (Simplified+Traditional), Japanese, Korean, Arabic, Hindi, Portuguese, Russian. Additional language packs SHALL be downloadable on demand and stored locally.

### P7-OCR-003 — OCR quality
THE system SHALL preprocess images before OCR: deskew, denoise, upscale to ≥300 DPI if lower. The preprocessing SHALL be configurable.

### P7-OCR-004 — Convert PDF to Word
WHEN the user exports a PDF to DOCX, THE system SHALL preserve text, basic formatting (bold, italic, headings), images, and table structures where detectable.

### P7-OCR-005 — Convert PDF to image
WHEN the user exports a PDF to image, THE system SHALL support PNG, JPG, TIFF, WebP at user-chosen DPI (72–600).

### P7-OCR-006 — Convert PDF to text
WHEN the user exports a PDF to plain text, THE system SHALL preserve reading order and emit Unicode UTF-8.

### P7-OCR-007 — Convert PDF to HTML
WHEN the user exports a PDF to HTML, THE system SHALL produce semantic HTML with images extracted to a sibling folder.

### P7-OCR-008 — Convert PDF to Excel
WHEN the user exports a PDF to XLSX, THE system SHALL extract detected tables to sheets and SHALL warn if no tables are detected.

### P7-OCR-009 — Create PDF from files
WHEN the user creates a PDF from files (DOCX, images, TXT, HTML), THE system SHALL preserve content, embed fonts, and produce a PDF/A-compatible output when the user requests it.

### P7-OCR-010 — Compress
WHEN the user compresses a PDF, THE system SHALL offer three levels (low / medium / high) targeting image recompression, font subsetting, and stream deflation. The system SHALL show before/after size.

### P7-OCR-011 — Linearize
WHEN the user invokes "optimize for web," THE system SHALL linearize the PDF (web-optimized / fast-web-view).

---

## P8 — AI & batch

> See `07_AI_FEATURES.md` for the full AI architecture. This phase is gated on the rest of the product being solid.

### P8-AI-001 — Local LLM backend
THE system SHALL detect a local Ollama installation and offer to use installed models for AI features. WHERE Ollama is not present, THE system SHALL offer a one-click install of a recommended model.

### P8-AI-002 — Summarize
WHEN the user invokes "Summarize," THE system SHALL produce a configurable-length summary (3-sentence / 1-paragraph / bullet list) of the current document.

### P8-AI-003 — Q&A
WHEN the user asks a question via the chat pane, THE system SHALL retrieve relevant passages from the open document via local embedding search and answer using the local LLM with citations to page numbers.

### P8-AI-004 — Smart redact
WHEN the user invokes "Smart redact," THE system SHALL run a local NER model to identify PII candidates (names, addresses, SSNs, credit cards, phone numbers, emails, dates of birth, account numbers) and present them as a confirm-before-apply list.

### P8-AI-005 — Extract structured data
WHEN the user invokes "Extract data," THE system SHALL detect tables, forms, and key/value pairs and offer to export as CSV, JSON, or XLSX.

### P8-AI-006 — Translate
WHEN the user invokes "Translate," THE system SHALL translate the document text to a chosen target language using a local model. The output SHALL be a new PDF, side-by-side or replacement.

### P8-AI-007 — Cloud opt-in
WHERE the user has explicitly enabled cloud AI and provided an API key, THE system SHALL offer cloud models as an alternative backend per-feature. The current backend SHALL be visible in the UI at all times.

### P8-BATCH-001 — Batch operations
WHEN the user opens batch mode, THE system SHALL allow applying any of: compress, OCR, watermark, redact, convert, password-protect, flatten — to a list of files. Progress SHALL be visible and cancellable.

### P8-BATCH-002 — Watch folder
WHERE the user configures a watch folder, THE system SHALL apply a chosen pipeline to every PDF added to the folder. The pipeline SHALL be runnable from the command line for scripting.

---

## Cross-cutting non-functional requirements

### NFR-PERF-001 — Cold start
THE system SHALL launch (cold) in under 2 seconds on a 2020-era laptop (8GB RAM, SSD).

### NFR-PERF-002 — Memory
THE system SHALL use less than 300MB of RAM with no document open, and less than 1GB with a 100-page document open at typical zoom.

### NFR-PERF-003 — Large files
THE system SHALL open a 500MB PDF without exhausting memory. Page rendering SHALL remain interactive (≥30fps scroll).

### NFR-PERF-004 — Save
THE system SHALL save edits to a 50MB PDF in under 3 seconds.

### NFR-A11Y-001 — Keyboard
EVERY function SHALL be reachable via keyboard. Tab order SHALL be logical and visible.

### NFR-A11Y-002 — Screen readers
THE system SHALL announce tool changes, modal dialogs, and document changes via the platform accessibility API.

### NFR-A11Y-003 — High contrast
THE system SHALL support a high-contrast theme and SHALL meet WCAG 2.2 AA contrast minimums in all built-in themes.

### NFR-PRIVACY-001 — No telemetry
THE system SHALL NOT send any telemetry, crash reports, or usage data without per-launch explicit user opt-in.

### NFR-PRIVACY-002 — No network on file ops
THE system SHALL NOT make any network request as part of opening, editing, or saving a PDF.

### NFR-PRIVACY-003 — Update checks
WHERE update checking is enabled, THE system SHALL only fetch a version manifest, never the document content.
