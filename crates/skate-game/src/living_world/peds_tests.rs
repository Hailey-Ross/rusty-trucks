//! Headless, seeded tests of the ped body (doc 26, peds M2): spawn records become peds with the
//! seeded look, they animate and move from the record and the tick alone (any frame rate), they
//! publish foot plants, despawn cleans up, mod overrides apply. No window, no assets (one
//! data-gated test loads the real bank and tables).

use super::peds::*;
use super::*;
use skate_core::animation::output::Sqt;
use skate_core::living_world::peds::anim::{ClipWindow, IDENTITY, PedClip, RemapClip, names};
use skate_core::living_world::peds::{Locomotion, PedAnimPlayer, PedAnimSet, PedCatalog, PedEntity, PedModel, PedOverrides, PedRig};
use std::collections::BTreeMap;
use std::sync::Arc;

fn sqt(t: [f32; 3]) -> Sqt {
    Sqt { scale: [1.0; 4], rotation: [0.0, 0.0, 0.0, 1.0], translation: [t[0], t[1], t[2], 1.0] }
}

fn clip(name: &str, frames: usize, speed: f32, looping: bool, windows: Vec<ClipWindow>) -> PedClip {
    let len = (frames - 1) as f32 / 30.0;
    PedClip {
        name: name.into(),
        fps: 30.0,
        frames: (0..frames).map(|f| vec![sqt([0.0, 0.0, speed * f as f32 / 30.0]), IDENTITY, IDENTITY]).collect(),
        looping,
        loop_rotation: [0.0, 0.0, 0.0, 1.0],
        loop_translation: [0.0, 0.0, speed * len],
        windows,
    }
}

fn data() -> PedData {
    let w = |c: &str, b: f32, e: f32| ClipWindow { channel: c.into(), begin: b, end: e, value: 1.0 };
    let clips: BTreeMap<String, PedClip> = [
        clip("IDLE", 31, 0.0, true, vec![w("LEFTTOEDOWN", 0.0, 1.0), w("RIGHTTOEDOWN", 0.0, 1.0)]),
        clip("WALK", 34, 1.3, true, vec![w("LEFTTOEDOWN", 0.25, 0.75), w("RIGHTTOEDOWN", 0.0, 0.3), w("RIGHTTOEDOWN", 0.76, 1.0)]),
        clip("START", 28, 0.8, false, vec![]),
        clip("STOP", 30, 0.5, false, vec![]),
        clip("TURN", 43, 0.0, false, vec![]),
    ]
    .into_iter()
    .map(|c| (c.name.clone(), c))
    .collect();
    let r = |c: &str| vec![RemapClip { clip: c.into(), windows: vec![] }];
    let mut set = PedAnimSet::default();
    for (n, c) in [(names::IDLE, "IDLE"), (names::WALK, "WALK"), (names::START, "START"), (names::STOP, "STOP"), (names::TURN_180, "TURN")] {
        set.entries.insert(n.into(), r(c));
    }
    let mut catalog = PedCatalog::default();
    catalog.categories.insert("aletown".into(), vec!["jock02".into(), "bum02".into()]);
    catalog.categories.insert("empty".into(), vec![]);
    for (e, m, recipe, voice) in [("jock02", "jock02", "male_jock_2", 55), ("bum02", "bum02", "male_bum_2", 60)] {
        catalog.entities.insert(e.into(), PedEntity { model: Some(m.into()), anim_set: Some("default".into()) });
        catalog.models.insert(m.into(), PedModel { recipe: recipe.into(), voice: Some(voice), ..Default::default() });
    }
    let rig = PedRig {
        names: vec!["TRAJECTORY".into(), "HIPS".into(), "HEADEND".into()],
        parents: vec![-1, 0, 1],
        mirrors: vec![0, 1, -1],
        reference: vec![IDENTITY, sqt([0.0, 0.92, 0.0]), IDENTITY],
        animated: vec![true, true, false],
    };
    let mut d = PedData::default();
    d.catalog = Arc::new(catalog);
    d.anim_sets = Arc::new([("default".to_string(), set)].into_iter().collect());
    d.rig = Arc::new(rig);
    d.clips = Arc::new(clips);
    d
}

