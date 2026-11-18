//! The team member photo (research b1013): the player's skater rendered into a 128 x 256 picture over the retail
//! backdrop, for FE 67's first card (`GetTeamData()[3]`). Details: docs/hails-additions/43-career.md#change-the-skater-photo
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
/// NOT RETAIL YET: the setup's camera vectors are not decoded (b1013 open 1): a framing of the whole skater instead;
/// the slot's pose clip (CAS_Single_Pose_5) is not loaded, so the skater stands as in the world.
const SIZE: [u32; 2] = [128, 256];
const FOV_DEGREES: f32 = 45.0;
const BACKDROP: &str = "image/fe/source/images/photo/team_profile_background_0";
const DISTANCE: f32 = 2.8;
const EYE_HEIGHT: f32 = 0.9;
/// Frames the photo camera renders before the picture is kept (its own pipeline variants compile first).
const FRAMES: u8 = 20;

#[derive(Resource, Default)]
pub(crate) struct SkaterPhoto {
    /// Take a new photo (FE 67 opening).
    pub request: bool,
    /// The last finished photo.
    pub image: Option<Handle<Image>>,
    pub ready: bool,
    frames: u8,
    camera: Option<Entity>,
    backdrop: Option<Entity>,
    layers: Vec<(Entity, Option<RenderLayers>)>,
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
        let mut photo = world.resource_mut::<SkaterPhoto>();
        photo.frames += 1;
        if photo.frames >= FRAMES {
            finish(world);
        }
        return;
    }
    // Not while the start is still loading: the scene's pipelines are not compiled yet and nothing would draw.
    let loading = world.get_resource::<crate::startup_loading::StartupLoading>().is_some_and(|s| !s.done());
    if request && !loading {
        start(world);
    }
}

fn start(world: &mut World) {
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
    info!("SKATER_PHOTO start: {} meshes, eye {eye:?}", layers.len());
    let mut photo = world.resource_mut::<SkaterPhoto>();
    *photo = SkaterPhoto { request: false, image: Some(image), ready: false, frames: 0, camera: Some(camera), backdrop, layers };
}

fn finish(world: &mut World) {
    let mut photo = world.resource_mut::<SkaterPhoto>();
    let (camera, backdrop, layers) = (photo.camera.take(), photo.backdrop.take(), std::mem::take(&mut photo.layers));
    photo.ready = true;
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
