//! Finding the things people redact (P6.D2a, SPEC P6-SEC-011).
//!
//! ## Why precision matters more than recall here
//!
//! The spec asks for patterns that "find matches and ask the user to confirm".
//! The confirm step is what makes a *missed* match recoverable — the user is
//! reading the list and can redact by hand — and what makes a *false* match
//! expensive: a list with forty spurious entries is a list nobody reads, and
//! then the real one gets confirmed along with everything else without being
//! looked at.
//!
//! So the built-ins lean strict. A credit-card pattern that matches any
//! sixteen digits would fire on invoice numbers and order references all over a
//! normal document; this one runs the Luhn check, which costs nothing and
//! removes almost all of them.
//!
//! This is the opposite bias to `security/redact.rs`, which removes more when
//! unsure. The difference is what happens next: there, the alternative to
//! over-removing is leaking, and nobody reviews it. Here a human reads every
//! result before anything is removed.

use std::ops::Range;

use regex::Regex;

use crate::error::CommandError;

/// SPEC: P6-SEC-011 — the built-in patterns the spec names, plus user regexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PatternKind {
    Ssn,
    CreditCard,
    Email,
    Phone,
    /// A regex the user supplied.
    Custom,
}

impl PatternKind {
    /// A short label for the confirm list.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ssn => "Social security number",
            Self::CreditCard => "Card number",
            Self::Email => "Email address",
            Self::Phone => "Phone number",
            Self::Custom => "Custom pattern",
        }
    }

    fn source(self) -> &'static str {
        match self {
            // Three-two-four with separators. The bare nine-digit form is
            // deliberately not matched: it is indistinguishable from an order
            // number, and every false positive costs attention on the list.
            Self::Ssn => r"\b\d{3}-\d{2}-\d{4}\b",
            // 13–19 digits, optionally spaced or hyphenated in groups. Filtered
            // by Luhn afterwards — see `looks_like_a_card`.
            Self::CreditCard => r"\b\d(?:[ -]?\d){12,18}\b",
            Self::Email => r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b",
            // Optional country code, optional parenthesised area code.
            Self::Phone => {
                r"(?:\+\d{1,3}[ .-]?)?(?:\(\d{3}\)|\b\d{3})[ .-]?\d{3}[ .-]?\d{4}\b"
            }
            Self::Custom => "",
        }
    }
}

/// What to search for.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternSet {
    /// Which built-ins to run.
    pub kinds: Vec<PatternKind>,
    /// Extra regexes, in the `regex` crate's syntax.
    pub custom: Vec<String>,
}

/// One thing found, as a byte range within the text it was found in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub range: Range<usize>,
    pub kind: PatternKind,
}

/// The compiled form of a [`PatternSet`].
#[derive(Debug)]
pub struct Matcher {
    compiled: Vec<(PatternKind, Regex)>,
}

impl Matcher {
    /// Compile a set, reporting a bad custom regex against the pattern the user
    /// actually typed rather than as a generic failure.
    pub fn new(set: &PatternSet) -> Result<Self, CommandError> {
        let mut compiled = Vec::new();
        for kind in &set.kinds {
            if *kind == PatternKind::Custom {
                continue; // custom patterns come from `set.custom`
            }
            let re = Regex::new(kind.source()).map_err(|e| {
                CommandError::Internal(format!("built-in pattern {kind:?} is malformed: {e}"))
            })?;
            compiled.push((*kind, re));
        }
        for source in &set.custom {
            let re = Regex::new(source).map_err(|e| {
                CommandError::InvalidInput(format!("That pattern isn't valid: {source} — {e}"))
            })?;
            compiled.push((PatternKind::Custom, re));
        }
        Ok(Self { compiled })
    }

    /// Every match in `text`, overlaps resolved, in document order.
    #[must_use]
    pub fn find_in(&self, text: &str) -> Vec<Found> {
        let mut found: Vec<Found> = Vec::new();
        for (kind, re) in &self.compiled {
            for m in re.find_iter(text) {
                if *kind == PatternKind::CreditCard && !looks_like_a_card(m.as_str()) {
                    continue;
                }
                found.push(Found {
                    range: m.start()..m.end(),
                    kind: *kind,
                });
            }
        }
        dedupe_overlaps(found)
    }
}

