//! The game's ThePauseMan (`skate_core::pause_man`, research b1053): a film's PauseSimulation, the NIS player and the
//! highlight reel push and pop its modes; the skater's simulation and the living world step while its world bit runs
//! (GameSession_StepSimulation). Audio is no client: it plays on over the frozen state. Mods hold their own pauses
//! by owner (`sdk.world.pause_simulation`, ours), released when the mod stops.
use bevy::prelude::*;
use skate_core::pause_man::{PauseMan, bit};
use std::collections::BTreeSet;

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct ThePauseMan {
    #[deref]
    pub man: PauseMan,
    /// The mods holding a pause of the world step.
    pub mods: BTreeSet<String>,
}

impl ThePauseMan {
    /// A mod holds (`on`) or releases its pause; true when that changed anything.
    pub(crate) fn hold_for_mod(&mut self, owner: &str, on: bool) -> bool {
        if on { self.mods.insert(owner.to_owned()) } else { self.mods.remove(owner) }
    }
}

/// The worlds step (GameSession_StepSimulation's check, 8285C928; and no mod holds a pause).
pub(crate) fn world_steps(pause: Option<&ThePauseMan>) -> bool {
    pause.is_none_or(|p| !p.paused(bit::WORLD) && p.mods.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mod_pause_holds_by_owner_and_does_not_touch_the_retail_modes() {
        let mut pm = ThePauseMan::default();
        assert!(world_steps(Some(&pm)));
        assert!(pm.hold_for_mod("dev.a", true));
        assert!(!pm.hold_for_mod("dev.a", true), "already held");
        pm.hold_for_mod("dev.b", true);
        assert!(!world_steps(Some(&pm)));
        pm.hold_for_mod("dev.a", false);
        assert!(!world_steps(Some(&pm)), "dev.b still holds");
        pm.push(skate_core::pause_man::mode::SIMULATION);
        pm.hold_for_mod("dev.b", false);
        assert!(!world_steps(Some(&pm)), "the film's mode 5 is untouched");
        pm.pop(skate_core::pause_man::mode::SIMULATION);
        assert!(world_steps(Some(&pm)));
    }
}
