//! NPC skaters, replay tier (doc 26, milestone 3): turns the population's skater spawn / despawn
//! records into visible skaters that ride their recorded lines.
//!
//! - **Entity** per spawn record: [`NpcSkater`] (stable `LivingWorldId`, character key, slot,
//!   seed, voice) + [`NpcReplay`] (the `skate_core::living_world::replay` cursor) + `Transform`.
//!   The id map [`NpcSkaterIndex`] finds it again for the despawn.
//! - **Motion** (`FixedUpdate`, after the population step): each cursor is kept at
//!   `population tick - spawn tick` recording frames (the 60 Hz lines at the retail 60 Hz world
//!   tick, `skate_core::living_world::clock`), so the state is a function of the spawn record and the tick; branch decisions use the
//!   retail score with the players and the other NPCs and are kept as records a client would
//!   mirror. The NPC's position goes back into the population (culls, the 5 m rule). The end of a
//!   line with no branch taken despawns the NPC (parked: retail behaviour not decoded).
//! - **Collision**: a kinematic proxy (capsule for the body, box for the board, infinite mass, the
//!   cursor's velocity) joins `physics::network::Proxies` like a mod's solid, so the player bumps
//!   into it. NPCs never react (replay tier).
//! - **Look**: the character's native roster GLB (`CustomModels::online_native_path`, the same
//!   files the customiser and online players use), or a mod's GLB from [`NpcSkaterLooks`], else
//!   the stock skater. Bound to the stock skeleton like a remote player (`AnimationStatus`).
//! - **Puppet animation** (`Update`): one stock clip per [`ReplayPhase`] ([`puppet_clip`]),
//!   evaluated with the player's evaluator; root = the recorded position and skater orientation.
//!   The stock graphs are not run (simplification until the simulated tier).
//! - **Audio**: [`NpcSkaterAudio`](crate::world_audio::NpcSkaterAudio) with a lite state from the
//!   cursor (position, velocity, air, ground trick as a grind) and the character's voice; #32's
//!   host picks the one audible NPC by retail's rule.
//! - **Events** for engine systems and the planned `sdk.living_world`: [`NpcSkaterEvent`].
//!
//! Multiplayer: nothing here decides a spawn; the entity is rebuilt from a `SpawnRecord`, the tick
//! and [`BranchRecord`]s alone (`Decider::Mirror` on a client).

use super::{LivingWorldDespawn, LivingWorldObservers, LivingWorldSettings, LivingWorldSpawn, NetRole, PopulationState};
use crate::world_audio::{AudioState, AudioVelocity, LiteSkater, NpcSkaterAudio};
use bevy::prelude::*;
use skate_core::living_world::replay::{BranchContext, BranchRecord, CursorEvent, Decider, LineCursor, ReplayLine, ReplayPhase, ReplaySample};
use skate_core::living_world::{DespawnReason, Kind, LivingWorldId, SpawnChoice};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Recording frames per world tick: the lines are recorded at 60 Hz ([data], `RECORDING_HZ`) and
/// the retail world tick that runs the skater manager is the 60 Hz fixed step
/// (`skate_core::living_world::clock::RETAIL_TICK_HZ`) [code], so one frame per tick.
pub(crate) const FRAMES_PER_TICK: u64 = 1;

/// Lines and per-character data of the loaded district, shared with the population.
#[derive(Clone, Default)]
pub(crate) struct NpcData {
    pub lines: Arc<BTreeMap<[u8; 16], ReplayLine>>,
    /// `characters_marquee` voice per character key (`skater_profiles.json`).
    pub voices: BTreeMap<String, u32>,
}

/// One replay-tier NPC skater.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct NpcSkater {
    pub id: LivingWorldId,
    pub character: String,
    pub slot: u8,
    pub seed: u64,
    pub voice: Option<u32>,
    pub spawn_tick: u64,
    pub start_line: [u8; 16],
}

#[derive(Component, Clone, Debug)]
pub(crate) struct NpcReplay {
    pub cursor: LineCursor,
    /// Branch decisions so far (what a host would send).
    pub branches: Vec<BranchRecord>,
    pub last: Option<ReplaySample>,
}

