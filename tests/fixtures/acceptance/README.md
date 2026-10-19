# Phase 1 acceptance fixtures

The Phase 1 roadmap acceptance demo (`docs/05_ROADMAP.md §Phase 1`) needs
three PDFs that are too big to commit to git. They live here once
generated; the directory is gitignored except for this README and the
generator.

## What each fixture is

| File | What | Used by |
|---|---|---|
| `p1-spec.pdf` | 1000-page text-heavy PDF (Lorem ipsum), each page numbered. Exercises navigation, search across many pages, outline absence handling. | P1-VIEW-005 nav perf, P1-VIEW-007 search-of-5000-page-target |
| `p1-encrypted.pdf` | `hello.pdf` re-encrypted with **user password `vibepdf`** and owner password `vibepdf-owner`. **RC4 128-bit** (`/V 2 /R 3`), not AES — the `generate.py` call does not request an AES algorithm. Corrected 2026-08-12 while building P6.C1; the AES-256 writing counterpart is `security/encrypt.rs`. | P1-VIEW-003 password prompt |
| `p1-large.pdf` | ≈500 MB filler PDF — repeated text-draw operators across enough pages that the on-disk size hits the NFR target. | NFR-PERF-003 (open without OOM, scroll ≥30 fps) |

## Generating

```bash
# All three at the default (1000-page spec, 500MB large):
python3 tests/fixtures/acceptance/generate.py all

# Quick smoke sizes (5-page spec, 5MB large) — for CI or first-time setup:
python3 tests/fixtures/acceptance/generate.py all --quick

# Individual fixtures:
python3 tests/fixtures/acceptance/generate.py spec  --pages 1000
python3 tests/fixtures/acceptance/generate.py large --size-mb 500
python3 tests/fixtures/acceptance/generate.py encrypted
```

The encrypted fixture requires `pypdf`:

```bash
pip install -r tests/fixtures/acceptance/requirements.txt
```

The other two use only the Python standard library — same minimal PDF
builder as `tests/fixtures/basic/generate-hello.py`.

## Why generated, not committed

- `p1-large.pdf` alone is half a gigabyte. Git LFS is overkill for a
  deterministic, regeneratable artifact.
- Anyone with the repo + Python 3.10+ can produce byte-equivalent
  fixtures in seconds (for the small ones) or minutes (for 500 MB).
- CI regenerates on demand. The fixtures never need to be reviewed
  in a PR diff.
