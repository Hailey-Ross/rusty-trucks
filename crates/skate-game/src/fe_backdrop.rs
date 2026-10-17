//! FE backdrop: the world behind a front-end screen blurs and darkens while that screen is on top of the FE screen
//! stack (doc 31, "Pause blur"). The ramp is `skate_core::fe_blur` (retail FE manager); which screens blur comes
//! from the menu tables (retail screen stack info table, extracted at setup); mods override it per screen with
//! `sdk.menus.backdrop`. The pass runs on the gameplay camera after the retail tone pass and before the UI, so the
//! menus themselves stay sharp.
use bevy::{
    asset::embedded_asset,
    core_pipeline::{
        FullscreenShader,
        core_3d::graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
    },
};
use serde_json::{Value, json};
use skate_core::fe_blur::{FeBlur, FeBlurParams};
use skate_core::fe_stack::{HudMode, ScreenInfo};
use skate_data::menu_tables::FeScreen;
use skate_mods::menus::BackdropOptions;

/// The pause crossbar's screen id (TU3 FE id 56, its key is unique; its record has the blur flag set).
pub(crate) const PAUSE_SCREEN: &str = "b4b7976b40b3e837";
/// The Game Settings screen's id (TU3 FE id 45, template 90 Options; its key is unique).
pub(crate) const SETTINGS_SCREEN: &str = "0ca9bfe5c1ba4995";
/// The Trick Guide screen's id (TU3 FE id 14, template 29 TrickTutorial; its key is unique).
pub(crate) const TRICK_GUIDE_SCREEN: &str = "5d1ecaaf6c7275bf";
/// The Video Calibration screen's id (TU3 FE id 51, template 91 VideoCalibration; its key is unique).
pub(crate) const VIDEO_CALIBRATION_SCREEN: &str = "1f3e2788f93b6a2e";

#[derive(Resource, Default)]
pub(crate) struct FeBackdrop {
    /// Retail screens in id order (menu tables; empty before setup extracted them).
    screens: Vec<FeScreen>,
    /// Screen ids on the FE stack, bottom first.
    stack: Vec<String>,
    blur: FeBlur,
    params: FeBlurParams,
    /// Mod overrides (mod id, screen id, options), oldest first so the newest wins.
    mods: Vec<(String, String, BackdropOptions)>,
    /// The PC overlay (`graphics_menu`) is showing: it blurs the world like the recomp's settings overlay.
    overlay: bool,
    /// The overlay's eased blur strength (retail units, see `OVERLAY_STRENGTH`).
    overlay_strength: f32,
}

/// The recomp's settings-overlay blur: a gaussian of sigma 9 px at 1080p (`skate3_menu_blur_sigma`). This pass's
/// sigma is half its radius and the radius is `strength * height / 720`, so 9 px at 1080p is strength 12.
const OVERLAY_STRENGTH: f32 = 12.0;
/// The recomp eases its menu sigma toward the target with a 60 ms time constant.
const OVERLAY_EASE: f32 = 0.06;

impl FeBackdrop {
    /// This build's index of a screen id (the FE id in its screen table).
    pub(crate) fn index(&self, id: &str) -> Option<usize> {
        self.screens.iter().position(|s| s.id == id)
    }

    pub(crate) fn id(&self, index: usize) -> Option<&str> {
        self.screens.get(index).map(|s| s.id.as_str())
    }

    /// The stack record of a screen (retail table; unknown screens get an empty record).
    pub(crate) fn info(&self, index: usize) -> ScreenInfo {
        self.screens.get(index).map_or_else(ScreenInfo::default, |s| ScreenInfo {
            suspends_gameplay: s.suspends_gameplay,
            chyron: s.chyron,
            blur: s.blur,
            hud: HudMode::from_retail(s.hud),
            input_to_movie: s.input_to_movie,
        })
    }

    /// The FE stack changed (retail applies the top screen's flag on every push / pop).
    pub(crate) fn set_stack(&mut self, keys: Vec<String>) {
        self.stack = keys;
    }

    pub(crate) fn set_mod(&mut self, owner: &str, screen: String, options: Option<BackdropOptions>) {
        self.mods.retain(|(o, s, _)| !(o == owner && *s == screen));
        if let Some(options) = options {
            self.mods.push((owner.to_owned(), screen, options));
        }
    }

    /// The PC overlay opened or closed (its tint is drawn by the overlay itself, like the recomp's).
    pub(crate) fn set_overlay(&mut self, showing: bool) {
        self.overlay = showing;
    }

    pub(crate) fn clear_owner(&mut self, owner: &str) {
        self.mods.retain(|(o, _, _)| o != owner);
    }

