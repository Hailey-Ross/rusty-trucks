//! Mod entry for the retail challenges (`sdk.challenges`): the map's challenges with their records, the run going,
//! start / abort, hiding a challenge and value overrides per mod, ended with the mod.
//! Details: sdk/ENGINE_API.md "Challenges", docs/hails-additions/42-challenges.md#mod-entry
use super::Mods;
use crate::challenges::{ChallengeSignUp, Challenges};
use bevy::prelude::*;
use serde_json::{json, Value};

pub(super) fn snapshot(world: &World) -> Value {
    let (Some(challenges), Some(map)) = (world.get_resource::<Challenges>(), world.get_resource::<crate::map_transition::CurrentMap>()) else {
        return json!({"on": false, "list": [], "run": null});
    };
    let list: Vec<Value> = challenges.on_map(map.path.as_deref()).map(|c| {
        let c = challenges.mods.apply(c.clone());
        let saved = challenges.records.get(&c.id);
        json!({
            "id": c.id, "family": c.family, "map": c.map, "title_id": c.title_id, "title": c.title,
            "position": &c.location.matrix[3][..3],
            "enabled": challenges.mods.enabled(&c.id),
            "owned_points": c.owned_points, "killed_points": c.killed_points,
            "completion": saved.completion, "high_score": saved.high_score, "discovery": saved.discovery,
        })
    }).collect();
    let run = challenges.run.as_ref().map(|r| json!({
        "id": r.challenge.id,
        "points": r.objectives.points,
        "requirement": r.objectives.requirement,
        "personal_best": r.ots.personal_best(),
        "out_of_bounds": r.ots.out_of_bounds_ticks().map(|t| t as f32 / skate_core::challenges::timers::TICKS_PER_SECOND),
    }));
    let on = world.get_resource::<crate::living_world::LivingWorldSettings>().is_some_and(|l| l.user_opt_ins.challenges);
    json!({"on": on, "list": list, "run": run})
}

/// Queued challenge events as `on_event` to every running mod.
pub(super) fn dispatch(world: &mut World, mods: &mut Mods) {
    let Some(mut challenges) = world.get_resource_mut::<Challenges>() else { return };
    for event in std::mem::take(&mut challenges.mods.events) {
        mods.manager.dispatch("on_event", event);
    }
}

/// Signs the player up as the D-pad press does; the next update starts the run.
pub(super) fn start(world: &mut World, id: &str) -> Result<(), String> {
    if !world.get_resource::<crate::living_world::LivingWorldSettings>().is_some_and(|l| l.user_opt_ins.challenges) {
        return Err("challenges are switched off (Opt-Ins: Challenges)".into());
    }
    let map = world.resource::<crate::map_transition::CurrentMap>().path.clone();
    let challenges = world.get_resource::<Challenges>().ok_or("challenges are not ready")?;
    if let Some(run) = &challenges.run {
        return Err(format!("challenge {} is running", run.challenge.id));
    }
    if !challenges.on_map(map.as_deref()).any(|c| c.id == id) {
        return Err(format!("no challenge {id:?} on this map"));
    }
    world.write_message(ChallengeSignUp { body: crate::trigger_volumes::PLAYER.into(), challenge: id.to_owned() });
    Ok(())
}

pub(super) fn abort(world: &mut World) -> Result<(), String> {
    let mut challenges = world.get_resource_mut::<Challenges>().ok_or("challenges are not ready")?;
    if challenges.run.is_none() {
        return Err("no challenge is running".into());
    }
    challenges.mods.abort = true;
    Ok(())
}

/// sdk.challenges.place_photographer(): the anywhere photo's photographer at the player, as the phone's option does.
pub(super) fn place_photographer(world: &mut World) -> Result<(), String> {
    let mut p = world.get_resource_mut::<crate::ftl_host::AnywherePhotographer>().ok_or("challenges are not ready")?;
    if !p.state.active {
        return Err("no anywhere photo is running".into());
    }
    p.place_requested = true;
    Ok(())
}

pub(super) fn enable(world: &mut World, owner: &str, id: &str, enabled: bool) -> Result<(), String> {
    let mut challenges = world.get_resource_mut::<Challenges>().ok_or("challenges are not ready")?;
    let owners = challenges.mods.disabled.entry(id.to_owned()).or_default();
    if enabled {
        owners.remove(owner);
    } else {
        owners.insert(owner.to_owned());
    }
    challenges.mods.disabled.retain(|_, o| !o.is_empty());
    Ok(())
}

/// Overrides apply from the next start; a run going keeps the values it started with.
pub(super) fn configure(world: &mut World, owner: &str, id: String, options: Option<skate_mods::ChallengeOptions>) -> Result<(), String> {
    let mut challenges = world.get_resource_mut::<Challenges>().ok_or("challenges are not ready")?;
    match options {
        Some(o) => { challenges.mods.overrides.insert((owner.to_owned(), id), o); }
        None => { challenges.mods.overrides.remove(&(owner.to_owned(), id)); }
    }
    Ok(())
}

