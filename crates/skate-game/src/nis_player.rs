//! The NIS player (in-engine cutscenes: the Skate School lesson demos, the story scenes): plays a scene of the scene
//! table (`private/hud/scene/scene_000.bin`) for the presentation's NIS event. The scene's actor wears its character
//! on the player rig (as FE 62's preview does) and poses with its clip; the scene ends after its length.
//! Details: docs/hails-additions/43-career.md#nis-scenes
use crate::{animation::AnimationStatus, presentation_run::Presenter, world::PlayerRoot};
use bevy::prelude::*;
use skate_data::{scene_clip::SceneClip, scene_table::Scene};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

static PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A scene is running (the gameplay HUD hides meanwhile).
pub(crate) fn playing() -> bool {
    PLAYING.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Default)]
struct Player {
    /// The scene table by name (loaded once; `None` = no table in the install).
    scenes: Option<Option<Arc<HashMap<String, Scene>>>>,
    running: Option<Running>,
}

struct Running {
    scene: Scene,
    clip: Arc<SceneClip>,
    time: f32,
    /// The player's character before the scene, put back at the end.
    saved: Option<Option<String>>,
    swapped: bool,
    bindings: Option<AnimationStatus>,
    skins: usize,
    /// The scene's ICE camera takes and its cut list (scene time, take).
    tracks: Vec<skate_core::camera_track::CameraTrack>,
    cuts: Vec<(f32, usize)>,
    /// Per take, its depth of field (nis_dof.rs).
    focus: Vec<Option<skate_data::ice_camera::TakeFocus>>,
    /// The gameplay camera's field of view before the scene.
    fov: Option<f32>,
    /// The scene's other marquee actors, each on its own rig (despawned at the end).
    extras: Vec<Extra>,
    /// The start allowed skipping (retail +0x4D, arg5 of the basic NIS start 8285E968: 1 from the presentation
    /// event and Lua EnterBasicNIS, research b1028).
    skippable: bool,
}

/// A further actor: its look under `root` (at the map origin), posed with its own clip in the scene frame.
struct Extra {
    character: String,
    clip: Arc<SceneClip>,
    root: Entity,
    scene: Entity,
    bindings: Option<AnimationStatus>,
    /// A `livingworld` actor (a ped look): posed in its GLB's joint frames with this bone basis once bound.
    ped: Option<Mat4>,
    /// Where its hips were last posed (the `SKATE_NIS_LOOK_AT` test aid).
    hips: Option<Vec3>,
}

/// Record kinds whose skip also waits for a minimum clip time (82820068: 0x0C, 0x0F, 0x12; research b1028).
const TIMED_SKIP_KINDS: [u32; 3] = [0x0C, 0x0F, 0x12];
/// That minimum (float 82060C50 = 2.0, research b1029; read as seconds of clip time).
const SKIP_MIN_SECONDS: f32 = 2.0;

/// Whether a skip request ends the running scene: skippable at its start, and past the minimum time for the timed
/// kinds. NOT RETAIL YET: the scene table's own "can skip" answer (vt+0x14) is not read.
fn may_skip(skippable: bool, kind: u32, time: f32) -> bool {
    skippable && (!TIMED_SKIP_KINDS.contains(&(kind >> 24)) || time >= SKIP_MIN_SECONDS)
}

const PAD_A: u16 = 0x1000;

/// The scene frame in the map (scene record floats 0..6): the clip and the camera keys are authored in it.
fn scene_frame(scene: &Scene) -> Mat4 {
    let (at, facing) = scene.origin();
    let facing = Vec3::from_array(facing);
    let turn = if facing.length_squared() > 1e-6 { Quat::from_rotation_arc(Vec3::NEG_Z, facing.normalize()) } else { Quat::IDENTITY };
    Mat4::from_rotation_translation(turn, Vec3::from_array(at))
}

/// The scene's camera takes and cuts from `ice_000.bin` (none: the gameplay camera stays).
type Takes = (Vec<skate_core::camera_track::CameraTrack>, Vec<(f32, usize)>, Vec<Option<skate_data::ice_camera::TakeFocus>>);

