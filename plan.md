---
description: Draft an EARS-syntax spec and implementation plan for a feature, then wait for human approval.
---

You are about to plan the feature: **$ARGUMENTS**

Do these steps in order. Do not skip ahead.

## 1. Locate the feature in the spec

Open `docs/02_PRODUCT_SPEC.md`. Find the spec line(s) that govern this feature.

- If the feature is fully specified, quote the exact spec line(s) with their IDs.
- If the feature is partially specified, quote what exists and note what is missing.
- If the feature is not in the spec at all, write a draft spec line in EARS syntax and ask the human to add it before proceeding.

**Do not** invent requirements. **Do not** silently extend the spec. If something is ambiguous, ask.

## 2. Locate the architecture

Open `docs/04_ARCHITECTURE.md`. Identify:

- Which top-level modules will be touched
- What new files (if any) will be created and where they belong
- What IPC commands are involved or new
- What event types (if any) are emitted

If the feature requires a new top-level module or a new architectural pattern, **stop** and propose the architecture change as a doc edit first.

## 3. Locate prior art

Run `grep` / file search for similar features already implemented. If a similar tool exists, your plan should reference it as the template: *"follows the same pattern as `src/tools/freehand/`."*

## 4. Draft the plan

Produce a plan with exactly these sections:

**Spec references.** Quoted spec lines + IDs from step 1.

**Files to create.** Full paths and a one-line purpose each.

**Files to modify.** Full paths and a description of the change (not the diff).

**IPC additions.** New Tauri commands and their type signatures. New event types if any.

**Tests to add.** List by test name. Reference spec IDs.

**Risks.** Three to five concrete risks specific to this feature. For each, a mitigation.

**Acceptance check.** How a human will verify this works. Include at least one specific PDF from `tests/fixtures/` to demo against. If no suitable fixture exists, propose what to add.

**Out of scope.** What you considered and decided not to include in this change. This matters as much as what's in.

## 5. Stop and wait

Print the plan. Do not write code. Do not modify any files (other than maybe creating a draft spec line in step 1, with the human's permission). Wait for the human to approve, push back, or modify the plan.

Once approved, the human will run `/ship`.