/// The id a mod's challenge has in the list: `mod:<owner>/<id>`.
pub(crate) fn mod_challenge_id(owner: &str, id: &str) -> String {
    format!("mod:{owner}/{id}")
}

/// A box volume of a mod's challenge (challenge group), its id from the challenge id and the part.
fn volume(full: &str, part: &str, b: &skate_mods::ChallengeBox) -> crate::challenges::Volume {
    let name = format!("{full}/{part}");
    let id = format!("{:016x}", name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01b3)));
    let (s, c) = b.yaw.sin_cos();
    let record = skate_data::trigger_volumes::TriggerVolumeRecord {
        id: id.clone(), name: name.clone(), full_name: None, instance_id: None, link_guid: None,
        group: skate_data::trigger_volumes::TriggerGroup::Challenge,
        shape: skate_data::trigger_volumes::TriggerShape::Box {
            center: b.center, axes: [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]], half_extents: b.size.map(|v| v * 0.5), fatness: 0.0,
        },
        aabb: None, stream: None, arena: None,
    };
    crate::challenges::Volume { name, id, volume: record }
}

fn place(full: &str, part: &str, p: &skate_mods::ChallengePlace) -> crate::challenges::Start {
    let (s, c) = p.yaw.sin_cos();
    let [x, y, z] = p.position;
    crate::challenges::Start { locator: format!("{full}/{part}"), matrix: [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [x, y, z, 1.0]] }
}

/// A mod's own the spot challenge (`sdk.challenges.add`, ours): built as an export row (family `own_the_spots`, type
/// 9; retail's own the spot values where the mod gives none) and added to the list; its boundary is armed at once when
/// it is on the loaded map, so discovery and the sign-up work right away.
pub(super) fn add(world: &mut World, owner: &str, id: &str, d: skate_mods::ChallengeDefinition) -> Result<(), String> {
    let full = mod_challenge_id(owner, id);
    let map = world.get_resource::<crate::map_transition::CurrentMap>().map(|m| m.name.clone()).unwrap_or_default();
    let spot = volume(&full, "spot", &d.spot);
    let start = place(&full, "start", &d.start);
    let c = crate::challenges::Challenge {
        id: full.clone(), family: "own_the_spots".into(), map: d.map.clone().unwrap_or(map), title_id: None, title: Some(d.title.clone()),
        description_id: None, description_arg: 0, objectives_enabled: false,
        boundary: volume(&full, "boundary", &d.boundary),
        scoring: d.scoring.as_ref().map_or_else(|| volume(&full, "scoring", &d.spot), |b| volume(&full, "scoring", b)),
        spot,
        location: d.sign_up.as_ref().map_or_else(|| place(&full, "sign_up", &d.start), |p| place(&full, "sign_up", p)),
        start,
        sign_up_radius: d.sign_up_radius.unwrap_or(10.0), discovery_radius: d.discovery_radius.unwrap_or(100.0),
        owned_points: d.owned_points.unwrap_or(1000), killed_points: d.killed_points.unwrap_or(1500), challenge_type: Some(9),
        out_of_bounds_time: d.out_of_bounds_time.or(Some(5)), owned_display_time: d.owned_display_time, killed_display_time: d.killed_display_time,
        return_to_area_message: None, objectives: None,
    };
    let path = world.get_resource::<crate::map_transition::CurrentMap>().and_then(|m| m.path.clone());
    let here = path.as_deref().is_some_and(|p| crate::teleport_menu::same_map(p, &c.map));
    let boundary = c.boundary.volume.clone();
    let mut challenges = world.get_resource_mut::<Challenges>().ok_or("challenges are not ready")?;
    if challenges.run.as_ref().is_some_and(|r| r.challenge.id == full) {
        return Err(format!("challenge {full} is running"));
    }
    challenges.retail.retain(|r| r.id != full);
    challenges.retail.push(c);
    challenges.mods.added.insert(full.clone(), owner.to_owned());
    if here {
        if let Some(mut volumes) = world.get_resource_mut::<crate::trigger_volumes::TriggerVolumes>() {
            volumes.map.retain(|v| v.id != boundary.id);
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(&boundary));
        }
    }
    info!("MODS {owner} challenge {full} added");
    Ok(())
}

/// Take one of the mod's challenges out (a run of it is aborted).
pub(super) fn remove(world: &mut World, owner: &str, id: &str) -> Result<(), String> {
    let full = mod_challenge_id(owner, id);
    remove_full(world, &full);
    Ok(())
}

