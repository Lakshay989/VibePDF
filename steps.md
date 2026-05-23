# VibePDF — Step index

Each phase of `docs/05_ROADMAP.md` is broken into decoupled, independently
shippable steps. The per-phase plans live under `steps/`.

| Phase | File | Steps | Status |
|---|---|---|---|
| Phase 1 — Open & view | [steps/P1.md](steps/P1.md) | 18 (A1..E5) | bootstrap shipped (`c7a54f5`); rest open |
| Phase 2 — Page operations | [steps/P2.md](steps/P2.md) | 13 (A1..D1) | not started |
| Phase 3 — Annotations | [steps/P3.md](steps/P3.md) | 13 (A1..E2) | not started |
| Phase 4 — Content editing | [steps/P4.md](steps/P4.md) | 14 (A1..D5) | not started |
| Phase 5 — Forms | [steps/P5.md](steps/P5.md) | 10 (A1..C2) | not started |
| Phase 6 — Signing & security | [steps/P6.md](steps/P6.md) | 13 (A1..D3) | not started |
| Phase 7 — OCR & conversion | [steps/P7.md](steps/P7.md) | 10 (A1..C2) | not started |
| Phase 8 — AI & batch | [steps/P8.md](steps/P8.md) | 10 (A1..C2) | not started |

**Total:** 101 steps. Every `P<n>-...` spec line in
`docs/02_PRODUCT_SPEC.md` is referenced in exactly one step; the
remainder are cross-cutting infrastructure (save, autosave, undo,
actor, annotation framework, text engine, signature library, OCR
pipeline, ONNX runtime).

---

## Step ID convention

Every step has a globally unique ID: **`P<phase>.<track><num>`**.

- `P2.A3` = Phase 2, Track A, step 3.
- Tracks are per-phase groupings (e.g. P2 uses A = Save infra, B = Single-page ops, C = Multi-page ops, D = From-other-PDF).
- Step IDs are stable. Once published they don't get renumbered. If a step is removed, its ID is left blank, never reused.

## Commit convention

Every commit that ships a step must reference the step ID **and** the spec ID:

```
feat: rotate page(s) with persistence (P2.B1 / P2-PAGE-001)
```

For infrastructure steps that don't map to a `P<phase>-...` spec ID, reference the step ID alone:

```
feat: undo/redo stack with page-level granularity (P2.A3)
```

This makes back-tracking trivial:
- **commit → step:** `git log --grep="P2.B1"`
- **commit → spec:** `git log --grep="P2-PAGE-001"`
- **step → commits:** look at the `[x] <sha>` annotation in the step doc
- **spec → step:** every spec line is referenced in exactly one step file

## Workflow rule

After every step's acceptance criteria are met:
1. Update the step in its phase doc from `[ ]` to `[x] <sha>` in the same commit.
2. Push to `origin/main` immediately. No batched commits.
3. If verification couldn't run locally (toolchain missing, etc.), say so in the commit body — don't claim a green check we didn't actually get.

Doc-only edits to the step files themselves (rewording, fixing a typo, adding context) **can** be batched. The "one commit per step" rule applies to feature steps, not doc maintenance.

## Phase gating

`docs/05_ROADMAP.md` is sequential by design. Do not start Phase N+1 steps while Phase N still has open `[ ]` items, except for phase-spanning infrastructure that's explicitly called out.

## How to pick the next step

1. Open the current phase's doc (`steps/P<N>.md`).
2. Find any step whose `Depends on:` line is empty or all-satisfied.
3. If you have a choice, prefer the lowest track letter, then the lowest step number — that's the suggested critical path.
