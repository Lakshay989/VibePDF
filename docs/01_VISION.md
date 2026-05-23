# 01 — Vision

## The pitch

**VibePDF is the PDF editor people wish existed.**

In May 2026, every functional PDF editor on the market is either paywalled (Acrobat Pro at $20+/month, Nitro at $180/year, Foxit at $135/year), limited to a handful of uses per day (Smallpdf, iLovePDF), or both. The free options either upload your documents to a server (privacy disaster for any sensitive document) or are missing the features people actually use (LibreOffice Draw cannot edit existing text reliably; Stirling PDF is impressive but requires Docker and has no native text editing).

There is no offline, free, full-featured PDF editor for desktop in 2026.

VibePDF fills that gap.

## What success looks like

A user who has been paying for Acrobat Pro can install VibePDF, open any PDF they have, and within five minutes do every one of these things without hitting a feature wall or a sign-up flow:

- Edit a typo in existing text without the font getting mangled
- Highlight, underline, comment on a passage
- Fill out a government form and save it
- Sign a contract with a drawn signature
- Redact a social security number from a scanned document (after running OCR)
- Merge three PDFs into one and reorder the pages
- Export the result as a Word document

They never see a "Pro" upsell, a feature paywall, a watermark on output, or a network request they didn't ask for.

## The four hard constraints

These are constraints, not features. Violating them changes the product into something else.

1. **Offline-first.** The editor must do every core operation with no network connection. Network is opt-in per-feature (e.g. checking for updates, cloud sync if a user enables it), and turned off by default.
2. **No account, no signup, no telemetry.** The first time the app opens, it just opens. No tracking pixel, no anonymous-usage-stats nag, no "please rate us." Period.
3. **No watermarks on output.** This is the line that separates free editors from free trials.
4. **Permissively licensed.** MIT or Apache 2.0 on our code. Every bundled dependency must be license-compatible. No GPL/AGPL contamination in the shipped binary. (This rules out one PDF library — see `03_TECH_STACK.md`.)

## The principles

Beyond the constraints, the design is guided by five principles when there's a real choice to be made:

### 1. Round-trip fidelity over feature breadth

If we add a feature that subtly corrupts PDFs that Acrobat opens fine, we have made the product worse, not better. Every write operation must round-trip through a real PDF viewer test. This is more important than checking off a feature list.

### 2. The 90% feature is better than 10 half-features

Form filling that works on 95% of PDFs in the wild beats form filling, XFA editing, JavaScript validation, calculated fields, and digital signature widget creation that all kind-of-work on 30% of PDFs. We will ship complete, polished operations and skip the ones we can't do well — gracefully.

### 3. Power without obscurity

Keyboard shortcuts match Acrobat where they exist (Cmd/Ctrl+E for edit, Cmd/Ctrl+J for page properties, etc.). Right-click menus expose the deep functionality. The toolbar shows the 12 things 95% of users need. Settings are searchable.

### 4. AI is a feature, not a religion

Local LLM features (summarize, find-PII, ask-this-PDF) ship as opt-in. They never block a core operation. They never send data over the network without explicit per-action consent. They run on Ollama / llama.cpp / ONNX models the user already has or can download from the app once.

### 5. The codebase is its own marketing

A clean, well-tested, idiomatic Rust+TS codebase invites contributions. We optimize for "a competent developer can read the redaction module and add a new redaction pattern in an afternoon." This means: small files, clear module boundaries, generous comments at decision points, and no clever code where boring code would do.

## What VibePDF is not

To prevent scope creep, we name the explicit non-goals up front:

- **Not a viewer.** It's an editor that also views. We will not optimize for "fast viewer with no edit features."
- **Not a Word competitor.** It does not author rich documents from scratch. PDF export from Word stays in Word.
- **Not a forms platform.** It fills and creates forms locally. It does not host forms, collect responses, or wire forms to webhooks.
- **Not a signature service.** It signs locally with the user's certificate. It does not run a signing workflow ("send to Bob to sign"). That's a different product.
- **Not a collaboration tool.** No real-time multi-user editing. One person, one document, one machine. (A future "shared annotation file" feature might exist, but it would be a file format, not a service.)
- **Not mobile.** Tauri 2 supports mobile but we will not target it in v1. Mobile PDF editing has different UX constraints and we will get desktop right first.
- **Not a cloud SaaS.** There is no server we operate. Optional sync (later) would be BYO-storage (Dropbox, iCloud Drive, etc.) at the filesystem level.

## The competitive frame

| | Acrobat Pro | Nitro | Foxit | LibreOffice Draw | Stirling PDF | **VibePDF** |
|---|---|---|---|---|---|---|
| Price | $20/mo | $180/yr | $135/yr | Free | Free | **Free** |
| Offline | ✓ | ✓ | ✓ | ✓ | Self-host | **✓** |
| Edit existing text | ✓ | ✓ | ✓ | Partial | ✗ | **✓** |
| OCR | ✓ | ✓ | ✓ | ✗ | ✓ | **✓** |
| Forms (AcroForm) | ✓ | ✓ | ✓ | Partial | ✗ | **✓** |
| Digital signatures | ✓ | ✓ | ✓ | ✗ | ✗ | **✓** |
| True redaction | ✓ | ✓ | ✓ | ✗ | ✗ | **✓** |
| Native desktop | ✓ | ✓ | ✓ | ✓ | Docker only | **✓** |
| Source available | ✗ | ✗ | ✗ | ✓ | ✓ | **✓** |
| Account required | ✓ | ✓ | ✓ | ✗ | ✗ | **✓ none** |

The blank cell where every competitor has a paywall and we don't is the entire reason this product exists.

## Why now

Three things have changed since the last serious open-source PDF editor attempt:

1. **PDFium is mature, free, and BSD-licensed.** It's the engine inside Chromium and powers Edge's built-in PDF viewer. The Rust bindings (`pdfium-render`) are production-ready as of 2026.
2. **Tauri 2 ships small native desktop apps.** ~8MB installer vs Electron's ~120MB, with similar developer experience. Memory footprint is 30-60% lower. For an editor people might keep open all day, this matters.
3. **Local LLMs are good enough for document AI.** Llama 3.1 8B and Phi-4 14B running on a consumer GPU can summarize a 20-page document, find PII, and answer questions about it — accurately enough to be useful, locally enough to be private.

The pieces exist. They have not been assembled. That's the opportunity.
