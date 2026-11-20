//! FE 62's skater preview (retail MarqueeViewer, research b575..b580, b585): the highlighted skater stands on a stage
//! above the map playing MARQUEE_STAND, the camera in front. Details: docs/hails-additions/31-retail-menus.md#fe-62-preview
use crate::{animation::AnimationStatus, spawn_skater_host::SpawnSkaterRequest, world::PlayerRoot};
use bevy::prelude::*;
use skate_core::spawn_skater::{Action, Skater};
use skate_data::scene_clip::SceneClip;
use std::sync::Arc;

/// The stage (vault `freeskate` layout, b576 / b578, placement traced b585), data so a mod can move it.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct MarqueeStage {
    /// World position of the character's feet and its heading (degrees about +Y).
    pub position: Vec3,
    pub heading_degrees: f32,
    /// Camera eye and forward (world), as the game camera got them (b585 trace, `MQSET`).
    pub eye: Vec3,
    pub forward: Vec3,
    pub fov_degrees: f32,
    /// `stock/data/anim/<bank>` and the clip (b579: loops by its own flag).
    pub bank: String,
    pub clip: String,
    /// The set around the stage, `private/hud/scene/<set>` (setup's export of DIST_CreateSakter, modeled in place).
    pub set: String,
    /// The camera up vector, and the stage origin the CAS camera rows are relative to (82531868 adds it; (0, 200, 0)
    /// from the b585 trace: eye y 200.48 = the freeskate row's 0.48 + 200, research b1036).
    pub up: Vec3,
    pub origin: Vec3,
}

impl Default for MarqueeStage {
    fn default() -> Self {
        Self {
            position: Vec3::new(-1.0, 200.0, -7.05),
            heading_degrees: -100.0,
            eye: Vec3::new(-4.0, 200.48, -7.4),
            forward: Vec3::new(0.989, 0.127, -0.076),
            fov_degrees: 60.0,
            bank: "sk83_marquee_viewer.abin".into(),
            clip: "MARQUEE_STAND".into(),
            set: "MarqueeStage.skate".into(),
            up: Vec3::Y,
            origin: Vec3::new(0.0, 200.0, 0.0),
        }
    }
}

/// Set by the retail menu stage: FE 62 is the top screen in a mode that previews (Free Play, Demo; not Call Skater),
/// or FE 15 Create-a-Skater is open (`own`: the player's own skater in its outfit stands on the set).
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct MarqueeView {
    pub open: bool,
    pub own: bool,
    /// FE 15's camera row (cas_host::CasCamera, stage space); None keeps the stage's own camera.
    pub camera: Option<crate::cas_host::CasCamera>,
    /// FE 15's turntable angle in radians, added to the character's yaw (8253B618 -> 825325D8).
    pub turntable: f32,
    /// FE 15's body target row (clip, yaw, place) and the skater's gender (the CAS_Stand bank is per gender).
    pub body: Option<crate::cas_host::CasBody>,
    pub male: bool,
}

/// The bank a body target's clip is in, None = the stage's own clip. CAS_Stand keeps the stage's MARQUEE_STAND: the
/// PS3 recording (3:00-10:15) shows the skater holding the board on the whole-body rows, and the only clip named
/// CAS_STAND (cac_viewer_<male|female>.abin) stands without it, so which clip retail's animator plays for CAS_Stand
/// is open. CAS_SINGLE_POSE_1..8 are only in sk83_marquee_viewer [inferred]. A gesture row names the male clip
/// (`cac_male_<gesture>`); its bank is miscload's `gesture/cac_<gender>_<gesture>.abin` (one clip each, the gender
/// swap [inferred from the file names]). Returns the bank and the clip name, None = the bank's first clip.
fn body_bank(clip: &str, male: bool) -> Option<(String, Option<String>)> {
    let lower = clip.to_ascii_lowercase();
    if lower.starts_with("cas_single_pose_") {
        return Some(("sk83_marquee_viewer.abin".into(), Some(clip.into())));
    }
    let gesture = lower.strip_prefix("cac_male_").or_else(|| lower.strip_prefix("cac_female_"))?;
    Some((format!("gesture/cac_{}_{gesture}.abin", if male { "male" } else { "female" }), None))
}

#[derive(Resource, Default)]
struct Wanted(Option<Skater>);

