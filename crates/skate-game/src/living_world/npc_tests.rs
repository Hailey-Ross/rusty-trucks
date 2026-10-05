//! Headless, seeded tests of the replay-tier NPC skaters: a fake player drives past synthetic
//! lines, the population spawns NPCs, the NPC systems ride them; positions, despawns, audio and
//! determinism are checked. No window, no assets (one data-gated test reads the stock clips).

use super::npc_skaters::*;
use super::*;
use skate_core::living_world::replay::{LineCursor, ReplayLine, ReplayNode, ReplayPhase};
use skate_core::living_world::{SkaterCharacter, SkaterLine};
use std::collections::BTreeMap;
use std::sync::Arc;

const NODES: u32 = 300;
const FRAMES: u8 = 4;
const STEP: f32 = 0.5; // 0.5 m per 4 frames = 7.5 m/s

fn id(i: u32) -> [u8; 16] {
    let mut id = [0u8; 16];
    id[0] = i as u8;
    id[1] = 0xA1;
    id
}

/// Lines every 25 m beside the player's road (z = 70), each riding +x for 20 s.
fn data() -> LoadedData {
    let config = PopulationConfig::retail();
    let starts: Vec<[f32; 3]> = (0..80).map(|i| [-1000.0 + i as f32 * 25.0, 0.0, 70.0]).collect();
    let lines = starts
        .iter()
        .enumerate()
        .map(|(i, s)| SkaterLine { id: id(i as u32), start: *s, heading: std::f32::consts::FRAC_PI_2, valid: true, allowed_skaters: u64::MAX, flags: 4 })
        .collect();
    let replay: BTreeMap<[u8; 16], ReplayLine> = starts
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let nodes = (0..NODES)
                .map(|n| ReplayNode {
                    position: [s[0] + n as f32 * STEP, s[1], s[2]],
                    // Heading +x: 90 deg about +y.
                    board: [128, 218, 128, 218],
                    skater: [128, 218, 128, 218],
                    frames: if n == 0 { 0 } else { FRAMES },
                    event: 0,
                    flags: if (100..110).contains(&n) { skate_core::living_world::replay::node_flags::AIRBORNE } else { 0 },
                    jump: None,
                })
                .collect();
            (id(i as u32), ReplayLine { id: id(i as u32), flags: 4, skill: 0, nodes, jumps: vec![], groups: vec![] })
        })
        .collect();
    let characters = (0..6).map(|i| SkaterCharacter { key: format!("pro_{i}"), pro_index: Some(i), capabilities: [false; 3], community: false }).collect();
    let voices = (0..6).map(|i| (format!("pro_{i}"), 10 + i)).collect();
    LoadedData {
        config,
        census: None,
        skaters: Some(SkaterData { lines, characters }),
        roads: None,
        vehicles: None,
        npc: NpcData { lines: Arc::new(replay), voices },
        status: "npc test".into(),
    }
}

#[derive(Resource)]
struct Drive {
    t: f32,
    speed: f32,
}

fn drive(time: Res<Time>, mut d: ResMut<Drive>, mut obs: ResMut<LivingWorldObservers>) {
    d.t += time.delta_secs();
    let x = -900.0 + d.t * d.speed;
    obs.observers = vec![Observer { position: [x, 0.0, 0.0], velocity: [d.speed, 0.0, 0.0] }];
    obs.player_slots = 1;
}

#[derive(Resource, Default)]
struct Seen(Vec<NpcSkaterEvent>);

fn collect(mut ev: MessageReader<NpcSkaterEvent>, mut seen: ResMut<Seen>) {
    seen.0.extend(ev.read().cloned());
}

fn app(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    state.install("Test", 1, &settings, data());
    app.insert_resource(settings).insert_resource(state).init_resource::<LivingWorldObservers>().init_resource::<NpcSkaterIndex>().init_resource::<Seen>();
    app.insert_resource(Drive { t: 0.0, speed: 8.0 });
    app.add_message::<LivingWorldSpawn>().add_message::<LivingWorldDespawn>().add_message::<NpcSkaterEvent>();
    app.add_systems(Update, (drive, step_population, apply_records, advance, collect).chain());
    app
}

