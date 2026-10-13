//! The Trick Guide's 3D demo behind FE 14 (TU3: screen init 0x82618A40 loads DIST_TrickGuide, update
//! sub_82619A20 posts "play <clip> with camera track <n>" to the trickbook object, which runs the ViewScene
//! scene: one character "current_player" playing `scene/trickguide/<name>1.abin`).
//! The player's own rig plays the clip (ViewScene's character is the current player), the set is spawned as a
//! child of the player root (clip and set share the scene space), and the gameplay camera draws only the demo
//! layer while the screen is open. Research: `.local/research/menus-m6/trick-guide-*.md`, doc 31 "Trick Guide demo".
//! NOT RETAIL YET: the camera (the 21 tracks in scene.big `ice_000.bin` are not decoded; a fixed side framing of
//! the hips stands in), csm_trickguide shadows, fog_trickguide, the set's sky dome.
use crate::{animation::AnimationStatus, map_render::{MapEntity, SceneCommands, StagedAssets}, world::PlayerRoot};
use bevy::{camera::visibility::RenderLayers, ecs::system::SystemState, mesh::skinning::SkinnedMesh, prelude::*};
use skate_core::animation::{output, pose_sample::{sample_key, select_frames}};
use skate_data::{scene_clip::SceneClip, skate_map::SkateMap};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// The demo's render layer: the set, the player's meshes while the demo runs, and the gameplay camera then.
pub(crate) const LAYER: usize = 27;
/// sub_82619A20: a new selection plays after S+332 = 60.0 counts down by the FE update's milliseconds.
const SETTLE_MS: f32 = 60.0;
/// Init 0x82618A40 copies this into S+204 (.rdata 0x82204B88): the clip before any trick is highlighted.
const DEFAULT_CLIP: &str = "trickguide_fs_noseblunt_01";

/// What the retail menu stage shows: FE 14 on top, and the highlighted entry's demo clip (S+204).
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub(crate) struct TrickGuideDemo {
    pub open: bool,
    pub animation: String,
}

#[derive(Component)]
struct SetEntity;

#[derive(Default)]
struct Scene {
    /// Scene folder: `SKATE3_TRICK_GUIDE_SCENE` (dev export) or `<assets>/private/hud/scene`.
    root: Option<PathBuf>,
    set: Option<Option<Arc<SkateMap>>>,
    clips: HashMap<String, Option<Arc<SceneClip>>>,
    active: bool,
    /// Requested clip and the settle countdown; the playing clip, its time and the rig bound to its bones.
    requested: String,
    settle: f32,
    playing: Option<(String, Arc<SceneClip>)>,
    time: f32,
    bindings: Option<AnimationStatus>,
    layers: Vec<(Entity, Option<RenderLayers>)>,
    camera: Option<(Entity, Option<RenderLayers>)>,
}

pub(crate) struct TrickGuideScenePlugin;
impl Plugin for TrickGuideScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrickGuideDemo>().add_systems(
            Update,
            drive.after(crate::app::FrameSet::Animation).after(crate::camera::present)
                .before(crate::app::FrameSet::Verification),
        );
    }
}