#[derive(Default)]
struct Viewer {
    active: bool,
    /// The player's character before the preview (`CustomModels::active`), put back on close.
    saved: Option<Option<String>>,
    shown: Option<Skater>,
    clip: Option<Option<Arc<SceneClip>>>,
    /// The bank and clip `clip` was loaded for (FE 15's body targets change it).
    clip_key: Option<(String, String)>,
    /// A play-once body target's clip that played out (the stand shows again) and the body clip posted last.
    once_done: Option<String>,
    last_body: Option<String>,
    /// SKATE_TRACE_MARQUEE only: the bone transforms written last frame.
    written: Vec<(Entity, Transform)>,
    bindings: Option<AnimationStatus>,
    /// Bound after the swap for `shown` finished, with the skinned mesh count then (outfit parts spawn later).
    bound_for: Option<Skater>,
    skins: usize,
    time: f32,
    fov: Option<f32>,
    set: Option<Option<Arc<skate_data::skate_map::SkateMap>>>,
    stage: Vec<Entity>,
}

pub(crate) struct MarqueePreviewPlugin;
impl Plugin for MarqueePreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarqueeStage>().init_resource::<MarqueeView>().init_resource::<Wanted>().add_systems(
            Update,
            (wanted, drive).chain().after(crate::app::FrameSet::Animation).after(crate::camera::present)
                .before(crate::app::FrameSet::Verification),
        );
    }
}

/// `layer`'s driven bones over `base`, matched by bone name, at the layer's frames and rate (the base loops).
fn layered(base: &SceneClip, layer: &SceneClip, driven: &[bool]) -> SceneClip {
    let from: Vec<Option<usize>> = base.bone_names.iter()
        .map(|n| layer.bone_names.iter().position(|l| l.eq_ignore_ascii_case(n)).filter(|&i| driven.get(i) == Some(&true)))
        .collect();
    let frames = (0..layer.frames.len()).map(|i| {
        let at = ((i as f32 / layer.fps * base.fps) as usize) % base.frames.len().max(1);
        base.frames.get(at).map(|b| b.iter().zip(&from)
            .map(|(word, l)| l.map_or(*word, |l| layer.frames[i][l])).collect()).unwrap_or_default()
    }).collect();
    SceneClip { name: layer.name.clone(), bone_names: base.bone_names.clone(), parents: base.parents.clone(), fps: layer.fps, frames }
}

/// FlowMessage 0x8D (82705E88): the highlighted skater.
fn wanted(mut requests: MessageReader<SpawnSkaterRequest>, mut wanted: ResMut<Wanted>) {
    for SpawnSkaterRequest(action) in requests.read() {
        if let Action::Preview(skater) = action {
            wanted.0 = Some(skater.clone());
        }
    }
}

