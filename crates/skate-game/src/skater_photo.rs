//! The team member photo (research b1013): the player's skater rendered into a 128 x 256 picture over the retail
//! backdrop, for FE 67's first card (`GetTeamData()[3]`), taken again after a preset pick; FE 68's preset preview
//! photos in the same setup. Details: docs/hails-additions/43-career.md#change-the-skater-photo
use crate::world::PlayerRoot;
use bevy::{
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// The photo's render layer (the player's meshes join it while the photo renders; 28 = the character light).
const LAYER: usize = 25;
/// The asset name `GetTeamData` gives the movie for the player's photo (the menu stage maps it to the picture).
pub(crate) const PHOTO_ASSET: &str = "skater_photo_0";
/// Slot 0 of the photo setup (class 569B02CE038D028A `team_profile_00`, b1013): size, field of view, backdrop.
/// NOT RETAIL YET: the setup's camera vectors are not decoded (b1013 open 1): a framing of the whole skater instead.
const SIZE: [u32; 2] = [128, 256];
const FOV_DEGREES: f32 = 45.0;
const BACKDROP: &str = "image/fe/source/images/photo/team_profile_background_0";
const DISTANCE: f32 = 2.8;
const EYE_HEIGHT: f32 = 0.9;
/// Frames the photo camera renders before the picture is kept (its own pipeline variants compile first).
const FRAMES: u8 = 20;
/// Slot 0's pose row (cac_body `CAS_Single_Pose_5`, b1013), a clip of the CAS animator's bank `cac_edit_<gender>`,
/// held at 0.1 s (pose row field 165367CDE7FF6727 [inferred]).
const POSE_CLIP: &str = "CAS_Single_Pose_5";
const POSE_TIME: f32 = 0.1;
/// Frames the worn outfit matches the look before the photo starts.
const SETTLED_FRAMES: u8 = 2;
/// The clip's forward (+X) turned to the camera.
const POSE_YAW_DEGREES: f32 = 90.0;

#[derive(Resource, Default)]
pub(crate) struct SkaterPhoto {
    /// Take a new photo of the player (FE 67 opening, a preset picked on FE 68).
    pub request: bool,
    /// The player's last finished photo.
    pub image: Option<Handle<Image>>,
    pub ready: bool,
    pub previews: Previews,
    shot: Option<Shot>,
    target: Option<Handle<Image>>,
    /// The pose clip by gender (true = male), read once.
    clips: std::collections::HashMap<bool, Option<std::sync::Arc<skate_data::scene_clip::SceneClip>>>,
    /// While a photo renders: the clip, the skeleton bound to it and the character's world matrix.
    pose: Option<(std::sync::Arc<skate_data::scene_clip::SceneClip>, crate::animation::AnimationStatus, Mat4)>,
    /// Frames the worn outfit has matched the look.
    settled: u8,
    frames: u8,
    camera: Option<Entity>,
    backdrop: Option<Entity>,
    layers: Vec<(Entity, Option<RenderLayers>)>,
}

/// FE 68's preview photos (82653750, 82653960 state 0): the highlighted preset is photographed in the same slot setup
/// as the player's card (collection FEFE76B65E8F6197) once the photographer is free; a highlight while it is busy
/// waits as the latest one (+0x324). The photos stay while the screen is open.
#[derive(Default)]
pub(crate) struct Previews {
    /// Finished photos by preset key.
    pub photos: std::collections::HashMap<String, Handle<Image>>,
    /// Bumped on every finished photo (the screen sends state 11 again).
    pub generation: u32,
    /// The player's look cannot be swapped (a custom model, or the customizer off): no photos are taken.
    pub unavailable: bool,
    /// Presets with no look to photograph (they read as loaded with no picture).
    pub missing: std::collections::HashSet<String>,
    /// The look the player wore before the first preview, put back when the screen closes without a pick.
    saved: Option<serde_json::Value>,
}

impl Previews {
    /// `IsPreviewPhotoLoaded` for a preset (825C4258: the row is in the photo map).
    pub(crate) fn has(&self, key: &str) -> bool {
        self.unavailable || self.photos.contains_key(key) || self.missing.contains(key)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Shot {
    Player,
    Preview(String),
}

impl SkaterPhoto {
    /// A preset was picked (82653960 states 2..8): its look stays on the player and the player's photo is taken again.
    pub(crate) fn retake_after_pick(&mut self) {
        self.previews.saved = None;
        self.request = true;
    }
}

/// The asset name `GetPreviewPhotoAssetId` gives the movie for a preset's photo.
pub(crate) fn preview_asset(key: &str) -> String {
    format!("skater_preview_{key}")
}

pub(crate) struct SkaterPhotoPlugin;
impl Plugin for SkaterPhotoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkaterPhoto>().add_systems(Update, drive.after(crate::app::FrameSet::Animation));
    }
}

fn drive(world: &mut World) {
    let (request, active) = {
        let p = world.resource::<SkaterPhoto>();
        (p.request, p.camera.is_some())
    };
    if active {
        hold_pose(world);
        let mut photo = world.resource_mut::<SkaterPhoto>();
        photo.frames += 1;
        if photo.frames >= FRAMES {
            finish(world);
        }
        return;
    }
    let (open, highlight) = world.get_resource::<crate::welcome_host::ImportSkaterScreen>()
        .map_or((false, None), |s| (s.open, s.highlight.clone()));
    if !open {
        let saved = world.resource_mut::<SkaterPhoto>().previews.saved.take();
        if let (Some(saved), Some(mut c)) = (saved, world.get_resource_mut::<crate::customiser::Customiser>()) {
            c.draft = saved;
        }
        let mut photo = world.resource_mut::<SkaterPhoto>();
        if !photo.previews.photos.is_empty() || !photo.previews.missing.is_empty() {
            photo.previews.photos.clear();
            photo.previews.missing.clear();
        }
    }
    // Not while the start is still loading: the scene's pipelines are not compiled yet and nothing would draw.
    let loading = world.get_resource::<crate::startup_loading::StartupLoading>().is_some_and(|s| !s.done());
    let wearable = wearable(world);
    world.resource_mut::<SkaterPhoto>().previews.unavailable = !wearable;
    // The skater rebuild finishes first (82653960 state 3 waits for the builder). The outfit swap lands after the
    // frame's visibility pass, so the swapped parts draw from the frame after it.
    if loading || !outfit_settled(world) {
        world.resource_mut::<SkaterPhoto>().settled = 0;
        return;
    }
    {
        let mut photo = world.resource_mut::<SkaterPhoto>();
        photo.settled = photo.settled.saturating_add(1);
        if photo.settled < SETTLED_FRAMES {
            return;
        }
    }
    if request {
        start(world, Shot::Player);
        return;
    }
    let Some(key) = highlight.filter(|k| open && wearable && !world.resource::<SkaterPhoto>().previews.has(k)) else { return };
    let look = {
        let (Some(state), Some(parts)) = (world.get_resource::<crate::career::CareerState>(), world.get_resource::<crate::customiser_parts::Parts>()) else { return };
        crate::career::preset_look(&state.presets, &key, parts)
    };
    let Some(look) = look else {
        // No look to wear: the row reads as loaded with no picture and is not asked again.
        let mut photo = world.resource_mut::<SkaterPhoto>();
        photo.previews.missing.insert(key);
        photo.previews.generation += 1;
        return;
    };
    let mut c = world.resource_mut::<crate::customiser::Customiser>();
    if c.draft != look {
        let worn = std::mem::replace(&mut c.draft, look);
        let mut photo = world.resource_mut::<SkaterPhoto>();
        photo.previews.saved.get_or_insert(worn);
        photo.settled = 0;
        return;
    }
    start(world, Shot::Preview(key));
}

/// The customizer's look is on the player's model (a custom model has no outfit to swap).
fn wearable(world: &World) -> bool {
    world.get_resource::<crate::customiser::Customiser>().is_some_and(|c| c.enabled)
        && world.get_resource::<crate::customiser_parts::Parts>().is_some()
        && world.get_resource::<crate::custom_models::CustomModels>().is_none_or(|m| m.active.is_none())
}

/// The worn outfit is the customizer's look (no part still loading or hidden).
fn outfit_settled(world: &World) -> bool {
    if !wearable(world) {
        return true;
    }
    let (Some(c), Some(parts)) = (world.get_resource::<crate::customiser::Customiser>(), world.get_resource::<crate::customiser_parts::Parts>()) else { return true };
    parts.applied == c.preview(parts)
}

fn start(world: &mut World, shot: Shot) {
    let Some(player) = world.query_filtered::<Entity, With<PlayerRoot>>().iter(world).next() else { return };
    // The skater's own world matrix (the root entity can still be at the origin at start).
    let root = world.get_resource::<crate::physics::SkaterRuntime>()
        .map(|s| GlobalTransform::from(crate::animation::native_matrix(s.animated_skeleton.roots.animation_to_world)))
        .unwrap_or_else(|| world.get::<GlobalTransform>(player).copied().unwrap_or_default());
    let mut target = Image::new_target_texture(SIZE[0], SIZE[1], TextureFormat::Rgba8UnormSrgb, None);
    target.sampler = bevy::image::ImageSampler::linear();
    let image = world.resource_mut::<Assets<Image>>().add(target);
    // The player's visible meshes join the photo layer (as the Trick Guide demo does).
    let camera_layers = world.query_filtered::<Option<&RenderLayers>, With<crate::camera::GameplayCamera>>()
        .iter(world).next().flatten().cloned().unwrap_or_default();
    let meshes: Vec<(Entity, Option<RenderLayers>)> = world
        .query_filtered::<(Entity, Option<&RenderLayers>, Option<&InheritedVisibility>), With<Mesh3d>>()
        .iter(world)
        .filter(|(_, l, v)| v.is_none_or(|v| v.get()) && l.cloned().unwrap_or_default().intersects(&camera_layers))
        .map(|(e, l, _)| (e, l.cloned()))
        .collect();
    let mut layers = Vec::new();
    for (entity, old) in meshes {
        let mut ancestors = world.query::<&ChildOf>();
        let mut at = entity;
        while let Ok(parent) = ancestors.get(world, at) {
            at = parent.parent();
            if at == player {
                world.entity_mut(entity).insert(old.clone().unwrap_or_default().with(LAYER));
                layers.push((entity, old));
                break;
            }
        }
    }
    let eye = root.transform_point(Vec3::new(0.0, EYE_HEIGHT, DISTANCE));
    let look = root.transform_point(Vec3::new(0.0, EYE_HEIGHT - 0.05, 0.0));
    let camera = world.spawn((
        Camera3d::default(),
        Camera { order: -6, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
        RenderTarget::Image(image.clone().into()),
        Projection::Perspective(PerspectiveProjection { fov: FOV_DEGREES.to_radians(), near: 0.01, far: 200.0, ..default() }),
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::from_layers(&[LAYER, 28]),
        Transform::from_translation(eye).looking_at(look, Vec3::Y),
    )).id();
    let backdrop = backdrop_image(world).map(|texture| {
        let behind = DISTANCE + 1.5;
        let at = eye + (look - eye).normalize() * behind;
        let height = 2.0 * (FOV_DEGREES.to_radians() * 0.5).tan() * behind * 1.02;
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Rectangle::new(height * SIZE[0] as f32 / SIZE[1] as f32, height));
        let material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color_texture: Some(texture),
            unlit: true,
            cull_mode: None,
            ..default()
        });
        world.spawn((Mesh3d(mesh), MeshMaterial3d(material), RenderLayers::layer(LAYER),
            Transform::from_translation(at).looking_at(at + (at - eye), Vec3::Y))).id()
    });
    if backdrop.is_none() {
        warn!("SKATER_PHOTO backdrop {BACKDROP} not in this install (Refresh Game Files adds it)");
    }
    // The slot's pose clip on the skater (the world can be held under the boot flow: nothing else animates it).
    let male = world.get_resource::<crate::customiser::Customiser>().is_none_or(|c| c.draft["gender"].as_str() != Some("female"));
    let clip = pose_clip(world, male);
    let bindings = clip.as_ref().and_then(|c| crate::trick_guide_scene::bind(world, player, c));
    let character = Mat4::from_rotation_translation(root.rotation() * Quat::from_rotation_y(POSE_YAW_DEGREES.to_radians()), root.translation());
    let mut parts: Vec<String> = layers.iter().filter_map(|(e, _)| {
        let mut at = *e;
        loop {
            if let Some(p) = world.get::<crate::customiser_parts::PartRoot>(at) {
                return Some(p.0.clone());
            }
            at = world.get::<ChildOf>(at)?.parent();
        }
    }).collect();
    parts.sort();
    parts.dedup();
    let mut wanted: Vec<String> = world.get_resource::<crate::customiser::Customiser>()
        .and_then(|c| c.draft["selections"].as_object().map(|s| s.values().filter_map(|v| v["asset_id"].as_str().map(str::to_owned)).collect()))
        .unwrap_or_default();
    wanted.sort();
    info!("SKATER_PHOTO start {shot:?}: {} meshes, eye {eye:?}, pose {}, parts {parts:?}, look {wanted:?}", layers.len(), bindings.is_some());
    let mut photo = world.resource_mut::<SkaterPhoto>();
    photo.pose = clip.zip(bindings).map(|(c, b)| (c, b, character));
    if shot == Shot::Player {
        photo.request = false;
        photo.ready = false;
    }
    photo.shot = Some(shot);
    photo.target = Some(image);
    photo.frames = 0;
    photo.camera = Some(camera);
    photo.backdrop = backdrop;
    photo.layers = layers;
    hold_pose(world);
}