/// LivingWorldId -> entity.
#[derive(Resource, Default)]
pub(crate) struct NpcSkaterIndex(pub BTreeMap<LivingWorldId, Entity>);

/// Mod / engine look overrides: character key -> GLB asset path. Empty = retail looks.
#[derive(Resource, Default, Clone)]
pub(crate) struct NpcSkaterLooks(pub BTreeMap<String, String>);

/// What happened to an NPC skater (engine systems, mods, speech).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) enum NpcSkaterEvent {
    Spawned { id: LivingWorldId, character: String, line: [u8; 16] },
    Despawned { id: LivingWorldId, reason: DespawnReason },
    Node { id: LivingWorldId, line: [u8; 16], node: u32, event: u8, flags: u8 },
    Branch { id: LivingWorldId, record: BranchRecord },
    LineEnd { id: LivingWorldId },
}

/// The stock clip a replay NPC shows per phase ([data]: names checked against the decoded stock
/// clip list, `crate::living_world` data-gated test). Riding uses the pro's style like the
/// customiser (`native_animation_style`).
pub(crate) fn puppet_clip(phase: ReplayPhase, style: &str) -> &'static str {
    match phase {
        ReplayPhase::Rolling => match style {
            "Aggressive" => "R_IDLE_RIDE_AGGR_0_CYC",
            "Loose" => "R_IDLE_RIDE_LOOSE_0_CYC",
            _ => "R_IDLE_RIDE_N_0_CYC",
        },
        ReplayPhase::Crouched => "R_IDLE_LCOM_000",
        ReplayPhase::Air => "IA_IDLE_N_N_0_CYC",
        ReplayPhase::AirTrick => "IA_IDLE_LO_N_0_CYC",
        ReplayPhase::GroundTrick => "G_5050_FS_LOW_0_CYC",
        ReplayPhase::OffBoard => "BR_STAND_0_CYC",
    }
}

pub(crate) const PUPPET_CLIPS: [&str; 8] =
    ["R_IDLE_RIDE_N_0_CYC", "R_IDLE_RIDE_AGGR_0_CYC", "R_IDLE_RIDE_LOOSE_0_CYC", "R_IDLE_LCOM_000", "IA_IDLE_N_N_0_CYC", "IA_IDLE_LO_N_0_CYC", "G_5050_FS_LOW_0_CYC", "BR_STAND_0_CYC"];

fn npc_lines(state: &PopulationState) -> Arc<BTreeMap<[u8; 16], ReplayLine>> {
    state.npc.lines.clone()
}

/// Spawn and despawn NPC entities from the population's records.
pub(crate) fn apply_records(
    mut commands: Commands,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    state: Res<PopulationState>,
    mut index: ResMut<NpcSkaterIndex>,
    mut events: MessageWriter<NpcSkaterEvent>,
) {
    for LivingWorldDespawn(r) in despawns.read() {
        if r.id.kind != Kind::Skater {
            continue;
        }
        if let Some(e) = index.0.remove(&r.id) {
            commands.entity(e).despawn();
            events.write(NpcSkaterEvent::Despawned { id: r.id, reason: r.reason });
        }
    }
    let lines = npc_lines(&state);
    for LivingWorldSpawn(s) in spawns.read() {
        let SpawnChoice::Skater { line, character, slot } = &s.choice else { continue };
        if index.0.contains_key(&s.id) {
            continue;
        }
        let cursor = LineCursor::spawn(&*lines, *line, 0);
        let npc = NpcSkater {
            id: s.id,
            character: character.clone(),
            slot: *slot,
            seed: s.seed,
            voice: state.npc.voices.get(character).copied(),
            spawn_tick: s.tick,
            start_line: *line,
        };
        let sample = cursor.sample(&*lines, 0.0);
        let at = sample.as_ref().map_or(Vec3::from_array(s.position), |x| Vec3::from_array(x.position));
        let e = commands
            .spawn((
                Name::new(format!("NPC skater {} ({character})", s.id.serial)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(s.heading)),
                Visibility::Inherited,
                NpcReplay { cursor, branches: Vec::new(), last: sample },
                NpcSkaterAudio { list_order: u32::from(*slot), voice: npc.voice, ..Default::default() },
                npc,
            ))
            .id();
        index.0.insert(s.id, e);
        events.write(NpcSkaterEvent::Spawned { id: s.id, character: character.clone(), line: *line });
    }
}

