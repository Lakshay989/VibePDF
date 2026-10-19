//! Crypto, signatures and redaction.
//!
//! The module tree has reserved this name since the architecture doc was
//! written (`docs/04_ARCHITECTURE.md`); P6.C1 is the first step to put anything
//! in it. What belongs here is code where a mistake is **silent** — a document
//! that looks protected and is not, or one that looks redacted and still
//! carries the text. That is why `docs/05_ROADMAP.md` requires a go-ahead before
//! anything here changes, and a recorded adversarial review of every diff —
//! tests passing or not — backed by mutation-checked tests and verification
//! against an outside implementation.
//!
//! Deliberately *not* here: the signature library (`settings/signatures.rs`),
//! which stores pictures. See the note in `docs/04_ARCHITECTURE.md` on why a
//! picture store behind the crypto gate helps nobody.

pub mod cms;
pub mod credential;
pub mod decrypt;
pub mod encrypt;
pub mod patterns;
pub mod redact;
pub mod sign;
pub mod verify;
