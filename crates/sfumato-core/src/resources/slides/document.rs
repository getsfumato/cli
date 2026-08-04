//! Marp document normalization, validation, titles, and path invariants.

use super::*;

pub(super) fn validate_title(title: &str) -> Result<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        bail!("Slide title cannot be empty");
    }
    if slugify(&title).is_empty() {
        bail!("Slide title must contain characters that can be used in a filename");
    }
    Ok(title)
}

pub(super) fn slide_artifact_paths(slides_dir: &Path, title: &str) -> Result<(PathBuf, PathBuf)> {
    let title = validate_title(title)?;
    let slug = slugify(title);
    Ok((
        slides_dir.join(format!("{slug}.md")),
        slides_dir.join(format!("{slug}.pdf")),
    ))
}

pub(super) fn extract_generated_title(generated: &str) -> Option<String> {
    let markdown = strip_code_fence(generated.trim());
    let markdown = sanitize_marp_markdown(markdown);
    let markdown = promote_marp_frontmatter(markdown);
    let markdown = body_without_frontmatter(&markdown);
    let mut code_fence: Option<(char, usize)> = None;

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
            continue;
        }
        if code_fence.is_some() {
            continue;
        }

        if let Some(title) = trimmed.strip_prefix("# ") {
            let title = clean_generated_title(title);
            if let Ok(title) = validate_title(&title) {
                return Some(title);
            }
        }
    }

    None
}

pub(super) fn validate_draft_title(generated: &str, instruction: &str) -> Result<String> {
    let title = extract_generated_title(generated).context(
        "The drafter did not provide a title. Return a concise title as the first `# H1` on the title slide",
    )?;
    if titles_are_equivalent(&title, instruction) {
        bail!(
            "The drafter reused the instruction as the title. Generate a concise subject title instead"
        );
    }
    Ok(title)
}

pub(super) fn parse_repaired_title(response: &str, instruction: &str) -> Result<String> {
    let line = strip_code_fence(response.trim())
        .lines()
        .find(|line| !line.trim().is_empty())
        .context("Title repair response was empty")?;
    let title = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches(['\'', '"', '`'])
        .trim_end_matches(['.', ':', ';'])
        .trim();
    let title = validate_title(&clean_generated_title(title))?;
    if titles_are_equivalent(&title, instruction) {
        bail!("Title repair reused the generation instruction");
    }
    Ok(title)
}

pub(super) fn markdown_headings(markdown: &str) -> Vec<String> {
    let markdown = strip_code_fence(markdown.trim());
    let mut code_fence: Option<(char, usize)> = None;
    let mut headings = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
            continue;
        }
        if code_fence.is_some() {
            continue;
        }
        let heading = trimmed.trim_start_matches('#').trim();
        if trimmed.starts_with('#') && !heading.is_empty() {
            headings.push(clean_generated_title(heading));
        }
    }
    headings
}