fn drive(world: &mut World, mut viewer: Local<Viewer>) {
    let MarqueeView { open, own, camera, turntable, body, male } = world.resource::<MarqueeView>().clone();
    let Some(player) = world.query_filtered::<Entity, With<PlayerRoot>>().iter(world).next() else { return };
    // The first highlight is posted in the frame FE 62 opens, before the stage reports it open: keep it.
    if !open {
        if viewer.active {
            close(world, &mut viewer);
            world.resource_mut::<Wanted>().0 = None;
        }
        return;
    }
    let Some(models) = world.get_resource::<crate::custom_models::CustomModels>() else { return };
    if !viewer.active {
        viewer.active = true;
        viewer.saved = Some(models.active.clone());
        info!("MARQUEE_VIEWER open (player {:?})", models.active);
        open_stage(world, &mut viewer);
    }
    let stage = world.resource::<MarqueeStage>().clone();
    // A new highlight swaps the character (retail unloads the external character and streams the previewed one).
    let wanted = if own { Some(Skater::Own) } else { world.resource::<Wanted>().0.clone() };
    if wanted.is_some() && wanted != viewer.shown {
        let id = match &wanted {
            Some(Skater::Character(id)) => Some(id.as_str()),
            _ => None,
        };
        let mut models = world.resource_mut::<crate::custom_models::CustomModels>();
        if !models.request_preview(id) {
            warn!("MARQUEE_VIEWER no character {id:?}");
        }
        info!("MARQUEE_VIEWER load {wanted:?}");
        viewer.shown = wanted;
        viewer.bindings = None;
        viewer.bound_for = None;
        viewer.time = 0.0;
    }
    // Every open frame, the swap frames included: the gameplay camera (`camera::present`) would show the map.
    let mut stage = stage;
    let stand = (stage.bank.clone(), stage.clip.clone());
    if let Some(c) = camera {
        let at = |v: [f32; 3]| stage.origin + Vec3::from_array(v);
        stage.eye = at(c.eye);
        stage.forward = (at(c.target) - stage.eye).normalize_or(stage.forward);
        stage.up = Vec3::from_array(c.up).normalize_or(Vec3::Y);
        stage.fov_degrees = c.fov_degrees;
    }
    stage_camera(world, &mut viewer, &stage);
    // FE 15's body target: the row's clip, yaw and place (82532800).
    let body_clip = body.as_ref().map(|b| b.clip.clone());
    if viewer.last_body != body_clip {
        viewer.last_body = body_clip;
        viewer.once_done = None;
    }
    let mut once = false;
    let mut first_clip = false;
    if let Some(body) = &body {
        let done = body.once && viewer.once_done.as_deref() == Some(body.clip.as_str());
        if let Some((bank, clip)) = body_bank(&body.clip, male).filter(|_| !done) {
            stage.bank = bank;
            first_clip = clip.is_none();
            stage.clip = clip.unwrap_or_else(|| body.clip.clone());
            once = body.once;
        }
        stage.heading_degrees = body.yaw_degrees;
        stage.position = stage.origin + Vec3::from_array(body.position);
    }
    if viewer.clip_key.as_ref() != Some(&(stage.bank.clone(), stage.clip.clone())) {
        viewer.clip_key = Some((stage.bank.clone(), stage.clip.clone()));
        viewer.clip = None;
        viewer.bound_for = None;
        viewer.time = 0.0;
    }
    if world.resource::<crate::custom_models::CustomModels>().swapping() || viewer.shown.is_none() {
        return;
    }
    let clip = viewer.clip.get_or_insert_with(|| {
        let anim = world.resource::<crate::config::Config>().asset_root.join("private/stock/data/anim");
        let path = anim.join(&stage.bank);
        let read = |path: &std::path::Path| std::fs::read(path).map_err(|e| e.to_string());
        // A gesture bank is a layer over the stand: its parts' bones on top of the stand clip [inferred: the banks'
        // poses store only some parts; retail's gesture stand cycle is not in the install].
        let parsed = if first_clip {
            read(&path).and_then(SceneClip::parse_layer).and_then(|(layer, driven)| {
                read(&anim.join(&stand.0)).and_then(|b| SceneClip::parse_named(b, Some(&stand.1)))
                    .map(|base| layered(&base, &layer, &driven))
            })
        } else {
            read(&path).and_then(|b| SceneClip::parse_named(b, Some(&stage.clip)))
        };
        parsed
            .inspect_err(|e| warn!("MARQUEE_VIEWER clip {} in {}: {e}", stage.clip, path.display()))
            .ok().map(Arc::new)
    }).clone();
    let Some(clip) = clip else { return };
    let skins = world.query::<&bevy::mesh::skinning::SkinnedMesh>().iter(world).count();
    if viewer.bound_for != viewer.shown || viewer.skins != skins {
        viewer.bindings = crate::trick_guide_scene::bind(world, player, &clip);
        viewer.bound_for = viewer.shown.clone();
        viewer.skins = skins;
        info!("MARQUEE_VIEWER show {:?}: {} joints bound", viewer.shown, viewer.bindings.as_ref().map_or(0, |b| b.online_bindings().len()));
    }
    let dt = world.resource::<Time<Real>>().delta_secs();
    viewer.time = if once {
        if viewer.time + dt >= clip.duration() {
            viewer.once_done = viewer.last_body.clone();
        }
        (viewer.time + dt).min(clip.duration())
    } else {
        (viewer.time + dt) % clip.duration().max(1e-3)
    };
    let Some(globals) = crate::trick_guide_scene::pose(&clip, viewer.time) else { return };
    // The character's world transform is translate(position) * rotY(heading) (b578), drawn through the player rig.
    let root = world.get::<GlobalTransform>(player).copied().unwrap_or_default().to_matrix();
    if std::env::var_os("SKATE_TRACE_MARQUEE").is_some() {
        let local = world.get::<Transform>(player).copied().unwrap_or_default();
        let global = root.to_scale_rotation_translation().2;
        info!("MARQUEE_TRACE t {:.3} global [{:.3}, {:.3}, {:.3}] local [{:.3}, {:.3}, {:.3}] gap {:.4} skins {skins}",
            world.resource::<Time<Real>>().elapsed_secs(), global.x, global.y, global.z,
            local.translation.x, local.translation.y, local.translation.z, global.distance(local.translation));
    }
    let place = root.inverse() * Mat4::from_rotation_translation(facing(stage.heading_degrees) * Quat::from_rotation_y(turntable), stage.position);
    if let Some(bindings) = &viewer.bindings {
        let matrices: Vec<Mat4> = globals.iter().map(|g| place * crate::animation::native_matrix(*g)).collect();
        let poses = bindings.pose_transforms(&matrices);
        if std::env::var_os("SKATE_TRACE_MARQUEE").is_some() {
            // The last frame's final world position of one bound bone (after every later writer), and whether any
            // bound bone's local transform changed since this system wrote it last frame (another writer).
            if let Some(bone) = poses.last().and_then(|(e, _)| world.get::<GlobalTransform>(*e)) {
                let p = bone.translation();
                info!("MARQUEE_BONE t {:.3} bone [{:.3}, {:.3}, {:.3}]", world.resource::<Time<Real>>().elapsed_secs(), p.x, p.y, p.z);
            }
            // A bone from the middle of the skeleton, which the stand clip moves (the idle still plays).
            if let Some(bone) = poses.get(poses.len() / 2).and_then(|(e, _)| world.get::<GlobalTransform>(*e)) {
                let p = bone.translation();
                info!("MARQUEE_MIDBONE t {:.3} bone [{:.3}, {:.3}, {:.3}]", world.resource::<Time<Real>>().elapsed_secs(), p.x, p.y, p.z);
            }
            let mut changed = 0usize;
            let mut worst = 0.0f32;
            for (entity, written) in viewer.written.iter() {
                if let Some(now) = world.get::<Transform>(*entity) {
                    let d = now.translation.distance(written.translation) + now.rotation.angle_between(written.rotation);
                    if d > 1e-4 {
                        changed += 1;
                        worst = worst.max(d);
                    }
                }
            }
            if changed > 0 {
                info!("MARQUEE_OVERWRITE t {:.3} bones {changed} of {} worst {worst:.4}",
                    world.resource::<Time<Real>>().elapsed_secs(), viewer.written.len());
            }
            viewer.written = poses.clone();
        }
        for (entity, transform) in poses {
            if let Some(mut node) = world.get_mut::<Transform>(entity) {
                *node = transform;
            }
        }
    }
}

