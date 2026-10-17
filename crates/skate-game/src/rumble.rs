//! Controller rumble on the pads: the TU3 rumble controller (`skate_core::rumble`) ticked at retail's 60 Hz
//! whatever the frame rate, gated by Game Settings > Control > Vibration (settings +163), sent through the pad
//! backend (`input::platform::set_rumble`). Who starts a rumble: switching Vibration on plays retail's test
//! rumble (0.5 / 0.5, rate 5.0, `sub_8260DF40`) on the active pad; mods start and stop channels
//! (`sdk.input.rumble` / `sdk.input.stop_rumble`, capability `input_rumble`). NOT PORTED YET: retail's gameplay rumble (the posters `sub_82BB8CF0` 0.5 / 0.5 / 1.2 and the
//! per-item channels of `sub_8259E3D0`, events not identified; research `.local/research/rumble.md`).
use bevy::prelude::*;
use skate_core::rumble::{motor_speed, PadRumble, Rumble, RumbleTuning, TICK_HZ};

const VIBRATION: &str = "ID_GAMESETTINGS_VIBRATION_TOGGLE";
/// Retail's settings test rumble (`sub_8260DF40`): channel 0, 0.5 / 0.5 at 0x8209975C, rate 5.0 at 0x821F1790.
const TEST: Rumble = Rumble { low: 0.5, high: 0.5, rate: 5.0 };

#[derive(Resource)]
pub(crate) struct RumbleOutput {
    pads: [PadRumble; 4],
    pub tuning: RumbleTuning,
    /// Real seconds not yet spent on 60 Hz ticks.
    clock: f32,
    levels: [[f32; 2]; 4],
    /// The Vibration setting as last seen (None before the first frame).
    vibration: Option<bool>,
    /// Channels each mod started (pad, internal channel), stopped when the mod goes.
    owners: std::collections::BTreeMap<String, std::collections::BTreeSet<(usize, u32)>>,
}

/// Mod channels live above retail's: bit 31 set, the mod id's FNV-1a hash in bits 10..31, its own channel (0..1023)
/// below, so two mods never share a channel.
fn mod_channel(owner: &str, channel: u32) -> u32 {
    let hash = owner.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    0x8000_0000 | ((hash & 0x1f_ffff) << 10) | (channel & 0x3ff)
}

impl Default for RumbleOutput {
    fn default() -> Self {
        Self {
            pads: std::array::from_fn(|_| PadRumble::new(true)),
            tuning: RumbleTuning::RETAIL,
            clock: 0.0,
            levels: [[0.0; 2]; 4],
            vibration: None,
            owners: Default::default(),
        }
    }
}

impl RumbleOutput {
    /// Start (or restart) a channel on a pad; ignored while Vibration is off.
    pub(crate) fn start(&mut self, pad: usize, channel: u32, rumble: Rumble) {
        if let Some(p) = self.pads.get_mut(pad) {
            p.start(channel, rumble, &self.tuning);
        }
    }

    /// `sdk.input.rumble`: a mod's channel on a pad (None = the active pad).
    pub(crate) fn start_mod(&mut self, owner: &str, pad: usize, channel: u32, rumble: Rumble) {
        info!("RUMBLE mod {owner} pad {pad} channel {channel}: low {} high {} rate {}{}", rumble.low, rumble.high, rumble.rate,
            if self.vibration == Some(false) { " (Vibration is off: not started)" } else { "" });
        let internal = mod_channel(owner, channel);
        self.owners.entry(owner.to_owned()).or_default().insert((pad, internal));
        self.start(pad, internal, rumble);
    }

    /// `sdk.input.stop_rumble`: a mod's channels stop (one pad, or all with None).
    pub(crate) fn stop_mod(&mut self, owner: &str, pad: Option<usize>) {
        let Some(set) = self.owners.get_mut(owner) else { return };
        if !set.is_empty() {
            info!("RUMBLE mod {owner} stopped (pad {pad:?})");
        }
        set.retain(|&(p, channel)| {
            let hit = pad.is_none_or(|q| q == p);
            if hit {
                self.pads[p].stop(channel);
            }
            !hit
        });
    }

    /// Advance by `seconds` of real time at the 60 Hz retail tick; Vibration On / Off as given. Returns whether
    /// the setting just switched on (the caller plays the test rumble).
    fn advance(&mut self, seconds: f32, vibration: bool) -> bool {
        let switched_on = self.vibration == Some(false) && vibration;
        if self.vibration != Some(vibration) {
            info!("RUMBLE Vibration {}", if vibration { "on" } else { "off" });
        }
        self.vibration = Some(vibration);
        for pad in &mut self.pads {
            pad.enabled = vibration;
        }
        self.clock = (self.clock + seconds).min(0.25);
        while self.clock >= 1.0 / TICK_HZ {
            self.clock -= 1.0 / TICK_HZ;
            for (pad, levels) in self.pads.iter_mut().zip(&mut self.levels) {
                *levels = pad.tick(&self.tuning);
            }
        }
        switched_on
    }
}

