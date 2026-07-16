use super::*;

fn issue(slide: usize, vertical: u32, horizontal: u32) -> SlideLayoutIssue {
    SlideLayoutIssue {
        slide,
        title: format!("Slide {slide}"),
        vertical_overflow_px: vertical,
        horizontal_overflow_px: horizontal,
    }
}

#[test]
fn accepts_only_measurable_layout_improvements() {
    let mut assessment = LayoutAssessment::new(vec![issue(2, 40, 0), issue(3, 10, 0)]);

    assert!(!assessment.accept_if_improved(vec![issue(2, 45, 0), issue(3, 10, 0)]));
    assert!(assessment.accept_if_improved(vec![issue(2, 55, 0)]));
    assert!(assessment.accept_if_improved(vec![issue(2, 30, 0)]));
    assert_eq!(
        assessment.issue_for_slide(2).unwrap().vertical_overflow_px,
        30
    );
    assert!(assessment.accept_if_improved(Vec::new()));
    assert!(assessment.into_issues().is_empty());
}