fn drive(world: &mut World, mut scene: Local<Scene>) {
    let demo = world.resource::<TrickGuideDemo>().clone();
    let Some(player) = world.query_filtered::<Entity, With<PlayerRoot>>().iter(world).next() else { return };
    if !demo.open {
        if scene.active {
            stop(world, &mut scene);
        }
        return;
    }
    if !scene.active {
        if !start(world, &mut scene, player) {
            return;
        }
    }
    let delta_ms = world.resource::<Time<Real>>().delta_secs() * 1000.0;
    // A new highlight waits SETTLE_MS; the scene restarts at time 0 only when the clip changes.
    // Rows without a clip (categories) leave S+204 as it was.
    if scene.requested.is_empty() {
        scene.requested = DEFAULT_CLIP.into();
    }
    if !demo.animation.is_empty() && demo.animation != scene.requested {
        scene.requested = demo.animation.clone();
        scene.settle = SETTLE_MS;
    }
    if scene.settle > 0.0 {
        scene.settle -= delta_ms;
    }
    let wanted = (scene.settle <= 0.0 && !scene.requested.is_empty()).then(|| scene.requested.clone());
    if let Some(name) = wanted.filter(|n| scene.playing.as_ref().is_none_or(|(p, _)| p != n)) {
        if let Some(clip) = clip(&mut scene, &name) {
            scene.bindings = bind(world, player, &clip);
            let hips = clip.bone_names.iter().position(|n| n == "HIPS").unwrap_or(0);
            let at = pose(&clip, 0.0).map(|g| crate::animation::native_matrix(g[hips]).w_axis.truncate());
            info!("TRICK_GUIDE_DEMO play {name}: {} frames at {} fps, {} joints bound, hips at {at:?}, root {:?}",
                clip.frames.len(), clip.fps, scene.bindings.as_ref().map_or(0, |b| b.online_bindings().len()),
                world.get::<GlobalTransform>(player).map(|t| t.translation()));
            scene.playing = Some((name, clip));
            scene.time = 0.0;
        }
    }
    let Some((_, clip)) = scene.playing.clone() else { return };
    // NOT RETAIL YET: looping assumed (ViewScene's loop mode; TrickGuideLoop is enum value 9, its use unread).
    scene.time = (scene.time + delta_ms / 1000.0) % clip.duration().max(1e-3);
    let Some(globals) = pose(&clip, scene.time) else { return };
    if let Some(bindings) = &scene.bindings {
        let matrices: Vec<Mat4> = globals.iter().copied().map(crate::animation::native_matrix).collect();
        for (entity, transform) in bindings.pose_transforms(&matrices) {
            if let Some(mut node) = world.get_mut::<Transform>(entity) {
                *node = transform;
            }
        }
    }
    // NOT RETAIL YET: stand-in framing (side view of the hips) until the ViewScene camera tracks are decoded.
    let hips = clip.bone_names.iter().position(|n| n == "HIPS").unwrap_or(0);
    let target = crate::animation::native_matrix(globals[hips]).w_axis.truncate();
    let root = world.get::<GlobalTransform>(player).copied().unwrap_or_default();
    // The menu covers the right half: aim right of the skater so they sit in the left third.
    let eye = root.transform_point(target + Vec3::new(1.2, 0.6, 5.5));
    let look = root.transform_point(target + Vec3::new(1.6, 0.0, 0.0));
    if let Some((camera, _)) = scene.camera {
        if let Some(mut transform) = world.get_mut::<Transform>(camera) {
            *transform = Transform::from_translation(eye).looking_at(look, Vec3::Y);
        }
    }
}

fn scene_root(world: &World) -> PathBuf {
    std::env::var_os("SKATE3_TRICK_GUIDE_SCENE").map(PathBuf::from).unwrap_or_else(|| {
        world.resource::<crate::config::Config>().asset_root.join("private/hud/scene")
    })
}

fn clip(scene: &mut Scene, name: &str) -> Option<Arc<SceneClip>> {
    let root = scene.root.clone()?;
    scene.clips.entry(name.to_owned()).or_insert_with(|| {
        // sub_82826488: "scene/trickguide/" + name + "1.abin".
        let path = root.join("trickguide").join(format!("{name}1.abin"));
        std::fs::read(&path).map_err(|e| e.to_string()).and_then(SceneClip::parse)
            .inspect_err(|e| warn!("TRICK_GUIDE_DEMO clip {}: {e}", path.display()))
            .ok().map(Arc::new)
    }).clone()
}

