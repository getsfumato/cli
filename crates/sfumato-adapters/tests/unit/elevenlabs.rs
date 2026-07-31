use super::*;

fn alignment(characters: &str, step: f32) -> Alignment {
    let characters = characters
        .chars()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let starts = (0..characters.len())
        .map(|index| index as f32 * step)
        .collect::<Vec<_>>();
    let ends = starts.iter().map(|start| start + step).collect();
    Alignment {
        characters,
        character_start_times_seconds: starts,
        character_end_times_seconds: ends,
    }
}

#[test]
fn character_timings_become_word_timings() {
    let words = words_from_alignment(&alignment("hi there", 0.1));
    assert_eq!(
        words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        vec!["hi", "there"]
    );
    // "hi" occupies the first two characters, so it ends where the second does
    // and the space that follows belongs to neither word.
    assert!((words[0].start_seconds - 0.0).abs() < 1e-5);
    assert!((words[0].end_seconds - 0.2).abs() < 1e-5);
    assert!((words[1].start_seconds - 0.3).abs() < 1e-5);
}

#[test]
fn a_trailing_word_is_not_dropped() {
    let words = words_from_alignment(&alignment("go", 0.2));
    assert_eq!(
        words.len(),
        1,
        "the last word closes without a trailing space"
    );
    assert_eq!(words[0].text, "go");
}

#[test]
fn a_truncated_alignment_stops_rather_than_inventing_timings() {
    let words = words_from_alignment(&Alignment {
        characters: vec!["a".into(), "b".into()],
        character_start_times_seconds: vec![0.0],
        character_end_times_seconds: vec![0.1],
    });
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].text, "a");
}

#[test]
fn an_empty_alignment_produces_no_words() {
    assert!(words_from_alignment(&Alignment::default()).is_empty());
}
