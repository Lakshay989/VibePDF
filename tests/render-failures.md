# Render-failure log

> SPEC: P1-VIEW-004 — render fidelity.
>
> Phase-1 interpretation: each fixture's render must match its committed
> golden, a regression baseline produced by our own PDFium pipeline —
> **not** an a mainstream PDF reader reference. The real a mainstream reader / W3C conformance
> comparison is future work; this scaffold is the machinery it plugs into.
>
> Regenerate goldens after an intentional renderer change:
>
>     cargo test --manifest-path src-tauri/Cargo.toml -- --ignored bless_goldens
>
> Tolerance: per-channel |Δ| ≤ 16, at most 2% of pixels may differ.
> This file is rewritten by `src-tauri/tests/render_compare.rs` on every run.

## Checked fixtures

| Fixture | Page | DPI | Status |
|---|---|---|---|
| basic/hello.pdf | 0 | 72 | ✅ match |

## Divergences

_None._