/// Local samples at `time`, composed into scene-space bone matrices (the clip places the skater in the set).
fn pose(clip: &SceneClip, time: f32) -> Option<Vec<output::NativeMatrix>> {
    let selection = select_frames(time, clip.fps, clip.frames.len(), true, 0.0).ok()?;
    let sqt = |w: skate_data::animation_frames::SampleWords| {
        let [sx, sy, sz, x, y, z, w, tx, ty, tz] = w.map(f32::from_bits);
        output::Sqt { scale: [sx, sy, sz, 1.0], rotation: [x, y, z, w], translation: [tx, ty, tz, 1.0] }
    };
    let locals: Vec<_> = clip.frames[selection.first].iter().zip(&clip.frames[selection.second])
        .map(|(&a, &b)| sample_key(sqt(a), sqt(b), selection)).collect();
    // The scene hierarchy has no trajectory link (HIPS parent -1); the HIPS delta adds up to identity, so the
    // trajectory carries the body. NOT RETAIL YET: parenting the root bones to it is inferred from the data
    // (test `bind_pose_stands_upright_over_the_scene_origin`); the scene character's composition is unread.
    let parents: Vec<i32> = clip.parents.iter().enumerate()
        .map(|(bone, &parent)| if parent == -1 && bone != 0 { 0 } else { parent }).collect();
    let mut globals: Vec<_> = locals.into_iter().map(output::sqt_to_matrix).collect();
    output::compose_hierarchy_in_place(globals.len() as i32, &parents, -1, &mut globals).ok()?;
    Some(globals)
}

fn bind(world: &mut World, player: Entity, clip: &SceneClip) -> Option<AnimationStatus> {
    let mut state: SystemState<(Query<(Entity, &SkinnedMesh)>, Query<(&Name, &Transform)>, Query<&ChildOf>)> =
        SystemState::new(world);
    let (skins, nodes, parents) = state.get(world);
    AnimationStatus::for_scene(player, &clip.bone_names, &skins, &nodes, &parents)
        .inspect_err(|e| warn!("TRICK_GUIDE_DEMO the player's rig cannot play scene clips: {e}"))
        .ok()
}

