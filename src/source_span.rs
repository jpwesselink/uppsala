//! Source byte ranges for attributes.
//!
//! [`Document::node_range`] gives the byte range of a whole node. This module
//! adds the ranges of the individual attributes inside an element's start
//! tag: the attribute name as written and the attribute value between its
//! quotes, both as byte offsets into [`Document::input_text`].
//!
//! The ranges are recovered lazily by re-scanning the element's start tag in
//! the original input, so parsing pays nothing for them. The input already
//! parsed as well-formed XML, so the scan only has to find the delimiters
//! (`=`, the quote characters, `>` and `/>`); it never has to diagnose errors.
//!
//! The value range is the *raw* source text: entity and character references
//! are not expanded and attribute-value normalization is not applied. It is
//! the range an editor replaces to change the value in place.

use std::ops::Range;

use crate::dom::{Document, NodeId, NodeKind, QName};

/// The source position of one attribute (or namespace declaration) as written
/// in an element's start tag.
///
/// All ranges are byte offsets into [`Document::input_text`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeSpan {
    /// The attribute's qualified name as written (e.g. `xsi:type`).
    pub name: Range<usize>,
    /// The raw attribute value, without the surrounding quotes. Entity and
    /// character references are left unexpanded.
    pub value: Range<usize>,
    /// The quote character that delimits the value: `b'"'` or `b'\''`.
    pub quote: u8,
}

impl AttributeSpan {
    /// The whole attribute, from the first byte of the name through the
    /// closing quote.
    pub fn full(&self) -> Range<usize> {
        self.name.start..self.value.end + 1
    }

    /// Whether this span is a namespace declaration (`xmlns` or `xmlns:*`)
    /// rather than an attribute, judged from the name text in `input`.
    pub fn is_namespace_declaration(&self, input: &str) -> bool {
        let name = &input[self.name.clone()];
        name == "xmlns" || name.starts_with("xmlns:")
    }
}

#[inline]
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Scan the start tag that begins at `start` (which must be a `<`) and return
/// the spans of every attribute and namespace declaration in source order.
/// `limit` bounds the scan (the end of the element's range). Returns `None`
/// if the bytes are not a start tag; that only happens for nodes whose range
/// no longer matches the input.
fn scan_start_tag(input: &str, start: usize, limit: usize) -> Option<Vec<AttributeSpan>> {
    let bytes = input.as_bytes();
    let limit = limit.min(bytes.len());
    let mut i = start;
    if bytes.get(i) != Some(&b'<') {
        return None;
    }
    i += 1;
    // Element name.
    while i < limit && !is_ws(bytes[i]) && bytes[i] != b'>' && bytes[i] != b'/' {
        i += 1;
    }
    let mut spans = Vec::new();
    loop {
        while i < limit && is_ws(bytes[i]) {
            i += 1;
        }
        match bytes.get(i) {
            _ if i >= limit => return None,
            Some(b'>') | Some(b'/') => return Some(spans),
            _ => {}
        }
        let name_start = i;
        while i < limit && !is_ws(bytes[i]) && bytes[i] != b'=' {
            i += 1;
        }
        let name_end = i;
        while i < limit && is_ws(bytes[i]) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') || i >= limit {
            return None;
        }
        i += 1;
        while i < limit && is_ws(bytes[i]) {
            i += 1;
        }
        let quote = *bytes.get(i)?;
        if i >= limit || (quote != b'"' && quote != b'\'') {
            return None;
        }
        i += 1;
        let value_start = i;
        let rel = bytes[value_start..limit].iter().position(|&b| b == quote)?;
        let value_end = value_start + rel;
        i = value_end + 1;
        if name_start == name_end {
            return None;
        }
        spans.push(AttributeSpan {
            name: name_start..name_end,
            value: value_start..value_end,
            quote,
        });
    }
}

