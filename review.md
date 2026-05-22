---
description: Self-review the current working tree against project conventions and acceptance criteria.
---

Run a structured self-review of the current uncommitted changes (or the changes named in $ARGUMENTS, if given).

Be adversarial. Your job is to find what's wrong, not to confirm it's fine. If you can't find anything wrong, you're not looking hard enough — look at the edge cases.

## 1. Mechanical checks

Run and report results:
- `npm run check` (typecheck, lint, cargo check, clippy)
- The test suite for touched modules
- `cargo deny check licenses` if any Rust dependency changed
- `git diff --stat` to summarize the surface area

If any of these fail, stop and report. Don't continue the review of code that doesn't compile.

## 2. Convention compliance

Check the diff against `docs/06_CONVENTIONS.md`:
- Naming follows the table?
- No `any` in TS, no `unwrap()` in non-test Rust?
- New IPC commands have typed frontend wrappers?
- Functions implementing spec lines annotated with `// SPEC: <id>`?
- New strings go through `t()` (i18n)?
- New dependencies justified and license-compatible?

List every violation with file:line.

## 3. Architecture compliance

Check the diff against `docs/04_ARCHITECTURE.md`:
- Do all PDF writes go through the document actor?
- Does the frontend touch PDF bytes anywhere? (It must not.)
- Are new files in the directory the architecture prescribes?
- Did any new top-level module appear without a doc update?

## 4. Correctness — the hard part

For each changed function, ask:
- What happens with an empty input?
- What happens with a malformed PDF?
- What happens with a 500MB PDF?
- What happens if the operation is interrupted halfway (crash during save)?
- Does the operation round-trip? (Open the result, does it match expectations?)
- For text edits: what if the font isn't embedded?
- For page ops: are internal references (links, bookmarks, named dests) updated?

List the cases the current code does NOT handle. For each, say whether it's acceptable to skip (and why) or a bug to fix.

## 5. Test coverage

- Does every changed spec line have a test?
- Is there a fixture exercising the realistic case AND at least one edge case?
- For PDF writes: is there a round-trip / visual regression test?

## 6. Privacy & security audit

- Any network calls introduced? (There should be none unless the feature explicitly needs them with consent.)
- Any logging of file contents or PII?
- Any change under `src-tauri/src/security/`? (If so, flag for mandatory human review.)

## Output

Produce a review report:
- ✅ Passes / ❌ Fails / ⚠️ Needs attention, per section above
- A prioritized list of issues (blocker / should-fix / nice-to-have)
- A one-line verdict: is this ready to commit, or not?

Do not fix anything during the review. Report only. The human decides what to fix.