pub(super) fn clean_generated_title(title: &str) -> String {
    let mut title = title.trim().trim_end_matches('#').trim();
    loop {
        let mut changed = false;
        for delimiter in ["**", "__", "`", "*", "_"] {
            if let Some(inner) = title
                .strip_prefix(delimiter)
                .and_then(|value| value.strip_suffix(delimiter))
            {
                title = inner.trim();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn titles_are_equivalent(title: &str, instruction: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    normalize(title) == normalize(instruction)
}

pub(super) fn request_chars(request: &TextGenerationRequest) -> usize {
    request.system_prompt.chars().count() + request.user_prompt.chars().count()
}

pub(super) fn generation_limit(
    error: &crate::errors::SfumatoError,
) -> Option<&crate::errors::SfumatoError> {
    matches!(
        error.class,
        crate::errors::ErrorClass::ContextLimit | crate::errors::ErrorClass::InvalidOutput
    )
    .then_some(error)
}

pub(super) fn compact_retry_failed(error: &SfumatoError) -> bool {
    error
        .details
        .get("compact_retry_failed")
        .is_some_and(|value| value == "true")
}

pub(super) fn normalize_marp_markdown(
    generated: &str,
    config: &EffectiveConfig,
    title: &str,
) -> Result<String> {
    let mut markdown = strip_code_fence(generated.trim()).to_string();
    markdown = sanitize_marp_markdown(&markdown);
    markdown = normalize_marp_math_delimiters(&markdown);
    markdown = promote_marp_frontmatter(markdown);
    let body = body_without_frontmatter(&markdown);

    markdown = canonical_marp_document(body, &config.theme);
    markdown = close_unclosed_mermaid_fences(&markdown);
    markdown = remove_duplicate_leading_title_slides(markdown, title);
    markdown = constrain_generated_images(&markdown);

    if !markdown.contains("\n---") {
        bail!("Generated deck does not contain Marp slide separators.");
    }

    markdown = ensure_title_slide(markdown, title)?;

    Ok(markdown)
}

/// Converts paired LaTeX-style math delimiters into Marp's dollar syntax.
///
/// Code fences and inline code are protected because their delimiters are
/// examples or program text, not formulas intended for MathJax rendering.
pub(super) fn normalize_marp_math_delimiters(markdown: &str) -> String {
    let protected = markdown_code_ranges(markdown);
    let mut output = String::with_capacity(markdown.len());
    let mut cursor = 0;
    let mut protected_index = 0;

    while cursor < markdown.len() {
        while protected
            .get(protected_index)
            .is_some_and(|range| range.end <= cursor)
        {
            protected_index += 1;
        }

        if let Some(range) = protected.get(protected_index)
            && range.start == cursor
        {
            output.push_str(&markdown[range.clone()]);
            cursor = range.end;
            continue;
        }

        let segment_end = protected
            .get(protected_index)
            .map(|range| range.start)
            .unwrap_or(markdown.len());
        let remaining = &markdown[cursor..segment_end];
        let delimiter = if remaining.starts_with("\\[") {
            Some(("\\]", "$$"))
        } else if remaining.starts_with("\\(") {
            Some(("\\)", "$"))
        } else {
            None
        };

        if let Some((closing, replacement)) = delimiter
            && !is_escaped_delimiter(markdown, cursor)
            && let Some(closing_start) =
                find_unescaped_delimiter(markdown, cursor + 2, segment_end, closing)
        {
            output.push_str(replacement);
            output.push_str(&markdown[cursor + 2..closing_start]);
            output.push_str(replacement);
            cursor = closing_start + 2;
            continue;
        }

        let character = markdown[cursor..]
            .chars()
            .next()
            .expect("cursor remains on a character boundary");
        output.push(character);
        cursor += character.len_utf8();
    }

    output
}

fn markdown_code_ranges(markdown: &str) -> Vec<std::ops::Range<usize>> {
    let mut fenced = Vec::new();
    let mut open_fence: Option<(char, usize, usize)> = None;
    let mut cursor = 0;

    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']).trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match open_fence {
                Some((open_marker, open_length, start))
                    if marker == open_marker && length >= open_length =>
                {
                    fenced.push(start..cursor + line.len());
                    open_fence = None;
                }
                None => open_fence = Some((marker, length, cursor)),
                _ => {}
            }
        }
        cursor += line.len();
    }

    if let Some((_, _, start)) = open_fence {
        fenced.push(start..markdown.len());
    }

    let mut protected = fenced.clone();
    let bytes = markdown.as_bytes();
    let mut position = 0;
    let mut fenced_index = 0;
    while position < bytes.len() {
        while fenced
            .get(fenced_index)
            .is_some_and(|range| range.end <= position)
        {
            fenced_index += 1;
        }
        if let Some(range) = fenced.get(fenced_index)
            && range.start <= position
        {
            position = range.end;
            continue;
        }
        if bytes[position] != b'`' {
            position += 1;
            continue;
        }

        let run = backtick_run(bytes, position);
        if let Some(end) = find_matching_backtick_run(bytes, position + run, run, &fenced) {
            protected.push(position..end + run);
            position = end + run;
        } else {
            position += run;
        }
    }

    protected.sort_by_key(|range| range.start);
    protected
}

fn backtick_run(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

fn find_matching_backtick_run(
    bytes: &[u8],
    mut cursor: usize,
    expected: usize,
    fenced: &[std::ops::Range<usize>],
) -> Option<usize> {
    let mut fenced_index = 0;
    while cursor < bytes.len() {
        while fenced
            .get(fenced_index)
            .is_some_and(|range| range.end <= cursor)
        {
            fenced_index += 1;
        }
        if let Some(range) = fenced.get(fenced_index)
            && range.start <= cursor
        {
            return None;
        }
        if bytes[cursor] == b'`' {
            let run = backtick_run(bytes, cursor);
            if run == expected {
                return Some(cursor);
            }
            cursor += run;
        } else {
            cursor += 1;
        }
    }
    None
}

fn find_unescaped_delimiter(
    markdown: &str,
    mut cursor: usize,
    end: usize,
    delimiter: &str,
) -> Option<usize> {
    while cursor < end {
        let relative = markdown[cursor..end].find(delimiter)?;
        let found = cursor + relative;
        if !is_escaped_delimiter(markdown, found) {
            return Some(found);
        }
        cursor = found + 1;
    }
    None
}

fn is_escaped_delimiter(markdown: &str, delimiter_start: usize) -> bool {
    let bytes = markdown.as_bytes();
    let mut cursor = delimiter_start;
    let mut preceding_backslashes = 0;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        preceding_backslashes += 1;
        cursor -= 1;
    }
    preceding_backslashes % 2 == 1
}

pub(super) fn close_unclosed_mermaid_fences(markdown: &str) -> String {
    let mut output = String::with_capacity(markdown.len() + 8);
    let mut mermaid_fence: Option<(char, usize)> = None;

    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some((marker, length)) = mermaid_fence {
            let closes_fence = trimmed
                .chars()
                .take_while(|character| *character == marker)
                .count()
                >= length
                && trimmed.chars().all(|character| character == marker);
            if closes_fence {
                mermaid_fence = None;
            } else if trimmed == "---" {
                output.push_str(&marker.to_string().repeat(length));
                output.push('\n');
                mermaid_fence = None;
            }
        } else if let Some((marker, length)) = markdown_code_fence(trimmed) {
            let language = trimmed[length..].trim();
            if language.eq_ignore_ascii_case("mermaid") {
                mermaid_fence = Some((marker, length));
            }
        }
        output.push_str(line);
        output.push('\n');
    }

    if let Some((marker, length)) = mermaid_fence {
        output.push_str(&marker.to_string().repeat(length));
        output.push('\n');
    }
    output.trim_end().to_string()
}

