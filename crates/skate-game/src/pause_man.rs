//! The game's ThePauseMan (`skate_core::pause_man`, research b1053): a film's PauseSimulation, the NIS player and the
//! highlight reel push and pop its modes; the skater's simulation and the living world step while its world bit runs
//! (GameSession_StepSimulation). Audio is no client: it plays on over the frozen state.
use bevy::prelude::*;
use skate_core::pause_man::{PauseMan, bit};

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct ThePauseMan(pub PauseMan);

/// The worlds step (GameSession_StepSimulation's check, 8285C928).
pub(crate) fn world_steps(pause: Option<&ThePauseMan>) -> bool {
    pause.is_none_or(|p| !p.paused(bit::WORLD))
}
