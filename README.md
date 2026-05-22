# PDF Editor — Claude Code Starter Kit

This kit is the **context bundle** you drop into a fresh project folder before you start vibe-coding with Claude Code. It encodes a clear product vision, a chosen tech stack, a phased roadmap, and a set of conventions, so Claude doesn't have to guess.

It is built around three principles that show up everywhere in the 2026 Claude Code best-practice literature:

1. **Specifications beat prompts.** A 20-decision feature has a ~1% chance of going right when every decision is improvised. Specs collapse those decisions ahead of time. ([source — AWS/Anthropic plan-mode workflow data])
2. **CLAUDE.md is advisory, not deterministic** — Claude follows it ~80% of the time, so keep it short and high-signal. Everything else lives in skills and reference docs that load on demand.
3. **Plan, then build, then verify.** Use plan mode (`Shift+Tab` twice) for anything multi-file. Treat the plan as a contract.

---

## How to use this kit

### Step 1 — Bootstrap

```bash
mkdir vibepdf && cd vibepdf
# Copy the contents of this kit into the new directory
cp -r /path/to/pdf-editor-claude-kit/. .
git init && git add . && git commit -m "chore: bootstrap with Claude Code kit"
```

You should now have:

```
vibepdf/
├── CLAUDE.md
├── INITIAL_PROMPT.md
├── PROMPTING_PLAYBOOK.md
├── README.md
├── docs/
│   ├── 01_VISION.md
│   ├── 02_PRODUCT_SPEC.md
│   ├── 03_TECH_STACK.md
│   ├── 04_ARCHITECTURE.md
│   ├── 05_ROADMAP.md
│   ├── 06_CONVENTIONS.md
│   └── 07_AI_FEATURES.md
└── .claude/
    ├── commands/
    │   ├── plan.md
    │   ├── ship.md
    │   ├── review.md
    │   └── test-pdf.md
    └── settings.example.json
```

### Step 2 — Open Claude Code in the project root

```bash
claude
```

Then **paste the contents of `INITIAL_PROMPT.md` as your first message**. It tells Claude exactly which docs to read and in what order, and ends in plan mode so you review before any code is written.

### Step 3 — Work phase by phase

The roadmap (`docs/05_ROADMAP.md`) breaks the build into eight phases. Each phase has a tight scope and an "acceptance demo." Do not jump ahead. Finish phase N, commit, then ask Claude to plan phase N+1.

### Step 4 — Use the slash commands

- `/plan <feature>` — Claude drafts an EARS-syntax spec and an implementation plan, then waits.
- `/ship <feature>` — Implement the most recent plan, with hooks running on every save.
- `/review` — Run a self-review pass: lint, type-check, smoke-test the touched PDF flows.
- `/test-pdf <path>` — Run the standard PDF regression suite against a file.

### Step 5 — Curate, don't accumulate

CLAUDE.md will rot if you let it. Once a month, prune anything Claude has stopped needing. The test is brutal: *"Would removing this line cause Claude to make a mistake?"* If no, cut it.

---

## What's NOT in this kit (deliberately)

- **No code.** This is a pre-flight kit. The whole point is for Claude to write the code with high context.
- **No design mockups.** UX comes after the rendering and edit pipelines are stable. See Phase 5 in the roadmap.
- **No business logic.** This is a tool. There's no users table, no billing, no telemetry.
- **No cloud.** Offline-first is a constraint, not a feature. Any network call must be opt-in and clearly labeled.

---

## The product, in one paragraph

**VibePDF** is a free, offline, open-source PDF editor for desktop (Windows, macOS, Linux) that matches Acrobat Pro on its core editing surface — text editing, annotation, forms, signing, OCR, page operations, redaction — and ships local AI features (summarization, PII detection, Q&A) that run on the user's machine. It uses Tauri 2 for a small native shell, a Rust backend bound to PDFium for heavy PDF work, PDF.js for high-fidelity rendering, and Tesseract for OCR. No paywall, no telemetry, no account.

---

## Credits

The structure of this kit pulls from Anthropic's published Claude Code best practices, the awesome-claude-code community, and the 2026 spec-driven development literature. Citations are inline in `docs/` where claims are non-obvious.