/// Advance every cursor to its tick, take branches, end finished lines, publish position and audio.
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance(
    mut commands: Commands,
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    mut state: ResMut<PopulationState>,
    mut index: ResMut<NpcSkaterIndex>,
    mut npcs: Query<(Entity, &NpcSkater, &mut NpcReplay, &mut Transform, &mut NpcSkaterAudio)>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
    mut events: MessageWriter<NpcSkaterEvent>,
) {
    let lines = npc_lines(&state);
    let tick = state.world.tick();
    let players: Vec<[f32; 3]> = observers.observers.iter().map(|o| o.position).collect();
    let mirror = settings.net_role == NetRole::Client;
    // Others' (line, node) and lines in use, by serial order (deterministic).
    let mut order: Vec<(LivingWorldId, [u8; 16], u32)> = npcs.iter().map(|(_, n, r, ..)| (n.id, r.cursor.line, r.cursor.node)).collect();
    order.sort_by_key(|x| x.0);
    let mut finished = Vec::new();
    let mut sorted: Vec<_> = npcs.iter_mut().collect();
    sorted.sort_by_key(|(_, n, ..)| n.id);
    for (e, npc, mut replay, mut transform, mut audio) in sorted {
        let target = tick.saturating_sub(npc.spawn_tick) * FRAMES_PER_TICK;
        let mut out = Vec::new();
        while replay.cursor.frames < target && !replay.cursor.finished {
            let s = replay.cursor.sample(&*lines, 0.0);
            let others: Vec<([u8; 16], u32)> = order.iter().filter(|o| o.0 != npc.id).map(|o| (o.1, o.2)).collect();
            let in_use: Vec<[u8; 16]> = others.iter().map(|o| o.0).collect();
            let (position, forward, speed) = s.as_ref().map_or(([0.0; 3], [0.0, 0.0, 1.0], 0.0), |s| (s.position, s.velocity, length(s.velocity)));
            let ctx = BranchContext { position, forward, speed, players: &players, others: &others, in_use: &in_use, preferred_skill: -1, online: observers.online };
            let records = replay.branches.clone();
            let mut decider = if mirror { Decider::Mirror(&records) } else { Decider::Decide(ctx) };
            replay.cursor.step(&*lines, &mut decider, &mut out);
            if let Some(o) = order.iter_mut().find(|o| o.0 == npc.id) {
                o.1 = replay.cursor.line;
                o.2 = replay.cursor.node;
            }
        }
        for ev in out {
            match ev {
                CursorEvent::Node { line, node, event, flags } => {
                    if event != 0 {
                        events.write(NpcSkaterEvent::Node { id: npc.id, line, node, event, flags });
                    }
                }
                CursorEvent::Branch(record) => {
                    if !mirror {
                        replay.branches.push(record.clone());
                    }
                    events.write(NpcSkaterEvent::Branch { id: npc.id, record });
                }
                CursorEvent::Finished => {
                    events.write(NpcSkaterEvent::LineEnd { id: npc.id });
                    finished.push((npc.id, e));
                }
            }
        }
        let Some(s) = replay.cursor.sample(&*lines, 0.0) else { continue };
        state.world.update_position(npc.id, s.position);
        transform.translation = Vec3::from_array(s.position);
        transform.rotation = root_rotation(&s);
        let material = physics.as_deref().map_or(skate_audio::player::state::NO_MATERIAL, |p| crate::game_audio::world_bridge::ground_material(p, transform.translation));
        audio.voice = npc.voice;
        audio.state = Some(lite_state(&s, material));
        commands.entity(e).insert(AudioVelocity(Vec3::from_array(s.velocity)));
        replay.last = Some(s);
    }
    // Line end without a branch: the NPC leaves (parked, see the module doc). Hosts decide this;
    // a client waits for the host's despawn record.
    if !mirror {
        for (id, e) in finished {
            if let Some(skate_core::living_world::Decision::Despawn(r)) = state.world.despawn(id, DespawnReason::External) {
                state.despawned += 1;
                despawns.write(LivingWorldDespawn(r));
            }
            if index.0.remove(&id).is_some() {
                commands.entity(e).despawn();
                events.write(NpcSkaterEvent::Despawned { id, reason: DespawnReason::External });
            }
        }
    }
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Root rotation of the puppet: the recorded skater orientation (x, y, z, w, +Z forward [data]).
pub(crate) fn root_rotation(s: &ReplaySample) -> Quat {
    let [x, y, z, w] = s.skater;
    let q = Quat::from_xyzw(x, y, z, w);
    if q.is_finite() && q.length_squared() > 0.5 {
        q.normalize()
    } else {
        Quat::from_rotation_y(s.heading)
    }
}

/// The lite audio state of a replay NPC (like a remote player's, plus air and ground tricks).
pub(crate) fn lite_state(s: &ReplaySample, material: u32) -> AudioState {
    let airborne = matches!(s.phase, ReplayPhase::Air | ReplayPhase::AirTrick);
    let off = s.phase == ReplayPhase::OffBoard;
    AudioState::rolling(&LiteSkater {
        position: s.position,
        velocity: s.velocity,
        heading: s.heading,
        wheels: if off { [false; 4] } else { [true; 4] },
        material: if off { skate_audio::player::state::NO_MATERIAL } else { material },
        grinding: s.phase == ReplayPhase::GroundTrick,
        grind_material: if s.phase == ReplayPhase::GroundTrick { material } else { skate_audio::player::state::NO_MATERIAL },
        airborne,
        air_time: if airborne { s.phase_frames as f32 / 60.0 } else { 0.0 },
        dt: (1.0 / skate_core::living_world::clock::RETAIL_TICK_HZ) as f32,
    })
}

/// Solid ids of NPC proxies: a tag in the top bits keeps them apart from mod bodies.
pub(crate) const PROXY_ID_TAG: u64 = 0x4E50_0000_0000_0000;

/// The kinematic collision proxy of one NPC (body capsule + board box), world space.
pub(crate) fn proxy(id: LivingWorldId, s: &ReplaySample) -> skate_dynamics::SolidBody {
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};
    let [x, y, z, w] = s.skater;
    let rotation = Rotation::from_xyzw(x, y, z, w).normalize();
    let at = |local: [f32; 3]| {
        let q = Quat::from_xyzw(x, y, z, w).normalize();
        Vec3::from_array(s.position) + q * Vec3::from_array(local)
    };
    let p = |v: Vec3| Vector::new(v.x, v.y, v.z);
    // Body: a 0.25 m capsule from 0.25 to 1.55 m above the deck; board: 0.8 x 0.1 x 0.2 m.
    let body = SharedShape::capsule_y(0.65, 0.25);
    let board = SharedShape::cuboid(0.1, 0.05, 0.4);
    let com = at([0.0, 0.9, 0.0]);
    skate_dynamics::SolidBody {
        id: PROXY_ID_TAG | id.to_u64(),
        pose: Pose::from_parts(p(Vec3::from_array(s.position)), rotation),
        center_of_mass: p(com),
        inertia_rotation: rotation,
        inverse_mass: 0.0,
        inverse_inertia: Vector::new(0.0, 0.0, 0.0),
        linvel: Vector::new(s.velocity[0], s.velocity[1], s.velocity[2]),
        angvel: Vector::new(0.0, 0.0, 0.0),
        contact_group: 0,
        colliders: vec![
            skate_dynamics::SolidCollider { shape: body, pose: Pose::from_parts(p(com), rotation), friction: 0.5 },
            skate_dynamics::SolidCollider { shape: board, pose: Pose::from_parts(p(at([0.0, 0.08, 0.0])), rotation), friction: 0.5 },
        ],
    }
}