/// Keep the longest match where two overlap.
///
/// The patterns deliberately overlap — a phone number and a card number can
/// both match the same digits — and listing one span twice makes the confirm
/// list longer without telling the user anything new. The longest wins because
/// it redacts the most; under the confirm-first design that is the safe tie-break.
fn dedupe_overlaps(mut found: Vec<Found>) -> Vec<Found> {
    found.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then_with(|| b.range.len().cmp(&a.range.len()))
    });

    let mut out: Vec<Found> = Vec::with_capacity(found.len());
    for candidate in found {
        let overlaps = out
            .last()
            .is_some_and(|prev| candidate.range.start < prev.range.end);
        if !overlaps {
            out.push(candidate);
        }
    }
    out
}

/// The Luhn checksum, which every real card number satisfies.
///
/// Not a security control — it is trivial to construct a number that passes.
/// It is a *noise* control: without it the card pattern fires on invoice
/// numbers, part numbers and order references throughout an ordinary document,
/// and a confirm list full of those is one the user stops reading.
fn looks_like_a_card(text: &str) -> bool {
    let digits: Vec<u32> = text.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                *d
            }
        })
        .sum();
    sum % 10 == 0
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn find(kinds: &[PatternKind], text: &str) -> Vec<Found> {
        let set = PatternSet {
            kinds: kinds.to_vec(),
            custom: Vec::new(),
        };
        Matcher::new(&set).expect("compile").find_in(text)
    }

    #[test]
    fn finds_a_social_security_number() {
        let hits = find(&[PatternKind::Ssn], "SSN: 123-45-6789 on file");
        assert_eq!(hits.len(), 1);
        assert_eq!(&"SSN: 123-45-6789 on file"[hits[0].range.clone()], "123-45-6789");
    }

    // Nine bare digits are indistinguishable from an order number, and every
    // false positive costs attention on a list a human has to read.
    #[test]
    fn does_not_match_nine_bare_digits_as_an_ssn() {
        assert!(find(&[PatternKind::Ssn], "Order 123456789 shipped").is_empty());
    }

    // The Luhn filter is the difference between a usable list and an unusable
    // one on any document with invoice or part numbers in it.
    #[test]
    fn a_card_number_must_pass_luhn() {
        // A well-known test number, and the same digits with one changed.
        assert_eq!(find(&[PatternKind::CreditCard], "4242 4242 4242 4242").len(), 1);
        assert!(find(&[PatternKind::CreditCard], "4242 4242 4242 4243").is_empty());
    }

    #[test]
    fn a_long_invoice_number_is_not_a_card() {
        // 16 digits that do not satisfy Luhn — the common false positive.
        assert!(find(&[PatternKind::CreditCard], "Invoice 1234567812345678").is_empty());
    }

    #[test]
    fn finds_emails_and_phones() {
        let hits = find(
            &[PatternKind::Email, PatternKind::Phone],
            "reach a.b-c@example.co.uk or (555) 123-4567",
        );
        assert_eq!(hits.len(), 2);
    }

    // Overlapping patterns must not put the same span on the list twice.
    #[test]
    fn overlapping_matches_are_reported_once() {
        let hits = find(
            &[PatternKind::Ssn, PatternKind::Phone, PatternKind::CreditCard],
            "call 555-123-4567 today",
        );
        assert_eq!(hits.len(), 1, "one span, one entry: {hits:?}");
    }

    #[test]
    fn a_custom_pattern_is_used() {
        let set = PatternSet {
            kinds: Vec::new(),
            custom: vec![r"PROJECT-\d+".into()],
        };
        let hits = Matcher::new(&set).expect("compile").find_in("see PROJECT-42 notes");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, PatternKind::Custom);
    }

    // A bad regex is the user's typo, and the message should say so with the
    // pattern in it rather than failing somewhere deeper as a mystery.
    #[test]
    fn a_malformed_custom_pattern_is_refused_by_name() {
        let set = PatternSet {
            kinds: Vec::new(),
            custom: vec!["[unclosed".into()],
        };
        let err = Matcher::new(&set).unwrap_err();
        assert!(format!("{err:?}").contains("[unclosed"));
    }

    #[test]
    fn nothing_configured_finds_nothing() {
        assert!(find(&[], "123-45-6789 a@b.com 4242424242424242").is_empty());
    }
}