/// `CAS_Single_Pose_5` from `cac_edit_<gender>.abin` (cached per gender; None when the install lacks it).
fn pose_clip(world: &mut World, male: bool) -> Option<std::sync::Arc<skate_data::scene_clip::SceneClip>> {
    if let Some(clip) = world.resource::<SkaterPhoto>().clips.get(&male) {
        return clip.clone();
    }
    let path = world.resource::<crate::config::Config>().asset_root
        .join(format!("private/stock/data/anim/cac_edit_{}.abin", if male { "male" } else { "female" }));
    let clip = std::fs::read(&path).map_err(|e| e.to_string())
        .and_then(|b| skate_data::scene_clip::SceneClip::parse_named(b, Some(POSE_CLIP)))
        .inspect_err(|e| warn!("SKATER_PHOTO pose {POSE_CLIP} in {}: {e}", path.display()))
        .ok().map(std::sync::Arc::new);
    world.resource_mut::<SkaterPhoto>().clips.insert(male, clip.clone());
    clip
}

/// The pose clip's sample written to the bound skeleton, every frame the photo renders.
fn hold_pose(world: &mut World) {
    let Some(player) = world.query_filtered::<Entity, With<PlayerRoot>>().iter(world).next() else { return };
    let root = world.get::<GlobalTransform>(player).copied().unwrap_or_default().to_matrix();
    let poses = {
        let photo = world.resource::<SkaterPhoto>();
        let Some((clip, bindings, character)) = &photo.pose else { return };
        let Some(globals) = crate::trick_guide_scene::pose(clip, POSE_TIME) else { return };
        let place = root.inverse() * *character;
        let matrices: Vec<Mat4> = globals.iter().map(|g| place * crate::animation::native_matrix(*g)).collect();
        bindings.pose_transforms(&matrices)
    };
    for (entity, transform) in poses {
        if let Some(mut node) = world.get_mut::<Transform>(entity) {
            *node = transform;
        }
    }
}