fn run(app: &mut App, seconds: f32, hz: f32) {
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / hz)));
    app.update();
    for _ in 0..(seconds * hz) as u32 {
        app.update();
    }
}

/// (id, position, character, voice, audio velocity) of every NPC entity, by id.
fn npcs(app: &mut App) -> Vec<(LivingWorldId, [f32; 3], String, Option<u32>, Option<[f32; 3]>, u64, [u8; 16])> {
    let mut q = app.world_mut().query::<(&NpcSkater, &NpcReplay, &Transform, &crate::world_audio::NpcSkaterAudio)>();
    let mut v: Vec<_> = q
        .iter(app.world())
        .map(|(n, r, t, a)| (n.id, t.translation.to_array(), n.character.clone(), a.voice, a.state.as_ref().map(|s| s.board_velocity), r.cursor.frames, n.start_line))
        .collect();
    v.sort_by_key(|x| x.0);
    v
}

#[test]
fn living_world_npc_skaters_ride_their_lines_from_the_spawn_record() {
    let mut a = app(11);
    let mut max = 0;
    let mut checked = 0;
    for _ in 0..40 {
        run(&mut a, 1.0, 60.0);
        let tick = a.world().resource::<PopulationState>().world.tick();
        let list = npcs(&mut a);
        let st = a.world().resource::<PopulationState>();
        // One entity per live population skater, same ids; positions written back.
        let live: Vec<_> = st.world.live(Kind::Skater).map(|l| (l.id, l.position)).collect();
        assert_eq!(live.iter().map(|l| l.0).collect::<Vec<_>>(), list.iter().map(|n| n.0).collect::<Vec<_>>());
        max = max.max(list.len());
        let lines = st.npc.lines.clone();
        for (nid, pos, character, voice, vel, frames, start) in &list {
            // The state is a function of the spawn record and the tick: rebuild the cursor.
            let spawn_tick = a.world().resource::<Seen>().0.iter().find_map(|e| match e {
                NpcSkaterEvent::Spawned { id, .. } if id == nid => Some(()),
                _ => None,
            });
            assert!(spawn_tick.is_some());
            let mut c = LineCursor::spawn(&*lines, *start, 0);
            c.advance(*frames as u32, &*lines, &mut skate_core::living_world::replay::Decider::Stay, &mut Vec::new());
            let s = c.sample(&*lines, 0.0).unwrap();
            assert_eq!(s.position, *pos, "npc {nid:?} at tick {tick}");
            assert_eq!(live.iter().find(|l| l.0 == *nid).unwrap().1, *pos);
            // Speed 7.5 m/s along +x, published to the audio.
            let v = vel.unwrap();
            assert!((v[0] - 7.5).abs() < 1e-3 && v[2].abs() < 1e-3, "{v:?}");
            assert_eq!(*voice, Some(10 + character[4..].parse::<u32>().unwrap()));
            checked += 1;
        }
    }
    assert_eq!(max, 3, "retail keeps 3 ambient NPC skaters");
    assert!(checked > 60);
    let seen = &a.world().resource::<Seen>().0;
    // Lines are 20 s long: some NPCs reached the end and left; air nodes raised no events (flags
    // only), entities are gone with their records.
    assert!(seen.iter().any(|e| matches!(e, NpcSkaterEvent::LineEnd { .. })));
    let despawned: Vec<_> = seen.iter().filter_map(|e| if let NpcSkaterEvent::Despawned { id, .. } = e { Some(*id) } else { None }).collect();
    assert!(!despawned.is_empty());
    let alive = npcs(&mut a);
    assert!(despawned.iter().all(|d| !alive.iter().any(|n| n.0 == *d)));
    assert_eq!(a.world().resource::<NpcSkaterIndex>().0.len(), alive.len());
}

