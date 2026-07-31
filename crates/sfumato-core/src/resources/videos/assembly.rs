//! Deterministic assembly of a video's master composition.
//!
//! The entry composition is generated rather than authored. Its content is
//! entirely mechanical — the canvas, one mount per planned scene, and a timeline
//! that runs for the requested duration — and every part of it is a contract the
//! renderer enforces. Asking a model to reproduce that contract on top of doing
//! the creative work is where authoring failures came from, and it grew with the
//! film: one response had to carry the whole master timeline plus every scene.

use sfumato_domain::VideoPlanDocument;

/// The mount identifier for one scene's composition.
///
/// Distinct from the scene's own composition ID because the host element and the
/// composition it loads are two different elements, and the renderer requires an
/// ID on each.
pub(super) fn scene_host_id(scene_id: &str) -> String {
    format!("mount-{scene_id}")
}

/// The path a scene's authored composition lives at.
pub(super) fn scene_composition_path(scene_id: &str) -> String {
    format!("compositions/{scene_id}.html")
}

/// Builds the entry composition that mounts every planned scene.
///
/// The mounting contract is measured against the renderer, not assumed: the host
/// carries `data-composition-id` alongside `data-composition-src`, and the file it
/// loads carries its own composition root. Dropping either makes the renderer
/// report a missing sub-composition.
pub(super) fn master_index_html(plan: &VideoPlanDocument, width: u32, height: u32) -> String {
    let duration = plan.duration_seconds();
    let mut mounts = String::new();
    for (index, scene) in plan.scenes().iter().enumerate() {
        mounts.push_str(&format!(
            "    <div id=\"{host}\" class=\"clip\" data-composition-id=\"{host}\" data-composition-src=\"{path}\" data-start=\"{start}\" data-duration=\"{duration}\" data-track-index=\"{track}\"></div>\n",
            host = scene_host_id(&scene.id),
            path = scene_composition_path(&scene.id),
            start = format_seconds(scene.start_seconds),
            duration = format_seconds(scene.duration_seconds),
            track = index,
        ));
    }
    format!(
        "<!DOCTYPE html>\n<html>\n<head><meta charset=\"UTF-8\">\n<style>\n  html, body {{ margin: 0; padding: 0; width: {width}px; height: {height}px; overflow: hidden; }}\n  #root {{ position: relative; width: {width}px; height: {height}px; }}\n  #root > .clip {{ position: absolute; inset: 0; }}\n</style>\n</head>\n<body>\n  <div id=\"root\" data-composition-id=\"root\" data-start=\"0\" data-width=\"{width}\" data-height=\"{height}\">\n{mounts}    <script src=\"./vendor/gsap.min.js\"></script>\n    <script>\n      const tl = gsap.timeline({{ paused: true }});\n      tl.set({{}}, {{}}, {duration});\n      window.__timelines = window.__timelines || {{}};\n      window.__timelines[\"root\"] = tl;\n    </script>\n  </div>\n</body>\n</html>\n"
    )
}

/// Renders a timeline position without a trailing `.0`, which reads as noise in
/// an attribute the renderer parses as a number either way.
fn format_seconds(value: f32) -> String {
    if (value.fract()).abs() < f32::EPSILON {
        format!("{}", value.trunc() as i64)
    } else {
        format!("{value}")
    }
}

/// The project manifest the renderer reads.
pub(super) fn master_meta_json(slug: &str) -> String {
    format!("{{\n  \"name\": \"{slug}\"\n}}\n")
}

/// Removes an outer code fence a model may wrap markup in.
pub(super) fn strip_markup_fence(value: &str) -> String {
    let trimmed = value.trim();
    for opener in ["```html", "```HTML", "```"] {
        if let Some(rest) = trimmed.strip_prefix(opener)
            && let Some(inner) = rest.trim_start_matches(['\n', '\r']).strip_suffix("```")
        {
            return inner.trim_end().to_owned();
        }
    }
    trimmed.to_owned()
}