impl<'a> Document<'a> {
    /// The source spans of every attribute and namespace declaration in the
    /// start tag of element `id`, in source order.
    ///
    /// Returns `None` if `id` is not an element parsed from source (built or
    /// invalid nodes have no source position).
    ///
    /// The spans describe the input the document was parsed from; they are
    /// not updated by DOM mutations.
    ///
    /// # Example
    /// ```
    /// let xml = r#"<a x="1" xmlns:p="urn:p" p:y='two'/>"#;
    /// let doc = uppsala::parse(xml).unwrap();
    /// let a = doc.document_element().unwrap();
    /// let spans = doc.attribute_spans(a).unwrap();
    /// let names: Vec<&str> = spans.iter().map(|s| &xml[s.name.clone()]).collect();
    /// assert_eq!(names, ["x", "xmlns:p", "p:y"]);
    /// assert_eq!(&xml[spans[2].value.clone()], "two");
    /// ```
    pub fn attribute_spans(&self, id: NodeId) -> Option<Vec<AttributeSpan>> {
        let node = self.nodes.get(id.0)?;
        if !matches!(node.kind, NodeKind::Element(_)) {
            return None;
        }
        let range = self.node_range(id)?;
        scan_start_tag(self.input, range.start, range.end)
    }

    /// The source span of the attribute `name` (matched on namespace URI and
    /// local name, as [`QName::matches`] does) on element `id`.
    ///
    /// Namespace declarations are never matched here; use
    /// [`Document::namespace_declaration_span`] for those. Returns `None`
    /// when the element has no such attribute in the source (including
    /// attributes added after parsing or defaulted from a DTD).
    pub fn attribute_span(&self, id: NodeId, name: &QName<'_>) -> Option<AttributeSpan> {
        let elem = self.element(id)?;
        let spans = self.attribute_spans(id)?;
        // The parser stores attributes in source order with the namespace
        // declarations split out, so the n-th non-xmlns span is the n-th
        // attribute. Check the written name against each pairing so a
        // mutated element can never yield the wrong span.
        let source_attrs = spans
            .into_iter()
            .filter(|s| !s.is_namespace_declaration(self.input));
        for (span, attr) in source_attrs.zip(elem.attributes.iter()) {
            if self.input[span.name.clone()] != *attr.name.prefixed_name() {
                return None;
            }
            if attr
                .name
                .matches(name.namespace_uri.as_deref(), &name.local_name)
            {
                return Some(span);
            }
        }
        None
    }

    /// The byte range of the raw value of attribute `name` on element `id`,
    /// without the quotes. Shorthand for
    /// `attribute_span(id, name).map(|s| s.value)`.
    ///
    /// # Example
    /// ```
    /// use uppsala::QName;
    /// let xml = "<a id=\"x &amp; y\"/>";
    /// let doc = uppsala::parse(xml).unwrap();
    /// let a = doc.document_element().unwrap();
    /// let r = doc.attribute_range(a, &QName::local("id")).unwrap();
    /// assert_eq!(&xml[r], "x &amp; y");
    /// assert_eq!(doc.get_attribute(a, "id"), Some("x & y"));
    /// ```
    pub fn attribute_range(&self, id: NodeId, name: &QName<'_>) -> Option<Range<usize>> {
        self.attribute_span(id, name).map(|s| s.value)
    }

    /// The source span of the namespace declaration for `prefix` on element
    /// `id` (`""` for the default namespace, `xmlns="…"`).
    pub fn namespace_declaration_span(&self, id: NodeId, prefix: &str) -> Option<AttributeSpan> {
        self.attribute_spans(id)?.into_iter().find(|s| {
            let n = &self.input[s.name.clone()];
            if prefix.is_empty() {
                n == "xmlns"
            } else {
                n.strip_prefix("xmlns:") == Some(prefix)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::dom::QName;

    fn value<'x>(xml: &'x str, q: QName<'_>) -> &'x str {
        let doc = crate::parse(xml).unwrap();
        let root = doc.document_element().unwrap();
        let r = doc.attribute_range(root, &q).unwrap();
        &xml[r]
    }

    #[test]
    fn scans_only_the_start_tag() {
        // A '>' in an attribute value and attribute-like text in content.
        let xml = r#"<a b="x>y" c='1'>d="no"</a>"#;
        let doc = crate::parse(xml).unwrap();
        let a = doc.document_element().unwrap();
        let spans = doc.attribute_spans(a).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(&xml[spans[0].value.clone()], "x>y");
        assert_eq!(&xml[spans[1].full()], "c='1'");
    }

    #[test]
    fn empty_value() {
        assert_eq!(value(r#"<a b=""/>"#, QName::local("b")), "");
    }

    #[test]
    fn text_nodes_have_no_spans() {
        let doc = crate::parse("<a>t</a>").unwrap();
        let a = doc.document_element().unwrap();
        let t = doc.first_child(a).unwrap();
        assert!(doc.attribute_spans(t).is_none());
    }
}