    pub(crate) fn clear_mods(&mut self) {
        self.mods.clear();
    }

    /// Blur flag and ramp for a screen: retail's record, then each mod's fields, newest last.
    fn resolve(&self, key: &str) -> (bool, FeBlurParams) {
        let mut on = self.screens.iter().find(|s| s.id == key).is_some_and(|s| s.blur);
        let mut p = FeBlurParams::RETAIL;
        for (_, _, o) in self.mods.iter().filter(|(_, s, _)| s == key) {
            on = o.blur.unwrap_or(on);
            p.strength = o.strength.unwrap_or(p.strength);
            p.step = o.step.unwrap_or(p.step);
            if let Some(t) = o.tint.as_deref().filter(|t| t.len() == 3) {
                p.tint = [t[0], t[1], t[2]];
            }
            p.tint_step = o.tint_step.unwrap_or(p.tint_step);
        }
        (on, p)
    }

    /// `sdk.menus.info().screens`: every retail screen id in table order with its blur flag.
    pub(crate) fn snapshot(&self) -> Value {
        Value::Array(self.screens.iter().map(|s| json!({"id": s.id, "blur": s.blur})).collect())
    }
}

/// HUD visibility from the FE screen stack (info record +8 of the main stack's top, sub_82D098F0 -> vt+44):
/// `hud` = the gameplay HUD, `online` = the online HUDs. Retail's gameplay screen (FE id 0, mode "on") sits at the
/// stack's bottom; ours has no screen there yet, so an empty stack means both on.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FeHud {
    pub hud: bool,
    pub online: bool,
}

impl Default for FeHud {
    fn default() -> Self {
        Self { hud: true, online: true }
    }
}

/// What the render pass draws this frame.
#[derive(Resource, Clone, Copy, Default, ExtractResource)]
struct Draw {
    /// Blur strength in retail units (8.0 = full).
    strength: f32,
    /// Tint as 0..255 per channel.
    tint: [i32; 3],
}

/// The cameras the backdrop applies to (the gameplay camera).
#[derive(Component, ExtractComponent, Clone, Copy, Default)]
struct BackdropView;

pub(crate) struct FeBackdropPlugin;

impl Plugin for FeBackdropPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "fe_backdrop.wgsl");
        app.init_resource::<FeBackdrop>()
            .init_resource::<FeHud>()
            .init_resource::<Draw>()
            .add_plugins((ExtractResourcePlugin::<Draw>::default(), ExtractComponentPlugin::<BackdropView>::default()))
            .add_systems(Startup, load)
            .add_systems(Update, (mark_camera, advance));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .add_systems(RenderStartup, initialize)
                .add_render_graph_node::<ViewNodeRunner<BackdropNode>>(Core3d, BackdropLabel)
                .add_render_graph_edges(Core3d, (Node3d::Tonemapping, BackdropLabel, Node3d::EndMainPassPostProcessing));
        }
    }

    fn finish(&self, app: &mut App) {
        // After the retail tone pass when it is there: the exposure meter must see the world, not the dimmed one.
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            let mut graph = render.world_mut().resource_mut::<RenderGraph>();
            if let Some(core) = graph.get_sub_graph_mut(Core3d) {
                if core.get_node_state(crate::retail_exposure::ExposureLabel).is_ok() {
                    let _ = core.try_add_node_edge(crate::retail_exposure::ExposureLabel, BackdropLabel);
                }
            }
        }
    }
}

fn load(config: Res<crate::config::Config>, mut backdrop: ResMut<FeBackdrop>) {
    backdrop.screens = std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
        .map(|t| t.screens)
        .unwrap_or_default();
}

fn mark_camera(mut commands: Commands, cameras: Query<Entity, (With<crate::camera::GameplayCamera>, Without<BackdropView>)>) {
    for camera in &cameras {
        commands.entity(camera).insert(BackdropView);
    }
}

/// Real time: the ramp runs while gameplay is suspended.
fn advance(time: Res<Time<Real>>, mut backdrop: ResMut<FeBackdrop>, mut draw: ResMut<Draw>) {
    let b = &mut *backdrop;
    match b.stack.last().cloned() {
        Some(top) => {
            let (on, params) = b.resolve(&top);
            b.params = params;
            b.blur.set_enabled(on);
        }
        // Empty stack: off, fading out with the last screen's ramp.
        None => b.blur.set_enabled(false),
    }
    b.blur.advance(time.delta_secs(), &b.params);
    let target = if b.overlay { OVERLAY_STRENGTH } else { 0.0 };
    b.overlay_strength += (target - b.overlay_strength) * (1.0 - (-time.delta_secs() / OVERLAY_EASE).exp());
    if b.overlay_strength < 0.05 && !b.overlay {
        b.overlay_strength = 0.0;
    }
    *draw = Draw { strength: b.blur.strength.max(b.overlay_strength), tint: b.blur.tint };
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
    sampler: Sampler,
    /// One uniform per pass (horizontal, then vertical + tint).
    params: [Buffer; 2],
}

