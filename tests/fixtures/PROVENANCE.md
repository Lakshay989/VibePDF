# Fixture provenance

Per `docs/06_CONVENTIONS.md`: every PDF in `tests/fixtures/` must have
documented provenance. No mystery PDFs.

| File | Origin | Why it's here |
|---|---|---|
| `basic/hello.pdf` | Hand-constructed in `tests/fixtures/basic/generate-hello.py` (run once, committed). Minimal valid PDF 1.4, US Letter, single page rendering "Hello, VibePDF." in built-in Helvetica. ~400 bytes. | Smoke-test fixture: proves PDFium and PDF.js can each open *something*. No fonts to embed, no encryption, no annotations. |
| `golden/hello-p0-72dpi.png` | **Self-generated** render of `basic/hello.pdf` page 0 at 72 DPI (612×792, 8-bit RGBA), produced by *our own* PDFium pipeline via the `bless_goldens` test in `src-tauri/tests/render_compare.rs`. **Not** an Adobe Acrobat reference. | Regression baseline for the P1.E2 render-fidelity scaffold (P1-VIEW-004). The gate test `renders_match_goldens` compares fresh renders against it within tolerance and logs divergences to `tests/render-failures.md`. Regenerate after an intentional renderer change with `cargo test ... -- --ignored bless_goldens`. |
| `basic/links.pdf` | Hand-constructed in `tests/fixtures/basic/generate-links.py` (run once, committed). Minimal valid PDF 1.4, **3** US-Letter pages; page 1 has a `/Link` annotation whose `/Dest` references page 3's *object* (`5 0 R`). ~1.3 KB. | P2-PAGE-003 reference-integrity fixture. `src-tauri/tests/delete_page.rs` deletes page 2 and asserts the page-1 link still targets page 3 (now index 1) — proving surviving object-ref destinations track renumbering. |
| `basic/many-pages.pdf` | Hand-constructed in `tests/fixtures/basic/generate-many.py` (run once, committed). Minimal valid PDF 1.4, **50** US-Letter pages, each rendering "Page N" in built-in Helvetica. ~13.5 KB. | P4-EDIT-009 watermark acceptance ("DRAFT on a 50-page PDF in <2s") in `src-tauri/tests/watermark.rs`; a generic multi-page fixture for Track D's later page-decoration features (header/footer, page numbers, Bates). |
| `basic/rotated.pdf` | Hand-constructed in `tests/fixtures/basic/generate-rotated.py` (run once, committed). PDF 1.4, **4** US-Letter pages with `/Rotate` 0/90/180/270, each rendering "Rotate N". ~1.4 KB. | P4.HF (FABLE_REVIEW 3.1): decoration writers must compensate for page rotation; per-angle matrix tests in the watermark / background / header-footer suites. |
| `basic/cropped.pdf` | Hand-constructed in `tests/fixtures/basic/generate-cropped.py` (run once, committed). PDF 1.4, one page with `/CropBox [100 100 512 692]` inside `/MediaBox [0 0 612 792]`. ~0.6 KB. | P4.HF (FABLE_REVIEW 3.4): decoration *placement* targets the visible CropBox; the background colour fill still covers the MediaBox. |
| `fonts/NotoSansCoptic-Regular.ttf` | **Noto Sans Coptic**, © The Noto Project Authors, redistributed under **SIL OFL 1.1** (see `fonts/OFL-NOTICE.txt`). A small (~28 KB) single-script TrueType font whose glyphs sit entirely outside the WinAnsi range. Vendored as-is. | P4.HF5 (FABLE_REVIEW 3.2 stage-2): a deterministic, offline font to exercise the PDFium font-embedding path — `font_embed.rs` and the non-WinAnsi header/footer branch embed it and assert the Coptic text round-trips through save/reopen. Not shipped in the app. |

Acceptance fixtures (`acceptance/p1-spec.pdf`, `p1-encrypted.pdf`,
`p1-large.pdf`) referenced in `docs/05_ROADMAP.md` are not yet checked
in — see Q4 in the bootstrap plan. They are generated on demand by
`python3 tests/fixtures/acceptance/generate.py` and gitignored.

`p1-encrypted.pdf` (user password `vibepdf`, owner password
`vibepdf-owner`, 256-bit AES via pypdf) is consumed by both the Phase
1 acceptance demo and by `src-tauri/tests/encrypted_open.rs`
(P1.B2 / P1-VIEW-003). The Rust test skips gracefully with a printed
regenerate instruction when the fixture is absent, so `cargo test` on
a fresh clone never silently mis-passes.
