# Prompting Playbook for VibePDF

A short guide to the prompting patterns that actually work for this project. These are not generic Claude Code tips — they're tuned to a graphics-heavy, file-format-heavy, offline-desktop codebase where small mistakes are expensive.

---

## The five-step loop

Every feature, from "add a freehand pen tool" to "implement OCR pipeline," should run through the same loop. Do not skip steps.

1. **Specify.** Open `docs/02_PRODUCT_SPEC.md`, find the feature, confirm it's written in EARS syntax. If it isn't, write it that way before you do anything else. ("WHEN the user clicks the redact tool AND drags a rectangle AND releases the mouse, THE system SHALL apply a true redaction that removes the underlying content stream, not just an overlay.")
2. **Plan.** Run `/plan <feature>` in Claude Code. It enters plan mode, drafts an implementation, and waits. Read the plan critically.
3. **Push back.** This step is the most under-used. If the plan touches three files when you thought it would touch one, ask why. If it adds a dependency, ask whether the existing stack can handle it. Treat the plan as a contract you are negotiating.
4. **Ship.** Run `/ship <feature>`. Hooks run formatters and type-checks on every save.
5. **Verify.** Run `/review`, then `/test-pdf <fixture>`. Look at the *output PDF* with your own eyes in a real viewer (several independent readers). Code that passes tests but produces broken PDFs is the most common failure mode.

---

## The plan-mode tax

It feels slow. It is not slow. Plan mode collapses 20 decisions that would otherwise be made one-by-one and hopefully-correctly into a single reviewed artifact. Anthropic's internal data shows un-planned attempts succeed about 33% of the time on multi-step features; planned attempts approach 100% on the decisions inside the plan.

Use plan mode for:
- Anything touching the PDF engine (`src-tauri/src/pdf/`)
- Any feature that crosses the IPC boundary (frontend ↔ Rust)
- Any change to the file-on-disk layout
- Anything that introduces a new dependency

Skip plan mode for:
- Single-file edits under ~50 lines
- Typo fixes, comment improvements
- Renames within one file
- Adding a missing test for existing code

---

## Patterns that work

### 1. Show, don't tell

If a similar feature already exists, link to it explicitly:

> Build the "add stamp" annotation tool. Use the same architecture as the freehand-draw tool in `src/tools/freehand.ts` — same state machine, same IPC contract, just a different render. Read that file first.

Claude is dramatically better at "build something like X" than at "build something." Reference code is the highest-leverage prompt ingredient.

### 2. Constrain by negative example

Tell Claude what you *don't* want:

> Implement the merge-PDFs operation. Do not stream pages in memory — for a 1GB merge that will crash. Use the PDFium `import_pages` API which writes directly to disk. Do not use pdf-lib for this; we chose PDFium specifically for memory characteristics on large files.

### 3. The "I'm reviewing your output in a real PDF viewer" reminder

For anything that writes a PDF, add to your prompt:

> When you're done, generate the output file at `/tmp/verify.pdf` and tell me to open it in a mainstream PDF reader to inspect. Do not assume that a passing test means the file is valid.

This is the single biggest defense against the most-common-failure-mode: tests that pass on the binary structure but the PDF looks broken in actual viewers.

### 4. Subagents for parallel research

If you're choosing between approaches — e.g. "should redaction use FPDF_RemoveTextObject or rebuild the page content stream?" — dispatch a subagent for each option:

> Spawn two research subagents in parallel. Agent A: investigate the FPDF_RemoveTextObject approach, list pros/cons, find any PDFium issues filed against it. Agent B: investigate the content-stream rebuild approach, same brief. Each gets 10 minutes max. Then synthesize.

Subagents have their own context windows, so your main context stays clean. This is gold for "I need to evaluate three libraries" questions.

### 5. The verification preamble

Before you say "ship it," ask:

> Before we ship, list the three most likely ways this implementation could break on a PDF you haven't seen yet. Then propose one fixture file per failure mode and add it to `tests/fixtures/edge-cases/`.

This forces adversarial thinking. It works.

---

## Patterns that fail

### 1. Vague pronouns

❌ "Make it better."
❌ "Fix the issue with the rendering."
❌ "It's too slow."

✅ "The page render in `pdf-canvas.tsx` triggers a re-layout on every scroll event. Debounce to 16ms or use `requestAnimationFrame`. Measure before and after with the perf overlay."

### 2. Multi-feature mega-prompts

❌ "Implement annotations, forms, and signing."
✅ Three separate `/plan` calls, three separate commits, three separate reviews.

### 3. The "and also..." trailing addition

If you start typing "and also" or "while you're at it," delete the message and start a new one. Trailing additions blow up scope without going through plan mode.

### 4. Asking Claude to grade its own work without an external signal

❌ "Is this implementation good?"
✅ "Run `/test-pdf` and paste the failures here. Run the prettier check. Open the output in a mainstream reader and tell me what looks wrong."

Claude is too agreeable when grading itself. Make it grade against external signals.

---

## Recovering from a derailed session

Sometimes Claude wanders. Symptoms: it starts inventing libraries, it forgets a constraint from CLAUDE.md, it produces code that doesn't match `docs/04_ARCHITECTURE.md`. When this happens:

1. **Don't try to fix it inline.** Trying to redirect a derailed session usually compounds the problem.
2. **Save the partial work** if any. `git stash`.
3. **`/clear` and restart.** Open a new session.
4. **Paste a tighter restart prompt:**

   > Read CLAUDE.md and docs/04_ARCHITECTURE.md. Then re-attempt `<the feature>`. The previous attempt drifted by `<one-line description>`. Do not repeat that mistake. Plan mode first.

5. **Add a constraint to CLAUDE.md** if the same drift happens twice. The repeat is the signal that the rule is missing.

---

## Cost & token discipline

This codebase will get big. To keep sessions responsive and tokens reasonable:

- `/clear` after every shipped feature. Context window resets are cheap.
- Don't paste large PDFs into chat. Put them in `tests/fixtures/` and reference by path.
- Don't ask Claude to dump entire files when a function name will do.
- Use `/compact` for long-running planning sessions, not implementation sessions.

---

## A final discipline: the verification ritual

Before any feature is marked done, do this ritual out loud (or in chat):

1. "I am opening the output PDF in a mainstream PDF reader right now." (Do it.)
2. "I am opening the output PDF in a platform viewer right now." (Do it.)
3. "I am opening the output PDF in a second independent reader right now." (Do it.)
4. "All three render correctly: yes/no."
5. "If yes, the test suite is green: yes/no."
6. "If both yes, mark the spec line complete."

This ritual catches more bugs than any test suite. PDF compliance is real and PDFium is not the only consumer of the files you'll be writing.
