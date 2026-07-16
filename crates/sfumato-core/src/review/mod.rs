//! Review domain re-exports.
//!
//! The implementation lives in `sfumato-domain`; this module keeps the
//! application-facing namespace concise while the v0.2 API settles.

pub use sfumato_domain::{
    Patch, PatchOperation, PatchReport, ReviewConstraint, ReviewError, ReviewFormat,
    ReviewSnapshot, ReviewableDocument, ValidationReport, parse_json_patch, validate_rfc6902_patch,
};

/// Deck-specific review types.
pub mod decks {
    pub use sfumato_domain::{
        DeckDocument as SlideDeckDocument, SlideDocument, SlideElement, SlideId, SlideKind,
    };
}
