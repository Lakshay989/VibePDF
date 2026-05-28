# Fixture provenance

Per `docs/06_CONVENTIONS.md`: every PDF in `tests/fixtures/` must have
documented provenance. No mystery PDFs.

| File | Origin | Why it's here |
|---|---|---|
| `basic/hello.pdf` | Hand-constructed in `tests/fixtures/basic/generate-hello.py` (run once, committed). Minimal valid PDF 1.4, US Letter, single page rendering "Hello, VibePDF." in built-in Helvetica. ~400 bytes. | Smoke-test fixture: proves PDFium and PDF.js can each open *something*. No fonts to embed, no encryption, no annotations. |

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
