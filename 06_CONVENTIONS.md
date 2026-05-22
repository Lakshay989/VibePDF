# 06 — Conventions

The rules that keep the codebase coherent across hundreds of commits. Most of these are enforced by hooks. Where they're not, Claude is expected to follow them anyway.

---

## Naming

| Thing | Style | Example |
|---|---|---|
| TS variables, functions | `camelCase` | `currentZoom`, `applyRedaction` |
| TS types, classes | `PascalCase` | `DocumentId`, `RedactionResult` |
| TS constants | `SCREAMING_SNAKE` | `MAX_ZOOM_LEVEL` |
| Rust variables, functions | `snake_case` | `current_zoom`, `apply_redaction` |
| Rust types, traits | `PascalCase` | `DocumentId`, `RedactionResult` |
| Rust constants, statics | `SCREAMING_SNAKE` | `MAX_ZOOM_LEVEL` |
| Files (TS) | `kebab-case` | `redact-tool.tsx`, `use-document.ts` |
| Files (Rust) | `snake_case` | `redact_tool.rs`, `document.rs` |
| React components | `PascalCase` filename matches export | `ThumbnailPanel.tsx` exports `ThumbnailPanel` |
| Tauri commands | `<domain>_<verb>_<noun>` | `pdf_open`, `page_rotate`, `annotation_add` |
| Spec IDs | `P<phase>-<DOMAIN>-<num>` | `P3-ANN-001` |

---

## TypeScript

- `strict: true`. Always. No exceptions.
- No `any`. Use `unknown` and narrow.
- No `@ts-ignore`. If absolutely necessary, use `@ts-expect-error` with a comment explaining why and a TODO.
- Imports use the `@/` alias for `src/`. No deep relative imports (`../../..`).
- Prefer `type` for unions and shape aliases, `interface` for object shapes that might be extended.
- React components: function components only, no class components.
- Hooks: name `use<Thing>`. One hook per file in `src/hooks/`, unless tightly coupled to a component.
- Avoid default exports for components (named exports help refactoring); default exports OK for one-export modules.
- Async functions return `Promise<T>`, not `Promise<T | Error>`. Errors throw.
- IPC errors are caught at the boundary and translated to user feedback via the global error handler.

---

## Rust

- `#![deny(warnings)]` in CI builds.
- `clippy::pedantic` is on. Allow specific lints inline with justification.
- Use `?` for error propagation. Use `anyhow::Result` at function signatures crossing module boundaries; use typed errors (`thiserror`) inside a module.
- Avoid `unwrap()` and `expect()` outside tests and `main.rs`. Every `unwrap()` is a latent crash.
- Avoid `Mutex` in hot paths. Use the document actor pattern (`mpsc`) for serializing access.
- Public APIs use `&str` and `&[T]` over `String` and `Vec<T>`.
- Document all public functions with `///` doc comments including at least one usage example for non-trivial APIs.
- Tests live in `tests/` for integration, in-file `#[cfg(test)] mod tests` for unit.

---

## Errors

Two principles:

1. **Errors are values, not strings.** Every Rust function returns typed errors via `thiserror`. Every IPC command returns `Result<T, CommandError>` (a typed enum). Strings are for humans; types are for code.

2. **Error context flows up, never sideways.** Use `anyhow::Context::context` at boundaries to add information, e.g.:
   ```rust
   redact_region(page, rect)
       .context(format!("redacting region on page {} of {}", page, doc_id))?;
   ```

The frontend error model mirrors this. We pattern-match on the typed `CommandError` variant and produce a localized user-facing message. We never show a raw error message to the user.

---

## Comments

- Comments explain **why**, not what. If the code needs a comment to be understandable, it should be refactored.
- A `// SAFETY:` comment is required above every `unsafe { ... }` block.
- A `// PERFORMANCE:` comment is required when code is intentionally written non-idiomatically for performance.
- A `// SPEC: P3-ANN-001` comment is encouraged at the top of functions that implement a specific spec line. This makes the spec ↔ code mapping searchable.
- TODO comments must include an owner and an issue link: `// TODO(github#42): unify with the freehand tool`.

