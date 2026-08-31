# Initial prompt

> Paste the block below into Claude Code as your first message after you open the project. Don't add anything to it. Don't summarize it. The wording is calibrated.

---

I'm building VibePDF, an offline open-source PDF editor that aims to match the paid editors on its core editing surface, with no paywall. The project is brand-new — the only thing in this repo right now is a `docs/` folder, a `CLAUDE.md`, and a `.claude/` config.

Before you do anything else:

1. Enter plan mode (Shift+Tab twice). Stay in plan mode until I explicitly approve.
2. Read in this exact order, and only read what's needed:
   - `CLAUDE.md`
   - `docs/01_VISION.md`
   - `docs/02_PRODUCT_SPEC.md` — skim for structure, then deep-read Phase 1 features only
   - `docs/03_TECH_STACK.md`
   - `docs/04_ARCHITECTURE.md`
   - `docs/05_ROADMAP.md` — read Phase 1 in detail
   - `docs/06_CONVENTIONS.md`
3. Do NOT read `docs/07_AI_FEATURES.md` yet — it's for a later phase.
4. Do NOT read the slash-command files in `.claude/commands/` — they will load when invoked.

After reading, produce a planning document with these sections, in this order:

**A. Reading audit.** For each doc, one sentence: what I learned, and one thing that's unclear or contradictory. If a doc contradicts another doc, flag it — do not silently resolve it.

**B. Phase 1 scope confirmation.** Restate Phase 1 in your own words. List the acceptance criteria as a checklist. If anything is ambiguous, list the ambiguity as a question for me, not as an assumption you've made.

**C. Bootstrap plan.** A step-by-step plan to get from an empty repo to a running Tauri 2 + React + TypeScript app that opens a PDF and renders it via PDF.js. Be specific:
   - Exact `cargo new` / `npm create tauri-app` commands
   - Exact dependency versions (look them up — do not guess)
   - File-by-file scaffolding tree
   - The one-command dev script
   - The minimum smoke test that proves rendering works

**D. Risks.** Three to five concrete risks for Phase 1 — not generic "PDFs are hard" risks, but things like "PDFium binary size on macOS may push the installer over 100MB" with a proposed mitigation for each.

**E. The first commit.** What the first commit should contain, as a tree, with a draft commit message.

Then **stop**. Do not write any code, do not create any files. Wait for me to approve the plan or push back.

Two rules while you read:
- If you find yourself reframing a requirement to make it sound easier than it was written, stop — that's a signal to ask a clarifying question, not a license to proceed.
- If a doc references a library or a version, look it up before you accept it. Some of these are aspirational and may not exist as described. Flag any of those in section A.