/// Add the NPC proxies to the skater solve (after the network proxies were rebuilt).
pub(crate) fn push_proxies(
    npcs: Query<(&NpcSkater, &NpcReplay)>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    replay: Res<crate::replay::Replay>,
) {
    if replay.active {
        return;
    }
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    let mut list: Vec<_> = npcs.iter().filter_map(|(n, r)| r.last.as_ref().map(|s| (n.id, s))).collect();
    list.sort_by_key(|x| x.0);
    for (id, s) in list {
        proxies.append_solid(proxy(id, s), &physics, &skater, false);
    }
    physics.network_proxies = proxies;
}

/// The look and the puppet pose of one NPC (render side).
#[derive(Component, Default)]
pub(crate) struct NpcPuppet {
    scene: Option<Entity>,
    bindings: Option<crate::animation::AnimationStatus>,
    style: &'static str,
}

/// Load the look and bind it to the stock skeleton (like a remote player's look).
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    looks: Res<NpcSkaterLooks>,
    models: Res<crate::custom_models::CustomModels>,
    skater: Res<crate::physics::SkaterRuntime>,
    mut npcs: Query<(Entity, &NpcSkater, Option<&mut NpcPuppet>)>,
    skins: Query<(Entity, &bevy::mesh::skinning::SkinnedMesh)>,
    nodes: Query<(&Name, &Transform)>,
    parents: Query<&ChildOf>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
) {
    for (e, npc, puppet) in &mut npcs {
        let Some(mut puppet) = puppet else {
            let path = looks
                .0
                .get(&npc.character)
                .cloned()
                .or_else(|| models.online_native_path(&npc.character))
                .unwrap_or_else(|| "private/skater.glb".to_owned());
            let scene = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))), Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            commands.entity(e).insert(NpcPuppet { scene: Some(scene), bindings: None, style: crate::custom_models::native_animation_style(&npc.character) });
            continue;
        };
        let Some(scene) = puppet.scene else { continue };
        if puppet.bindings.is_some() || !instances.get(scene).is_ok_and(|i| spawner.instance_is_ready(**i)) {
            continue;
        }
        match crate::animation::AnimationStatus::for_scene(scene, &skater.animation.evaluator.frames.bone_names, &skins, &nodes, &parents) {
            Ok(b) => {
                for (mesh, _) in &skins {
                    if parents.iter_ancestors(mesh).any(|p| p == scene) {
                        commands.entity(mesh).insert((bevy::camera::visibility::NoFrustumCulling, bevy::camera::visibility::RenderLayers::from_layers(&[0, 28])));
                    }
                }
                commands.entity(scene).insert(Visibility::Inherited);
                info!("LIVING_WORLD npc look bound #{} {}", npc.id.serial, npc.character);
                puppet.bindings = Some(b);
            }
            Err(err) => {
                warn!("NPC skater look rejected ({}): {err}", npc.character);
                commands.entity(scene).despawn();
                puppet.scene = None;
            }
        }
    }
}

