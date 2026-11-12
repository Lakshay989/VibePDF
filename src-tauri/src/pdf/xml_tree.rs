//! A small namespace-aware XML tree over `quick-xml` (P7.C1b1), for reading
//! Office Open XML.
//!
//! Elements and attributes are keyed by **namespace URI and local name**, not
//! by prefix. Word writes `w:p`, but nothing requires the prefix to be `w`,
//! and ISO "Strict" OOXML uses different URIs for the same vocabulary; both
//! are normalised here to the familiar Transitional ones, so the reader above
//! matches one name per element.
//!
//! A tree rather than a streaming walk because `WordprocessingML` is read with
//! lookups in every direction — styles by id, numbering by level, images by
//! relationship — and a `document.xml` is a few megabytes at most.

use std::borrow::Cow;
use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::error::CommandError;

pub const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
pub const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
pub const V: &str = "urn:schemas-microsoft-com:vml";
pub const PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// ISO 29500 Strict namespaces, and the Transitional ones they mean.
const STRICT: [(&str, &str); 5] = [
    ("http://purl.oclc.org/ooxml/wordprocessingml/main", W),
    ("http://purl.oclc.org/ooxml/officeDocument/relationships", R),
    ("http://purl.oclc.org/ooxml/drawingml/main", A),
    ("http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing", WP),
    ("http://purl.oclc.org/ooxml/officeDocument/relationships/image", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"),
];

fn normalise(uri: &str) -> String {
    STRICT.iter().find(|(strict, _)| *strict == uri).map_or_else(|| uri.to_owned(), |(_, t)| (*t).to_owned())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub ns: String,
    pub name: String,
    /// `(namespace, local name) → value`; an unprefixed attribute has no namespace.
    pub attrs: Vec<(String, String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    #[must_use]
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }

    /// The value of attribute `name` in namespace `ns` (`""` for none).
    #[must_use]
    pub fn attr(&self, ns: &str, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, l, _)| n == ns && l == name).map(|(_, _, v)| v.as_str())
    }

    /// The child elements, in order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The first child element named `ns:name`.
    #[must_use]
    pub fn child(&self, ns: &str, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.is(ns, name))
    }

    /// The first element named `ns:name` anywhere below this one.
    #[must_use]
    pub fn descendant(&self, ns: &str, name: &str) -> Option<&Element> {
        self.elements().find_map(|e| if e.is(ns, name) { Some(e) } else { e.descendant(ns, name) })
    }

    /// Every element named `ns:name` below this one, document order.
    #[must_use]
    pub fn descendants(&self, ns: &str, name: &str) -> Vec<&Element> {
        let mut out = Vec::new();
        for e in self.elements() {
            if e.is(ns, name) {
                out.push(e);
            }
            out.extend(e.descendants(ns, name));
        }
        out
    }

    /// All text below this element, concatenated.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for node in &self.children {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => out.push_str(&e.text()),
            }
        }
        out
    }
}

/// Resolve an entity reference: the five XML predefines, or a character reference.
fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// The deepest element nesting [`parse`] accepts; the root element is level 1.
///
/// The tree is freed, searched and read by recursion, one stack frame or more
/// per level, on worker threads with a 2 MiB stack. Without a limit a file of
/// a few kilobytes, nested ten thousand deep, parses and then overflows the
/// stack when the tree is dropped, and a stack overflow closes the app: it is
/// not a panic and nothing can catch it. Limiting the depth here bounds every
/// one of those walks at once, where making `Drop` iterative would fix only
/// the drop.
///
/// 256 is libxml2's default for the same safety limit (`xmlParserMaxDepth`,
/// 2.12.0). The committed Word fixture peaks at 9 levels, and the app's own
/// Word exports at 12.
pub const MAX_DEPTH: usize = 256;

/// Refuse an element that would sit deeper than [`MAX_DEPTH`]. `open` counts the
/// synthetic document node plus every element still open, which is the level
/// the new element would take.
///
/// On refusal the open elements are a flat list, each holding only subtrees
/// that already closed within the limit, so dropping them is shallow too.
fn refuse_too_deep(open: usize) -> Result<(), CommandError> {
    if open > MAX_DEPTH {
        return Err(CommandError::InvalidInput(format!("the document's XML is nested too deeply (more than {MAX_DEPTH} levels)")));
    }
    Ok(())
}

