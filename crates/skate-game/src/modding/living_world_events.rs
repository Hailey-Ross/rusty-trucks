//! Living-world events for mods (doc 26, "Modding"): `on_event {name = "living_world", event = ...}`.
//!
//! - `spawn` / `despawn`: every NPC skater, ped and vehicle the population creates or removes, with
//!   the same serialisable fields a future host sends (`WireRecord`: kind, stable id, tick,
//!   position, heading, seed, the choice; for a despawn the reason);
//! - `npc_trick`: an NPC skater's trick choice (`id`, `line`, `node`, `recorded`, `chosen` as
//!   catalog names);
//! - `npc_line_end`: an NPC skater ran out of line;
//! - `vehicle_contact`: a car pushed a ped (`VehicleContactEvent`).
//!
//! Engine-facing first: the same messages drive the engine systems; this only forwards them. Extends
//! engine modding; there is no retail to match.

use super::Mods;
use crate::living_world::npc_skaters::{trick_name, NpcSkaterEvent};
use crate::living_world::vehicle_contacts::VehicleContactEvent;
use crate::living_world::{LivingWorldDespawn, LivingWorldSpawn, WireRecord};
use bevy::prelude::*;
use serde_json::{json, Value};
use skate_core::living_world::Decision;

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, forward.after(crate::app::FrameSet::Animation).before(super::update));
}

fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// The event payloads for this frame's messages (also used by the tests).
pub(crate) fn payloads(
    spawns: impl IntoIterator<Item = LivingWorldSpawn>,
    despawns: impl IntoIterator<Item = LivingWorldDespawn>,
    npc: impl IntoIterator<Item = NpcSkaterEvent>,
    contacts: impl IntoIterator<Item = VehicleContactEvent>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let record = |d: Decision| serde_json::to_value(WireRecord::from_decision(&d)).unwrap_or(Value::Null);
    for s in spawns {
        out.push(json!({"name": "living_world", "event": "spawn", "record": record(Decision::Spawn(s.0))}));
    }
    for d in despawns {
        out.push(json!({"name": "living_world", "event": "despawn", "record": record(Decision::Despawn(d.0))}));
    }
    for e in npc {
        match e {
            NpcSkaterEvent::Trick { id, record } => out.push(json!({
                "name": "living_world", "event": "npc_trick", "id": id.to_u64(), "line": hex(&record.line), "node": record.node,
                "recorded": trick_name(record.recorded), "chosen": trick_name(record.chosen),
            })),
            NpcSkaterEvent::LineEnd { id } => out.push(json!({"name": "living_world", "event": "npc_line_end", "id": id.to_u64()})),
            _ => {}
        }
    }
    for c in contacts {
        out.push(json!({"name": "living_world", "event": "vehicle_contact", "contact": serde_json::to_value(&c).unwrap_or(Value::Null)}));
    }
    out
}

fn forward(
    mods: Option<ResMut<Mods>>,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    mut npc: MessageReader<NpcSkaterEvent>,
    mut contacts: MessageReader<VehicleContactEvent>,
) {
    let Some(mut mods) = mods else {
        spawns.clear();
        despawns.clear();
        npc.clear();
        contacts.clear();
        return;
    };
    for payload in payloads(spawns.read().cloned(), despawns.read().cloned(), npc.read().cloned(), contacts.read().cloned()) {
        mods.manager.dispatch("on_event", payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::living_world::{DespawnReason, DespawnRecord, Kind, LivingWorldId};

    #[test]
    fn living_world_messages_become_mod_events() {
        let id = LivingWorldId { kind: Kind::Pedestrian, serial: 7 };
        let out = payloads(
            [],
            [LivingWorldDespawn(DespawnRecord { id, tick: 12, reason: DespawnReason::External })],
            [NpcSkaterEvent::Trick {
                id: LivingWorldId { kind: Kind::Skater, serial: 2 },
                record: skate_core::living_world::replay::TrickRecord { frame: 3, line: [0xab; 16], node: 9, recorded: 128, chosen: 96 },
            }],
            [],
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["event"], "despawn");
        assert_eq!(out[0]["name"], "living_world");
        assert!(out[0]["record"].is_object(), "{}", out[0]);
        assert_eq!(out[1]["event"], "npc_trick");
        assert_eq!((out[1]["recorded"].as_str(), out[1]["chosen"].as_str()), (Some("ollie"), Some("kickflip")));
        assert_eq!(out[1]["node"], 9);
    }
}
