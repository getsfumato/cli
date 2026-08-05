use super::*;

#[test]
fn the_svg_namespace_is_not_a_remote_reference() {
    // `document.createElementNS("http://www.w3.org/2000/svg", "svg")` is the only way
    // the DOM makes an SVG element, so rejecting this forbids scripted SVG entirely.
    assert!(is_xml_namespace("http://www.w3.org/2000/svg"));
    assert!(is_xml_namespace("http://www.w3.org/1999/xlink"));
    assert!(is_xml_namespace("http://www.w3.org/1999/xhtml"));
    assert!(is_xml_namespace("http://www.w3.org/1998/Math/MathML"));
    // `setAttributeNS` needs this one, and `xml:lang` needs the next.
    assert!(is_xml_namespace("http://www.w3.org/2000/xmlns/"));
    assert!(is_xml_namespace("http://www.w3.org/XML/1998/namespace"));
}

#[test]
fn the_https_spelling_of_a_namespace_is_still_a_namespace() {
    // Not what a browser matches on, but still an identifier rather than a fetch.
    assert!(is_xml_namespace("https://www.w3.org/2000/svg"));
}

#[test]
fn a_real_remote_url_is_not_mistaken_for_a_namespace() {
    assert!(!is_xml_namespace("https://cdn.jsdelivr.net/npm/chart.js"));
    // A host that merely starts with the same domain must not pass: the allowance is
    // for the namespace paths, not for w3.org.
    assert!(!is_xml_namespace("http://www.w3.org/TR/SVG11/"));
    assert!(!is_xml_namespace("http://www.w3.org.evil.example/2000/svg"));
}
