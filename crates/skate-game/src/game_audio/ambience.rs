//! One ambience bed per map, cross-faded on map change. Retail switches beds by
//! zone (per-map `.ems` emitter files); here each map gets the bed that fits
//! it best. The choices are by name and ear, not from retail data
//! (docs/hails-additions/11-audio.md).
use super::{Category, Library, Play, Voices, library::Clip, voices::VoiceId};
use bevy::prelude::*;

/// Bed level before the ambience and master volumes.
const LEVEL: f32 = 0.6;
const CROSSFADE: f32 = 2.0;

/// Map name (maps.json / CurrentMap) -> ambience bed. Unlisted maps are silent.
const BEDS: &[(&str, &str)] = &[
    ("University", "09_univ_campus"),
    ("StartPark", "10_univ_housing"),
    ("DownTown", "04_dt_main"),
    ("DownTownSkatePark", "05_dt_parks"),
    ("MegaPark", "05_dt_parks"),
    ("Industrial", "11_indu_shipyard"),
    ("IndustrialSkatePark", "20_indu_old_factory"),
    ("SkateSchool", "18_skate_school"),
    ("MaloofMoneyCup", "21_interior_arena_amb"),
    ("BlackBoxPark", "22_interior_tunnel_amb"),
];

pub(super) fn bed_for(map: &str) -> Option<&'static str> {
    BEDS.iter().find(|(name, _)| name.eq_ignore_ascii_case(map)).map(|(_, bed)| *bed)
}

#[derive(Default)]
pub(super) struct State {
    map: Option<(String, u64)>,
    current: Option<(VoiceId, Clip)>,
    fading: Vec<Clip>,
}

pub(super) fn update(
    mut state: Local<State>,
    mut commands: Commands,
    map: Res<crate::map_transition::CurrentMap>,
    library: Option<ResMut<Library>>,
    mut voices: ResMut<Voices>,
    mut assets: ResMut<Assets<AudioSource>>,
    time: Res<Time<Real>>,
) {
    let Some(mut library) = library else { return };
    // Free beds whose fade-out has finished.
    let fading = std::mem::take(&mut state.fading);
    for clip in fading {
        if voices.uses(&clip) { state.fading.push(clip); } else { library.release(&mut assets, &clip); }
    }
    let identity = (map.name.clone(), map.generation);
    if state.map.as_ref() == Some(&identity) {
        return;
    }
    state.map = Some(identity);
    let bed = bed_for(&map.name);
    if let Some((voice, clip)) = state.current.take() {
        // Released only once no voice uses it (a reload may restart the same bed).
        voices.stop(voice, CROSSFADE);
        state.fading.push(clip);
    }
    let Some(bed) = bed else {
        info!("Ambience: none for map {:?}", map.name);
        return;
    };
    let Some(clip) = library.ambience(&mut assets, bed) else { return };
    let play = Play { category: Category::Ambience, volume: LEVEL, pitch: 1.0, position: None, looping: true, fade_in: CROSSFADE, envelope: None };
    match voices.play(&mut commands, &clip, play, time.elapsed_secs_f64()) {
        Some(voice) => {
            info!("Ambience: {bed} for map {:?}", map.name);
            state.current = Some((voice, clip));
        }
        None => state.fading.push(clip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_installed_map_has_a_bed_from_the_export_list() {
        for map in ["University", "BlackBoxPark", "DownTown", "DownTownSkatePark", "Industrial",
                    "IndustrialSkatePark", "MaloofMoneyCup", "MegaPark", "SkateSchool", "StartPark"] {
            assert!(bed_for(map).is_some(), "{map}");
        }
        assert_eq!(bed_for("university"), Some("09_univ_campus"));
        assert_eq!(bed_for("Test world"), None);
    }
}