fn record(serial: u32, category: &str, seed: u64, tick: u64) -> SpawnRecord {
    SpawnRecord {
        id: LivingWorldId { kind: Kind::Pedestrian, serial },
        tick,
        position: [serial as f32 * 3.0, 0.0, 10.0],
        heading: 0.5,
        seed,
        initial: false,
        choice: SpawnChoice::Census { record: "aletown".into(), category: category.into() },
    }
}

#[derive(Resource, Default)]
struct Seen(Vec<PedEvent>);

fn collect(mut ev: MessageReader<PedEvent>, mut seen: ResMut<Seen>) {
    seen.0.extend(ev.read().cloned());
}

fn app(hz: f32) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed: 3, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    state.install("Test", 1, &settings, LoadedData { config: PopulationConfig::retail(), census: None, skaters: None, roads: None, vehicles: None, npc: Default::default(), status: "peds test".into() });
    app.insert_resource(settings)
        .insert_resource(state)
        .insert_resource(data())
        .init_resource::<LivingWorldObservers>()
        .init_resource::<PedLooks>()
        .init_resource::<PedIndex>()
        .init_resource::<PedRejected>()
        .init_resource::<PedNavSettings>()
        .init_resource::<Seen>()
        .add_message::<LivingWorldSpawn>()
        .add_message::<LivingWorldDespawn>()
        .add_message::<PedEvent>()
        .add_systems(Update, (step_population, apply_ped_records, release_rejected, advance_peds, collect).chain());
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / hz)));
    app.update();
    app
}

fn run(app: &mut App, seconds: f32, hz: f32) {
    for _ in 0..(seconds * hz).round() as u32 {
        app.update();
    }
}

fn spawn_at_tick(app: &mut App, r: SpawnRecord) {
    app.world_mut().write_message(LivingWorldSpawn(r));
}

fn peds(app: &mut App) -> Vec<(LivingWorldId, String, Vec3, f32, Locomotion, u64, [bool; 2], Option<u32>)> {
    let mut q = app.world_mut().query::<(&Pedestrian, &PedBody, &crate::world_audio::PedAudio)>();
    let mut v: Vec<_> = q.iter(app.world()).map(|(p, b, a)| (p.id, p.recipe.clone(), b.position, b.heading, b.player.state, b.ticks, a.feet_down, a.voice)).collect();
    v.sort_by_key(|x| x.0);
    v
}

#[test]
fn living_world_peds_spawn_with_the_seeded_look_and_despawn_cleanly() {
    let mut a = app(60.0);
    let tick = a.world().resource::<PopulationState>().world.tick();
    for i in 1..=6 {
        spawn_at_tick(&mut a, record(i, "aletown", 100 + i as u64, tick));
    }
    spawn_at_tick(&mut a, record(7, "empty", 1, tick));
    run(&mut a, 0.5, 60.0);
    let list = peds(&mut a);
    assert_eq!(list.len(), 6);
    let d = data();
    for (id, recipe, pos, ..) in &list {
        let look = d.catalog.choose("aletown", 100 + id.serial as u64, &PedOverrides::default()).unwrap();
        assert_eq!(recipe, &look.recipe, "look = f(category, seed)");
        assert!(pos.is_finite());
    }
    assert!(list.iter().all(|p| p.7 == Some(55) || p.7 == Some(60)), "voice from the model record");
    let seen = &a.world().resource::<Seen>().0;
    assert!(seen.iter().any(|e| matches!(e, PedEvent::Rejected { category, .. } if category == "empty")));
    assert_eq!(seen.iter().filter(|e| matches!(e, PedEvent::Spawned { .. })).count(), 6);
    // Despawn: the entity and its index entry go.
    let id = list[0].0;
    a.world_mut().write_message(LivingWorldDespawn(DespawnRecord { id, tick, reason: DespawnReason::Distance }));
    run(&mut a, 0.1, 60.0);
    assert_eq!(peds(&mut a).len(), 5);
    assert!(!a.world().resource::<PedIndex>().0.contains_key(&id));
    assert!(a.world().resource::<Seen>().0.iter().any(|e| matches!(e, PedEvent::Despawned { id: d, .. } if *d == id)));
}

