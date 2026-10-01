//! Game audio: per-map ambience, board rolling, trick cues and footsteps,
//! played from the owned disc's sounds that setup decodes to PCM WAV
//! (tools/asset_pipeline/audio_export.py -> assets/private/audio).
//!
//! Gameplay events have no event bus, so each cue is found by comparing this
//! frame's physics state with the last one, as the mod observation layer does.
//! Which retail sample plays for which event is our own hand-made table
//! (docs/hails-additions/11-audio.md); EA's AEMS event runtime is not
//! re-implemented.
//!
//! Loudness is deliberately conservative: master volume defaults to 25%,
//! a cue may raise a quiet clip only until the clip's own peak reaches full
//! scale (at most x4) before the master and category volumes (both <= 1)
//! scale it down (voices.rs), sounds fade in, voice counts are capped, and nothing plays while
//! the menu is open or a replay runs. `--mute` silences game and mod audio.
mod ambience;
mod cues;
mod library;
mod skate_events;
mod voices;
mod water;

use bevy::{audio::Volume, prelude::*};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) use library::Library;
pub(crate) use voices::{Category, Play, Voices};

/// Volume steps for the menu (percent).
const STEP: u32 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct SavedSettings {
    master: u32,
    ambience: u32,
    effects: u32,
}
impl Default for SavedSettings {
    fn default() -> Self {
        Self { master: 25, ambience: 100, effects: 100 }
    }
}
impl SavedSettings {
    fn validated(mut self) -> Self {
        for value in [&mut self.master, &mut self.ambience, &mut self.effects] {
            *value = (*value).min(100) / STEP * STEP;
        }
        self
    }
}