pub(super) fn validate_normalized_deck(markdown: &str, title: &str) -> Result<()> {
    SlideDeckDocument::from_marp(markdown, title)
        .context("Generated slide deck is invalid after normalization")?;
    Ok(())
}

pub(super) fn canonical_marp_document(body: &str, theme: &str) -> String {
    format!(
        "---\nmarp: true\ntheme: {theme}\npaginate: true\nmath: mathjax\n---\n\n{}",
        body.trim()
    )
}

pub(super) fn insert_title_slide(markdown: String, title: &str) -> String {
    let fences = markdown_fences(&markdown);
    if fences.len() < 2 || fences[0].start != 0 {
        return format!("# {title}\n\n---\n\n{markdown}");
    }

    format!(
        "{}\n\n# {title}\n\n---\n\n{}",
        markdown[..fences[1].end].trim_end(),
        markdown[fences[1].end..].trim_start()
    )
}

pub(super) fn ensure_title_slide(mut markdown: String, title: &str) -> Result<String> {
    let first_slide = slide_ranges(&markdown)?
        .into_iter()
        .next()
        .context("Generated deck does not contain a title slide")?;
    if let Some((start, end)) = first_h1_range(&markdown[first_slide.start..first_slide.end]) {
        markdown.replace_range(
            first_slide.start + start..first_slide.start + end,
            &format!("# {title}"),
        );
        Ok(markdown)
    } else {
        Ok(insert_title_slide(markdown, title))
    }
}

pub(super) fn first_h1_range(markdown: &str) -> Option<(usize, usize)> {
    let mut cursor = 0;
    let mut code_fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let without_newline = line.trim_end_matches(['\n', '\r']);
        let trimmed = without_newline.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
        } else if code_fence.is_none() && trimmed.starts_with("# ") {
            let indentation = without_newline.len() - trimmed.len();
            return Some((cursor + indentation, cursor + without_newline.len()));
        }
        cursor += line.len();
    }
    None
}

