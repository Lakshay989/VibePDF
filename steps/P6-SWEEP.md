# P6 verification sweep

Everything shipped in Phase 6 that a human still has to check, in the order
worth doing it. Nothing here is `[x]` — the automated suites are green for all
of it, and that is exactly why these items exist.

Two kinds of item, kept apart on purpose:

- **Review** — reading a diff. Required by `docs/05_ROADMAP.md` for anything
  under `src-tauri/src/security/`, tests passing or not, because crypto bugs are
  silent.
- **Cross-reader** — opening a file somewhere that is not PDFium. A passing test
  proves our reader agrees with us; it says nothing about Acrobat. Phase 6 has
  already produced two files that PDFium accepted and something else did not, or
  vice versa.

---

## 1. `security/` review — C1 and C2 (blocking)

Two diffs: `8e8394b`, `f7384e3`, `536510b`.

- [ ] `security/encrypt.rs::permissions_entry` — the workaround for lopdf 0.36
      writing `/Perms` in plaintext. Check it encrypts a **named** binding and
      returns that; the upstream bug is encrypting a temporary copy.
- [ ] `security/encrypt.rs::file_encryption_key` — OS CSPRNG, nothing derived
      from the passwords or the clock.
- [ ] `commands/pdf.rs::pdf_protect` / `pdf_remove_protection` — no password
      enters a log line, a `CommandError` message, or the actor (which logs its
      path on every message).
- [ ] `security/decrypt.rs` — the two named failures are honest about what went
      wrong rather than blaming the password.
- [ ] `security/encrypt.rs::fix_permissions_entry` (C3) — `/Perms` is built from
      the `/P` read back out of the document, not from a second copy of the
      permission set. Two values that must agree are one value here; check it
      stayed that way.
- [ ] `DocumentPermissions::default()` — hand-written, granting everything. The
      derived `Default` would clear every bit and silently produce the most
      restricted document possible.

## 2. Cross-reader: encryption (blocking)

Files: `Sample PDFs/vibepdf-verify-encrypted-user.pdf`, `…-both.pdf`.
Password to open: `open-me`. Permissions password on the second: `owner-only`.

- [ ] **Acrobat** prompts, opens with `open-me`, refuses a wrong password
- [ ] **A third reader** (Chrome, Okular, Sumatra) — same
- [ ] Preview — same *(already done 2026-08-12: renders correctly)*
- [ ] **Unlock** one in-app, then open the result in all three: opens with **no**
      password anywhere

## 2b. Cross-reader: permissions (C3, blocking)

File: `Sample PDFs/vibepdf-verify-no-print.pdf` — printing and copying withheld.
Opens with `open-me`; permissions password `owner-only`.

This is the roadmap's own acceptance demo, and **the only way to check C3 at
all**: the tests assert what the document *says*, and nothing in a PDF makes a
reader obey it. A green suite here means the bits are right, not that the
feature works.

- [ ] **Acrobat**, opened with `open-me`: **printing is blocked** (greyed out or
      refused), and copying text is blocked
- [ ] **Acrobat**, opened with `owner-only`: printing works
- [ ] A **third reader**: note what it does. Ignoring `/P` entirely is a legitimate
      reader choice, not a defect in our file — record which readers honour it so
      the dialog's wording can stay honest.
- [ ] The other two encrypted files still print and copy freely (that the
      restriction is *ours* and not an accident of encrypting at all)

> The one that would matter most: a file some readers open and others reject is
> the exact failure the `/Perms` bug produced, and it looked fine locally.

## 3. Cross-reader + undo: signature placement (A5a)

File: `Sample PDFs/vibepdf-verify-signature.pdf` — two placements, full and 50%
opacity.

- [ ] **Acrobat** and a **third reader**: both render, transparent background,
      no white box, correct proportions, the half-opacity one visibly lighter
- [ ] **Undo** after placing a signature in-app → the page returns to exactly as
      it was *(never checked; the only A5a item with no coverage at all)*
- [ ] Zoom a **thresholded** import to 400% — a judgement call, not pass/fail.
      P6.A4 decoding showed 0% partial alpha on those, so the edges are hard.
      Placement is the first time it is visible at size.

## 4. In-app, worth one pass each

- [ ] **Protect…** with only a permissions password → refused, and the message
      explains that the protection could not be removed afterwards
- [ ] **Unlock…** with the password that *opens* an AES-256 file → refused; with
      the permissions password → works. Surprising, and deliberate: see
      `steps/P6.md`.
- [ ] Signature placement: first Place after any tool change works — the
      "select twice" bug (`1d20da3`) was invisible to every test until it was
      reported

## 5. Cross-reader: clean document (D3)

Files: `Sample PDFs/vibepdf-verify-dirty.pdf` and `…-cleaned.pdf` — the same
document before and after every toggle.

The automated tests prove the strings are gone from the bytes, which is the part
that matters most. What they cannot show is whether a *reader* agrees the file
is still well-formed after seven kinds of surgery.

- [ ] Open **both** in Acrobat: Document Properties on the dirty one shows
      `SECRETAUTHOR`; on the cleaned one shows nothing. Same in Preview's
      Inspector, which reads XMP rather than `/Info` — the one that catches a
      half-done metadata clean.
- [ ] The cleaned file still renders `VisibleBodyText`, still has one page, and
      the bookmarks/comments panels are empty
- [ ] Search the cleaned file for `SECRETHIDDEN` in a reader: no hit
- [ ] The cleaned form field is **still fillable** — the value is gone, the field
      is not
- [ ] In-app: clean, then **Undo** → everything comes back. Then save, reopen,
      and confirm undo no longer offers it (documented behaviour, not a bug)

---

## Known and accepted, not defects

Listed so a sweep does not re-report them.

| | |
|---|---|
| A placed signature lists as **Stamp** in the annotation panel | It is a `/Stamp`. Revisit at B2. |
| **Owner-only protection is refused** | C2 cannot undo it. Revisit when lopdf's R6 user auth is fixed. |
| AES-256 files carrying `/Length` cannot be unlocked | Other tools' files, including pypdf's. Named in the error. |
| Flattened form text is top-anchored; `/Q` not honoured | Carried from P5. |
| Cleaning **hidden text** makes a scanned page unsearchable | That layer *is* the searchability. Off by default; the dialog says so. |
| Clean does not remove hidden **layers** (OCGs) | P6-SEC-012 does not name them. Revisit if a real file needs it. |

## Upstream

- [ ] File the lopdf report (`/Perms` written in plaintext, plus the R6
      user-authentication finding). Draft prepared 2026-08-12; not yet filed.