fn finish(world: &mut World) {
    let mut photo = world.resource_mut::<SkaterPhoto>();
    let (camera, backdrop, layers) = (photo.camera.take(), photo.backdrop.take(), std::mem::take(&mut photo.layers));
    let (shot, target) = (photo.shot.take(), photo.target.take());
    photo.pose = None;
    match shot {
        Some(Shot::Preview(key)) => {
            if let Some(image) = target {
                photo.previews.photos.insert(key, image);
            }
            photo.previews.generation += 1;
        }
        _ => {
            photo.image = target;
            photo.ready = true;
        }
    }
    for entity in camera.into_iter().chain(backdrop) {
        world.despawn(entity);
    }
    for (entity, old) in layers {
        if let Ok(mut e) = world.get_entity_mut(entity) {
            match old {
                Some(l) => { e.insert(l); }
                None => { e.remove::<RenderLayers>(); }
            }
        }
    }
    info!("SKATER_PHOTO ready");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_keeps_its_look_and_retakes_the_player_photo() {
        let mut photo = SkaterPhoto { ready: true, ..default() };
        photo.previews.saved = Some(serde_json::json!({"gender": "male"}));
        photo.retake_after_pick();
        assert!(photo.request && photo.previews.saved.is_none());
    }

    #[test]
    fn a_preview_counts_as_loaded_when_taken_missing_or_unavailable() {
        let mut previews = Previews::default();
        assert!(!previews.has("a"));
        previews.photos.insert("a".into(), Handle::default());
        previews.missing.insert("b".into());
        assert!(previews.has("a") && previews.has("b") && !previews.has("c"));
        previews.unavailable = true;
        assert!(previews.has("c"));
    }
}

/// The slot's backdrop from the menu export's named images.
fn backdrop_image(world: &mut World) -> Option<Handle<Image>> {
    let root = crate::retail_menu_movies::set_root(world.resource::<crate::config::Config>());
    let (width, height, bytes) = {
        let movies = world.get_resource::<crate::retail_menu_movies::RetailMenuMovies>()?;
        match movies.image(BACKDROP)? {
            crate::retail_menu_movies::MenuImage::Retail { path, width, height } => {
                let bytes = std::fs::read(root.join(path)).ok().filter(|b| b.len() == width as usize * height as usize * 4)?;
                (width, height, bytes)
            }
            crate::retail_menu_movies::MenuImage::Mod(m) => (m.width, m.height, m.rgba.clone()),
        }
    };
    Some(world.resource_mut::<Assets<Image>>().add(Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 }, TextureDimension::D2, bytes,
        TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::RENDER_WORLD)))
}