/// Place the root between fixed steps and pose the skeleton from the phase's stock clip.
pub(crate) fn present_pose(
    skater: Res<crate::physics::SkaterRuntime>,
    state: Res<PopulationState>,
    fixed: Res<Time<Fixed>>,
    mut npcs: Query<(&NpcReplay, &NpcPuppet, &mut Transform)>,
    mut joints: Query<&mut Transform, Without<NpcReplay>>,
) {
    let lines = npc_lines(&state);
    // Render interpolation: fraction of the next world tick (FRAMES_PER_TICK recording frames).
    let hz = state.world.clock().hz;
    let ahead = (state.world.clock().overstep() + fixed.overstep_fraction() as f64 * fixed.timestep().as_secs_f64() * hz).clamp(0.0, 1.0) as f32
        * FRAMES_PER_TICK as f32;
    for (replay, puppet, mut root) in &mut npcs {
        // A look-ahead of the cursor (no branching inside it; the next fixed step corrects).
        let sample = if ahead > 0.0 && !replay.cursor.finished {
            let mut c = replay.cursor.clone();
            let whole = ahead.floor() as u32;
            c.advance(whole, &*lines, &mut Decider::Stay, &mut Vec::new());
            c.sample(&*lines, ahead - whole as f32)
        } else {
            replay.cursor.sample(&*lines, 0.0)
        };
        let Some(s) = sample.or_else(|| replay.last.clone()) else { continue };
        root.translation = Vec3::from_array(s.position);
        root.rotation = root_rotation(&s);
        let Some(bindings) = puppet.bindings.as_ref() else { continue };
        let clip = puppet_clip(s.phase, puppet.style);
        let Some(globals) = puppet_pose(&skater, clip, s.phase_frames as f32 / 60.0) else { continue };
        for (joint, local) in bindings.pose_transforms(&globals) {
            if let Ok(mut t) = joints.get_mut(joint) {
                *t = local;
            }
        }
    }
}