#[test]
fn living_world_ped_motion_follows_from_the_record_and_tick_at_any_frame_rate() {
    let mut results = Vec::new();
    for hz in [60.0, 144.0] {
        let mut a = app(hz);
        let tick = a.world().resource::<PopulationState>().world.tick();
        spawn_at_tick(&mut a, record(1, "aletown", 77, tick));
        run(&mut a, 30.0, hz);
        let tick_now = a.world().resource::<PopulationState>().world.tick();
        let p = peds(&mut a).remove(0);
        // Rebuild from the record: same player, same number of console ticks.
        assert_eq!(p.5, tick_now - tick, "{hz} Hz");
        results.push((p.2, p.3, p.4, p.5));
    }
    // Same tick count = identical state (the two rates end on different ticks; compare a rebuild).
    let d = data();
    for (pos, heading, state, ticks) in results {
        let set = &d.anim_sets["default"];
        let mut body = PedBody {
            player: PedAnimPlayer::new(set, 77).unwrap(),
            path: skate_core::living_world::peds::anim::TestPath::new(77), nav: Default::default(), blocked: 0.0,
            position: Vec3::new(3.0, 0.0, 10.0),
            heading: 0.5,
            ticks: 0,
            feet_down: [false; 2],
            body_fall: 0.0,
        };
        for _ in 0..ticks {
            body.player.intent = body.path.intent(tick_seconds(60.0), body.player.state);
            let out = body.player.step(tick_seconds(60.0), set, &d);
            body.position += Quat::from_rotation_y(body.heading) * Vec3::from_array(out.root.translation);
            body.heading += out.root.yaw;
        }
        assert_eq!((body.position, body.heading, body.player.state), (pos, heading, state));
        // It walked: the test path moves it a few metres from the spawn point.
        assert!(pos.distance(Vec3::new(3.0, 0.0, 10.0)) > 1.0, "moved {pos}");
    }
}

#[test]
fn living_world_peds_publish_foot_plants_while_walking() {
    let mut a = app(60.0);
    let tick = a.world().resource::<PopulationState>().world.tick();
    spawn_at_tick(&mut a, record(1, "aletown", 5, tick));
    let mut feet = std::collections::BTreeSet::new();
    let mut states = std::collections::BTreeSet::new();
    for _ in 0..(20 * 60) {
        a.update();
        let p = peds(&mut a).remove(0);
        feet.insert(p.6);
        states.insert(p.4);
    }
    assert!(states.contains(&Locomotion::Walk), "{states:?}");
    assert!(feet.contains(&[true, false]) && feet.contains(&[false, true]) && feet.contains(&[true, true]), "{feet:?}");
    assert!(a.world().resource::<Seen>().0.iter().any(|e| matches!(e, PedEvent::State { state: Locomotion::Walk, .. })));
}

#[test]
fn living_world_ped_mod_overrides_and_render_helpers() {
    let mut a = app(60.0);
    {
        let mut looks = a.world_mut().resource_mut::<PedLooks>();
        looks.overrides.category_entities.insert("aletown".into(), vec!["bum02".into()]);
        looks.glb.insert("male_bum_2".into(), "mods/x/bum.glb".into());
    }
    let tick = a.world().resource::<PopulationState>().world.tick();
    for i in 1..=4 {
        spawn_at_tick(&mut a, record(i, "aletown", i as u64, tick));
    }
    run(&mut a, 0.2, 60.0);
    assert!(peds(&mut a).iter().all(|p| p.1 == "male_bum_2"));
    let looks = a.world().resource::<PedLooks>().clone();
    assert_eq!(glb_path(&looks, "male_bum_2"), "mods/x/bum.glb");
    assert_eq!(glb_path(&PedLooks::default(), "male_jock_2"), "private/living_world/models/male_jock_2.glb");
    // LOD hysteresis on the 45 / 55 pair.
    assert_eq!(lod_for(40.0, 1, [45.0, 55.0]), 0);
    assert_eq!(lod_for(50.0, 0, [45.0, 55.0]), 0);
    assert_eq!(lod_for(50.0, 1, [45.0, 55.0]), 1);
    assert_eq!(lod_for(60.0, 0, [45.0, 55.0]), 1);
    // Followers keep the bind offset to their parent.
    let rig = data().rig;
    let bind: BTreeMap<usize, Mat4> = [(1, Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0))), (2, Mat4::from_translation(Vec3::new(0.0, 1.6, 0.1)))].into_iter().collect();
    let f = follower_offsets(&rig, &bind);
    assert_eq!(f.len(), 1);
    assert!((f[0].2.w_axis.truncate() - Vec3::new(0.0, 0.6, 0.1)).length() < 1e-6);
    let d = data();
    let body = PedBody {
        player: PedAnimPlayer::new(&d.anim_sets["default"], 1).unwrap(),
        path: skate_core::living_world::peds::anim::TestPath::new(1), nav: Default::default(), blocked: 0.0,
        position: Vec3::ZERO,
        heading: 0.0,
        ticks: 0,
        feet_down: [false; 2],
        body_fall: 0.0,
    };
    let g = ped_globals(&d.rig, &body, &d, 0.0, &f).unwrap();
    assert!((g[1].w_axis.y - 0.92).abs() < 1e-5);
    assert!((g[2].w_axis.truncate() - Vec3::new(0.0, 1.52, 0.1)).length() < 1e-5, "{:?}", g[2].w_axis);
}

