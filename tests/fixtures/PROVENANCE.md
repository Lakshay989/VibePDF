# Fixture provenance

Per `docs/06_CONVENTIONS.md`: every PDF in `tests/fixtures/` must have
documented provenance. No mystery PDFs.

| File | Origin | Why it's here |
|---|---|---|
| `basic/hello.pdf` | Hand-constructed in `tests/fixtures/basic/generate-hello.py` (run once, committed). Minimal valid PDF 1.4, US Letter, single page rendering "Hello, VibePDF." in built-in Helvetica. ~400 bytes. | Smoke-test fixture: proves PDFium and PDF.js can each open *something*. No fonts to embed, no encryption, no annotations. |

Acceptance fixtures (`acceptance/p1-spec.pdf`, `p1-encrypted.pdf`,
`p1-large.pdf`) referenced in `docs/05_ROADMAP.md` are not yet checked
in — see Q4 in the bootstrap plan.
