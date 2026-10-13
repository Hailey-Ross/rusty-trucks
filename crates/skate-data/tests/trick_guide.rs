//! The Trick Guide data from local retail files (never committed): the TU3 memory image's tables and the installed
//! skater collections. Run with `cargo test -p skate-data --test trick_guide -- --ignored --nocapture`; skips when
//! a file is absent.
use skate_core::trick_guide::{TrickGuideState, movie_state};
use skate_data::{collections::Collections, menu_tables::extract, xex::XexImage};
use std::path::PathBuf;

fn local(relative: &str) -> Option<PathBuf> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.ancestors().map(|a| a.join(relative)).find(|p| p.exists())
}

#[test]
#[ignore = "needs the local TU3 image and an installed game"]
fn retail_trick_guide_lists_and_slots() {
    let (Some(image), Some(assets)) = (local(".local/tu3-image/default_82000000_011B0000.bin"), local("assets/private/stock/skater-collections.json")) else {
        eprintln!("skipped: no TU3 image or installed collections");
        return;
    };
    let tables = extract(&XexImage { base_address: 0x8200_0000, image: std::fs::read(image).unwrap() }).unwrap();
    let collections = Collections::load(assets.parent().unwrap().parent().unwrap().parent().unwrap()).unwrap();
    let guide = skate_data::trick_guide::load(&collections, &tables.trick_guide);
    let mut state = TrickGuideState::open(&guide);
    let categories = state.items(&guide);
    assert_eq!(categories.len(), 8);
    assert_eq!(categories[0], "ID_TRICK_TYPE_FLIP_TRICKS");
    assert_eq!(categories[3], "ID_TRICK_TYPE_MANUALS");
    // The whole tree, as the screen walks it (eprintln for review).
    let mut listed = 0;
    for c in 0..categories.len() {
        let mut s = TrickGuideState::open(&guide);
        s.select(&guide, c);
        eprintln!("{} (level {}): {:?}", categories[c], s.level, s.items(&guide));
        if s.level == 1 {
            for i in 0..s.items(&guide).len() {
                let mut t = s.clone();
                t.hilight(&guide, i);
                if t.has_submenu(&guide) {
                    t.select(&guide, i);
                    listed += t.items(&guide).len();
                    eprintln!("    {:?}", t.items(&guide));
                }
            }
        } else {
            listed += s.items(&guide).len();
        }
    }
    eprintln!("{listed} tricks listed");
    assert!(listed > 100, "{listed}");
    // Flip tricks > Ollie flips: Kickflip's slot is gesture 48 (ollie_kickflip).
    state.select(&guide, 0);
    state.select(&guide, 0);
    let kickflip = state.items(&guide).iter().position(|l| l == "ID_TRICK_FLIP_KICKFLIP").expect("kickflip listed");
    assert_eq!(state.hilight(&guide, kickflip), movie_state::GESTURES);
    assert_eq!(state.gestures_buttons(&guide, Default::default()).1[0][0], "ollie_kickflip");
}