fn cameras(root: &std::path::Path, scene: &Scene) -> Takes {
    let Ok(ice) = std::fs::read(root.join("ice_000.bin")) else { return Default::default() };
    let tracks = skate_data::ice_camera::scene_tracks(&ice, scene.hash)
        .inspect_err(|e| warn!("NIS {} camera: {e}", scene.name)).unwrap_or_default();
    let cuts = skate_data::ice_camera::scene_cuts(&ice, scene.hash).unwrap_or_default();
    let focus = skate_data::ice_camera::take_focus(&ice, scene.hash).unwrap_or_default();
    (tracks, cuts, focus)
}

pub(crate) struct NisPlayerPlugin;
impl Plugin for NisPlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (skip_input, drive).chain().after(crate::app::FrameSet::Animation).after(crate::camera::present)
            .before(crate::app::FrameSet::Verification));
    }
}

/// A (AptNext, key 2) on FE screen 1's key handler 825EFF98 asks for the skip while a scene plays (research b1029);
/// Enter on the keyboard.
fn skip_input(keys: Res<ButtonInput<KeyCode>>, pads: Option<Res<crate::input::ControllerInput>>, presenter: Option<Res<Presenter>>,
              mut previous: Local<u16>) {
    let buttons = pads.map_or(0, |c| c.raw_input().buttons);
    let pressed = buttons & PAD_A != 0 && *previous & PAD_A == 0;
    *previous = buttons;
    if playing() && (pressed || keys.just_pressed(KeyCode::Enter)) {
        if let Some(Ok(mut h)) = presenter.as_ref().map(|p| p.nis.lock()) {
            h.skip = true;
        }
    }
}

fn scene_root(world: &World) -> PathBuf {
    world.resource::<crate::config::Config>().asset_root.join("private/hud/scene")
}

fn load_scenes(root: &std::path::Path) -> Option<Arc<HashMap<String, Scene>>> {
    let bytes = std::fs::read(root.join("scene_000.bin")).inspect_err(|e| warn!("NIS no scene table: {e}")).ok()?;
    let scenes = skate_data::scene_table::parse(&bytes).inspect_err(|e| warn!("NIS scene table: {e}")).ok()?;
    info!("NIS scene table: {} scenes", scenes.len());
    Some(Arc::new(scenes.into_iter().map(|s| (s.name.clone(), s)).collect()))
}

/// The scene's first actor with a clip on disk: its clip (`scene/anim/<file>` -> `anim/<file>`).
fn actor_clip(root: &std::path::Path, scene: &Scene) -> Option<(usize, Arc<SceneClip>)> {
    scene.actors.iter().enumerate().find_map(|(i, a)| {
        let file = a.clip.rsplit('/').next()?;
        let path = root.join("anim").join(file);
        std::fs::read(&path).map_err(|e| e.to_string()).and_then(SceneClip::parse)
            .inspect_err(|e| warn!("NIS {} actor {}: clip {}: {e}", scene.name, a.character, path.display()))
            .ok().map(|c| (i, Arc::new(c)))
    })
}

fn report_finished(world: &World) {
    if let Ok(mut h) = world.resource::<Presenter>().nis.lock() {
        h.finished = true;
    }
}

