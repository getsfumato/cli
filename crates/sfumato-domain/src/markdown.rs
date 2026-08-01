//! Format-agnostic Markdown scanning shared by structured documents.
//!
//! A deck and a sectioned document describe different media, so they keep their
//! own element vocabularies and their own splitting rules. What they genuinely
//! share is how Markdown itself is read: where a fenced block opens and closes,
//! where a top-level `---` sits, which images are referenced, and how a stable
//! revision is derived from text. Those live here so the two formats cannot
//! disagree about the source they are both parsing.

use crate::RevisionId;

/// Byte range of one top-level `---` line, including its newline.
#[derive(Clone, Copy)]
pub(crate) struct Fence {
    /// Offset of the first character of the line.
    pub(crate) start: usize,
    /// Offset just past the line's newline.
    pub(crate) end: usize,
}

/// Recognizes a fenced-code delimiter, returning its marker and run length.
pub(crate) fn code_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = line.chars().take_while(|value| *value == marker).count();
    (length >= 3).then_some((marker, length))
}

/// Locates every top-level `---` line, ignoring separators inside fenced code.
pub(crate) fn fences(markdown: &str) -> Vec<Fence> {
    let mut located = Vec::new();
    let mut cursor = 0;
    let mut open: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let without_newline = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = without_newline.trim_start();
        if let Some((marker, length)) = code_fence(trimmed) {
            match open {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    open = None;
                }
                None => open = Some((marker, length)),
                _ => {}
            }
        } else if open.is_none() && without_newline.trim() == "---" {
            located.push(Fence {
                start: cursor,
                end: cursor + line.len(),
            });
        }
        cursor += line.len();
    }
    located
}

/// Collects every Markdown image source exactly as written.
pub(crate) fn image_sources(markdown: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut remainder = markdown;
    while let Some(image_start) = remainder.find("![") {
        remainder = &remainder[image_start + 2..];
        let Some(label_end) = remainder.find("](") else {
            break;
        };
        remainder = &remainder[label_end + 2..];
        let Some(source_end) = remainder.find(')') else {
            break;
        };
        let source = remainder[..source_end].trim();
        if !source.is_empty() {
            sources.push(source.to_owned());
        }
        remainder = &remainder[source_end + 1..];
    }
    sources
}

/// Returns the first ATX heading outside fenced code, with its level.
pub(crate) fn first_heading_with_level(markdown: &str) -> Option<(u8, String)> {
    let mut open: Option<(char, usize)> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, length)) = code_fence(trimmed) {
            match open {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    open = None;
                }
                None => open = Some((marker, length)),
                _ => {}
            }
        } else if open.is_none() {
            let heading = trimmed.trim_start_matches('#');
            let level = trimmed.len() - heading.len();
            if level > 0 && level <= 6 && heading.starts_with(' ') {
                return Some((
                    u8::try_from(level).expect("level is between 1 and 6"),
                    clean_heading(heading),
                ));
            }
        }
    }
    None
}

/// Returns the first heading text outside fenced code.
pub(crate) fn first_heading(markdown: &str) -> Option<String> {
    first_heading_with_level(markdown).map(|(_, heading)| heading)
}

/// Strips trailing marks and simple emphasis from one heading's text.
pub(crate) fn clean_heading(heading: &str) -> String {
    let mut heading = heading.trim().trim_end_matches('#').trim();
    loop {
        let mut changed = false;
        for delimiter in ["**", "__", "`", "*", "_"] {
            if let Some(inner) = heading
                .strip_prefix(delimiter)
                .and_then(|value| value.strip_suffix(delimiter))
            {
                heading = inner.trim();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    heading.to_owned()
}

/// Rejects an unclosed fenced code block, naming the node that carries it.
pub(crate) fn validate_fenced_code_blocks(markdown: &str, node: &str) -> Result<(), String> {
    let mut open: Option<(char, usize, String)> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let Some((marker, length)) = code_fence(trimmed) else {
            continue;
        };
        match &open {
            Some((open_marker, open_length, _))
                if marker == *open_marker && length >= *open_length =>
            {
                open = None;
            }
            None => open = Some((marker, length, trimmed[length..].trim().to_owned())),
            _ => {}
        }
    }
    if let Some((_, _, language)) = open {
        let kind = if language.is_empty() {
            "code".to_owned()
        } else {
            language
        };
        return Err(format!(
            "`{node}` has an unclosed `{kind}` fenced code block"
        ));
    }
    Ok(())
}

/// Derives a stable revision token from text.
pub(crate) fn revision_for(value: &str) -> RevisionId {
    // Stable FNV-1a is sufficient for optimistic concurrency; this is not a
    // cryptographic content-addressing boundary.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    RevisionId::new(format!("{hash:016x}")).expect("hex revision is a valid domain token")
}

/// Validates a stable lowercase-hyphen node identifier.
pub(crate) fn validate_node_id(id: &str, label: &str) -> Result<(), String> {
    if id.is_empty()
        || !id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || id.starts_with('-')
        || id.ends_with('-')
    {
        return Err(format!(
            "invalid {label} ID `{id}`; use lowercase letters, numbers, and hyphens"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/markdown.rs"]
mod tests;