/// Parse `xml` into its root element.
///
/// # Errors
/// When the XML is malformed — unbalanced, or an undeclared prefix — or
/// nested deeper than [`MAX_DEPTH`].
pub fn parse(xml: &str) -> Result<Element, CommandError> {
    let bad = |what: &dyn std::fmt::Display| CommandError::InvalidInput(format!("the document's XML is damaged: {what}"));
    let mut reader = Reader::from_str(xml);
    // Namespace scopes, innermost last: prefix ("" for the default) → URI.
    let mut scopes: Vec<HashMap<String, String>> = vec![HashMap::from([("xml".to_owned(), "http://www.w3.org/XML/1998/namespace".to_owned())])];
    let mut stack: Vec<Element> = vec![Element::default()];

    let open = |start: &BytesStart, scopes: &mut Vec<HashMap<String, String>>| -> Result<Element, CommandError> {
        let mut scope = HashMap::new();
        let mut raw = Vec::new();
        for attr in start.attributes().with_checks(false) {
            let attr = attr.map_err(|e| bad(&e))?;
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = attr.unescape_value().map_err(|e| bad(&e))?.into_owned();
            if key == "xmlns" {
                scope.insert(String::new(), value);
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                scope.insert(prefix.to_owned(), value);
            } else {
                raw.push((key, value));
            }
        }
        scopes.push(scope);
        let lookup = |prefix: &str, scopes: &[HashMap<String, String>]| -> Option<String> {
            scopes.iter().rev().find_map(|s| s.get(prefix)).map(|uri| normalise(uri))
        };
        let qname = String::from_utf8_lossy(start.name().as_ref()).into_owned();
        let (ns, name) = match qname.split_once(':') {
            Some((prefix, local)) => (lookup(prefix, scopes).ok_or_else(|| bad(&format!("undeclared prefix {prefix}")))?, local.to_owned()),
            None => (lookup("", scopes).unwrap_or_default(), qname),
        };
        let attrs = raw
            .into_iter()
            .map(|(key, value)| match key.split_once(':') {
                // An unknown attribute prefix is kept, unresolved, rather than failing the file.
                Some((prefix, local)) => (lookup(prefix, scopes).unwrap_or_else(|| prefix.to_owned()), local.to_owned(), value),
                None => (String::new(), key, value),
            })
            .collect();
        Ok(Element { ns, name, attrs, children: Vec::new() })
    };

    let push_text = |stack: &mut Vec<Element>, text: Cow<'_, str>| {
        if text.is_empty() {
            return;
        }
        if let Some(parent) = stack.last_mut() {
            if let Some(Node::Text(last)) = parent.children.last_mut() {
                last.push_str(&text);
            } else {
                parent.children.push(Node::Text(text.into_owned()));
            }
        }
    };

    loop {
        match reader.read_event().map_err(|e| bad(&e))? {
            Event::Start(start) => {
                refuse_too_deep(stack.len())?;
                let element = open(&start, &mut scopes)?;
                stack.push(element);
            }
            Event::Empty(start) => {
                refuse_too_deep(stack.len())?;
                let element = open(&start, &mut scopes)?;
                scopes.pop();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::Element(element));
                }
            }
            Event::End(_) => {
                scopes.pop();
                let element = stack.pop().ok_or_else(|| bad(&"an end tag with no start"))?;
                let parent = stack.last_mut().ok_or_else(|| bad(&"an end tag with no start"))?;
                parent.children.push(Node::Element(element));
            }
            Event::Text(text) => push_text(&mut stack, text.xml_content().map_err(|e| bad(&e))?),
            Event::CData(data) => push_text(&mut stack, data.decode().map_err(|e| bad(&e))?),
            Event::GeneralRef(reference) => {
                let name = String::from_utf8_lossy(reference.as_ref()).into_owned();
                let c = entity(&name).ok_or_else(|| bad(&format!("unknown entity &{name};")))?;
                push_text(&mut stack, Cow::Owned(c.to_string()));
            }
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
    let document = stack.pop().ok_or_else(|| bad(&"no document"))?;
    if !stack.is_empty() {
        return Err(bad(&"an element was never closed"));
    }
    document
        .children
        .into_iter()
        .find_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| bad(&"no root element"))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_through_prefixes_and_defaults() {
        let root = parse(&format!(
            r#"<?xml version="1.0"?><x:document xmlns:x="{W}"><x:body><p xmlns="{W}" x:val="1">a</p></x:body></x:document>"#
        ))
        .unwrap();
        assert!(root.is(W, "document"));
        let p = root.descendant(W, "p").expect("p in the default namespace");
        assert_eq!(p.attr(W, "val"), Some("1"));
        assert_eq!(p.text(), "a");
    }

    #[test]
    fn strict_namespaces_mean_the_transitional_ones() {
        let root = parse(r#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body/></w:document>"#).unwrap();
        assert!(root.is(W, "document") && root.child(W, "body").is_some());
    }

    #[test]
    fn entities_and_character_references_become_text() {
        let root = parse(r"<a>Revenue &amp; Costs &lt;5% &#8364; &#x201C;q&#x201D; <![CDATA[<raw>]]></a>").unwrap();
        assert_eq!(root.text(), "Revenue & Costs <5% € “q” <raw>");
        let root = parse(r#"<a v="&quot;x&quot; &amp; y"/>"#).unwrap();
        assert_eq!(root.attr("", "v"), Some("\"x\" & y"));
    }

    #[test]
    fn damaged_xml_is_an_error_not_a_partial_tree() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<w:a/>").is_err(), "undeclared prefix");
        assert!(parse("<a>&nonsense;</a>").is_err());
    }

    /// `depth` elements, each inside the last, around `inside`.
    fn nested(depth: usize, inside: &str) -> String {
        format!("{}{inside}{}", "<a>".repeat(depth), "</a>".repeat(depth))
    }

    /// How many levels the tree really has, counted without recursion so the
    /// count itself cannot be what overflows.
    fn levels(root: &Element) -> usize {
        let mut deepest = 0;
        let mut pending = vec![(root, 1usize)];
        while let Some((element, level)) = pending.pop() {
            deepest = deepest.max(level);
            pending.extend(element.elements().map(|child| (child, level + 1)));
        }
        deepest
    }

    fn refused_as_too_deep(xml: &str) -> bool {
        matches!(parse(xml), Err(CommandError::InvalidInput(message)) if message.contains("nested too deeply"))
    }

    // SPEC: P1-VIEW-013.
    #[test]
    fn nesting_at_the_limit_parses_and_one_past_it_is_refused() {
        let root = parse(&nested(MAX_DEPTH, "deepest")).expect("exactly the limit");
        assert_eq!(levels(&root), MAX_DEPTH);
        // The recursive readers reach the bottom of the deepest tree allowed.
        assert_eq!(root.text(), "deepest");
        assert_eq!(root.descendants("", "a").len(), MAX_DEPTH - 1);
        assert!(refused_as_too_deep(&nested(MAX_DEPTH + 1, "")), "one level more");

        // An empty element takes a level like any other.
        let root = parse(&nested(MAX_DEPTH - 1, "<b/>")).expect("an empty element on the last level");
        assert_eq!(levels(&root), MAX_DEPTH);
        assert!(root.descendant("", "b").is_some());
        assert!(refused_as_too_deep(&nested(MAX_DEPTH, "<b/>")), "an empty element one level too far");
    }

    // SPEC: P1-VIEW-013. Before the limit, the first of these parsed and then
    // overflowed the stack when the tree was dropped: a crashed test binary.
    #[test]
    fn nesting_far_past_the_limit_is_refused_closed_or_unclosed() {
        assert!(refused_as_too_deep(&nested(100_000, "")));
        assert!(refused_as_too_deep(&"<a>".repeat(100_000)), "never closed");
    }

    // SPEC: P1-VIEW-013 — the limit is on depth, not on size.
    #[test]
    fn a_wide_document_is_not_mistaken_for_a_deep_one() {
        let xml = format!("<r>{}</r>", "<a><b/></a>".repeat(100_000));
        let root = parse(&xml).expect("wide, three levels deep");
        assert_eq!(root.elements().count(), 100_000);
        assert_eq!(levels(&root), 3);
    }
}
