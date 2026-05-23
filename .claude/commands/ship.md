---
description: Implement the most recently approved plan, with verification at each step.
---

You are implementing the feature: **$ARGUMENTS**

This assumes a plan was already produced by `/plan` and approved by the human. If no approved plan exists in this session, **stop** and tell the human to run `/plan` first.

## Implementation discipline

Work in small, verifiable increments. After each increment, the code should compile and the existing tests should still pass. Do not write the whole feature and then fix compile errors at the end.

Suggested increment order:
1. Types first (Rust structs/enums, TS types). Get the shapes right before the logic.
2. The IPC command signature(s) with `todo!()` bodies. Confirm the boundary compiles.
3. The Rust implementation behind the command. Behind the document actor if it touches a PDF.
4. The frontend IPC wrapper in `src/ipc/`.
5. The UI / tool logic.
6. The tests.

## Rules you must follow (from CLAUDE.md and docs/06_CONVENTIONS.md)

- All PDF writes go through the document actor. The frontend never writes bytes.
- TypeScript strict, no `any`. Rust: no `unwrap()` outside tests, `clippy::pedantic` clean.
- New IPC commands need a typed frontend wrapper. No raw `invoke()` in component code.
- Annotate functions that implement a spec line with `// SPEC: <id>`.
- Do not touch `src-tauri/src/security/` without explicit per-change human approval.
- Do not add a network call. If the feature seems to need one, stop and ask.
- Do not add a dependency without stating the justification.

## After implementation

1. Run `npm run check` (typecheck + lint + cargo clippy). Fix everything it flags.
2. Run the tests you added plus any tests in touched modules.
3. **For any PDF write path:** generate an output PDF at `/tmp/vibepdf-verify.pdf`, then tell the human:
   > "I've written a verification file to /tmp/vibepdf-verify.pdf. Please open it in Adobe Acrobat, macOS Preview, and a third reader (Okular/Sumatra) and confirm it renders correctly. A passing test does not prove the PDF is valid to other viewers."
4. Produce a summary: what changed, which spec IDs, what was tested, what was NOT tested, and a draft commit message following Conventional Commits.

## What you must NOT do

- Do not mark the feature complete. The human marks it complete after the verification ritual.
- Do not delete or weaken existing tests to make new code pass.
- Do not expand scope beyond the approved plan. If you discover the plan was wrong, stop and report it — don't silently improvise a different design.
- Do not start the next feature.