fn remove_full(world: &mut World, full: &str) {
    let mut ids = Vec::new();
    if let Some(mut challenges) = world.get_resource_mut::<Challenges>() {
        if challenges.run.as_ref().is_some_and(|r| r.challenge.id == full) {
            challenges.mods.abort = true;
        }
        if let Some(c) = challenges.retail.iter().find(|r| r.id == full) {
            ids = vec![c.boundary.id.clone(), c.spot.id.clone(), c.scoring.id.clone()];
        }
        challenges.retail.retain(|r| r.id != full);
        challenges.mods.added.remove(full);
    }
    if let Some(mut volumes) = world.get_resource_mut::<crate::trigger_volumes::TriggerVolumes>() {
        volumes.map.retain(|v| !ids.contains(&v.id));
    }
}

pub(super) fn clear_owner(world: &mut World, owner: &str) {
    let added: Vec<String> = world.get_resource::<Challenges>()
        .map(|c| c.mods.added.iter().filter(|(_, o)| o.as_str() == owner).map(|(id, _)| id.clone()).collect()).unwrap_or_default();
    for full in added {
        remove_full(world, &full);
    }
    if let Some(mut challenges) = world.get_resource_mut::<Challenges>() {
        challenges.mods.clear_owner(owner);
    }
}

pub(super) fn clear(world: &mut World) {
    if let Some(mut challenges) = world.get_resource_mut::<Challenges>() {
        challenges.mods = crate::challenges::ModControl::default();
    }
}

#[cfg(test)]
mod tests {
    use crate::challenges::ModControl;

    #[test]
    fn overrides_and_hiding_belong_to_one_mod_and_end_with_it() {
        let mut m = ModControl::default();
        m.disabled.entry("ots_a".into()).or_default().insert("one".into());
        m.disabled.entry("ots_a".into()).or_default().insert("two".into());
        m.overrides.insert(("one".into(), "ots_a".into()), skate_mods::ChallengeOptions { owned_points: Some(10), ..Default::default() });
        m.clear_owner("one");
        assert!(!m.enabled("ots_a"));
        assert!(m.overrides.is_empty());
        m.clear_owner("two");
        assert!(m.enabled("ots_a"));
    }

    /// `sdk.challenges.add`: the mod's challenge joins the loaded map's list as an own the spot row (its boundary
    /// armed at once, retail values where the mod gave none) and goes, volume and all, when the mod stops.
    #[test]
    fn a_mods_own_challenge_joins_the_map_and_goes_with_the_mod() {
        use bevy::prelude::*;
        let mut w = World::new();
        w.insert_resource(crate::challenges::Challenges::default());
        w.insert_resource(crate::trigger_volumes::TriggerVolumes::default());
        w.insert_resource(crate::map_transition::CurrentMap {
            path: Some("maps/DownTown.glb".into()), name: "DownTown".into(), spawn: [0.0; 3], heading: 0.0, generation: 1, audio_tag: None, layout: None,
        });
        let b = |center: [f32; 3], size: [f32; 3]| skate_mods::ChallengeBox { center, size, yaw: 0.0 };
        let d = skate_mods::ChallengeDefinition {
            title: "Plaza".into(), boundary: b([0.0; 3], [80.0, 20.0, 80.0]), spot: b([5.0, 0.0, 5.0], [10.0, 4.0, 10.0]),
            start: skate_mods::ChallengePlace { position: [0.0, 0.0, -20.0], yaw: 0.0 }, owned_points: Some(500), ..Default::default()
        };
        super::add(&mut w, "dev.a", "plaza", d).unwrap();
        let c = w.resource::<crate::challenges::Challenges>();
        let map = w.resource::<crate::map_transition::CurrentMap>().path.clone();
        let row = c.on_map(map.as_deref()).find(|r| r.id == "mod:dev.a/plaza").expect("on the loaded map");
        assert_eq!((row.family.as_str(), row.owned_points, row.killed_points, row.sign_up_radius), ("own_the_spots", 500, 1500, 10.0));
        assert_eq!(row.location.matrix[3][..3], [0.0, 0.0, -20.0], "the sign-up is the start");
        let boundary = row.boundary.id.clone();
        assert!(w.resource::<crate::trigger_volumes::TriggerVolumes>().get(&boundary).is_some(), "armed at once");
        super::clear_owner(&mut w, "dev.a");
        assert!(w.resource::<crate::challenges::Challenges>().retail.is_empty());
        assert!(w.resource::<crate::trigger_volumes::TriggerVolumes>().get(&boundary).is_none());
    }

    #[test]
    fn events_keep_the_newest() {
        let mut m = ModControl::default();
        for i in 0..100 {
            m.event(serde_json::json!(i));
        }
        assert_eq!(m.events.len(), 64);
        assert_eq!(m.events[0], serde_json::json!(36));
    }
}
