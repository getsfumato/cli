//! Bookkeeping for the focused format-repair loop.
//!
//! Repair is only worth applying when it leaves the document measurably better.
//! Without that gate a rewrite that trades a wide table for an orphaned heading
//! looks like progress and the loop keeps spending model calls on churn.

use std::collections::BTreeSet;

use crate::generation::DocumentFormatIssue;

pub(super) struct FormatAssessment {
    issues: Vec<DocumentFormatIssue>,
    abandoned: BTreeSet<usize>,
}

impl FormatAssessment {
    pub(super) fn new(issues: Vec<DocumentFormatIssue>) -> Self {
        Self {
            issues,
            abandoned: BTreeSet::new(),
        }
    }

    /// The next defect worth a repair attempt, worst first.
    ///
    /// Worst first because one oversized element usually reports on several
    /// pages, and fixing the largest offender clears the rest with it.
    pub(super) fn next_issue(&self) -> Option<DocumentFormatIssue> {
        self.issues
            .iter()
            .filter(|issue| !self.abandoned.contains(&issue.section))
            .max_by_key(|issue| issue.overflow_px)
            .cloned()
    }

    /// Stops attempting one section, so a stubborn defect cannot loop forever.
    pub(super) fn give_up_on(&mut self, section: usize) {
        self.abandoned.insert(section);
    }

    /// Adopts a re-measured set only when it is an improvement.
    pub(super) fn accept_if_improved(&mut self, candidate: Vec<DocumentFormatIssue>) -> bool {
        if format_score(&candidate) < format_score(&self.issues) {
            self.issues = candidate;
            true
        } else {
            false
        }
    }

    pub(super) fn into_issues(self) -> Vec<DocumentFormatIssue> {
        self.issues
    }
}

/// Ranks a defect set by count first, then by total severity.
///
/// Count leads because two small defects are worse than one slightly larger one:
/// each is a separate place the reader notices the page is wrong.
pub(super) fn format_score(issues: &[DocumentFormatIssue]) -> (usize, u64) {
    (
        issues.len(),
        issues
            .iter()
            .map(|issue| u64::from(issue.overflow_px))
            .sum(),
    )
}
