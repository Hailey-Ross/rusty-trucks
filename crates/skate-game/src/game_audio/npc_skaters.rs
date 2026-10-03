//! NPC (AI) skaters' board sounds on the native runtime (`skate_audio::world::skaters`): the
//! local player's components run for the MixMap Player slot's second instance, for the NPC skater
//! retail's `CSTATEMGR_Player` would give it to (the first one, in the skater list's order, within
//! 30 m of the camera; spec `.claude/notes/world-npc-skater-audio.md`). **Inert until an AI-skater
//! system publishes skaters**: [`NpcSkaters`] stays empty and [`frame`] returns at once.
//! `SKATE_AEMS_NPC_SKATERS=0` turns it off even with skaters.
//!
//! The hook for a future AI-skater system: each frame, fill [`NpcSkaters::skaters`] with every
//! live NPC skater in its list order (stable `id`, an `AudioState` filled like the local player's
//! `skate_events::audio_state`); drop the ones that despawn. Everything else happens here, per
//! MixMap evaluation, after `native::mixmap_frame` (like `world_sources.rs`, so the inputs an
//! evaluation sees are one console frame old — the seam to move into `mixmap_frame` once a system
//! exists):
//! - the instance assignment (`skaters::Slots`);
//! - the held skater's components `update` from this evaluation's outputs, then its inputs and
//!   `process` for the next one, with the local player's tuning and banks (the components post
//!   into the banks the local player's host loaded);
//! - its collision messages go to the local player's collision manager (retail's one
//!   `CSTATEMGR_Collision`).
//!
//! Not yet: the NPC's granular rolling bed (the routing's grain binds are dropped: the runtime has
//! one bed), wheels, tricks, footsteps, clothing (module docs of `skaters`).
use std::collections::HashMap;

use bevy::prelude::*;
use skate_audio::eval::NodeId;
use skate_audio::mixmap::cadence::CONSOLE_DT;
use skate_audio::player::components::{Command, Slot};
use skate_audio::player::objpos::Listener;
use skate_audio::world::skaters::{self, NpcSkater, NpcSkaterAudioState, Parts, Slots, Tuning};

use super::native::Native;

/// What an AI-skater system publishes each frame (empty: nothing plays), in its skater list's
/// order.
#[derive(Resource, Default)]
pub(crate) struct NpcSkaters {
    pub(crate) skaters: Vec<NpcSkaterAudioState>,
}

/// `SKATE_AEMS_NPC_SKATERS=0` keeps the NPC skaters' board sounds off.
fn requested() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("SKATE_AEMS_NPC_SKATERS").is_ok_and(|v| v == "0"))
}

fn on(var: &str) -> bool {
    !std::env::var(var).is_ok_and(|v| v == "0")
}

#[derive(Resource, Default)]
pub(crate) struct NpcHost {
    slots: Slots,
    objects: HashMap<u64, NpcSkater>,
    nodes: HashMap<(u64, Slot), NodeId>,
    classes: HashMap<&'static str, usize>,
    last_tick: u64,
    last_camera: Option<[f32; 3]>,
    announced: bool,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<NpcSkaters>().init_resource::<NpcHost>().add_systems(Update, frame.after(super::native::mixmap_frame));
}

impl NpcHost {
    /// Apply one skater's commands to the runtime (its packets keyed by skater and slot).
    pub(crate) fn apply(&mut self, rt: &mut skate_audio::runtime::Runtime, owner: u64, cmds: Vec<Command>) {
        for cmd in cmds {
            match cmd {
                Command::Post { slot, class, words } => {
                    let id = *self.classes.entry(class).or_insert_with(|| rt.eval.class_id(class).unwrap_or(usize::MAX));
                    if id == usize::MAX {
                        continue;
                    }
                    if let Some(old) = self.nodes.remove(&(owner, slot)) {
                        rt.release(old);
                    }
                    self.nodes.insert((owner, slot), rt.post(id, &words));
                }
                Command::Redeliver { slot, words } => {
                    if let Some(&node) = self.nodes.get(&(owner, slot)) {
                        rt.redeliver(node, &words);
                    }
                }
                Command::Release { slot } => {
                    if let Some(node) = self.nodes.remove(&(owner, slot)) {
                        rt.release(node);
                    }
                }
            }
        }
    }

