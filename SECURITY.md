# Security policy

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately through GitHub Security Advisories:
[**Report a vulnerability**](https://github.com/Lakshay989/VibePDF/security/advisories/new).
That opens a private thread visible only to the maintainers, and it can become a
published advisory with a CVE once a fix ships.

Please include what you have: the affected version or commit, what an attacker
gains, and the smallest reproduction you can manage. **A PDF that demonstrates
the bug is worth more than a description of it** — attach it, or describe how to
generate one. If a file is too sensitive to attach, say so and describe its
shape instead.

Expect an acknowledgement within a week. This is a small project with no
security team and no bounty programme; what it can offer is that a report will
be read carefully by someone who knows the code.

## What counts as a vulnerability here

VibePDF is an offline desktop application. It has no server, no accounts, and no
network surface, so the usual web categories mostly do not apply. The bugs that
matter are the ones where **the application's output silently misrepresents its
own guarantees** — a document that looks protected and is not.

Especially interested in:

- **Incomplete redaction.** Content still recoverable from a file that VibePDF
  reported as redacted — text left in a content stream, an image not removed,
  data surviving in an object that was detached but not deleted, or anything
  recoverable from an earlier incremental-update generation.
- **Encryption that does not hold.** A document VibePDF encrypted that opens
  without the password, permission flags that a reader ignores because the
  `/Perms` block disagrees with `/P`, or key material derived incorrectly.
- **Signature forgery or misreporting.** A signature that verifies over bytes it
  does not actually cover, a tampered document reported as intact, or the
  verification UI showing a stronger status than the evidence supports.
- **Credential exposure.** Private key material from a user's PKCS#12 reaching a
  log, a crash dump, an error message, or the frontend.
- **Escape from the offline constraint.** Any code path that reaches the network
  without the user having explicitly enabled it.
- **Malicious-PDF handling.** A crafted document that achieves code execution,
  reads files outside the document, or hangs the application indefinitely.

Also in scope, though lower severity: crashes on malformed input, and any write
path that corrupts a document it claimed to save successfully.

## Out of scope

- Findings against a build with a modified `src-tauri/src/security/`.
- The **committed test signing keys** in `tests/fixtures/certs/`. They are
  deliberate, self-signed, chain to nothing, and are documented in that
  directory's `README.md`.
- Vulnerabilities in PDFium, PDF.js, or another upstream dependency — report
  those upstream. If VibePDF's *use* of one is what makes it exploitable, that
  is in scope here.
- Missing hardening with no demonstrated impact (a flag not set, an algorithm
  you would have chosen differently) unless you can show what it costs.

## Supported versions

There is no released version yet. Until there is, only the current `main` is
supported, and fixes land there.

## Where the risk is concentrated

`src-tauri/src/security/` holds signing, encryption, and redaction — the code
where a mistake is silent rather than loud. Every change there gets a human
review pass on the diff regardless of whether the tests pass, and that rule is
enforced by `.github/CODEOWNERS`.
