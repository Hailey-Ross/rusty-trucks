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
    // Span 10000: markers 90 frames apart clear the 0.0089 spacing (89 frames).
    let clip = edit.add_clip(0, 10_000).unwrap();
    for k in 1..=100 {
        edit.set_marker(clip, Lane::Brightness, 90 * k, 20).unwrap();
    }
    assert_eq!(edit.clips[clip].markers[Lane::Brightness.index()].len(), 100);
    assert_eq!(edit.set_marker(clip, Lane::Brightness, 9500, 20), Err(EditError::NoMoreMarkers));
    // Editing the marker at the cursor still works at the cap.
    assert_eq!(edit.set_marker(clip, Lane::Brightness, 95, 40), Ok(()));
    assert_eq!(edit.value_at(clip, Lane::Brightness, 90), Some(40));
}

#[test]
fn replay_editor_keeps_retail_marker_spacing() {
    let mut edit = ReplayEdit::default();
    // Span 1000: 0.0089 = 8.9 frames.
    let clip = edit.add_clip(0, 1000).unwrap();
    assert_eq!(edit.set_marker(clip, Lane::Shake, 5, 1), Err(EditError::TooClose));
    assert_eq!(edit.set_marker(clip, Lane::Shake, 995, 1), Err(EditError::TooClose));
    edit.set_marker(clip, Lane::Shake, 500, 1).unwrap();
    // Within 8 frames of it is the same marker (at the cursor): its value changes, no second marker.
    edit.set_marker(clip, Lane::Shake, 506, 2).unwrap();
    assert_eq!(edit.clips[clip].markers[Lane::Shake.index()], [(500, 2)]);
    edit.set_marker(clip, Lane::Shake, 520, 3).unwrap();
    assert_eq!(edit.clips[clip].markers[Lane::Shake.index()].len(), 2);
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

#[test]
fn replay_look_replaces_the_world_grade_terms_it_sets() {
    use crate::colour_matrix::{ColourMatrixRecord, apply, build};
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1000).unwrap();
    let mut base = ColourMatrixRecord::default();
    base.near.add = [0.05; 3];
    base.far.saturation = 0.5;
    // Untouched lanes keep the world's grade.
    let look = edit.look_at(clip, 10, &base).unwrap();
    assert_eq!(look.record, base);
    assert_eq!(look.tint, [0.0; 4]);
    // Saturation 0 (greyscale) replaces both bands; brightness +40 is the add term; contrast 20 = x2.
    for _ in 0..10 {
        edit.adjust(clip, Lane::Saturation, -1).unwrap();
    }
    edit.adjust(clip, Lane::Brightness, 1).unwrap();
    edit.adjust(clip, Lane::Brightness, 1).unwrap();
    for _ in 0..5 {
        edit.adjust(clip, Lane::Contrast, 1).unwrap();
    }
    let look = edit.look_at(clip, 10, &base).unwrap();
    assert_eq!(look.record.near.saturation, 0.0);
    assert_eq!(look.record.far.saturation, 0.0);
    assert!((look.record.near.add[0] - 0.4).abs() < 1e-6 && (look.record.far.add[2] - 0.4).abs() < 1e-6);
    assert!((look.record.near.contrast - 2.0).abs() < 1e-6);
    let grey = apply(&build(&look.record.near), [1.0, 0.0, 0.0]);
    assert!((grey[0] - grey[1]).abs() < 1e-5 && (grey[1] - grey[2]).abs() < 1e-5, "{grey:?}");
    // A tint marker shows from its frame on.
    edit.set_marker(clip, Lane::TintA, 500, 250).unwrap();
    assert_eq!(edit.look_at(clip, 499, &base).unwrap().tint[3], 0.0);
    assert!((edit.look_at(clip, 500, &base).unwrap().tint[3] - 250.0 / 255.0).abs() < 1e-6);
}