#[test]
fn living_world_npc_skaters_same_at_any_engine_rate() {
    let mut x = app(23);
    run(&mut x, 15.0, 60.0);
    let mut y = app(23);
    run(&mut y, 15.0, 144.0);
    let tick = |a: &App| a.world().resource::<PopulationState>().world.tick();
    // 2160 steps of a rounded 1/144 s can end a hair short of tick 450: let it catch up.
    for _ in 0..4 {
        if tick(&y) < tick(&x) {
            y.update();
        }
    }
    assert_eq!(tick(&x), tick(&y));
    let (a, b) = (npcs(&mut x), npcs(&mut y));
    assert!(!a.is_empty());
    assert_eq!(a.iter().map(|n| (n.0, n.1, n.5)).collect::<Vec<_>>(), b.iter().map(|n| (n.0, n.1, n.5)).collect::<Vec<_>>());
}

#[test]
fn living_world_npc_skaters_despawn_with_the_population() {
    let mut a = app(5);
    run(&mut a, 10.0, 60.0);
    assert!(!npcs(&mut a).is_empty());
    a.world_mut().resource_mut::<LivingWorldSettings>().skaters.enabled = false;
    run(&mut a, 1.0, 60.0);
    assert!(npcs(&mut a).is_empty());
    assert!(a.world().resource::<NpcSkaterIndex>().0.is_empty());
    let mut q = a.world_mut().query::<&NpcSkater>();
    assert_eq!(q.iter(a.world()).count(), 0);
}

#[test]
fn living_world_npc_proxy_audio_and_clips() {
    let lines = data().npc.lines;
    let mut c = LineCursor::spawn(&*lines, id(0), 0);
    c.advance(4 * 102, &*lines, &mut skate_core::living_world::replay::Decider::Stay, &mut Vec::new());
    let s = c.sample(&*lines, 0.0).unwrap();
    assert_eq!(s.phase, ReplayPhase::Air);
    let st = lite_state(&s, 3);
    assert!(st.airborne && st.wheel_count == 0);
    let nid = LivingWorldId { kind: Kind::Skater, serial: 7 };
    let p = proxy(nid, &s);
    assert_eq!(p.id, PROXY_ID_TAG | nid.to_u64());
    assert_eq!(p.inverse_mass, 0.0);
    assert!((p.linvel.x - 7.5).abs() < 1e-3);
    assert_eq!(p.colliders.len(), 2);
    // The skater orientation turns the model's +Z onto the travel direction (+x).
    let f = root_rotation(&s) * Vec3::Z;
    assert!(f.x > 0.99, "{f:?}");
    for phase in [ReplayPhase::Rolling, ReplayPhase::Crouched, ReplayPhase::Air, ReplayPhase::AirTrick, ReplayPhase::GroundTrick, ReplayPhase::OffBoard] {
        for style in ["Aggressive", "Loose", "DannyWay"] {
            assert!(PUPPET_CLIPS.contains(&puppet_clip(phase, style)));
        }
    }
    let r = npc_readout(&[(nid, "pro_1".into(), Some(s.clone()))], Some([s.position[0], 0.0, 0.0]));
    assert!(r.contains("npc skaters 1") && r.contains("70 m") && r.contains("air"), "{r}");
}

/// Data-gated: every puppet clip exists in the user's stock animation banks and evaluates.
#[test]
fn living_world_npc_puppet_clips_exist_in_the_stock_banks() {
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from).filter(|r| r.join("private/stock/data/anim/OnBoard.abin").exists()) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to the converted assets");
        return;
    };
    let evaluator = crate::animation_pose::PoseEvaluator::load(&root).unwrap();
    use skate_core::animation::playback_tree::PoseCommand;
    for clip in PUPPET_CLIPS {
        let pose = evaluator
            .evaluate(&[
                PoseCommand::Clip { name: clip.to_owned(), previous_time: 0.5, time: 0.5, loops: 0 },
                PoseCommand::Pose { name: "RIG_TPOSE".into() },
                PoseCommand::Add { motion_is_a: true },
            ])
            .unwrap_or_else(|e| panic!("{clip}: {e}"));
        assert_eq!(pose.len(), evaluator.frames.bone_names.len(), "{clip}");
    }
}
