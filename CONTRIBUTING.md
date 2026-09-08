# Contributing to VibePDF

Thanks for looking. A few things are worth knowing before you spend time on a
change, because this project has some rules that are not the usual ones.

## Before you write code

**The roadmap is sequential on purpose.** `docs/05_ROADMAP.md` defines phases,
and a phase is finished before the next starts. A pull request implementing a
Phase 8 feature while Phase 6 has open acceptance criteria will be asked to
wait, however good it is. Contributions land most easily when they sit inside
the current phase or fix something already built.

**Open an issue first for anything non-trivial.** Not ceremony — the spec drives
the design here, and it is cheaper to disagree about a requirement than about an
implementation of it.

**`docs/02_PRODUCT_SPEC.md` is the source of truth**, written in EARS syntax,
and it is the maintainer's to change. If your feature is not specified, propose
the spec line in an issue rather than adding it in a PR.

## The rules that trip people up

### Offline is a hard constraint, not a default

There is no telemetry, no cloud component, no analytics, and no phone-home. The
only network access in the codebase is an optional, off-by-default local Ollama
integration and the build-time PDFium fetch. **A PR that adds a network call
will be rejected** unless it is gated behind an explicit user-enabled setting
and degrades gracefully when offline.

### `src-tauri/src/security/` is different

Signing, encryption, and redaction live there. These are the paths where a bug
is *silent*: the document looks protected and is not, or looks redacted and
still carries the text. Nobody notices until it matters.

Changes there get a human review pass on the diff **regardless of whether the
tests pass**, and reviews are slower as a result. Please don't take it
personally.

If you have found a security problem, do not open a PR that fixes it in public.
See [`SECURITY.md`](SECURITY.md).

### Tests for removal assert on the data, not the mechanism

A redaction test greps the saved bytes for the secret; it does not check that a
key was unset. The second version passes against the bug where the object
survives detached from the tree — which is the bug that actually happens.

The same principle applies to cryptography: signatures are verified against an
outside implementation (`openssl cms -verify`) with a paired counter-test that
requires a tampered document to be *rejected*. A signature computed over the
wrong bytes passes every self-consistent test there is.

**New tests are expected to be mutation-checked.** Break the code the test
covers and confirm the test fails. A test that passes both ways is worse than no
test, because it looks like coverage.

### Fixtures need provenance and a generator

Every PDF in `tests/fixtures/` has a committed, dependency-free generator script
beside it and an entry in `tests/fixtures/PROVENANCE.md`. No mystery PDFs — the
corpus has to be rebuildable and reviewable rather than taken on trust.

Scratch and verification PDFs belong in the gitignored `Sample PDFs/` tree, not
in the repository.

### Citations you cannot resolve

Comments across the Rust and TypeScript source cite `FABLE_REVIEW` by section
number — `FABLE_REVIEW §3.9`, and so on. That document is an internal design
review kept out of the repository, so those references will not resolve from a
clone. They are left in place because the surrounding comment always says what
the finding *was*; the citation is provenance, not the explanation. If one is
genuinely unclear, ask in an issue and it will be quoted back.

## Code style

- **TypeScript:** strict, no `any`. Prefer `unknown` plus narrowing.
- **Rust:** `clippy::pedantic` clean with `-D warnings`. No `unwrap()` outside
  tests. `?` for propagation, `anyhow` at boundaries, `thiserror` inside modules.
- **Naming:** `camelCase` in TS, `snake_case` in Rust, `kebab-case` for files.
- **Comments explain _why_, never _what_.**
- Code implementing a spec line carries a `// SPEC: <id>` comment naming it.

Full detail in [`docs/06_CONVENTIONS.md`](docs/06_CONVENTIONS.md).

## Architecture constraints

- **All PDF writes go through the document actor.** The frontend never writes
  bytes.
- **Every write path round-trips**: the output is reopened and verified before it
  is returned. No silent breakage of an existing PDF.
- **New IPC commands need a typed wrapper** in `src/ipc/`. No raw `invoke()` in
  component code.
- **No new top-level module** without updating `docs/04_ARCHITECTURE.md` first.
- **No additional PDF library.** PDFium, lopdf and PDF.js each have a documented
  role in `docs/03_TECH_STACK.md`; a fourth needs that doc updated and a real
  argument.

## Dependencies

New dependencies need justification in the commit message: what it does, why the
existing tree cannot, and what it drags in. Vendor lock-in is worse than
implementation work, and every crate is supply-chain surface in an application
whose entire pitch is that it can be trusted with your documents.

The shipped binary must stay copyleft-free — see
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

## Before opening a pull request

```bash
npm run check      # tsc + eslint + cargo clippy, all warnings are errors
npm run test       # frontend (vitest)
npm run test:rust  # Rust, including the PDF engine
```

All three must be green. Then check:

1. Any change under `src-tauri/src/pdf/` comes with a deterministic test against
   a fixture PDF.
2. Any write path has been opened in **several independent PDF readers**, not
   only in VibePDF. A passing test does not prove the output is valid to other
   viewers — this project has been bitten by exactly that.
3. New dependencies are justified in the commit message.
4. Commits follow [Conventional Commits](https://www.conventionalcommits.org/).

## Licensing of contributions

Unless you state otherwise, contributions are dual-licensed `MIT OR Apache-2.0`,
matching the project. See [`COPYRIGHT`](COPYRIGHT). There is no CLA.
