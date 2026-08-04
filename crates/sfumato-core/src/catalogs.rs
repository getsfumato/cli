//! Shared shape for catalog listings that survive a damaged member.
//!
//! Every catalog resolves each entry to present it, so one unreadable entry used
//! to fail the whole listing: the first `?` discarded the entries that had already
//! resolved. The data was never lost — each healthy entry stayed reachable through
//! `show` — but discovery was, which is the one thing a listing is for.
//!
//! `ThemeRepository::list` avoids the problem by listing names without loading
//! anything. The other catalogs genuinely need to read each entry to describe it,
//! so they report per entry instead: what resolved, and what did not.

/// A catalog entry that could not be read, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnreadableEntry {
    /// Directory or record name, which is knowable even when nothing else is.
    pub name: String,
    /// Why the entry could not be presented.
    pub problem: String,
}

/// Entries that resolved, alongside the ones that could not.
#[derive(Clone, Debug)]
pub struct CatalogListing<T> {
    /// Entries that resolved and can be presented.
    pub entries: Vec<T>,
    /// Entries that were skipped, reported so the damage stays visible.
    pub unreadable: Vec<UnreadableEntry>,
}

impl<T> Default for CatalogListing<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            unreadable: Vec::new(),
        }
    }
}

impl<T> CatalogListing<T> {
    /// Creates a listing with no damaged entries.
    pub fn healthy(entries: Vec<T>) -> Self {
        Self {
            entries,
            unreadable: Vec::new(),
        }
    }

    /// Reports whether every entry resolved.
    pub fn is_complete(&self) -> bool {
        self.unreadable.is_empty()
    }
}