fn initialize(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "fe backdrop",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<[Vec4; 2]>(false),
            ),
        ),
    );
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("fe backdrop".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load(
                bevy::asset::AssetPath::from(bevy::asset::embedded_path!("fe_backdrop.wgsl")).with_source("embedded"),
            ),
            targets: vec![Some(ColorTargetState {
                format: ViewTarget::TEXTURE_FORMAT_HDR,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    let buffer = |label| {
        device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: 32,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    commands.insert_resource(Pipeline {
        layout,
        pipeline,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        }),
        params: [buffer("fe backdrop horizontal"), buffer("fe backdrop vertical")],
    });
}

fn bytes(values: [Vec4; 2]) -> Vec<u8> {
    values.into_iter().flat_map(|v| v.to_array()).flat_map(f32::to_le_bytes).collect()
}

/// sRGB-encoded 0..255 to linear: retail multiplies the displayed colour, and this target holds linear values
/// that Bevy encodes to sRGB on output (doc 31: assumed, the retail shader is not ported).
fn linear(c: i32) -> f32 {
    let v = c.clamp(0, 255) as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct BackdropLabel;

#[derive(Default)]
struct BackdropNode;

impl ViewNode for BackdropNode {
    type ViewQuery = (&'static ViewTarget, &'static BackdropView);

    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext,
        (view, _): QueryItem<Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let draw = *world.resource::<Draw>();
        if draw.strength <= 0.0 && draw.tint == [255; 3] {
            return Ok(());
        }
        let p = world.resource::<Pipeline>();
        let cache = world.resource::<PipelineCache>();
        let Some(pipeline) = cache.get_render_pipeline(p.pipeline) else { return Ok(()) };
        // Retail strength is for its 1280 x 720 frame: scale the radius to this target's height.
        let height = view.main_texture().height() as f32;
        let radius = draw.strength * height / 720.0;
        let tint = Vec4::new(linear(draw.tint[0]), linear(draw.tint[1]), linear(draw.tint[2]), 1.0);
        let queue = world.resource::<bevy::render::renderer::RenderQueue>();
        queue.write_buffer(&p.params[0], 0, &bytes([Vec4::new(1.0, 0.0, radius, 0.0), Vec4::ONE]));
        queue.write_buffer(&p.params[1], 0, &bytes([Vec4::new(0.0, 1.0, radius, 0.0), tint]));
        for (pass_index, label) in [(0usize, "fe backdrop horizontal"), (1, "fe backdrop vertical")] {
            let post = view.post_process_write();
            let bind = context.render_device().create_bind_group(
                label,
                &cache.get_bind_group_layout(&p.layout),
                &BindGroupEntries::sequential((post.source, &p.sampler, p.params[pass_index].as_entire_binding())),
            );
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: post.destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations::default(),
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_render_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backdrop() -> FeBackdrop {
        FeBackdrop {
            screens: vec![
                FeScreen { id: "0000000000000001".into(), key: "0000000000000001".into(), blur: false, suspends_gameplay: false, chyron: false, hud: 1, input_to_movie: false },
                FeScreen { id: PAUSE_SCREEN.into(), key: PAUSE_SCREEN.into(), blur: true, suspends_gameplay: true, chyron: false, hud: 1, input_to_movie: true },
            ],
            ..default()
        }
    }

    #[test]
    fn retail_flag_then_newest_mod_wins() {
        let mut b = backdrop();
        assert_eq!(b.resolve(PAUSE_SCREEN), (true, FeBlurParams::RETAIL));
        assert!(!b.resolve("0000000000000001").0);
        b.set_mod("a", PAUSE_SCREEN.into(), Some(BackdropOptions { blur: Some(false), ..default() }));
        b.set_mod("b", PAUSE_SCREEN.into(), Some(BackdropOptions { tint: Some(vec![30, 30, 30]), ..default() }));
        let (on, p) = b.resolve(PAUSE_SCREEN);
        assert!(!on);
        assert_eq!(p.tint, [30, 30, 30]);
        b.clear_owner("a");
        assert!(b.resolve(PAUSE_SCREEN).0);
        b.set_mod("b", PAUSE_SCREEN.into(), None);
        assert_eq!(b.resolve(PAUSE_SCREEN), (true, FeBlurParams::RETAIL));
    }
}