fn drive(world: &mut World, mut player: Local<Player>) {
    let Some(rig) = world.query_filtered::<Entity, With<PlayerRoot>>().iter(world).next() else { return };
    let (request, stop, skip) = match world.get_resource::<Presenter>().and_then(|p| p.nis.lock().ok()
        .map(|mut h| (h.request.take(), std::mem::take(&mut h.stop), std::mem::take(&mut h.skip)))) {
        Some(v) => v,
        None => return,
    };
    if stop && player.running.is_some() {
        end(world, &mut player, "stopped");
    }
    if skip {
        match player.running.as_ref().map(|r| may_skip(r.skippable, r.scene.kind, r.time)) {
            Some(true) => end(world, &mut player, "skipped"),
            Some(false) => info!("NIS skip refused"),
            None => {}
        }
    }
    if let Some(name) = request {
        if player.running.is_some() {
            end(world, &mut player, "replaced");
        }
        let root = scene_root(world);
        let scenes = player.scenes.get_or_insert_with(|| load_scenes(&root)).clone();
        let scene = scenes.as_ref().and_then(|s| s.get(&name)).cloned();
        match scene.as_ref().map(|s| (s, actor_clip(&root, s))) {
            Some((scene, Some((actor, clip)))) => {
                let character = scene.actors[actor].character.clone();
                let hips = clip.bone_names.iter().position(|n| n == "HIPS").unwrap_or(0);
                let at = crate::trick_guide_scene::pose(&clip, 0.0).map(|g| crate::animation::native_matrix(g[hips]).w_axis.truncate());
                info!("NIS play {name}: {:.2} s, actor {character} ({}), clip {} frames at {} fps, hips at {at:?}, player at {:?}",
                    scene.length(), scene.actors[actor].kind, clip.frames.len(), clip.fps,
                    world.get::<GlobalTransform>(rig).map(|t| t.translation()));
                let (tracks, cuts, focus) = cameras(&root, scene);
                let first = tracks.first().and_then(|t| t.sample(0.0)).map(|s| ([s.eye[1], s.eye[2], s.eye[0]], [s.look[1], s.look[2], s.look[0]]));
                info!("NIS {name} camera: {} takes, cuts {cuts:?}, first eye / look {first:?}, actor hips in the map {:?}",
                    tracks.len(), at.map(|p| scene_frame(scene).transform_point3(p)));
                let mut running = Running { scene: scene.clone(), clip, time: 0.0, saved: None, swapped: false, bindings: None,
                    skins: 0, tracks, cuts, focus, fov: None, skippable: true, extras: Vec::new() };
                running.extras = spawn_extras(world, &root, scene, actor);
                // `current_player` keeps the player's own character.
                if character != "current_player" {
                    if let Some(mut models) = world.get_resource_mut::<crate::custom_models::CustomModels>() {
                        running.saved = Some(models.active.clone());
                        if models.request_preview(Some(&character)) {
                            running.swapped = true;
                        } else {
                            warn!("NIS {name}: no character {character}, the player's stands in");
                        }
                    }
                }
                player.running = Some(running);
                PLAYING.store(true, std::sync::atomic::Ordering::Relaxed);
                // Basic NIS start (8285E968): pause mode 12, which stops the world step while the scene plays.
                if let Some(mut pause) = world.get_resource_mut::<crate::pause_man::ThePauseMan>() {
                    pause.push(skate_core::pause_man::mode::NIS);
                }
            }
            Some((scene, None)) => {
                info!("NIS {name}: no actor clip ({} actors): done at once", scene.actors.len());
                report_finished(world);
            }
            None => {
                warn!("NIS {name}: not in the scene table: done at once");
                report_finished(world);
            }
        }
    }
    let Some(running) = player.running.as_mut() else { return };
    if world.get_resource::<crate::custom_models::CustomModels>().is_some_and(|m| m.swapping()) {
        return;
    }
    let skins = world.query::<&bevy::mesh::skinning::SkinnedMesh>().iter(world).count();
    if running.bindings.is_none() || running.skins != skins {
        running.bindings = crate::trick_guide_scene::bind(world, rig, &running.clip);
        running.skins = skins;
    }
    running.time += world.resource::<Time>().delta_secs();
    // Test aid (no simulated input): SKATE_NIS_SKIP_AT=<seconds> sends a skip request at that scene time.
    if let Some(at) = std::env::var("SKATE_NIS_SKIP_AT").ok().and_then(|v| v.parse::<f32>().ok()) {
        if running.time >= at && running.time - world.resource::<Time>().delta_secs() < at {
            if let Ok(mut h) = world.resource::<Presenter>().nis.lock() {
                h.skip = true;
            }
        }
    }
    let Some(running) = player.running.as_mut() else { return };
    if running.time >= running.scene.length() {
        end(world, &mut player, "ended");
        return;
    }
    camera(world, player.running.as_mut().unwrap());
    pose_extras(world, player.running.as_mut().unwrap());
    let Some(running) = player.running.as_ref() else { return };
    let Some(globals) = crate::trick_guide_scene::pose(&running.clip, running.time.min(running.clip.duration())) else { return };
    // The clip is in the scene frame (record floats 0..6), drawn through the player rig.
    let root = world.get::<GlobalTransform>(rig).copied().unwrap_or_default().to_matrix();
    let place = root.inverse() * scene_frame(&running.scene);
    if trace_tick(running.time, world.resource::<Time>().delta_secs()) {
        let hips = running.clip.bone_names.iter().position(|n| n == "HIPS").unwrap_or(0);
        let at = globals.get(hips).map(|g| scene_frame(&running.scene).transform_point3(crate::animation::native_matrix(*g).w_axis.truncate()));
        info!("NIS_TRACE t {:.1} actor {} hips {at:?} rig at {:?}", running.time, running.scene.actors.first().map_or("", |a| a.character.as_str()), root.w_axis.truncate());
    }
    if let Some(bindings) = &running.bindings {
        let matrices: Vec<Mat4> = globals.iter().map(|g| place * crate::animation::native_matrix(*g)).collect();
        for (entity, transform) in bindings.pose_transforms(&matrices) {
            if let Some(mut node) = world.get_mut::<Transform>(entity) {
                *node = transform;
            }
        }
    }
}

