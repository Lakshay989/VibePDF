# 07 — AI Features

> Read this only when working on Phase 8. It is irrelevant to phases 1–7.

The AI feature set is the place where it would be easiest to violate the offline-first constraint and the simplest mission of the product. This document exists to keep that from happening.

---

## The three hard rules for AI

1. **AI is optional.** Removing the AI subsystem entirely does not break the product. Every AI feature has either an explicit "enable AI" path or simply doesn't appear in the UI until a backend is configured.

2. **Local before cloud.** The default backend is local. Cloud is a per-feature opt-in with explicit per-action consent (a checkbox in the modal, not a setting buried in preferences).

3. **No data leaves the machine without consent.** Not for "improving the product." Not for "anonymous telemetry." Not ever, without a user-visible network request the user just approved.

If any of these three rules is tempting to bend, the bend is a bug. Stop.

---

## Backend architecture

Two backends, used for different categories of work:

### Generative backend — Ollama (local HTTP)

Ollama runs locally on the user's machine. It exposes `http://localhost:11434/api/generate` and `http://localhost:11434/api/chat`. We detect it on startup by probing the port; if responsive, we enumerate installed models via `/api/tags`.

**Used for:**
- Summarization
- Q&A (after retrieval)
- Translation
- Any free-form text generation

**Default model preference (in order):**
1. The model the user has selected in settings, if installed
2. `llama3.1:8b-instruct-q4_K_M` if installed
3. `phi3.5:3.8b-mini-instruct-q4_K_M` if installed
4. Smallest available model the user has

**Why Ollama and not bundled `llama.cpp`:**
- Bundling a model adds 4-8 GB to the installer. Unacceptable.
- Ollama is a common installation in the target audience.
- Ollama auto-handles model downloads, memory, and GPU acceleration.
- If the user doesn't have Ollama, we offer a one-click install URL — we don't try to install it for them.

### Inference backend — ONNX Runtime (`ort` crate)

For deterministic, fast, small-model tasks. The models are shipped in the installer (~80 MB total).

**Used for:**
- PII NER (named entity recognition for smart redaction)
- Embedding generation (for retrieval-augmented Q&A)

**Models bundled:**
- `bge-small-en-v1.5` (33 MB) — sentence embeddings
- `piiranha-v1` (43 MB) — multilingual PII detection (custom ONNX export of a fine-tuned BERT-base)

These are loaded once at startup, kept in memory, and executed on CPU by default. GPU acceleration via the CUDAExecutionProvider when available.

---

## The retrieval pipeline (for Q&A)

```
┌──────────────┐
│ User opens   │
│ document     │
└──────┬───────┘
       │
       ▼
┌────────────────┐
│ Extract text   │
│ per page       │  ← PDFium (existing)
└──────┬─────────┘
       │
       ▼
┌────────────────┐
│ Chunk: 800     │
│ chars, 100     │
│ overlap        │
└──────┬─────────┘
       │
       ▼
┌────────────────┐
│ Embed chunks   │  ← ONNX bge-small
│ with metadata  │     (~5ms / chunk)
│ {page, span}   │
└──────┬─────────┘
       │
       ▼
┌────────────────┐
│ Store in       │
│ in-memory      │  ← no disk persistence by default;
│ vector index   │     in-memory HNSW via `hnsw_rs`
└────────────────┘

When a question arrives:
       │
       ▼
┌────────────────┐
│ Embed question │
└──────┬─────────┘
       │
       ▼
┌────────────────┐
│ k-NN, top 5    │
└──────┬─────────┘
       │
       ▼
┌────────────────┐
│ Pass chunks +  │
│ question to    │  ← Ollama
│ LLM            │
└──────┬─────────┘
       │
       ▼
┌────────────────┐
│ Stream answer  │
│ + page         │
│ citations to   │
│ user           │
└────────────────┘
```

**Persistence:** Vector indices are kept per-document in memory while the document is open and discarded when it closes. If "remember this document for AI" is enabled per-document by the user, the index is persisted to `~/.vibepdf/embeddings/<docHash>.bin`.

**Latency budget:**
- Embed a 50-page document: ≤ 3 s on a 2020-era CPU
- Answer a question: ≤ 2 s for first token, full answer streams thereafter

---

## Smart redaction pipeline

```
User clicks "Smart redact"
       │
       ▼
Extract text per page (existing pdfium-render code)
       │
       ▼
Run piiranha-v1 NER per page → entities with type + offsets
       │
       ▼
Map entities back to PDF coordinate quads (via text-extraction-with-positions)
       │
       ▼
Show user a checklist:
  ☑ "John Smith" (NAME, page 3, 2 occurrences)
  ☑ "555-12-3456" (SSN, page 3)
  ☐ "Acme Corp." (ORG, page 1)   ← unchecked by default
       │
       ▼
User confirms
       │
       ▼
Apply existing P6-SEC-010 redaction to each confirmed entity
```

**Key principle:** Smart redact is a UI on top of regular redact. The redaction primitive is exactly the same code path as a user dragging a redaction box. The AI only proposes what to redact; it never performs an unconfirmed redaction.

---

## The cloud opt-in (P8-AI-007)

Cloud AI is supported but deliberately friction-ed. The flow:

1. User goes to Settings → AI → Backends → "Add cloud backend"
2. User selects a provider (Anthropic, OpenAI, Mistral, etc.)
3. User enters API key (stored in the OS keychain via `keyring` crate, never in plaintext)
4. User selects which features can use cloud (per-feature toggle)
5. Per-feature toggle defaults to OFF

In the UI, every AI feature shows its current backend (`Local · llama3.1:8b` or `Cloud · claude-opus-4-7`) in the modal. Switching backends mid-task is allowed.

**Privacy guard:** When cloud is active, any document content sent over the network is logged locally (with hashes only, not contents) in `~/.vibepdf/logs/cloud-requests.log`. The user can always inspect what the app sent.

---

## What we are NOT building

- An "AI assistant" chatbot for the app itself ("how do I redact?"). The user can read docs.
- An AI that auto-applies edits without confirmation. Every AI action gates through a confirm step.
- A model marketplace. Users get models via Ollama or via our two bundled ONNX models. Period.
- Fine-tuning on user data. We don't collect data; we have nothing to fine-tune on.
- Voice. Out of scope.

---

## Testing AI features

This is where Claude needs to be most careful. Generative tests are stochastic. Two principles:

1. **Test the pipeline, not the output.** We test that the right model is called with the right prompt and that the response is rendered correctly. We do not assert the exact text of summaries.

2. **Test the deterministic parts deterministically.** PII detection produces structured output (entity type, offsets). That gets golden-file tested on a stable input.

3. **Test backend selection.** If the user has cloud enabled for summarization and local enabled for redaction, both backends should be called correctly. This is a unit-testable invariant.

Mock the actual model calls in unit tests. Real-model integration tests live in `tests/ai/` and run only on demand (not in CI by default), gated by an env var.