    /// Release every packet a skater holds (it lost its instance).
    fn release_all(&mut self, rt: &mut skate_audio::runtime::Runtime, owner: u64) {
        let slots: Vec<(u64, Slot)> = self.nodes.keys().filter(|k| k.0 == owner).copied().collect();
        for k in slots {
            if let Some(node) = self.nodes.remove(&k) {
                rt.release(node);
            }
        }
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn frame(
    native: Option<ResMut<Native>>,
    published: Res<NpcSkaters>,
    mut host: ResMut<NpcHost>,
    cues: Res<super::skate_events::Cues>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
) {
    // Inert: nothing published and nothing held.
    if published.skaters.is_empty() && host.objects.is_empty() {
        return;
    }
    if !requested() {
        return;
    }
    let Some(mut native) = native else { return };
    let Ok(camera) = listener.single() else { return };
    let host = &mut *host;
    let Native { mixmap, player, shared, .. } = &mut *native;
    let (Some(m), Some(player)) = (mixmap.as_mut(), player.as_mut()) else { return };
    if !player.components {
        return;
    }
    if m.ticks == host.last_tick {
        return;
    }
    let evaluations = m.ticks - host.last_tick;
    host.last_tick = m.ticks;
    let dt = CONSOLE_DT * evaluations.min(4) as f32;
    if !host.announced {
        info!("AUDIO_NPC on: {} NPC skaters published", published.skaters.len());
        host.announced = true;
    }
    let cam = camera.translation().to_array();
    let cam_velocity = host.last_camera.map_or([0.0; 3], |last| std::array::from_fn(|i| (cam[i] - last[i]) / dt));
    host.last_camera = Some(cam);
    let local = cues.riding.audio;
    let l = Listener {
        camera: cam,
        view: camera.forward().as_vec3().to_array(),
        camera_velocity: cam_velocity,
        followed: local.com_position,
        facing: local.com_velocity,
        followed_velocity: local.com_velocity,
    };
    let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) else { return };
    let rt = &mut *runtime;

    let candidates: Vec<(u64, f32)> = published.skaters.iter().map(|p| (p.id, distance(p.state.com_position, cam))).collect();
    let assignment = host.slots.assign(&candidates);
    for (id, g) in assignment.released {
        if let Some(mut npc) = host.objects.remove(&id) {
            npc.deactivate(m, &l);
        }
        host.release_all(rt, id);
        info!("AUDIO_NPC release skater {id} (instance {g})");
    }
    let parts = Parts { rolling: player.rolling_on, rattle: player.rattle_on, contacts: player.contacts_on };
    for (id, g) in assignment.claimed {
        let npc = NpcSkater::new(g as u32, parts, on("SKATE_AEMS_GRIND_ONOFF"), on("SKATE_AEMS_PLANT_LIFT"), on("SKATE_AEMS_BODY_IMPACTS"));
        host.objects.insert(id, npc);
        info!("AUDIO_NPC claim skater {id} (instance {g})");
    }

    let tuning = Tuning { player: &player.tuning, contacts: &player.contact_tuning };
    let mut collisions = Vec::new();
    let held: Vec<(u32, u64)> = host.slots.holders().collect();
    for (_, id) in held {
        let Some(p) = published.skaters.iter().find(|p| p.id == id) else { continue };
        let Some(mut npc) = host.objects.remove(&id) else { continue };
        let mut s = skaters::component_state(&p.state, local.soft_wheels);
        s.dt = dt;
        let cmds = npc.update(m, &s, tuning, &mut rt.splice_host());
        host.apply(rt, id, cmds);
        npc.write_inputs(m, &s, &l, local.com_velocity, tuning.player);
        let cmds = npc.process(m, &s, tuning, &mut rt.splice_host());
        host.apply(rt, id, cmds);
        // No per-owner grain bed yet: the routing's binds are dropped.
        npc.routed.grains.clear();
        collisions.extend(npc.take_collisions());
        host.objects.insert(id, npc);
    }
    player.post_collisions(collisions, rt);
}

#[cfg(test)]
mod tests;