fn stage_camera(world: &mut World, viewer: &mut Viewer, stage: &MarqueeStage) {
    let camera = world.query_filtered::<Entity, With<crate::camera::GameplayCamera>>().iter(world).next();
    if let Some(camera) = camera {
        if let Some(mut transform) = world.get_mut::<Transform>(camera) {
            *transform = Transform::from_translation(stage.eye).looking_to(stage.forward, stage.up);
        }
        if let Some(mut projection) = world.get_mut::<Projection>(camera) {
            if let Projection::Perspective(p) = &mut *projection {
                viewer.fov.get_or_insert(p.fov);
                p.fov = stage.fov_degrees.to_radians();
            }
        }
    }
}

/// The set the viewer's streamer move shows (retail streams the stage in with the camera move, b575).
fn open_stage(world: &mut World, viewer: &mut Viewer) {
    let asset_root = world.resource::<crate::config::Config>().asset_root.clone();
    let name = world.resource::<MarqueeStage>().set.clone();
    let set = viewer.set.get_or_insert_with(|| {
        let path = asset_root.join("private/hud/scene").join(&name);
        std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| skate_data::skate_map::SkateMap::parse_render_only(&b))
            .inspect_err(|e| warn!("MARQUEE_VIEWER stage {} unavailable (run setup's refresh): {e}", path.display()))
            .ok().map(Arc::new)
    }).clone();
    let Some(set) = set else { return };
    // Lit like the map around it (no render location of its own found).
    let environment = world.get_resource::<crate::map_transition::CurrentMap>()
        .and_then(|map| crate::retail_sky::SkyPackage::load(&asset_root, &map.name).ok())
        .map(|s| s.environment()).unwrap_or_default();
    viewer.stage = crate::trick_guide_scene::spawn_set(world, &set, &environment, None);
}