#[derive(Clone, Copy)]
pub(super) struct Fence {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SlideRange {
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) fn promote_marp_frontmatter(markdown: String) -> String {
    let fences = markdown_fences(&markdown);
    let Some(frontmatter_index) = fences.windows(2).position(|window| {
        frontmatter_contains_key(&markdown[window[0].end..window[1].start], "marp")
    }) else {
        return markdown;
    };
    if frontmatter_index == 0 {
        return markdown;
    }

    let frontmatter = markdown[fences[frontmatter_index].start..fences[frontmatter_index + 1].end]
        .trim()
        .to_string();
    let prefix_body = if fences[0].start == 0 {
        markdown[fences[0].end..fences[frontmatter_index].start]
            .trim()
            .to_string()
    } else {
        markdown[..fences[frontmatter_index].start]
            .trim()
            .to_string()
    };
    let suffix_body = markdown[fences[frontmatter_index + 1].end..].trim_start();
    let suffix_body = if !prefix_body.is_empty()
        && !suffix_body.is_empty()
        && !suffix_body.trim_start().starts_with("---")
    {
        format!("---\n\n{suffix_body}")
    } else {
        suffix_body.to_string()
    };

    [frontmatter, prefix_body, suffix_body]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn markdown_fences(markdown: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut cursor = 0;
    let mut code_fence: Option<(char, usize)> = None;

    for line in markdown.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = line_without_newline.trim_start();
        if let Some((marker, length)) = markdown_code_fence(trimmed) {
            match code_fence {
                Some((open_marker, open_length))
                    if marker == open_marker && length >= open_length =>
                {
                    code_fence = None;
                }
                None => code_fence = Some((marker, length)),
                _ => {}
            }
        } else if code_fence.is_none() && line_without_newline.trim() == "---" {
            fences.push(Fence {
                start: cursor,
                end: cursor + line.len(),
            });
        }
        cursor += line.len();
    }

    if cursor < markdown.len() && markdown[cursor..].trim() == "---" {
        fences.push(Fence {
            start: cursor,
            end: markdown.len(),
        });
    }

    fences
}

pub(super) fn markdown_code_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then_some((marker, length))
}

pub(super) fn slide_ranges(markdown: &str) -> Result<Vec<SlideRange>> {
    let fences = markdown_fences(markdown);
    if fences.len() < 2
        || fences[0].start != 0
        || !frontmatter_contains_key(&markdown[fences[0].end..fences[1].start], "marp")
    {
        bail!("Cannot locate canonical Marp frontmatter for slide replacement");
    }

    let mut ranges = Vec::new();
    let mut start = fences[1].end;
    for separator in fences.iter().skip(2) {
        ranges.push(SlideRange {
            start,
            end: separator.start,
        });
        start = separator.end;
    }
    ranges.push(SlideRange {
        start,
        end: markdown.len(),
    });
    Ok(ranges)
}

pub(super) fn normalize_slide_replacement(generated: &str) -> Result<String> {
    let mut fragment = strip_code_fence(generated.trim()).trim().to_string();
    fragment = sanitize_marp_markdown(&fragment).trim().to_string();
    fragment = normalize_marp_math_delimiters(&fragment);

    let fences = markdown_fences(&fragment);
    if fences.len() >= 2
        && fences[0].start == 0
        && frontmatter_contains_key(&fragment[fences[0].end..fences[1].start], "marp")
    {
        fragment = fragment[fences[1].end..].trim_start().to_string();
    }

    fragment = trim_outer_slide_separators(&fragment).to_string();
    fragment = constrain_generated_images(&fragment);
    if fragment.trim().is_empty() {
        bail!("Reviewer returned an empty slide replacement");
    }
    Ok(fragment.trim().to_string())
}

pub(super) fn trim_outer_slide_separators(fragment: &str) -> &str {
    let mut fragment = fragment.trim();
    loop {
        let fences = markdown_fences(fragment);
        if fences.first().is_some_and(|fence| fence.start == 0) {
            fragment = fragment[fences[0].end..].trim_start();
            continue;
        }
        if fences
            .last()
            .is_some_and(|fence| fence.end == fragment.len())
        {
            fragment = fragment[..fences.last().expect("checked above").start].trim_end();
            continue;
        }
        return fragment;
    }
}

pub(super) fn frontmatter_contains_key(frontmatter: &str, key: &str) -> bool {
    let prefix = format!("{key}:");
    frontmatter
        .lines()
        .any(|line| line.trim_start().starts_with(&prefix))
}

pub(super) fn body_without_frontmatter(markdown: &str) -> &str {
    let fences = markdown_fences(markdown);
    if fences.len() >= 2 && fences[0].start == 0 {
        markdown[fences[1].end..].trim_start()
    } else {
        markdown.trim()
    }
}

pub(super) fn remove_duplicate_leading_title_slides(markdown: String, title: &str) -> String {
    let mut markdown = markdown;

    loop {
        let fences = markdown_fences(&markdown);
        if fences.len() < 3 || fences[0].start != 0 {
            return markdown;
        }

        let first_slide = markdown[fences[1].end..fences[2].start].trim();
        if !is_only_title_slide(first_slide, title) {
            return markdown;
        }

        let remaining = markdown[fences[2].end..].trim_start();
        if !remaining_starts_with_title_slide(remaining, title) {
            return markdown;
        }

        markdown = format!("{}\n\n{}", markdown[..fences[1].end].trim_end(), remaining);
    }
}