/// Global (model-space) bone matrices of a stock clip at `time` s, root trajectory held at the
/// origin (the cursor moves the root).
pub(crate) fn puppet_pose(skater: &crate::physics::SkaterRuntime, clip: &str, time: f32) -> Option<Vec<Mat4>> {
    use skate_core::animation::playback_tree::PoseCommand;
    let evaluator = &skater.animation.evaluator;
    let pose = evaluator
        .evaluate(&[
            PoseCommand::Clip { name: clip.to_owned(), previous_time: time, time, loops: 0 },
            PoseCommand::Pose { name: "RIG_TPOSE".into() },
            PoseCommand::Add { motion_is_a: true },
        ])
        .ok()?;
    let locals: Vec<Mat4> = pose.iter().copied().map(skate_core::animation::output::sqt_to_matrix).map(crate::animation::native_matrix).collect();
    let parents = &evaluator.frames.parents;
    let mut globals: Vec<Mat4> = Vec::with_capacity(locals.len());
    for (i, local) in locals.iter().enumerate() {
        let g = match parents.get(i).copied() {
            Some(p) if p >= 0 && (p as usize) < i => globals[p as usize] * *local,
            _ => *local,
        };
        globals.push(g);
    }
    Some(globals)
}

/// One-line NPC summary for the debug readout: count, the nearest NPC (distance, line, phase).
pub(crate) fn npc_readout(npcs: &[(LivingWorldId, String, Option<ReplaySample>)], player: Option<[f32; 3]>) -> String {
    let nearest = player.and_then(|p| {
        npcs.iter()
            .filter_map(|(id, c, s)| s.as_ref().map(|s| (id, c, s, length([s.position[0] - p[0], s.position[1] - p[1], s.position[2] - p[2]]))))
            .min_by(|a, b| a.3.total_cmp(&b.3))
    });
    match nearest {
        Some((id, c, s, d)) => format!(
            "npc skaters {} nearest #{} {c} {:.0} m line {} node {} {} {:.1} m/s",
            npcs.len(),
            id.serial,
            d,
            s.line.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            s.node,
            s.phase.name(),
            length(s.velocity)
        ),
        None => format!("npc skaters {}", npcs.len()),
    }
}

/// The debug readout system (`SKATE_LIVING_WORLD_DEBUG=1`, every 5 s with the population line).
pub(crate) fn log_readout(settings: Res<LivingWorldSettings>, state: Res<PopulationState>, observers: Res<LivingWorldObservers>, npcs: Query<(&NpcSkater, &NpcReplay)>, mut last: Local<u64>) {
    if !settings.debug || state.world.tick() < *last + 150 {
        return;
    }
    *last = state.world.tick();
    let list: Vec<_> = npcs.iter().map(|(n, r)| (n.id, n.character.clone(), r.last.clone())).collect();
    info!("LIVING_WORLD {}", npc_readout(&list, observers.observers.first().map(|o| o.position)));
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<NpcSkaterIndex>()
        .init_resource::<NpcSkaterLooks>()
        .add_message::<NpcSkaterEvent>()
        .add_systems(FixedUpdate, (apply_records, advance, log_readout).chain().after(super::step_population))
        .add_systems(
            FixedUpdate,
            push_proxies.after(crate::multiplayer::prepare).after(crate::app::SimulationSet::Controls).before(crate::app::SimulationSet::Physics),
        )
        .add_systems(Update, (present_looks, present_pose).chain().after(crate::app::FrameSet::Animation));
}
