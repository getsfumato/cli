//! Fuzzy jump to any destination, and the key reference.
//!
//! Eleven menu entries mean the only way to reach one was to leave whatever screen
//! you were on, walk the list, and press enter. Typing what you want is how Claude
//! Code and opencode make a keyboard UI navigable once it has more than a handful of
//! places to be, and it costs one overlay.
//!
//! The overlay also carries the key reference, because the two answer the same
//! question — "what can I do from here" — and a user who opens one often wants the
//! other.

use super::GenerateFieldId;

/// What the palette overlay is currently showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Overlay {
    /// Fuzzy destination search, with the query typed so far.
    Palette { query: String, selected: usize },
    /// The key reference for the screen underneath.
    Help,
    /// Values for one form field, filtered as the user types.
    ///
    /// Carries the field it will write back to, so the picker can be opened from any
    /// form without the form having to hold picker state of its own.
    Choice {
        target: ChoiceTarget,
        query: String,
        selected: usize,
    },
}

/// Which form field a picker writes back to.
///
/// The generate form addresses its fields by a stable id and the operation forms are
/// built per action, so they are addressed positionally. One overlay serves both rather
/// than each form growing its own picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChoiceTarget {
    Generate(GenerateFieldId),
    Operation(usize),
}

impl Overlay {
    /// Opens an empty picker for one field.
    pub(super) fn choice(target: ChoiceTarget) -> Self {
        Self::Choice {
            target,
            query: String::new(),
            selected: 0,
        }
    }

    /// Opens an empty palette.
    pub(super) fn palette() -> Self {
        Self::Palette {
            query: String::new(),
            selected: 0,
        }
    }
}

/// Scores a destination against a query.
///
/// Subsequence matching, not substring: `cnx` finds `Connectors`, which is the point
/// of typing instead of scrolling. A match earlier in the label and with fewer gaps
/// scores higher, so `pro` puts `Projects` above `Prompts` rather than leaving the
/// order to chance.
pub(super) fn score(label: &str, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let label_lower = label.to_lowercase();
    let mut haystack = label_lower.chars().enumerate();
    let mut total = 0_u32;
    let mut previous: Option<usize> = None;
    for wanted in query.to_lowercase().chars() {
        let (position, _) = haystack
            .by_ref()
            .find(|(_, candidate)| *candidate == wanted)?;
        // A run of adjacent characters is a better match than the same characters
        // scattered across the label.
        let gap = previous.map_or(position, |last| position.saturating_sub(last + 1));
        total += (gap as u32) * 4 + position as u32;
        previous = Some(position);
    }
    // Lower is better while scoring; invert so callers can sort descending.
    Some(u32::MAX - total)
}

/// Filters and ranks destinations for a query.
pub(super) fn matches<'a>(labels: &[&'a str], query: &str) -> Vec<&'a str> {
    let mut scored: Vec<(&str, u32)> = labels
        .iter()
        .filter_map(|label| score(label, query).map(|value| (*label, value)))
        .collect();
    // Stable within a score so an empty query keeps the menu's own order.
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(label, _)| label).collect()
}