/// The scene's other marquee actors (82824610 queues a load of each marquee character): their look as the living
/// world's NPC skaters wear it (`npc_skaters::look_path`); `livingworld` actors as the ped look of that recipe
/// (`peds::glb_path`; the `zprop_` world props are livingworld models on the same skeleton). NOT RETAIL YET: the ped tint.
fn spawn_extras(world: &mut World, root: &std::path::Path, scene: &Scene, first: usize) -> Vec<Extra> {
    let mut extras = Vec::new();
    for (i, a) in scene.actors.iter().enumerate() {
        if i == first || a.character == "current_player" {
            continue;
        }
        let Some(ped) = extra_look(a) else {
            info!("NIS {} actor {} ({}): NOT RETAIL YET, not drawn", scene.name, a.character, a.kind);
            continue;
        };
        let Some(file) = a.clip.rsplit('/').next() else { continue };
        let clip = match std::fs::read(root.join("anim").join(file)).map_err(|e| e.to_string()).and_then(SceneClip::parse) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                warn!("NIS {} actor {}: clip {file}: {e}", scene.name, a.character);
                continue;
            }
        };
        let path = if ped {
            match world.get_resource::<crate::living_world::peds::PedLooks>() {
                Some(looks) => crate::living_world::peds::glb_path(looks, &a.character),
                None => crate::living_world::peds::glb_path(&Default::default(), &a.character),
            }
        } else {
            match (world.get_resource::<crate::living_world::npc_skaters::NpcSkaterLooks>(), world.get_resource::<crate::custom_models::CustomModels>()) {
                (Some(looks), Some(models)) => crate::living_world::npc_skaters::look_path(looks, models, &a.character),
                _ => "private/skater.glb".to_owned(),
            }
        };
        let handle = world.resource::<AssetServer>().load(bevy::gltf::GltfAssetLabel::Scene(0).from_asset(path.clone()));
        let actor_root = world.spawn((Transform::default(), Visibility::Visible, Name::new(format!("NIS actor {}", a.character)))).id();
        let look = world.spawn((SceneRoot(handle), Transform::default(), Visibility::Hidden, ChildOf(actor_root))).id();
        info!("NIS {} actor {} ({}): look {path}", scene.name, a.character, a.kind);
        extras.push(Extra { character: a.character.clone(), clip, root: actor_root, scene: look, bindings: None, ped: ped.then_some(Mat4::IDENTITY), hips: None });
    }
    extras
}

/// How a further actor is drawn: `Some(false)` a skater look, `Some(true)` a ped look, `None` not drawn.
fn extra_look(a: &skate_data::scene_table::SceneActor) -> Option<bool> {
    match a.kind.as_str() {
        "marquee" => Some(false),
        "livingworld" => Some(true),
        _ => None,
    }
}

