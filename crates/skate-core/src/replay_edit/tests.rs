use super::*;

#[test]
fn replay_editor_opens_at_retail_150_frames() {
    let edit = ReplayEdit::default();
    assert!(!edit.can_open(149));
    assert!(edit.can_open(150));
}

#[test]
fn replay_editor_steps_clamps_and_wraps_like_retail() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 300).unwrap();
    // Speed: default 100, steps of 10, never below 10 or above 100.
    assert_eq!(edit.clips[clip].values[Lane::Speed.index()], 100);
    assert_eq!(edit.adjust(clip, Lane::Speed, 1).unwrap(), 100);
    for _ in 0..20 {
        edit.adjust(clip, Lane::Speed, -1).unwrap();
    }
    assert_eq!(edit.clips[clip].values[Lane::Speed.index()], 10);
    // Shake wraps 3 -> 0 and 0 -> 3.
    assert_eq!(edit.adjust(clip, Lane::Shake, -1).unwrap(), 3);
    assert_eq!(edit.adjust(clip, Lane::Shake, 1).unwrap(), 0);
    // Contrast 10..30 step 2; brightness -100..100 step 20; tint 0..250 step 25.
    assert_eq!(edit.adjust(clip, Lane::Contrast, 1).unwrap(), 12);
    assert_eq!(edit.adjust(clip, Lane::Brightness, -1).unwrap(), -20);
    assert_eq!(edit.adjust(clip, Lane::TintR, 1).unwrap(), 25);
    assert_eq!(Lane::from_id("tint_a"), Some(Lane::TintA));
    assert!(LANES.iter().all(|l| Lane::from_id(l.id()) == Some(*l)));
}

#[test]
fn replay_editor_lanes_step_at_markers_without_ramps() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(100, 400).unwrap();
    edit.set_marker(clip, Lane::Speed, 200, 50).unwrap();
    edit.set_marker(clip, Lane::Speed, 300, 20).unwrap();
    assert_eq!(edit.value_at(clip, Lane::Speed, 150), Some(100));
    assert_eq!(edit.value_at(clip, Lane::Speed, 200), Some(50));
    assert_eq!(edit.value_at(clip, Lane::Speed, 299), Some(50));
    assert_eq!(edit.value_at(clip, Lane::Speed, 350), Some(20));
    assert!((edit.effect_at(clip, Lane::Speed, 350).unwrap() - 0.2).abs() < 1e-6);
    // Same frame replaces; values clamp to the lane's range; outside the clip is refused.
    edit.set_marker(clip, Lane::Speed, 300, 5).unwrap();
    assert_eq!(edit.value_at(clip, Lane::Speed, 300), Some(10));
    assert_eq!(edit.set_marker(clip, Lane::Speed, 50, 50), Err(EditError::OutsideClip));
    assert_eq!(edit.remove_marker(clip, Lane::Speed, 200), Ok(true));
    assert_eq!(edit.value_at(clip, Lane::Speed, 250), Some(100));
    // Trimming drops markers outside the new span.
    edit.trim(clip, 100, 250).unwrap();
    assert!(edit.clips[clip].markers[Lane::Speed.index()].is_empty());
}

#[test]
fn replay_editor_refuses_markers_past_the_retail_cap() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1000).unwrap();
    for frame in 0..100 {
        edit.set_marker(clip, Lane::Brightness, frame, 20).unwrap();
    }
    assert_eq!(edit.set_marker(clip, Lane::Brightness, 500, 20), Err(EditError::NoMoreMarkers));
    // Replacing an existing marker still works at the cap.
    assert_eq!(edit.set_marker(clip, Lane::Brightness, 5, 40), Ok(()));
}

#[test]
fn replay_editor_encoded_length_follows_retail_formula() {
    let mut edit = ReplayEdit::default();
    // 30 s at 100 %: 30 s.
    let a = edit.add_clip(0, 900).unwrap();
    assert_eq!(edit.encoded_seconds(), 30);
    assert!(edit.can_share());
    // The same clip at 50 % plays 60 s: still shareable; a 10 frame second clip pushes it over.
    for _ in 0..5 {
        edit.adjust(a, Lane::Speed, -1).unwrap();
    }
    assert_eq!(edit.encoded_seconds(), 60);
    assert!(edit.can_share());
    edit.add_clip(900, 920).unwrap();
    assert_eq!(edit.encoded_seconds(), 61);
    assert!(!edit.can_share());
    assert_eq!(edit.add_clip(5, 5), Err(EditError::EmptySpan));
}