/// Player volume settings, saved beside the graphics settings.
#[derive(Resource)]
pub(crate) struct AudioSettings {
    saved: SavedSettings,
    path: PathBuf,
    muted: bool,
}
impl AudioSettings {
    fn load(config: &crate::config::Config) -> Self {
        let path = config.asset_root.parent().unwrap_or(&config.asset_root).join("settings/audio.json");
        let saved = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<SavedSettings>(&bytes).unwrap_or_else(|e| {
                warn!("Audio settings: {e}");
                SavedSettings::default()
            }),
            Err(_) => SavedSettings::default(),
        }
        .validated();
        Self { saved, path, muted: config.mute }
    }
    /// Linear master gain (0 when muted).
    pub(crate) fn master(&self) -> f32 {
        if self.muted { 0.0 } else { self.saved.master as f32 / 100.0 }
    }
    pub(crate) fn category(&self, category: Category) -> f32 {
        let percent = match category {
            Category::Ambience => self.saved.ambience,
            Category::Effects => self.saved.effects,
        };
        percent as f32 / 100.0
    }
    fn field(&mut self, row: AudioRow) -> &mut u32 {
        match row {
            AudioRow::Master => &mut self.saved.master,
            AudioRow::Ambience => &mut self.saved.ambience,
            AudioRow::Effects => &mut self.saved.effects,
        }
    }
    /// Step a menu row by `direction` (wrapping 0..=100) and save; returns a status line.
    pub(crate) fn adjust(&mut self, row: AudioRow, direction: i32) -> String {
        let steps = (100 / STEP + 1) as i32;
        let value = self.field(row);
        *value = ((*value / STEP) as i32 + direction).rem_euclid(steps) as u32 * STEP;
        let save = (|| -> Result<(), String> {
            std::fs::create_dir_all(self.path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&self.path, serde_json::to_vec_pretty(&self.saved).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        })();
        match save {
            Ok(()) if self.muted => "Saved (audio is muted by --mute)".into(),
            Ok(()) => "Saved".into(),
            Err(e) => format!("Could not save: {e}"),
        }
    }
    pub(crate) fn label(&self, row: AudioRow) -> String {
        let (name, value) = match row {
            AudioRow::Master => ("Master volume", self.saved.master),
            AudioRow::Ambience => ("Ambience volume", self.saved.ambience),
            AudioRow::Effects => ("Effects volume", self.saved.effects),
        };
        if row == AudioRow::Master && self.muted {
            format!("{name:<22}{value}%  (muted)")
        } else {
            format!("{name:<22}{value}%")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioRow {
    Master,
    Ambience,
    Effects,
}

/// The one spatial listener (game and mod audio), following the gameplay camera.
#[derive(Component)]
struct GameAudioListener;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct CueSet;

pub(crate) struct GameAudioPlugin;
impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Voices>()
            .init_resource::<skate_events::Cues>()
            .add_systems(Startup, setup)
            .add_systems(FixedUpdate, skate_events::observe.after(crate::app::SimulationSet::Physics))
            .add_systems(Update, (ambience::update, skate_events::play, water::update).in_set(CueSet).after(crate::app::FrameSet::Animation))
            .add_systems(Update, voices::sync.after(CueSet))
            .add_systems(
                PostUpdate,
                (apply_global_volume, follow_camera).before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

fn setup(mut commands: Commands, config: Res<crate::config::Config>, mut assets: ResMut<Assets<AudioSource>>) {
    commands.insert_resource(AudioSettings::load(&config));
    commands.spawn((GameAudioListener, SpatialListener::new(0.2), Transform::default()));
    match Library::load(&config.asset_root) {
        Ok(mut library) => {
            let mut samples: Vec<(&str, &[usize])> = cues::ALL.iter().map(|c| (c.bank, c.samples)).collect();
            samples.extend(water::PRELOAD);
            let started = std::time::Instant::now();
            let count = library.preload(&mut assets, &samples);
            info!("Game audio: preloaded {count} clips in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
            commands.insert_resource(library);
        }
        Err(error) => info!("Game audio unavailable (run setup to extract it): {error}"),
    }
}

/// Mod voices scale by GlobalVolume, so the master volume and --mute apply to them too.
fn apply_global_volume(settings: Res<AudioSettings>, mut global: ResMut<GlobalVolume>) {
    let master = Volume::Linear(settings.master());
    if global.volume != master {
        global.volume = master;
    }
}

fn follow_camera(
    camera: Query<&Transform, (With<crate::camera::GameplayCamera>, Without<GameAudioListener>)>,
    mut listener: Query<&mut Transform, With<GameAudioListener>>,
) {
    if let (Some(camera), Ok(mut listener)) = (camera.iter().next(), listener.single_mut()) {
        *listener = *camera;
    }
}

/// True while game sound must be silent: menu open, replay running.
fn silenced(menu: Option<&crate::graphics_menu::Menu>, replay: &crate::replay::Replay) -> bool {
    menu.is_some_and(|m| m.open) || replay.active
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(master: u32) -> AudioSettings {
        AudioSettings {
            saved: SavedSettings { master, ..SavedSettings::default() },
            path: std::env::temp_dir().join(format!("skate-audio-test-{}-{master}/audio.json", std::process::id())),
            muted: false,
        }
    }

    #[test]
    fn defaults_are_quiet_and_saved_values_are_bounded() {
        assert_eq!(SavedSettings::default().master, 25);
        let loaded: SavedSettings = serde_json::from_str(r#"{"master":400,"ambience":33,"effects":7}"#).unwrap();
        assert_eq!(loaded.validated(), SavedSettings { master: 100, ambience: 30, effects: 5 });
    }

    #[test]
    fn adjust_wraps_in_steps_and_mute_wins() {
        let mut s = settings(95);
        s.adjust(AudioRow::Master, 1);
        assert_eq!(s.saved.master, 100);
        s.adjust(AudioRow::Master, 1);
        assert_eq!(s.saved.master, 0);
        s.adjust(AudioRow::Master, -1);
        assert_eq!(s.saved.master, 100);
        s.muted = true;
        assert_eq!(s.master(), 0.0);
        let _ = std::fs::remove_dir_all(s.path.parent().unwrap());
    }
}