/// Compares a heading against the deck title, case-insensitively.
///
/// Unicode lowercasing, not ASCII. `eq_ignore_ascii_case` does not fold accented
/// characters, so `Fibra Óptica` did not match `fibra óptica` and `ÍNDICE` did not
/// match `índice` — while the unaccented spellings matched fine. That made
/// duplicate-title detection fail for Spanish decks specifically, which is this
/// project's own use case, and shipped the deck with two title slides.
///
/// Not normalized: a precomposed `ó` and `o` followed by a combining acute are
/// still different strings. Folding those needs a normalization dependency the
/// workspace does not carry, and a model emitting one form in the title and the
/// other in the heading of the same response is not a failure seen in practice.
pub(super) fn title_matches(heading: &str, title: &str) -> bool {
    heading.trim().to_lowercase() == title.trim().to_lowercase()
}

pub(super) fn is_only_title_slide(slide: &str, title: &str) -> bool {
    slide
        .strip_prefix("# ")
        .map(|heading| title_matches(heading, title))
        .unwrap_or(false)
}

pub(super) fn remaining_starts_with_title_slide(remaining: &str, title: &str) -> bool {
    let first_slide = remaining
        .split_once("\n---")
        .map(|(first, _)| first)
        .unwrap_or(remaining);
    first_slide
        .lines()
        .any(|line| title_matches(line.trim_start_matches("# "), title))
        || first_slide.contains("<!-- _class: title -->")
}

pub(super) fn sanitize_marp_markdown(markdown: &str) -> String {
    let without_svg = strip_html_blocks(markdown, "svg");
    let without_style = strip_html_blocks(&without_svg, "style");
    let without_css_fences =
        strip_code_blocks_by_language(&without_style, &["css", "scss", "sass"]);
    remove_html_tags_by_names(
        &without_css_fences,
        &[
            "article", "div", "section", "span", "p", "br", "svg", "style",
        ],
    )
}

pub(super) fn constrain_generated_images(markdown: &str) -> String {
    let mut output = String::with_capacity(markdown.len());
    let mut cursor = 0;

    while let Some(relative_start) = markdown[cursor..].find("![") {
        let image_start = cursor + relative_start;
        output.push_str(&markdown[cursor..image_start]);

        let alt_start = image_start + 2;
        let Some(relative_alt_end) = markdown[alt_start..].find("](") else {
            output.push_str(&markdown[image_start..]);
            return output;
        };
        let alt_end = alt_start + relative_alt_end;
        let target_start = alt_end + 2;
        let Some(relative_target_end) = markdown[target_start..].find(')') else {
            output.push_str(&markdown[image_start..]);
            return output;
        };
        let target_end = target_start + relative_target_end;
        let alt = &markdown[alt_start..alt_end];
        let target = markdown[target_start..target_end].trim();

        if is_generated_image_target(target) && !has_marp_image_layout(alt) {
            output.push_str(&format!(
                "![height:{GENERATED_IMAGE_MARP_HEIGHT}]({target})"
            ));
        } else {
            output.push_str(&markdown[image_start..=target_end]);
        }
        cursor = target_end + 1;
    }

    output.push_str(&markdown[cursor..]);
    output
}

pub(super) fn is_generated_image_target(target: &str) -> bool {
    target.starts_with("images/") || target.starts_with("./images/")
}

pub(super) fn has_marp_image_layout(alt: &str) -> bool {
    alt.split_whitespace().any(|part| {
        let option = part.to_ascii_lowercase();
        option == "bg"
            || option.starts_with("bg:")
            || ["width:", "height:", "w:", "h:"]
                .iter()
                .any(|prefix| option.starts_with(prefix))
    })
}