#[test]
fn living_world_ped_data_loads_from_the_export() {
    let Some(raw) = std::env::var_os("SKATE3_ASSET_ROOT") else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT (an asset root with stock data and living_world)");
        return;
    };
    let Some(root) = std::env::split_paths(&raw).find(|r| r.join(skate_data::ped_anim::PED_BANK).exists() && r.join("private/living_world/tables.json").exists()) else {
        eprintln!("skipped: no single root holds both the ped bank and living_world/tables.json");
        return;
    };
    let d = PedData::load(&root);
    eprintln!("{}", d.status);
    assert!(d.ready(), "{}", d.status);
    assert!(d.clips.len() > 50);
    let look = d.catalog.choose("aletown", 1, &PedOverrides::default()).unwrap();
    let set = &d.anim_sets[&look.anim_set];
    let body = PedBody { player: PedAnimPlayer::new(set, 1).unwrap(), path: skate_core::living_world::peds::anim::TestPath::new(1), nav: Default::default(), blocked: 0.0, position: Vec3::ZERO, heading: 0.0, ticks: 0, feet_down: [false; 2], body_fall: 0.0 };
    let g = ped_globals(&d.rig, &body, &d, 0.0, &[]).unwrap();
    assert!((0.8..1.1).contains(&g[1].w_axis.y), "hips height {}", g[1].w_axis.y);
}

/// Visual check helper (no window): with `SKATE_PED_POSE_DUMP=<file.json>` and the data roots,
/// write the native model-space matrices of a mid-walk pose of the `default` set (all 50 rig
/// bones, column-major) for `.claude/skills/living-world/tools/render_glb.py --pose`.
#[test]
fn living_world_ped_pose_dump_for_a_render_check() {
    let (Some(out), Some(raw)) = (std::env::var_os("SKATE_PED_POSE_DUMP"), std::env::var_os("SKATE3_ASSET_ROOT")) else { return };
    let Some(root) = std::env::split_paths(&raw).find(|r| r.join(skate_data::ped_anim::PED_BANK).exists() && r.join("private/living_world/tables.json").exists()) else { return };
    let d = PedData::load(&root);
    let set = &d.anim_sets["default"];
    let mut body = PedBody { player: PedAnimPlayer::new(set, 1).unwrap(), path: skate_core::living_world::peds::anim::TestPath::new(1), nav: Default::default(), blocked: 0.0, position: Vec3::ZERO, heading: 0.0, ticks: 0, feet_down: [false; 2], body_fall: 0.0 };
    body.player.intent = skate_core::living_world::peds::anim::Intent::Walk;
    let mut poses = Vec::new();
    for t in 0..180 {
        body.player.step(tick_seconds(60.0), set, &d);
        if t == 0 || t == 120 || t == 136 {
            let g = ped_globals(&d.rig, &body, &d, 0.0, &[]).unwrap();
            poses.push(serde_json::json!({"clip": body.player.current_clip(), "time": body.player.current_time(),
                "bones": g.iter().map(|m| m.to_cols_array().to_vec()).collect::<Vec<_>>()}));
        }
    }
    let doc = serde_json::json!({"names": d.rig.names, "parents": d.rig.parents, "animated": d.rig.animated, "poses": poses});
    std::fs::write(out, serde_json::to_string(&doc).unwrap()).unwrap();
}

