use super::*;

fn word(text: &str, start: f32, end: f32) -> SpeechWordTiming {
    SpeechWordTiming {
        text: text.into(),
        start_seconds: start,
        end_seconds: end,
    }
}

#[test]
fn caption_groups_break_on_sentence_boundaries() {
    let groups = caption_groups(
        &[
            word("Fibre", 0.0, 0.3),
            word("carries", 0.3, 0.7),
            word("light.", 0.7, 1.1),
            word("Copper", 1.2, 1.6),
            word("carries", 1.6, 2.0),
            word("current.", 2.0, 2.4),
        ],
        0.0,
    );
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].text, "Fibre carries light.");
    assert_eq!(groups[1].text, "Copper carries current.");
}

#[test]
fn caption_groups_break_on_a_real_pause() {
    let groups = caption_groups(&[word("one", 0.0, 0.4), word("two", 1.5, 1.9)], 0.0);
    assert_eq!(groups.len(), 2, "half a second of silence ends a caption");
}

#[test]
fn caption_groups_cap_their_word_count() {
    let words = (0..12)
        .map(|index| {
            let start = index as f32 * 0.2;
            word("word", start, start + 0.15)
        })
        .collect::<Vec<_>>();
    let groups = caption_groups(&words, 0.0);
    assert!(
        groups.iter().all(|group| group.words.len() <= 5),
        "no caption may outrun a glance: {groups:?}"
    );
}

#[test]
fn caption_groups_are_offset_onto_the_film_timeline() {
    let groups = caption_groups(&[word("later.", 0.0, 0.5)], 12.0);
    assert_eq!(groups[0].start_seconds, 12.0);
    assert_eq!(groups[0].end_seconds, 12.5);
}

#[test]
fn narration_track_totals_include_the_gap_after_every_passage() {
    let track = NarrationTrack {
        segments: vec![
            NarrationSegment {
                id: "scene-1".into(),
                text: "one".into(),
                path: PathBuf::from("/tmp/one.mp3"),
                reference: "assets/audio/one.mp3".into(),
                duration_seconds: 2.0,
                words: Vec::new(),
            },
            NarrationSegment {
                id: "scene-2".into(),
                text: "two".into(),
                path: PathBuf::from("/tmp/two.mp3"),
                reference: "assets/audio/two.mp3".into(),
                duration_seconds: 3.0,
                words: Vec::new(),
            },
        ],
        segment_gap_seconds: 0.5,
    };
    assert_eq!(track.total_seconds(), 6.0);
    assert_eq!(
        track.segment("scene-2").map(|value| value.duration_seconds),
        Some(3.0)
    );
}

#[test]
fn a_passage_identifier_cannot_become_a_path_that_escapes_the_output_directory() {
    // The identifier is interpolated into `narration-<id>-<digest>.<ext>` and
    // joined onto the output directory, so anything that is not a plain name
    // component writes somewhere the caller did not choose.
    for id in [
        "../../../../tmp/pwned",
        "..",
        "nested/scene",
        "scene\\windows",
        "/absolute",
        "scene.mp3",
        "",
    ] {
        assert!(
            validate_segment_id(id).is_err(),
            "`{id}` is not a safe file-name component"
        );
    }
    for id in ["intro", "scene-2", "chapter_3", "A1"] {
        assert!(
            validate_segment_id(id).is_ok(),
            "`{id}` is a normal passage name and must stay valid"
        );
    }
    assert!(validate_segment_id(&"a".repeat(64)).is_ok());
    assert!(validate_segment_id(&"a".repeat(65)).is_err());
}

#[test]
fn audio_extension_follows_the_requested_container() {
    assert_eq!(audio_extension(Some("mp3_44100_128")), "mp3");
    assert_eq!(audio_extension(Some("wav_44100")), "wav");
    assert_eq!(audio_extension(Some("opus_48000_96")), "opus");
    assert_eq!(audio_extension(None), "mp3");
    assert_eq!(audio_media_type(Some("wav_44100")), "audio/wav");
}
