//! Pedestrian body (doc 26, peds milestone M2): which entity / model a census spawn becomes, the
//! ped animation player and the foot events the audio reads. Pure and engine-independent (no
//! ECS, no I/O); `skate-data::ped_anim` fills the data types from the user's export, the game
//! (`skate-game::living_world::peds`) hosts it.
//!
//! Retail reference (TU3, addresses are evidence only):
//! - entity inside the census category: `sub_826B8B88` after the category roll; the draw is
//!   `sub_826BB058` (world RNG u32 x 2^-32, `0x822F88F4`), the index
//!   `trunc(draw x 100) % entities.len()` (`0x820ED57C` = 100) [code];
//! - model tints: `sub_827B4170`: one rand `r`, `tints_a[r % na]` and `tints_b[r % nb]` [code];
//! - clips: `PedestrianSkeletonPres.abin`, additive over its `PEDESTRIAN_RIG_TPOSE` pose record
//!   (the clips hold 6 of the rig's 10 parts: bones 0..=26) [data];
//! - foot plants: the clips' `LEFTTOEDOWN` / `RIGHTTOEDOWN` attributes (phase windows), the
//!   body falls `BODYFALLTYPE` (value windows) [data]; retail's audio reads them as `S+74` /
//!   `S+73` and `S+76` (`world-ped-audio.md`).
//!
//! Multiplayer: a ped's look is a pure function of its spawn record (category + seed) and the
//! catalog; its animation steps once per world tick from that seed, so the state follows from
//! the spawn record and the tick.

pub mod anim;
pub mod choice;

pub use anim::{Locomotion, PedAnimPlayer, PedAnimSet, PedClip, PedEvaluator, PedFrame, PedRig};
pub use choice::{PedCatalog, PedEntity, PedLook, PedModel, PedOverrides};

/// Map one rig's bone names onto another's by name (case-insensitive). Returns, per `target`
/// bone, the index in `source` (or `None`), plus the source bones no target uses.
pub fn match_bones(target: &[String], source: &[String]) -> (Vec<Option<usize>>, Vec<String>) {
    let map: Vec<Option<usize>> = target.iter().map(|t| source.iter().position(|s| s.eq_ignore_ascii_case(t))).collect();
    let unused = source.iter().enumerate().filter(|(i, _)| !map.contains(&Some(*i))).map(|(_, s)| s.clone()).collect();
    (map, unused)
}

#[cfg(test)]
mod tests;
