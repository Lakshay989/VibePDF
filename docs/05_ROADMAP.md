# 05 — Roadmap

VibePDF ships in eight phases. The point of the phases is not to predict the future; it is to **resist the temptation to spread thin**. Each phase has a tight scope and a demonstrable acceptance test. Do not skip ahead.

Estimated effort assumes one human paired with Claude Code, working seriously. Real elapsed time depends on the human.

---

## Phase 1 — Open and view (foundation)

**Goal:** Tauri 2 + React + TS app that opens any PDF and renders it correctly with smooth navigation.

**Scope:**
- Project scaffold (Tauri 2, React, TS strict, Tailwind, Vite)
- File open via menu, drag-drop, and CLI argument
- PDF.js integration on the frontend
- PDFium initialization on the Rust side (no edits yet, just confirms it loads)
- Multi-page rendering with virtual scrolling
- Zoom controls, fit modes
- Thumbnails sidebar
- Outline/bookmarks sidebar
- Text search across document
- Light/dark/system themes
- Multi-document tabs
- Encrypted-PDF password prompt

**Out of scope:** ANY editing. ANY mutation of the PDF. ANY save button. ANY annotations.

**Acceptance demo:**
The user can open `tests/fixtures/acceptance/p1-spec.pdf` (the actual PDF specification, 1000+ pages, complex layout) and:
- Scroll smoothly from page 1 to page 1000 in under 10 seconds with no rendering glitches
- Search for "transparency group" and jump between matches
- Open `tests/fixtures/acceptance/p1-encrypted.pdf`, enter the password, see it render
- Open `tests/fixtures/acceptance/p1-large.pdf` (500MB) without memory exhaustion
- Toggle dark mode and see pages re-render correctly
- Open three PDFs in tabs and switch between them

**Spec lines covered:** P1-VIEW-001 through P1-VIEW-012, NFR-PERF-001, NFR-PERF-002, NFR-PERF-003.

---

## Phase 2 — Page operations

**Goal:** Move, copy, delete, merge, split, extract, rotate pages — saving real PDFs as output.

**Scope:**
- The save button (finally)
- Auto-save / recovery
- Undo/redo stack (page-level granularity)
- Rotate single, range, all
- Delete pages
- Reorder via thumbnail drag
- Insert blank page
- Insert pages from another PDF
- Extract pages to new PDF
- Split (by every N, by ranges, by size, by bookmarks)
- Merge multiple PDFs
- Crop pages
- Resize pages

**Acceptance demo:**
- Open `tests/fixtures/acceptance/p2-multi.pdf`, delete page 3, insert a blank, rotate page 5 by 90°, save. Re-open in a mainstream PDF reader: changes are visible and the file is not corrupt.
- Merge five PDFs from `tests/fixtures/acceptance/p2-merge-*.pdf`. Form fields and bookmarks survive.
- Split a 100-page PDF into 10 files. Each file opens cleanly.

**Spec lines covered:** P2-PAGE-001 through P2-PAGE-010.

---

## Phase 3 — Annotations

**Goal:** Standard PDF annotations that round-trip through a mainstream reader.

**Scope:**
- Highlight, underline, strikethrough, squiggly
- Sticky notes / comments
- Free-text annotations with rich text
- Shapes: rectangle, ellipse, line, arrow, polygon
- Freehand ink with smoothing
- Stamps (built-in library + custom)
- Measurement tools (distance, perimeter, area)
- Annotation sidebar with filtering
- Reply threads
- Import/export XFDF, FDF
- Flatten annotations

**Acceptance demo:**
- Add five annotations of different types to `tests/fixtures/acceptance/p3-doc.pdf`. Save. Open in three independent readers. All five render correctly in all three.
- Open `tests/fixtures/acceptance/p3-annotated.pdf` (annotated in a mainstream reader). All annotations show up in our sidebar with correct authors and timestamps.
- Export to XFDF, delete all annotations, re-import the XFDF: annotations restored identically.

**Spec lines covered:** P3-ANN-001 through P3-ANN-011.

---

## Phase 4 — Content editing

**Goal:** Edit existing text without breaking the document. Add new text, images, and decorations.

**This is the hardest phase.** It's where the product earns the "elite" label or doesn't.

**Scope:**
- Edit existing text runs (preserve font, size, color)
- Font fallback warnings when original font unavailable
- Add new text boxes (full font/style control)
- Delete text
- Add images (PNG, JPG, GIF, BMP, TIFF, WebP)
- Edit existing images (move, resize, rotate, replace)
- Hyperlinks (text and region)
- Backgrounds
- Watermarks (text and image)
- Headers and footers with placeholders
- Page numbers (formats + ranges)
- Bates numbering

**Acceptance demo:**
- Open `tests/fixtures/acceptance/p4-edit-typo.pdf` (a real contract with a typo). Edit the typo. Save. Diff the result against a mainstream reader's edit of the same document — text is in the same position, same font, no visual drift.
- Add a watermark to a 50-page PDF in under 2 seconds.
- Add Bates numbers across three merged PDFs starting at "ABC000001" — confirm sequence is correct on every page.

