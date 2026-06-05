# Fixture provenance

Per `docs/06_CONVENTIONS.md`: every PDF in `tests/fixtures/` must have
documented provenance. No mystery PDFs.

| File | Origin | Why it's here |
|---|---|---|
| `basic/hello.pdf` | Hand-constructed in `tests/fixtures/basic/generate-hello.py` (run once, committed). Minimal valid PDF 1.4, US Letter, single page rendering "Hello, VibePDF." in built-in Helvetica. ~400 bytes. | Smoke-test fixture: proves PDFium and PDF.js can each open *something*. No fonts to embed, no encryption, no annotations. |
| `golden/hello-p0-72dpi.png` | **Self-generated** render of `basic/hello.pdf` page 0 at 72 DPI (612×792, 8-bit RGBA), produced by *our own* PDFium pipeline via the `bless_goldens` test in `src-tauri/tests/render_compare.rs`. **Not** an Adobe Acrobat reference. | Regression baseline for the P1.E2 render-fidelity scaffold (P1-VIEW-004). The gate test `renders_match_goldens` compares fresh renders against it within tolerance and logs divergences to `tests/render-failures.md`. Regenerate after an intentional renderer change with `cargo test ... -- --ignored bless_goldens`. |
| `basic/links.pdf` | Hand-constructed in `tests/fixtures/basic/generate-links.py` (run once, committed). Minimal valid PDF 1.4, **3** US-Letter pages; page 1 has a `/Link` annotation whose `/Dest` references page 3's *object* (`5 0 R`). ~1.3 KB. | P2-PAGE-003 reference-integrity fixture. `src-tauri/tests/delete_page.rs` deletes page 2 and asserts the page-1 link still targets page 3 (now index 1) — proving surviving object-ref destinations track renumbering. |

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
