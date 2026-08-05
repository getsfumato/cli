//! Facts about the web platform that generated-output validation depends on.
//!
//! Shared rather than restated per pipeline: the video validator already knew that an
//! `xmlns` is not a fetch, and the page validator did not, so a page that built SVG the
//! only way the DOM allows was rejected for referencing a remote URL.

/// Namespace URLs that are declarations, not fetches.
///
/// An `xmlns` never causes a request, and `document.createElementNS` cannot create an
/// SVG element without one — so a validator that treats these as remote references
/// forbids scripted SVG entirely.
///
/// Spelled in lowercase because every comparison against them lowercases first; the
/// canonical MathML namespace is mixed case.
pub const XML_NAMESPACES: &[&str] = &[
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1999/xlink",
    "http://www.w3.org/1999/xhtml",
    "http://www.w3.org/1998/math/mathml",
    // Used by `setAttributeNS` to declare a namespace, and by `xml:` attributes. Found
    // by a test that wrote SVG the way the DOM actually requires.
    "http://www.w3.org/2000/xmlns/",
    "http://www.w3.org/xml/1998/namespace",
];

/// Whether `value` begins one of the namespace declarations above.
///
/// Both schemes are accepted: `https` is not the spelling a browser matches on, but it
/// is still a namespace identifier rather than something that gets fetched.
pub fn is_xml_namespace(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    XML_NAMESPACES.iter().any(|namespace| {
        lowercase.starts_with(namespace)
            || namespace
                .strip_prefix("http://")
                .is_some_and(|rest| lowercase.starts_with(&format!("https://{rest}")))
    })
}

#[cfg(test)]
#[path = "../tests/unit/web.rs"]
mod tests;
