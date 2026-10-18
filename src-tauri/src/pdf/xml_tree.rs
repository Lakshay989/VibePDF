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

/// Parse `xml` into its root element.
///
/// # Errors
/// When the XML is malformed — unbalanced, or an undeclared prefix.
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
                let element = open(&start, &mut scopes)?;
                stack.push(element);
            }
            Event::Empty(start) => {
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
}