---

## Tests

- Every spec line in `docs/02_PRODUCT_SPEC.md` must have at least one corresponding test.
- Test names mirror spec IDs where possible: `fn test_p3_ann_001_highlight_roundtrip()`.
- Test PDFs in `tests/fixtures/` must have provenance in `tests/fixtures/PROVENANCE.md`. No mystery PDFs.
- Golden PDFs (used for byte-for-byte regression) are committed under `tests/fixtures/golden/`. They are regenerated only via a documented script.
- Visual regression: page is rendered to PNG, compared via `pixelmatch` against a golden PNG. Threshold 0.5%.

---

## Commits

- Conventional Commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, `perf:`, `build:`, `ci:`.
- Reference spec IDs in the body where applicable.
- One concern per commit. If a commit message needs "and," split the commit.
- Branches: `feat/p3-ann-stamp-tool`, `fix/p1-view-large-file-crash`.

Example commit:

```
feat: add freehand ink annotation tool (P3-ANN-005)

Implements the pen tool with Catmull-Rom smoothing.
Pressure is captured from PointerEvent.pressure where available
and ignored on devices that don't report it.

Tests: tests/integration/annotations.rs::test_p3_ann_005_*
```

---

## Performance budgets

Numbers come from `02_PRODUCT_SPEC.md` NFRs. They're hard limits enforced in CI via the `npm run bench` job.

| Operation | Budget |
|---|---|
| Cold start (no doc) | ≤ 2.0 s on 2020 MBA |
| Idle memory (no doc) | ≤ 300 MB RSS |
| Open 50MB PDF | First page visible ≤ 1.0 s |
| Open 500MB PDF | Open without OOM, scroll ≥ 30 fps |
| Save 50MB PDF | ≤ 3.0 s |
| Search 5000-page PDF | First match ≤ 5.0 s |
| Page render (any page in cached doc) | ≤ 50 ms at 100% zoom |
| Thumbnail generation (cached) | ≤ 5 ms |

A PR that regresses any of these by > 10% requires explicit human approval and a written justification in the PR description.

---

## Accessibility

- Every interactive element has a visible focus indicator at 3:1 contrast minimum.
- Every icon-only button has an `aria-label`.
- Tab order matches visual order. We do NOT use `tabindex > 0`.
- We test against NVDA (Windows), VoiceOver (macOS), Orca (Linux) at major release boundaries.
- Color contrast meets WCAG 2.2 AA in all themes, AAA where reasonable.
- The app is operable without a mouse. There is no feature that requires pointer input.

---

## Internationalization

- All user-facing strings live in `src/i18n/<locale>/*.json`.
- New strings go to `src/i18n/en/<feature>.json` first; translation comes later.
- The `t()` function is the only way to produce a user-visible string. Hardcoded strings in JSX are caught by ESLint.

---

## Logging

- `tracing` in Rust, `console` in TS dev (stripped in prod).
- Log at the right level: `error` for unrecoverable, `warn` for degraded, `info` for milestones, `debug` for development context, `trace` for hot loops.
- Never log file contents. Never log user input that might contain PII.
- Log file paths as basenames in shared logs; full paths only at `trace`.

---

## Dependencies

- Every new dependency in a PR needs a one-line justification in the commit message.
- Every dependency is checked against the license allowlist (MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, MPL-2.0, Unlicense).
- GPL and AGPL are blocked at CI via `cargo-deny` and `license-checker`.
- Indirect (transitive) GPL/AGPL is blocked too.

---

## What "review" means here

When you run `/review`, Claude self-checks:

1. Typecheck and lint clean
2. Cargo check clean, clippy clean
3. Touched test files pass
4. Touched IPC commands have typed wrappers on the frontend
5. Touched spec IDs are referenced in commit messages
6. Touched performance-sensitive code: bench numbers within budget
7. Touched PDF write paths: a regression fixture has been added or updated

Then Claude produces a one-paragraph summary of what changed, what was tested, and what wasn't.