pub(crate) struct RumblePlugin;

impl Plugin for RumblePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RumbleOutput>().add_systems(Update, update);
    }
}

/// Retail's known-device curve is the 360 pad's; Xbox-family pads get it, other pads retail's other-device curve.
fn xbox_pad(kind: Option<&crate::input::controller_kind::ControllerKind>) -> bool {
    use crate::input::controller_kind::Family;
    kind.is_some_and(|k| matches!(k.family, Family::Xbox360 | Family::XboxOne | Family::XboxElite | Family::XinputGamepad))
}

fn update(
    time: Res<Time<Real>>,
    menus: Option<Res<crate::retail_menus::RetailMenus>>,
    controllers: Res<crate::input::ControllerInput>,
    mut out: ResMut<RumbleOutput>,
    mut running: Local<[bool; 4]>,
) {
    let vibration = menus
        .as_deref()
        .and_then(|m| crate::retail_menus::setting(m, VIBRATION))
        .is_none_or(|v| skate_core::menu_values::number(&v) != 0.0);
    if out.advance(time.delta_secs(), vibration) {
        match controllers.active_slot() {
            Some(pad) => {
                info!("RUMBLE settings test rumble on pad {pad} (low 0.5 high 0.5 rate 5)");
                out.start(pad, 0, TEST);
            }
            None => info!("RUMBLE settings test rumble: no controller connected"),
        }
    }
    for pad in 0..4 {
        if controllers.status[pad] != crate::input::ControllerStatus::Ready {
            continue;
        }
        let gamepad = xbox_pad(controllers.kind(pad));
        let active = out.levels[pad] != [0.0, 0.0];
        if active != running[pad] {
            running[pad] = active;
            info!("RUMBLE pad {pad} {} ({} motor curve)", if active { "rumbling" } else { "idle" }, if gamepad { "gamepad" } else { "other-device" });
        }
        let speeds = std::array::from_fn(|m| motor_speed(m, out.levels[pad][m], gamepad, &out.tuning));
        crate::input::platform::set_rumble(pad, speeds);
    }
}

/// A mod stopped, failed or reloaded: its rumble stops at once.
pub(crate) fn stop_owner(world: &mut World, owner: &str) {
    if let Some(mut out) = world.get_resource_mut::<RumbleOutput>() {
        out.stop_mod(owner, None);
        out.owners.remove(owner);
    }
}

/// Every mod went (reload all).
pub(crate) fn clear_mods(world: &mut World) {
    if let Some(mut out) = world.get_resource_mut::<RumbleOutput>() {
        let owners: Vec<String> = out.owners.keys().cloned().collect();
        for owner in owners {
            out.stop_mod(&owner, None);
        }
        out.owners.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_channels_are_separate_and_stop_with_their_mod() {
        let mut out = RumbleOutput::default();
        out.advance(0.0, true);
        out.start_mod("a", 0, 1, Rumble { low: 0.4, high: 0.0, rate: 0.0 });
        out.start_mod("b", 0, 1, Rumble { low: 0.4, high: 0.0, rate: 0.0 });
        assert_ne!(mod_channel("a", 1), mod_channel("b", 1));
        out.advance(1.0 / 60.0, true);
        assert!(out.levels[0][0] > 0.4, "both mods' channels mix");
        out.stop_mod("a", None);
        out.advance(1.0 / 60.0, true);
        assert!((out.levels[0][0] - 0.4).abs() < 1e-6, "only b's channel is left");
        out.stop_mod("b", Some(0));
        out.advance(1.0 / 60.0, true);
        assert_eq!(out.levels[0][0], 0.0);
    }

    #[test]
    fn ticks_at_60_hz_whatever_the_frame_rate() {
        let at = |fps: f32| {
            let mut out = RumbleOutput::default();
            out.advance(0.0, true);
            out.start(0, 0, TEST);
            for _ in 0..(fps as usize / 2) {
                out.advance(1.0 / fps, true);
            }
            out.levels[0][0]
        };
        // Half a second = 30 ticks at any frame rate (to float rounding of the clock).
        let (a, b) = (at(30.0), at(240.0));
        assert!((a - b).abs() < 0.02, "{a} vs {b}");
        assert!(a > 0.0 && a < 0.5);
    }

    #[test]
    fn switching_vibration_on_reports_it_and_off_blocks_starts() {
        let mut out = RumbleOutput::default();
        assert!(!out.advance(0.0, true), "the first frame is not a switch");
        assert!(!out.advance(0.0, false));
        out.start(0, 0, TEST);
        out.advance(1.0 / 60.0, false);
        assert_eq!(out.levels[0], [0.0, 0.0], "no start while off");
        assert!(out.advance(0.0, true), "off -> on plays the test rumble");
    }
}
