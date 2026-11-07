//! Tests for the attribute source-span API: `Document::attribute_spans`,
//! `attribute_span`, `attribute_range` and `namespace_declaration_span`.

mod common;
use common::parse;
use uppsala::QName;

fn root_range(xml: &str, q: QName<'_>) -> Option<std::ops::Range<usize>> {
    let doc = parse(xml).unwrap();
    let root = doc.document_element().unwrap();
    doc.attribute_range(root, &q)
}

#[test]
fn double_and_single_quotes() {
    let xml = r#"<a d="dq" s='sq' mixed1="it's" mixed2='say "hi"'/>"#;
    let doc = parse(xml).unwrap();
    let a = doc.document_element().unwrap();
    let get = |n: &str| &xml[doc.attribute_range(a, &QName::local(n)).unwrap()];
    assert_eq!(get("d"), "dq");
    assert_eq!(get("s"), "sq");
    assert_eq!(get("mixed1"), "it's");
    assert_eq!(get("mixed2"), r#"say "hi""#);
    let spans = doc.attribute_spans(a).unwrap();
    assert_eq!(spans[0].quote, b'"');
    assert_eq!(spans[1].quote, b'\'');
    assert_eq!(&xml[spans[1].full()], "s='sq'");
}

#[test]
fn entity_and_character_references_stay_raw() {
    let xml = r#"<a v="x &amp; &lt;y&gt; &#65;&#x42; &quot;q&quot;" w='&apos;'/>"#;
    let doc = parse(xml).unwrap();
    let a = doc.document_element().unwrap();
    let r = doc.attribute_range(a, &QName::local("v")).unwrap();
    assert_eq!(&xml[r], r#"x &amp; &lt;y&gt; &#65;&#x42; &quot;q&quot;"#);
    // The DOM still holds the expanded value.
    assert_eq!(doc.get_attribute(a, "v"), Some(r#"x & <y> AB "q""#));
    let r = doc.attribute_range(a, &QName::local("w")).unwrap();
    assert_eq!(&xml[r], "&apos;");
}

#[test]
fn internal_entity_reference_stays_raw() {
    let xml = "<!DOCTYPE a [<!ENTITY e \"expanded\">]><a v=\"pre &e; post\"/>";
    let r = root_range(xml, QName::local("v")).unwrap();
    assert_eq!(&xml[r], "pre &e; post");
}

#[test]
fn multi_line_attributes_and_whitespace_around_equals() {
    let xml = "<a\n  first = \"line1\nline2\"\r\n\tsecond\n=\n'x\ty'\n/>";
    let doc = parse(xml).unwrap();
    let a = doc.document_element().unwrap();
    let r = doc.attribute_range(a, &QName::local("first")).unwrap();
    assert_eq!(&xml[r.clone()], "line1\nline2");
    // uppsala 0.12 does not apply attribute-value normalization (XML 1.0
    // section 3.3.3), so the DOM value equals the raw text here.
    assert_eq!(doc.get_attribute(a, "first"), Some("line1\nline2"));
    let s = doc.attribute_span(a, &QName::local("second")).unwrap();
    assert_eq!(&xml[s.name.clone()], "second");
    assert_eq!(&xml[s.value.clone()], "x\ty");
    assert_eq!(&xml[s.full()], "second\n=\n'x\ty'");
}

#[test]
fn utf8_values_and_names() {
    let xml = "<a naïve=\"café ☕\" b='日本'/>";
    assert_eq!(
        &xml[root_range(xml, QName::local("naïve")).unwrap()],
        "café ☕"
    );
    assert_eq!(&xml[root_range(xml, QName::local("b")).unwrap()], "日本");
}

#[test]
fn greater_than_and_slash_inside_values() {
    let xml = r#"<a x="1>0" y='a/b' z="/>"><b/></a>"#;
    let doc = parse(xml).unwrap();
    let a = doc.document_element().unwrap();
    let spans = doc.attribute_spans(a).unwrap();
    let vals: Vec<&str> = spans.iter().map(|s| &xml[s.value.clone()]).collect();
    assert_eq!(vals, ["1>0", "a/b", "/>"]);
}

#[test]
fn namespaced_attributes_match_on_uri_not_prefix() {
    let xml = r#"<r xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:p="urn:p"
        xsi:type="T" type="plain" p:type="pp"/>"#;
    let doc = parse(xml).unwrap();
    let r = doc.document_element().unwrap();
    let xsi = QName::with_namespace("http://www.w3.org/2001/XMLSchema-instance", "type");
    assert_eq!(&xml[doc.attribute_range(r, &xsi).unwrap()], "T");
    assert_eq!(
        &xml[doc.attribute_range(r, &QName::local("type")).unwrap()],
        "plain"
    );
    let p = QName::with_namespace("urn:p", "type");
    assert_eq!(&xml[doc.attribute_range(r, &p).unwrap()], "pp");
    // Same local name in another namespace does not match.
    assert!(doc
        .attribute_range(r, &QName::with_namespace("urn:other", "type"))
        .is_none());
}

#[test]
fn namespace_declarations_are_spanned_but_not_matched_as_attributes() {
    let xml = r#"<r xmlns="urn:d" xmlns:x="urn:x" a="1"/>"#;
    let doc = parse(xml).unwrap();
    let r = doc.document_element().unwrap();
    let spans = doc.attribute_spans(r).unwrap();
    assert_eq!(spans.len(), 3);
    assert!(spans[0].is_namespace_declaration(xml));
    assert!(!spans[2].is_namespace_declaration(xml));
    let d = doc.namespace_declaration_span(r, "").unwrap();
    assert_eq!(&xml[d.value], "urn:d");
    let x = doc.namespace_declaration_span(r, "x").unwrap();
    assert_eq!(&xml[x.value], "urn:x");
    assert!(doc.attribute_range(r, &QName::local("xmlns")).is_none());
    assert_eq!(
        &xml[doc.attribute_range(r, &QName::local("a")).unwrap()],
        "1"
    );
}

#[test]
fn nested_elements_and_missing_attribute() {
    let xml = r#"<r a="outer"><c a="inner"/><d/></r>"#;
    let doc = parse(xml).unwrap();
    let r = doc.document_element().unwrap();
    let kids = doc.children(r);
    assert_eq!(
        &xml[doc.attribute_range(r, &QName::local("a")).unwrap()],
        "outer"
    );
    assert_eq!(
        &xml[doc.attribute_range(kids[0], &QName::local("a")).unwrap()],
        "inner"
    );
    assert!(doc.attribute_range(kids[1], &QName::local("a")).is_none());
    assert_eq!(doc.attribute_spans(kids[1]).unwrap(), vec![]);
}

#[test]
fn spans_agree_with_dom_values_after_decoding() {
    // Every attribute value in a sample document: expanding the predefined
    // entities and character references in the raw slice gives the DOM
    // value (uppsala 0.12 applies no whitespace normalization).
    let xml = "<r a='1 &amp; 2' b=\"\tx\r\ny\" c='&#x9;'><k z=\"&lt;&gt;\"/></r>";
    let doc = parse(xml).unwrap();
    for id in doc.descendants(doc.root()) {
        let Some(elem) = doc.element(id) else {
            continue;
        };
        for attr in &elem.attributes {
            let raw = &xml[doc.attribute_range(id, &attr.name).unwrap()];
            let decoded = raw
                .replace("&#x9;", "\t")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&");
            assert_eq!(decoded, attr.value, "attribute {}", attr.name);
        }
    }
}

#[test]
fn built_nodes_and_mutated_elements_do_not_lie() {
    let xml = r#"<r a="1" b="2"/>"#;
    let mut doc = parse(xml).unwrap();
    let r = doc.document_element().unwrap();
    // Remove the first attribute: the source still has it, the DOM does not.
    doc.element_mut(r).unwrap().remove_attribute("a");
    // `b` is now attribute #0 in the DOM but the source pairing check fails,
    // so no wrong span is returned.
    assert!(doc.attribute_range(r, &QName::local("b")).is_none());
    // Created nodes have no source.
    let n = doc.create_element(QName::local("n"));
    assert!(doc.attribute_spans(n).is_none());
}