/// Binds each further actor once its look is loaded, then poses it with its clip in the scene frame.
fn pose_extras(world: &mut World, running: &mut Running) {
    let frame = scene_frame(&running.scene);
    for extra in &mut running.extras {
        if extra.bindings.is_none() {
            let ready = world.get::<bevy::scene::SceneInstance>(extra.scene)
                .is_some_and(|i| world.resource::<SceneSpawner>().instance_is_ready(**i));
            if !ready {
                continue;
            }
            extra.bindings = crate::trick_guide_scene::bind(world, extra.scene, &extra.clip);
            if extra.bindings.is_some() {
                world.entity_mut(extra.scene).insert(Visibility::Inherited);
                // Its skinned meshes keep the bind pose's bounds at the map origin: no frustum culling, as the living
                // world's NPC looks (`npc_skaters::present_looks`).
                let meshes: Vec<Entity> = world.query_filtered::<Entity, With<bevy::mesh::skinning::SkinnedMesh>>().iter(world)
                    .filter(|&m| {
                        let mut at = m;
                        while let Some(parent) = world.get::<ChildOf>(at).map(|c| c.parent()) {
                            if parent == extra.scene {
                                return true;
                            }
                            at = parent;
                        }
                        false
                    })
                    .collect();
                for mesh in meshes {
                    world.entity_mut(mesh).insert(bevy::camera::visibility::NoFrustumCulling);
                }
                if extra.ped.is_some() {
                    extra.ped = Some(crate::living_world::peds::look_basis(world, extra.scene));
                    // 82784DD0 (research b1065): the record's first palette entries, no rand; the living world's tint
                    // system bakes them like an ambient ped's pair.
                    let (tint_a, tint_b) = world.get_resource::<crate::living_world::peds::PedData>()
                        .map(|d| skate_core::living_world::peds::choice::presentation_tints(&d.catalog, &extra.character))
                        .unwrap_or((skate_core::living_world::peds::choice::PRESENTATION_DEFAULT_TINT, skate_core::living_world::peds::choice::PRESENTATION_DEFAULT_TINT));
                    info!("NIS actor {} tints {tint_a:?} / {tint_b:?}", extra.character);
                    world.entity_mut(extra.root).insert(crate::living_world::peds::NisPedTint {
                        scene: extra.scene, recipe: extra.character.clone(), tint_a, tint_b });
                }
                info!("NIS actor {} bound{}", extra.character, if extra.ped.is_some() { " (ped look)" } else { "" });
            }
        }
        let Some(bindings) = &extra.bindings else { continue };
        let Some(globals) = crate::trick_guide_scene::pose(&extra.clip, running.time.min(extra.clip.duration())) else { continue };
        let mut matrices: Vec<Mat4> = globals.iter().map(|g| frame * crate::animation::native_matrix(*g)).collect();
        if let Some(basis) = extra.ped {
            matrices = crate::living_world::peds::ped_joint_globals(&matrices, basis);
        }
        let hips = extra.clip.bone_names.iter().position(|n| n == "HIPS").unwrap_or(0);
        extra.hips = matrices.get(hips).map(|m| m.w_axis.truncate());
        if trace_tick(running.time, world.resource::<Time>().delta_secs()) {
            info!("NIS_TRACE t {:.1} actor {} hips {:?}", running.time, extra.character, extra.hips);
        }
        for (entity, transform) in bindings.pose_transforms(&matrices) {
            if let Some(mut node) = world.get_mut::<Transform>(entity) {
                *node = transform;
            }
        }
    }
}

/// Every 2 s of scene time (NIS_TRACE lines: where the camera and each actor are).
fn trace_tick(time: f32, dt: f32) -> bool {
    (time / 2.0).floor() != ((time - dt) / 2.0).floor()
}

fn gameplay_camera(world: &mut World) -> Option<Entity> {
    world.query_filtered::<Entity, With<crate::camera::GameplayCamera>>().iter(world).next()
}

/// The take the cut list shows at the scene time, sampled on scene time. SPACE 1 (every key of the lesson scenes) is
/// a point fixed in the scene's frame (828F36B0 has no case for it); NOT RETAIL YET: that frame is taken as the map's.
fn camera(world: &mut World, running: &mut Running) {
    let take = skate_data::ice_camera::take_at(&running.cuts, running.time).unwrap_or(0);
    let focus = running.focus.get(take).copied().flatten();
    if let Some(mut f) = world.get_resource_mut::<crate::nis_dof::NisTakeFocus>() {
        if f.0 != focus {
            f.0 = focus;
        }
    }
    let Some(sample) = running.tracks.get(take).and_then(|t| t.sample(running.time)) else { return };
    let point = |[x, y, z]: [f32; 3]| Vec3::new(y, z, x);
    // The keys are map space (an offset by the scene frame put 04_ProChallenge_ChrisCole's camera ~30 m up).
    let (mut eye, mut look) = (point(sample.eye), point(sample.look));
    // Test aid: SKATE_NIS_LOOK_AT=<actor> frames that actor from 3 m (a shot of each look up close).
    if let Some(at) = std::env::var("SKATE_NIS_LOOK_AT").ok().and_then(|n| running.extras.iter().find(|x| x.character == n)?.hips) {
        let back = (eye - at).with_y(0.0).normalize_or(Vec3::X);
        (eye, look) = (at + back * 3.0 + Vec3::Y * 0.4, at);
    }
    if trace_tick(running.time, world.resource::<Time>().delta_secs()) {
        info!("NIS_TRACE t {:.1} camera take {take} eye {eye:?} look {look:?}", running.time);
    }
    let Some(camera) = gameplay_camera(world) else { return };
    if let Some(mut transform) = world.get_mut::<Transform>(camera) {
        *transform = Transform::from_translation(eye).looking_at(look, Vec3::Y);
        transform.rotate_local_z(-sample.dutch * std::f32::consts::TAU);
    }
    if let Some(mut projection) = world.get_mut::<Projection>(camera) {
        if let Projection::Perspective(p) = &mut *projection {
            running.fov.get_or_insert(p.fov);
            p.fov = sample.fov_radians();
        }
    }
}

