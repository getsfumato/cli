//! Measured layout repair state and acceptance policy.

use crate::generation::SlideLayoutIssue;

/// Tracks the best locally measured deck while focused repairs are evaluated.
pub(super) struct LayoutAssessment {
    issues: Vec<SlideLayoutIssue>,
}

impl LayoutAssessment {
    pub(super) fn new(issues: Vec<SlideLayoutIssue>) -> Self {
        Self { issues }
    }

    pub(super) fn issue_for_slide(&self, slide: usize) -> Option<SlideLayoutIssue> {
        self.issues
            .iter()
            .find(|issue| issue.slide == slide)
            .cloned()
    }

    pub(super) fn accept_if_improved(&mut self, candidate: Vec<SlideLayoutIssue>) -> bool {
        if layout_score(&candidate) < layout_score(&self.issues) {
            self.issues = candidate;
            true
        } else {
            false
        }
    }

    pub(super) fn into_issues(self) -> Vec<SlideLayoutIssue> {
        self.issues
    }
}

pub(super) fn layout_score(issues: &[SlideLayoutIssue]) -> (usize, u64) {
    let overflow = issues
        .iter()
        .map(|issue| {
            u64::from(issue.vertical_overflow_px) + u64::from(issue.horizontal_overflow_px)
        })
        .sum();
    (issues.len(), overflow)
}

#[cfg(test)]
#[path = "../../../tests/unit/resources_slides_layout.rs"]
mod tests;