**Spec lines covered:** P4-EDIT-001 through P4-EDIT-012.

**Risk:** Text editing in PDFs is intrinsically lossy when fonts aren't embedded. The product needs to be honest with the user about this. We must NOT silently substitute fonts and pretend the result is the same.

---

## Phase 5 — Forms

**Goal:** Fill, create, and manage interactive PDF forms.

**Scope:**
- Detect AcroForm fields, surface "form mode"
- Fill text fields, checkboxes, radios, choice fields
- XFA-only forms: graceful degradation (read-only display)
- Create form fields (drag-drop in form-edit mode)
- Field property editor (name, default, tooltip, max length, required, etc.)
- Tab order management
- Export/import form data (FDF, XFDF, JSON, CSV)
- Flatten forms

**Acceptance demo:**
- Fill out `tests/fixtures/acceptance/p5-irs-w9.pdf` (a real IRS W-9 form). Save. Open in a mainstream reader: all fields show the values; the form is still interactive.
- Create a new form from scratch with 8 fields of various types. Export the data after filling. Re-import to a blank copy: fields are filled identically.

**Spec lines covered:** P5-FORM-001 through P5-FORM-010.

---

## Phase 6 — Signing and security

**Goal:** Sign PDFs, encrypt them, redact them — properly.

**Scope:**
- Draw / type / image signature
- Place signature as stamp or fill signature field
- Certificate-based signing (PKCS#12 / .pfx) producing PAdES signatures
- Signature verification on opened files
- Password protect (user + owner passwords, AES-256)
- Permission flags
- True redaction (content removal, not overlay)
- Pattern-based redaction (regex + built-in PII patterns; SSN, credit card, email, phone)
- Metadata cleaning

**Acceptance demo:**
- Sign `tests/fixtures/acceptance/p6-contract.pdf` with a test certificate. Open in a mainstream reader: signature is verified as valid, chain shown.
- Redact an SSN from `tests/fixtures/acceptance/p6-document.pdf`. Use `pdftotext` on the result: the SSN does not appear in the extracted text.
- Encrypt a PDF with both user and owner passwords. Open with only the user password: print is blocked. Open with the owner password: print works.

**Spec lines covered:** P6-SEC-001 through P6-SEC-012.

**Hard rule:** The security module is the one place where Claude does NOT modify code without an explicit human go-ahead per change. Crypto bugs are silent and dangerous.

---

## Phase 7 — OCR and conversion

**Goal:** Scanned PDFs become searchable. PDFs convert to other formats and back.

**Scope:**
- Tesseract integration with deskew/denoise/upscale preprocessing
- Bundle English; on-demand language packs
- Searchable PDF output (invisible text layer)
- Convert PDF → DOCX, XLSX, PPTX, image, text, HTML
- Create PDF from DOCX, images, text, HTML
- Compress (low/medium/high)
- Linearize (web-optimized)

**Acceptance demo:**
- Open `tests/fixtures/acceptance/p7-scanned.pdf` (a 30-page scanned manual). Run OCR. The result is searchable; visual layout is unchanged.
- Convert `tests/fixtures/acceptance/p7-report.pdf` to DOCX. Open in Word: headings are headings, tables are tables, images are present.
- Compress `tests/fixtures/acceptance/p7-large.pdf` (200MB of scans). Result is under 30MB with no visible quality loss at 100% zoom.

**Spec lines covered:** P7-OCR-001 through P7-OCR-011.

---

## Phase 8 — AI and batch

**Goal:** Optional, local-first AI features. Batch processing.

**Scope:**
- Ollama detection and integration
- ONNX runtime for embeddings and NER
- Summarization
- Q&A over open document (RAG with local embeddings)
- Smart redact (PII NER + confirm-list)
- Structured data extraction (tables, key-value)
- Translation
- Optional cloud backend with explicit per-action consent
- Batch operations panel (compress, OCR, watermark, etc.)
- Watch folder + CLI mode

**Acceptance demo:**
- With Ollama running locally, ask "summarize this in 3 sentences" of `tests/fixtures/acceptance/p8-paper.pdf`. Get a coherent summary with page citations.
- Run "smart redact" on `tests/fixtures/acceptance/p8-pii.pdf`. The tool identifies 12 PII entities; the user confirms 10 and rejects 2; redaction applies only to the confirmed.
- Set up a watch folder. Drop a PDF in. It's OCR'd, compressed, and renamed automatically per the configured pipeline.

**Spec lines covered:** P8-AI-001 through P8-AI-007, P8-BATCH-001, P8-BATCH-002.

---

## After phase 8

The product is "done" in the sense that it meets the original vision. Future directions, in no particular order:

- Plugin/extension system (JS plugins via a sandboxed runtime)
- Real-time collaborative annotations (via a shared annotation file format, not a server)
- Mobile (Tauri 2 mobile)
- a mainstream reader-compatible JavaScript form actions
- Document comparison (visual + text diff)
- A11y audit and tagging editor
- Custom branding / theming (for orgs that want to ship a re-skinned VibePDF internally)

These are not promises. They're the natural next moves once the core is solid.
