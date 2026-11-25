//! The mouse cursor hides over the game window after a few seconds without mouse movement or clicks, and shows again
//! the moment the mouse moves (upstream issue 69). Ours: retail is a console game with no cursor; the delay is a
//! setting (`SKATE_CURSOR_HIDE_SECONDS`, 0 keeps the cursor shown).

use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

pub(crate) const DEFAULT_HIDE_SECONDS: f32 = 2.0;

#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct CursorAutoHide {
    /// Seconds without mouse input before the cursor hides; 0 never hides it.
    pub seconds: f32,
    idle: f32,
}

impl Default for CursorAutoHide {
    fn default() -> Self {
        let seconds = std::env::var("SKATE_CURSOR_HIDE_SECONDS").ok().and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(DEFAULT_HIDE_SECONDS);
        Self { seconds, idle: 0.0 }
    }
}

impl CursorAutoHide {
    /// Advances the idle time and answers whether the cursor shows.
    fn step(&mut self, mouse_used: bool, dt: f32) -> bool {
        self.idle = if mouse_used { 0.0 } else { self.idle + dt };
        self.seconds <= 0.0 || self.idle < self.seconds
    }
}

pub(crate) struct CursorAutoHidePlugin;

impl Plugin for CursorAutoHidePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorAutoHide>().add_systems(Update, update);
    }
}

fn update(
    mut hide: ResMut<CursorAutoHide>,
    time: Res<Time>,
    (mut motion, mut buttons, mut wheel): (MessageReader<MouseMotion>, MessageReader<MouseButtonInput>, MessageReader<MouseWheel>),
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let used = motion.read().count() + buttons.read().count() + wheel.read().count() > 0;
    let show = hide.step(used, time.delta_secs());
    for (window, mut cursor) in &mut windows {
        // An unfocused window leaves the cursor alone (it belongs to the other app).
        let want = show || !window.focused;
        if cursor.visible != want {
            cursor.visible = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_hides_after_the_idle_seconds_and_shows_on_mouse_input() {
        let mut h = CursorAutoHide { seconds: 2.0, idle: 0.0 };
        assert!(h.step(false, 1.0));
        assert!(!h.step(false, 1.5), "hidden after 2 s without mouse input");
        assert!(h.step(true, 0.016), "mouse input shows it at once");
        let mut never = CursorAutoHide { seconds: 0.0, idle: 0.0 };
        assert!(never.step(false, 100.0), "0 keeps the cursor shown");
    }
}