/// A flat 40 x 40 m navmesh of 2 m squares with a 6 x 6 m hole in the middle (M3).
fn plaza() -> skate_core::living_world::peds::NavMeshInput {
    use skate_core::living_world::peds::NavPolyInput;
    let hole = |i: i32, j: i32| (8..11).contains(&i) && (8..11).contains(&j);
    let index = |i: i32, j: i32| if (0..20).contains(&i) && (0..20).contains(&j) && !hole(i, j) { Some((j * 20 + i) as u32) } else { None };
    let mut polygons = Vec::new();
    for j in 0..20 {
        for i in 0..20 {
            let (x, z) = (i as f32 * 2.0, j as f32 * 2.0);
            let walkable = !hole(i, j);
            polygons.push(NavPolyInput {
                verts: vec![[x, 0.0, z], [x + 2.0, 0.0, z], [x + 2.0, 0.0, z + 2.0], [x, 0.0, z + 2.0]],
                neighbours: if walkable { vec![index(i, j - 1), index(i + 1, j), index(i, j + 1), index(i - 1, j)] } else { vec![None; 4] },
                area: if walkable { 0x11 } else { 0xF1 },
            });
        }
    }
    skate_core::living_world::peds::NavMeshInput { agent: [0.12, 0.35, 0.2, 1.6], polygons }
}

#[test]
fn living_world_peds_wander_on_the_navmesh_deterministically() {
    let mut finals = Vec::new();
    for _ in 0..2 {
        let mut a = app(60.0);
        {
            let mut d = a.world_mut().resource_mut::<PedData>();
            let input = plaza();
            d.nav = Some(Arc::new(skate_core::living_world::peds::NavMesh::build(&input, Default::default())));
            d.nav_input = Some(Arc::new(input));
        }
        let tick = a.world().resource::<PopulationState>().world.tick();
        for i in 1..=4 {
            let mut r = record(i, "aletown", 200 + i as u64, tick);
            r.position = [4.0 + 8.0 * i as f32, 0.0, 4.0];
            spawn_at_tick(&mut a, r);
        }
        let mut targets = 0;
        for _ in 0..(60 * 40) {
            a.update();
            let mut q = a.world_mut().query::<&PedBody>();
            let bodies: Vec<PedBody> = q.iter(a.world()).cloned().collect();
            let mesh = a.world().resource::<PedData>().nav.clone().unwrap();
            for b in &bodies {
                assert!(mesh.locate(b.position.to_array()).is_some(), "off the navmesh at {}", b.position);
                for o in &bodies {
                    let d = (b.position - o.position).length();
                    assert!(d == 0.0 || d >= 0.7 - 1e-3, "peds closer than twice the agent radius: {d}");
                }
            }
            targets = bodies.iter().map(|b| b.nav.targets_chosen).sum::<u32>();
        }
        assert!(targets >= 4, "every ped chose a wander target ({targets})");
        let list = peds(&mut a);
        assert!(list.iter().filter(|p| p.2.distance(Vec3::new(4.0 + 8.0 * p.0.serial as f32, 0.0, 4.0)) > 3.0).count() >= 3, "they walked: {list:?}");
        finals.push(list);
    }
    assert_eq!(finals[0], finals[1], "same records, same walk");
}

#[test]
fn living_world_ped_navmesh_loads_from_the_export() {
    let Some(raw) = std::env::var_os("SKATE3_ASSET_ROOT") else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT (an asset root with living_world/navmesh.bin)");
        return;
    };
    let Some(root) = std::env::split_paths(&raw).find(|r| r.join(skate_data::ped_nav::NAVMESH).exists()) else {
        eprintln!("skipped: no root holds living_world/navmesh.bin");
        return;
    };
    for (district, polygons) in [("DownTown", 36443), ("Industrial", 8693), ("University", 18147)] {
        let mut d = PedData::default();
        d.load_nav(&root, district, &Default::default());
        assert_eq!(d.nav.as_ref().map(|m| m.polys.len()), Some(polygons), "{district}: {}", d.status);
    }
    let mut d = PedData::default();
    d.load_nav(&root, "SomePark", &Default::default());
    assert!(d.nav.is_none() && d.status.contains("no navmesh"));
}