fn start(world: &mut World, scene: &mut Scene, player: Entity) -> bool {
    let root = scene_root(world);
    if scene.root.as_ref() != Some(&root) {
        *scene = Scene { root: Some(root.clone()), ..default() };
    }
    let set = scene.set.get_or_insert_with(|| {
        let path = root.join("TrickGuide.skate");
        std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| SkateMap::parse_render_only(&b))
            .inspect_err(|e| warn!("TRICK_GUIDE_DEMO set {} unavailable (run setup's refresh): {e}", path.display()))
            .ok().map(Arc::new)
    }).clone();
    let Some(set) = set else { return false };
    // The set: retail world materials through the map path, tagged as ours and parented to the player root.
    let asset_root = world.resource::<crate::config::Config>().asset_root.clone();
    let tuning = crate::retail_render::MaterialTuning::load(&asset_root);
    let mut meshes = StagedAssets::<Mesh>::staged(world);
    let mut materials = StagedAssets::<crate::retail_render::WorldMaterial>::staged(world);
    let mut images = StagedAssets::<Image>::staged(world);
    let mut buffers = StagedAssets::<bevy::render::storage::ShaderStorageBuffer>::staged(world);
    let mut commands = SceneCommands::default();
    crate::skate_world::spawn(&set, &tuning, &Default::default(), &mut commands, &mut meshes, &mut materials,
        &mut images, &mut buffers);
    // The set's own sky dome (mesh 0, sky.default), lit by the current map's sun.
    let map_sky = world.get_resource::<crate::map_transition::CurrentMap>()
        .and_then(|map| crate::retail_sky::SkyPackage::load(&asset_root, &map.name).ok());
    let mut skies = StagedAssets::<crate::retail_sky::SkyMaterial>::staged(world);
    match crate::retail_sky::SkyPackage::load_scene(&root, map_sky.as_ref()) {
        Ok(sky) => sky.spawn(&mut commands, &mut meshes, &mut images, &mut skies),
        Err(e) => warn!("TRICK_GUIDE_DEMO sky {}: {e}", root.display()),
    }
    meshes.publish_now(world);
    images.publish_now(world);
    materials.publish_now(world);
    buffers.publish_now(world);
    skies.publish_now(world);
    // The spawn path marks everything `MapEntity`: re-tag what this spawned as the demo set (not retired by a map
    // change), parent its roots to the player root and put every spawned entity on the demo layer.
    let before: std::collections::HashSet<Entity> =
        world.query_filtered::<Entity, With<MapEntity>>().iter(world).collect();
    commands.apply_now(world);
    let spawned: Vec<Entity> = world.query_filtered::<Entity, With<MapEntity>>().iter(world)
        .filter(|e| !before.contains(e)).collect();
    for root in spawned {
        let mut family = vec![root];
        while let Some(entity) = family.pop() {
            if let Some(children) = world.get::<Children>(entity) {
                family.extend(children.iter());
            }
            world.entity_mut(entity).insert(RenderLayers::layer(LAYER));
        }
        world.entity_mut(root).remove::<MapEntity>().insert((SetEntity, ChildOf(player)));
    }
    // The player's meshes the gameplay camera draws join the demo layer (visible ones on a layer the camera sees:
    // hidden rigs, e.g. the default skater under a custom model, stay out); the camera then sees only the demo
    // layer (and the character light's 28).
    let camera_layers = world.query_filtered::<Option<&RenderLayers>, With<crate::camera::GameplayCamera>>()
        .iter(world).next().flatten().cloned().unwrap_or_default();
    let mut meshes = world.query_filtered::<(Entity, Option<&RenderLayers>, Option<&InheritedVisibility>),
        (With<Mesh3d>, Without<SetEntity>)>();
    let owned: Vec<_> = meshes.iter(world)
        .filter(|(_, l, v)| v.is_none_or(|v| v.get()) && l.cloned().unwrap_or_default().intersects(&camera_layers))
        .map(|(e, l, _)| (e, l.cloned())).collect();
    for (entity, layers) in owned {
        let mut ancestors = world.query::<&ChildOf>();
        let mut at = entity;
        let mut under = false;
        while let Ok(parent) = ancestors.get(world, at) {
            at = parent.parent();
            if at == player { under = true; break; }
        }
        if under {
            world.entity_mut(entity).insert(layers.clone().unwrap_or_default().with(LAYER));
            scene.layers.push((entity, layers));
        }
    }
    let camera = world.query_filtered::<(Entity, Option<&RenderLayers>), With<crate::camera::GameplayCamera>>()
        .iter(world).next().map(|(e, l)| (e, l.cloned()));
    if let Some((entity, layers)) = &camera {
        world.entity_mut(*entity).insert(RenderLayers::from_layers(&[LAYER, 28]));
        scene.camera = Some((*entity, layers.clone()));
    }
    scene.active = true;
    info!("TRICK_GUIDE_DEMO open: set {} triangles", set.geometry.indices.len() / 3);
    true
}

/// Screen close (vt+24 0x82618CA8 unloads the set): despawn it, give the meshes and camera their layers back.
/// The gameplay presentation rewrites the rig and camera on its next frame.
fn stop(world: &mut World, scene: &mut Scene) {
    let set: Vec<Entity> = world.query_filtered::<Entity, With<SetEntity>>().iter(world).collect();
    for entity in set {
        world.despawn(entity);
    }
    for (entity, layers) in std::mem::take(&mut scene.layers) {
        if let Ok(mut e) = world.get_entity_mut(entity) {
            match layers {
                Some(l) => { e.insert(l); }
                None => { e.remove::<RenderLayers>(); }
            }
        }
    }
    if let Some((entity, layers)) = scene.camera.take() {
        if let Ok(mut e) = world.get_entity_mut(entity) {
            match layers {
                Some(l) => { e.insert(l); }
                None => { e.remove::<RenderLayers>(); }
            }
        }
    }
    scene.active = false;
    scene.playing = None;
    scene.requested.clear();
    scene.bindings = None;
    info!("TRICK_GUIDE_DEMO closed");
}