/// Retail's heading on our axes: the clip's forward is +X, a quarter turn from the heading's zero (in game
/// 2026-10-29: -100 degrees as is turned the character side on).
fn facing(heading_degrees: f32) -> Quat {
    Quat::from_rotation_y((heading_degrees - 90.0).to_radians())
}

/// Shutdown (0x2E60BEA0, b575 / b578): the player's own character comes back unless FE 62 made a real pick; the
/// gameplay presentation rewrites the rig and camera on its next frame.
fn close(world: &mut World, viewer: &mut Viewer) {
    if let Some(fov) = viewer.fov.take() {
        let camera = world.query_filtered::<Entity, With<crate::camera::GameplayCamera>>().iter(world).next();
        if let Some(mut projection) = camera.and_then(|c| world.get_mut::<Projection>(c)) {
            if let Projection::Perspective(p) = &mut *projection {
                p.fov = fov;
            }
        }
    }
    let saved = viewer.saved.take().flatten();
    if let Some(mut models) = world.get_resource_mut::<crate::custom_models::CustomModels>() {
        if models.previewing() {
            models.request_preview(saved.as_deref());
        }
    }
    for entity in std::mem::take(&mut viewer.stage) {
        if let Ok(e) = world.get_entity_mut(entity) {
            e.despawn();
        }
    }
    info!("MARQUEE_VIEWER closed (back to {saved:?})");
    *viewer = Viewer { clip: viewer.clip.take(), set: viewer.set.take(), ..default() };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail's heading turns the clip's forward (+X) toward the camera eye (b578 left the sign open; the vault's
    /// -100 degrees and the traced eye agree within 4 degrees).
    #[test]
    fn the_retail_stage_faces_the_character_to_the_camera() {
        let s = MarqueeStage::default();
        let forward = facing(s.heading_degrees) * Vec3::X;
        let to_eye = ((s.eye - s.position) * Vec3::new(1.0, 0.0, 1.0)).normalize();
        assert!(forward.angle_between(to_eye).to_degrees() < 5.0, "{forward} vs {to_eye}");
    }

    #[test]
    fn gesture_rows_play_from_the_gender_s_gesture_bank() {
        assert_eq!(body_bank("cac_male_spock", false), Some(("gesture/cac_female_spock.abin".into(), None)));
        assert_eq!(body_bank("cac_male_spock", true), Some(("gesture/cac_male_spock.abin".into(), None)));
        assert_eq!(body_bank("CAS_Single_Pose_3", true).map(|b| b.0), Some("sk83_marquee_viewer.abin".into()));
        assert_eq!(body_bank("CAS_Stand", true), None);
    }

    #[test]
    #[ignore = "requires the installed assets (SKATE3_ASSET_ROOT with private/stock/data/anim/gesture)"]
    fn every_gesture_bank_parses_as_a_scene_clip() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let dir = root.join("private/stock/data/anim/gesture");
        let stage = MarqueeStage::default();
        let stand = SceneClip::parse_named(std::fs::read(root.join("private/stock/data/anim").join(&stage.bank)).unwrap(), Some(&stage.clip)).unwrap();
        let mut failures = vec![];
        let mut clipless = vec![];
        let mut count = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            count += 1;
            match SceneClip::parse_layer(std::fs::read(&path).unwrap()) {
                Ok((clip, driven)) => {
                    let merged = layered(&stand, &clip, &driven);
                    let shared = stand.bone_names.iter().filter(|n| clip.bone_names.iter().enumerate()
                        .any(|(i, l)| driven[i] && l.eq_ignore_ascii_case(n))).count();
                    if clip.duration() <= 0.0 || shared == 0 || merged.frames.len() != clip.frames.len() {
                        failures.push(format!("{}: {shared} bones shared with the stand", path.display()));
                    }
                }
                // Unused banks without a clip (cac_<gender>_axe: no vault row names it).
                Err(e) if e.contains("has no clip") => clipless.push(path.display().to_string()),
                Err(e) => failures.push(format!("{}: {e}", path.display())),
            }
        }
        assert!(count >= 90, "{count} banks");
        assert!(clipless.len() <= 2, "{clipless:?}");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