pub(super) fn strip_code_blocks_by_language(markdown: &str, languages: &[&str]) -> String {
    let mut output = String::new();
    let mut cursor = 0;

    while let Some(relative_start) = markdown[cursor..].find("```") {
        let fence_start = cursor + relative_start;
        output.push_str(&markdown[cursor..fence_start]);

        let after_ticks = fence_start + 3;
        let line_end = markdown[after_ticks..]
            .find('\n')
            .map(|offset| after_ticks + offset)
            .unwrap_or(markdown.len());
        let language = markdown[after_ticks..line_end].trim();

        if !languages
            .iter()
            .any(|candidate| language.eq_ignore_ascii_case(candidate))
        {
            output.push_str(&markdown[fence_start..line_end]);
            cursor = line_end;
            continue;
        }

        let content_start = if line_end < markdown.len() {
            line_end + 1
        } else {
            line_end
        };
        if let Some(relative_end) = markdown[content_start..].find("\n```") {
            cursor = content_start + relative_end + "\n```".len();
        } else {
            cursor = markdown.len();
        }
    }

    output.push_str(&markdown[cursor..]);
    output
}

pub(super) fn strip_html_blocks(markdown: &str, tag_name: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    let lower = markdown.to_lowercase();
    let opening = format!("<{}", tag_name.to_lowercase());
    let closing = format!("</{}>", tag_name.to_lowercase());

    while let Some(relative_start) = lower[cursor..].find(&opening) {
        let start = cursor + relative_start;
        output.push_str(&markdown[cursor..start]);

        let after_start = start + opening.len();
        let is_tag_boundary = lower[after_start..]
            .chars()
            .next()
            .map(|next| next.is_whitespace() || next == '>' || next == '/')
            .unwrap_or(false);
        if !is_tag_boundary {
            output.push_str(&markdown[start..after_start]);
            cursor = after_start;
            continue;
        }

        if let Some(relative_end) = lower[after_start..].find(&closing) {
            cursor = after_start + relative_end + closing.len();
        } else if let Some(relative_tag_end) = lower[after_start..].find('>') {
            cursor = after_start + relative_tag_end + 1;
        } else {
            cursor = markdown.len();
        }
    }

    output.push_str(&markdown[cursor..]);
    output
}

pub(super) fn remove_html_tags_by_names(markdown: &str, tag_names: &[&str]) -> String {
    let mut output = String::new();
    let mut cursor = 0;

    while let Some(relative_start) = markdown[cursor..].find('<') {
        let start = cursor + relative_start;
        output.push_str(&markdown[cursor..start]);

        let Some(relative_end) = markdown[start..].find('>') else {
            output.push_str(&markdown[start..]);
            return output;
        };
        let end = start + relative_end + 1;
        let tag = &markdown[start + 1..end - 1];

        if is_named_html_tag(tag, tag_names) {
            cursor = end;
        } else {
            output.push_str(&markdown[start..end]);
            cursor = end;
        }
    }

    output.push_str(&markdown[cursor..]);
    output
}

pub(super) fn is_named_html_tag(tag: &str, tag_names: &[&str]) -> bool {
    let tag = tag.trim_start().trim_start_matches('/').trim_start();
    let name = tag
        .split(|character: char| character.is_whitespace() || character == '/' || character == '>')
        .next()
        .unwrap_or_default();
    tag_names
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

pub(super) fn strip_code_fence(text: &str) -> &str {
    let text = text.trim();
    for marker in ["```", "~~~"] {
        let Some(after_marker) = text.strip_prefix(marker) else {
            continue;
        };
        let Some(opening_line_end) = after_marker.find('\n') else {
            continue;
        };
        let language = after_marker[..opening_line_end].trim();
        let body = &after_marker[opening_line_end + 1..];
        if is_markdown_document_language(language) {
            return strip_optional_document_closing_fence(body, marker);
        }
        let Some(body) = body.trim_end().strip_suffix(marker) else {
            continue;
        };
        return body.trim();
    }
    text
}

pub(super) fn is_markdown_document_language(language: &str) -> bool {
    matches!(
        language.to_ascii_lowercase().as_str(),
        "marp" | "markdown" | "md"
    )
}

pub(super) fn strip_optional_document_closing_fence<'a>(body: &'a str, marker: &str) -> &'a str {
    let marker_character = marker.chars().next().unwrap_or('`');
    let marker_count = body
        .lines()
        .filter_map(|line| markdown_code_fence(line.trim_start()))
        .filter(|(character, length)| *character == marker_character && *length >= marker.len())
        .count();
    let body = body.trim();

    if marker_count % 2 == 1 {
        body.strip_suffix(marker).map(str::trim_end).unwrap_or(body)
    } else {
        body
    }
}

pub(super) fn ensure_inside(root: &Path, path: &Path) -> Result<()> {
    if !path.starts_with(root) {
        bail!(
            "Refusing to write {} because it is outside {}",
            path.display(),
            root.display()
        );
    }

    Ok(())
}