/// Rejects an authored scene composition that the renderer could not mount.
///
/// Measured contract: the file needs `<template>` content holding an element with
/// its own `data-composition-id` and dimensions. Without those the renderer
/// reports the sub-composition as missing or empty and the film renders without
/// that scene.
pub(super) fn validate_scene_composition(
    scene_id: &str,
    markup: &str,
) -> Result<(), String> {
    let lowercase = markup.to_ascii_lowercase();
    if !lowercase.contains("<template") {
        return Err(format!(
            "scene `{scene_id}` must wrap its content in `<template>`; the renderer mounts nothing otherwise"
        ));
    }
    let expected = format!("data-composition-id=\"{scene_id}\"");
    if !markup.contains(&expected) {
        return Err(format!(
            "scene `{scene_id}` must carry a root element with {expected}"
        ));
    }
    for attribute in ["data-width", "data-height"] {
        if !markup.contains(attribute) {
            return Err(format!("scene `{scene_id}` is missing {attribute}"));
        }
    }
    if !markup.contains("window.__timelines") {
        return Err(format!(
            "scene `{scene_id}` must register its own paused timeline on `window.__timelines`"
        ));
    }
    if let Some(family) = unresolvable_font(markup) {
        return Err(format!(
            "scene `{scene_id}` names the font family `{family}`, which the renderer cannot supply. \
             Every family in a stack is resolved, fallbacks included, so name only one of {} or a \
             generic keyword such as sans-serif or monospace",
            BUNDLED_FONTS.join(", ")
        ));
    }
    Ok(())
}

/// Font families the renderer ships, which it can render without an `@font-face`
/// rule.
///
/// Read off the renderer's own alias table rather than guessed: naming anything it
/// cannot resolve fails `hyperframes check` with `font_family_without_font_face`.
/// Real films failed on exactly that — a model wrote `JetBrains Mono, Fira Code,
/// monospace`, and the unbundled *fallback* alone was enough to fail the render.
const BUNDLED_FONTS: [&str; 18] = [
    "inter",
    "montserrat",
    "outfit",
    "nunito",
    "oswald",
    "league gothic",
    "archivo black",
    "space mono",
    "ibm plex mono",
    "jetbrains mono",
    "eb garamond",
    "playfair display",
    "source code pro",
    "noto sans jp",
    "roboto",
    "open sans",
    "lato",
    "poppins",
];

/// System font names the renderer substitutes with a bundled family.
///
/// Accepted rather than rejected because the renderer resolves them and reports
/// only that it swapped one in; themes legitimately ship stacks like
/// `Inter, Arial, sans-serif`, and rejecting those would fight the theme.
const SUBSTITUTED_FONTS: [&str; 51] = [
    "helvetica neue",
    "helvetica",
    "arial",
    "helvetica bold",
    "futura",
    "din alternate",
    "arial black",
    "bebas neue",
    "courier new",
    "courier",
    "garamond",
    "noto sans japanese",
    "segoe ui",
    "sf pro",
    "sf pro display",
    "sf pro text",
    "sf pro rounded",
    "avenir",
    "avenir next",
    "lucida grande",
    "geneva",
    "optima",
    "verdana",
    "tahoma",
    "trebuchet ms",
    "calibri",
    "candara",
    "corbel",
    "lucida sans",
    "lucida sans unicode",
    "noto sans",
    "dejavu sans",
    "liberation sans",
    "sf mono",
    "menlo",
    "monaco",
    "consolas",
    "lucida console",
    "lucida sans typewriter",
    "andale mono",
    "dejavu sans mono",
    "liberation mono",
    "georgia",
    "palatino",
    "palatino linotype",
    "book antiqua",
    "cambria",
    "times",
    "times new roman",
    "dejavu serif",
    "liberation serif",
];

/// CSS-defined family keywords, which name no concrete font to load.
const GENERIC_FAMILIES: [&str; 12] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
];