fn end(world: &mut World, player: &mut Player, why: &str) {
    let Some(running) = player.running.take() else { return };
    info!("NIS {} {why} at {:.2} s", running.scene.name, running.time);
    PLAYING.store(false, std::sync::atomic::Ordering::Relaxed);
    if let Some(mut f) = world.get_resource_mut::<crate::nis_dof::NisTakeFocus>() {
        f.0 = None;
    }
    for extra in &running.extras {
        if let Ok(e) = world.get_entity_mut(extra.root) {
            e.despawn();
        }
    }
    // NIS complete (8285F720) pops mode 12.
    if let Some(mut pause) = world.get_resource_mut::<crate::pause_man::ThePauseMan>() {
        pause.pop(skate_core::pause_man::mode::NIS);
    }
    if let (Some(fov), Some(camera)) = (running.fov, gameplay_camera(world)) {
        if let Some(mut projection) = world.get_mut::<Projection>(camera) {
            if let Projection::Perspective(p) = &mut *projection {
                p.fov = fov;
            }
        }
    }
    if running.swapped {
        let saved = running.saved.flatten();
        if let Some(mut models) = world.get_resource_mut::<crate::custom_models::CustomModels>() {
            if models.previewing() {
                models.request_preview(saved.as_deref());
            }
        }
    }
    report_finished(world);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marquee_actors_get_skater_looks_and_livingworld_peds_ped_looks() {
        let actor = |character: &str, kind: &str| skate_data::scene_table::SceneActor { character: character.into(), kind: kind.into(), clip: String::new() };
        assert_eq!(extra_look(&actor("chris_cole", "marquee")), Some(false));
        assert_eq!(extra_look(&actor("male_adult_2", "livingworld")), Some(true));
        assert_eq!(extra_look(&actor("zprop_ghettoBlaster", "livingworld")), Some(true), "a world prop: a livingworld model on the ped skeleton");
        assert_eq!(extra_look(&actor("x", "other")), None);
        // The ped look follows the recipe key (a mod's override first).
        assert_eq!(crate::living_world::peds::glb_path(&Default::default(), "female_adult_1"), "private/living_world/models/female_adult_1.glb");
    }

    #[test]
    fn a_scene_plays_in_its_frame() {
        let scene = |floats| Scene { name: "s".into(), hash: 0, kind: 0, floats, actors: Vec::new() };
        let lesson = scene_frame(&scene([0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 22.3]));
        assert!(lesson.abs_diff_eq(Mat4::IDENTITY, 1e-6), "the lesson demos are in map space");
        let pro = scene_frame(&scene([-169.5, 30.5, 271.7, 0.0, 0.0, -1.0, 14.7]));
        assert!(pro.transform_point3(Vec3::new(-8.7, 0.9, -1.6)).abs_diff_eq(Vec3::new(-178.2, 31.4, 270.1), 1e-3));
        let turned = scene_frame(&scene([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
        assert!(turned.transform_vector3(Vec3::NEG_Z).abs_diff_eq(Vec3::X, 1e-6), "the facing turns the scene");
    }

    #[test]
    fn skippable_starts_skip_except_the_timed_kinds() {
        assert!(may_skip(true, 0x1300_0000, 0.0));
        assert!(!may_skip(false, 0x1300_0000, 5.0));
        for kind in TIMED_SKIP_KINDS {
            assert!(!may_skip(true, kind << 24, 1.9));
            assert!(may_skip(true, kind << 24, 2.0));
        }
    }
}