/// The first font family in the markup that the renderer cannot resolve.
///
/// Reads both the `font-family` property and the `font` shorthand, and both the
/// CSS and SVG attribute spellings, because authored scenes use all of them.
fn unresolvable_font(markup: &str) -> Option<String> {
    let lowercase = markup.to_ascii_lowercase();
    let mut rest = lowercase.as_str();
    while let Some(offset) = rest.find("font") {
        rest = &rest[offset..];
        let (declaration, after) = match declaration_value(rest) {
            Some(parts) => parts,
            None => {
                rest = &rest["font".len()..];
                continue;
            }
        };
        rest = after;
        for family in families(declaration) {
            // A custom property names no font: whatever it resolves to is declared
            // elsewhere and gets checked there. Rejecting it would fight the same
            // prompt that asks authors to theme through semantic variables.
            let known = family.starts_with("var(")
                || BUNDLED_FONTS.contains(&family.as_str())
                || SUBSTITUTED_FONTS.contains(&family.as_str())
                || GENERIC_FAMILIES.contains(&family.as_str());
            if !known {
                return Some(family);
            }
        }
    }
    None
}

/// Splits a `font` or `font-family` declaration into its value and the remainder.
///
/// Returns `None` when the match is some other property that merely starts with
/// `font`, such as `font-size`, which carries no family name.
fn declaration_value(input: &str) -> Option<(&str, &str)> {
    let head = if let Some(rest) = input.strip_prefix("font-family") {
        rest
    } else {
        input.strip_prefix("font")?
    };
    let head = head.trim_start();
    // Which spelling this is decides where the value ends, and quotes mean opposite
    // things in the two: the attribute form quotes the whole list, while the CSS
    // form quotes individual family names inside it.
    let attribute = head.starts_with('=');
    let value = head.strip_prefix([':', '='])?.trim_start();
    if attribute {
        let quote = value.chars().next().filter(|value| "\"'".contains(*value));
        let (value, terminators) = match quote {
            Some(quote) => (&value[quote.len_utf8()..], vec![quote]),
            None => (value, vec![' ', '>', '/']),
        };
        let end = terminators
            .iter()
            .filter_map(|terminator| value.find(*terminator))
            .min()
            .unwrap_or(value.len());
        return Some((&value[..end], &value[end..]));
    }
    let end = [';', '}', '>']
        .iter()
        .filter_map(|delimiter| value.find(*delimiter))
        .min()
        .unwrap_or(value.len());
    Some((&value[..end], &value[end..]))
}

/// The family names a declaration value lists, in order.
///
/// The `font` shorthand puts size, weight, and line height before the families,
/// so leading tokens that are not part of a name are dropped from the first entry.
fn families(value: &str) -> Vec<String> {
    split_top_level(value)
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let entry = entry.trim().trim_matches(['"', '\'']).trim();
            let name = if index == 0 {
                strip_shorthand_prefix(entry)
            } else {
                entry
            };
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

/// Splits a comma-separated list, ignoring commas nested in parentheses.
///
/// `var(--font-body, sans-serif)` is one entry, not two: a plain split turned its
/// fallback into the bogus family name `sans-serif)` and rejected a valid scene.
fn split_top_level(value: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                entries.push(&value[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    entries.push(&value[start..]);
    entries
}

/// Drops the non-family tokens the `font` shorthand puts in front of the first
/// family, such as `500 25px/1`.
fn strip_shorthand_prefix(entry: &str) -> &str {
    const MODIFIERS: [&str; 12] = [
        "normal",
        "italic",
        "oblique",
        "bold",
        "bolder",
        "lighter",
        "small-caps",
        "condensed",
        "expanded",
        "caption",
        "icon",
        "menu",
    ];
    let mut rest = entry;
    loop {
        let Some((token, tail)) = rest.split_once(char::is_whitespace) else {
            return rest;
        };
        let numeric = token.starts_with(|value: char| value.is_ascii_digit())
            || token.contains('/')
            || token.ends_with('%');
        if numeric || MODIFIERS.contains(&token) {
            rest = tail.trim_start();
        } else {
            return rest;
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/resources_videos_assembly.rs"]
mod tests;
