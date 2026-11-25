//! Depth of field in NIS scenes and photo shots (research b1071, b1073).
//!
//! Retail: an ICE take with RAWFOCUS_OVERRIDE hands four values to the camera (826765C0); PostFx_Update (827F3D20)
//! turns them into the focal planes 827F1158 uploads: sharp up to focalFar = focus + range / 2, a linear ramp to full
//! blur at farBlur = focalFar + falloff, a blur radius of max(CoC * maxCoC, 0.1) with maxCoC = blur * 5. The near
//! planes are fixed far behind the camera (-1000 / -500), so only the far side ever blurs. `bloom_dof_dof_downsamplePS`
//! writes the CoC, `bloom_dof_tap9dofPS` blurs nine taps weighted by their own CoC and `postfx_uberPS` blends by it.
//! Ours: one full-screen pass before tone mapping with the same planes and the nine weighted taps; the values come from
//! the take on screen (`NisDof`, set by the NIS player and the reel) and the scales are `DofTuning` (retail defaults).
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
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{binding_types::*, *},
        renderer::RenderContext,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use skate_data::ice_camera::CameraFocus;

/// The scales and fixed planes of retail's DOF path, retail values as defaults (a mod may change them).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct DofTuning {
    /// maxCoC = the camera's blur value * this (0x821F1790).
    pub coc_scale: f32,
    /// A focus range below this takes the fallback planes (827F3D20).
    pub min_range: f32,
    pub fallback_focal_far: f32,
    pub fallback_far_blur: f32,
    /// The fixed near planes (0x822272E0 / 0x822F8EA0): behind the camera, so the near side never blurs.
    pub near_blur: f32,
    pub focal_near: f32,
    /// The tap9 pass's smallest radius (`max(CoC * f_maxCoC, 0.1)`).
    pub min_radius: f32,
    /// Screen pixels per CoC unit: retail measures the radius in its DOF target's texels [inferred: a half size target].
    pub pixels_per_unit: f32,
}

impl Default for DofTuning {
    fn default() -> Self {
        Self { coc_scale: 5.0, min_range: 0.01, fallback_focal_far: 100.0, fallback_far_blur: 200.0, near_blur: -1000.0,
            focal_near: -500.0, min_radius: 0.1, pixels_per_unit: 2.0 }
    }
}

/// The DOF on screen this frame; `on` only while a take with RAWFOCUS_OVERRIDE plays.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
pub(crate) struct NisDof {
    pub on: bool,
    pub focal_far: f32,
    pub far_blur: f32,
    pub max_coc: f32,
    pub near_blur: f32,
    pub focal_near: f32,
    pub min_radius: f32,
    pub pixels_per_unit: f32,
}

impl NisDof {
    /// The planes PostFx_Update builds from the camera's four values.
    pub(crate) fn from_camera(c: &CameraFocus, t: &DofTuning) -> Self {
        let mut focal_far = c.focus + 0.5 * c.range;
        let mut far_blur = focal_far + c.falloff;
        if c.range < t.min_range {
            (focal_far, far_blur) = (t.fallback_focal_far, t.fallback_far_blur);
        }
        Self { on: true, focal_far, far_blur, max_coc: c.blur * t.coc_scale, near_blur: t.near_blur,
            focal_near: t.focal_near, min_radius: t.min_radius, pixels_per_unit: t.pixels_per_unit }
    }

    /// The circle of confusion at view depth `z` (bloom_dof_dof_downsamplePS): the far plane from focalNear on.
    pub(crate) fn coc(&self, z: f32) -> f32 {
        if z >= self.focal_near {
            let slope = -1.0 / (self.far_blur - self.focal_far).max(1e-4);
            ((self.focal_far - z) * slope).clamp(0.0, 1.0)
        } else {
            let slope = 1.0 / (self.focal_near - self.near_blur).max(1e-4);
            ((self.focal_near - z) * slope).clamp(0.0, 1.0)
        }
    }
}

/// The focus of the NIS player's take on screen (None: no scene, or the take has no DOF).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub(crate) struct NisTakeFocus(pub Option<skate_data::ice_camera::TakeFocus>);

/// The take on screen: a reel clip's while the replay plays one, else the NIS player's; the photo picker's DOF edit
/// adds to its intensity (826765C0).
fn update(tuning: Res<DofTuning>, nis: Res<NisTakeFocus>, replay: Option<Res<crate::replay::Replay>>, mut dof: ResMut<NisDof>) {
    let reel = replay.as_ref().filter(|r| r.active).and_then(|r| r.reel_shot.as_ref()).map(|s| s.focus);
    let photo = replay.as_ref().map_or(0.0, |r| r.photo_dof);
    let focus = match reel { Some(f) => f, None => nis.0 };
    let next = focus.map(|f| NisDof::from_camera(&f.camera(photo), &tuning)).unwrap_or_default();
    if *dof != next {
        if next.on != dof.on {
            info!("NIS_DOF {}", if next.on { format!("on focal_far={} far_blur={} max_coc={}", next.focal_far, next.far_blur, next.max_coc) } else { "off".into() });
        }
        *dof = next;
    }
}

pub(crate) struct NisDofPlugin;

impl Plugin for NisDofPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "retail_dof.wgsl");
        app.init_resource::<NisDof>().init_resource::<DofTuning>().init_resource::<NisTakeFocus>()
            .add_plugins(ExtractResourcePlugin::<NisDof>::default())
            .add_systems(PostUpdate, update);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(RenderStartup, initialize)
                .add_render_graph_node::<ViewNodeRunner<DofNode>>(Core3d, DofLabel)
                .add_render_graph_edges(Core3d, (Node3d::EndMainPass, DofLabel, Node3d::Tonemapping));
        }
    }
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedRenderPipelineId,
    sampler: Sampler,
}

fn initialize(mut commands: Commands, cache: Res<PipelineCache>, assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>, device: Res<bevy::render::renderer::RenderDevice>) {
    let layout = BindGroupLayoutDescriptor::new("retail dof", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (
        texture_2d(TextureSampleType::Float { filterable: true }),
        sampler(SamplerBindingType::Filtering),
        texture_depth_2d(),
        uniform_buffer::<[Vec4; 3]>(false),
    )));
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("retail dof".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load(bevy::asset::AssetPath::from(bevy::asset::embedded_path!("retail_dof.wgsl")).with_source("embedded")),
            targets: vec![Some(ColorTargetState { format: ViewTarget::TEXTURE_FORMAT_HDR, blend: None, write_mask: ColorWrites::ALL })],
            ..default()
        }),
        ..default()
    });
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    commands.insert_resource(Pipeline { layout, id, sampler });
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct DofLabel;

#[derive(Default)]
struct DofNode;

impl ViewNode for DofNode {
    type ViewQuery = (&'static ViewTarget, &'static ExtractedView, Option<&'static ViewDepthTexture>);

    fn run<'w>(&self, _: &mut RenderGraphContext, context: &mut RenderContext, (target, view, depth): QueryItem<Self::ViewQuery>,
        world: &'w World) -> Result<(), NodeRunError> {
        let dof = world.resource::<NisDof>();
        if !dof.on || dof.max_coc <= 0.0 {
            return Ok(());
        }
        let (Some(p), Some(depth)) = (world.get_resource::<Pipeline>(), depth) else { return Ok(()) };
        if depth.texture.sample_count() != 1 || !depth.texture.usage().contains(TextureUsages::TEXTURE_BINDING) {
            return Ok(());
        }
        let cache = world.resource::<PipelineCache>();
        let Some(pipeline) = cache.get_render_pipeline(p.id) else { return Ok(()) };
        // Reverse-Z infinite perspective: view depth = near / depth, near = clip_from_view[3][2].
        let near = view.clip_from_view.w_axis.z;
        let params = [
            Vec4::new(dof.focal_far, dof.far_blur, dof.focal_near, dof.near_blur),
            Vec4::new(dof.max_coc, dof.min_radius, dof.pixels_per_unit, near),
            Vec4::ZERO,
        ];
        let bytes: Vec<u8> = params.iter().flat_map(|v| v.to_array()).flat_map(f32::to_le_bytes).collect();
        let device = context.render_device();
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("retail dof params"), contents: &bytes, usage: BufferUsages::UNIFORM,
        });
        let post = target.post_process_write();
        let group = device.create_bind_group("retail dof", &cache.get_bind_group_layout(&p.layout),
            &BindGroupEntries::sequential((post.source, &p.sampler, depth.view(), buffer.as_entire_binding())));
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("retail dof"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None,
                ops: Operations::default() })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_blurs_only_beyond_its_far_focus_and_falls_back_on_a_tiny_range() {
        let t = DofTuning::default();
        // FAR_FOCUS 6, NEAR_FOCUS 2, falloff 0.5 (x2 = 1 m ramp), intensity 0.5 (x3 = 1.5, x5 = 7.5).
        let take = skate_data::ice_camera::TakeFocus { near: 2.0, far: 6.0, falloff: 0.5, intensity: 0.5, relative: false };
        let d = NisDof::from_camera(&take.camera(0.0), &t);
        assert_eq!((d.focal_far, d.far_blur, d.max_coc), (6.0, 7.0, 7.5));
        assert_eq!(d.coc(1.0), 0.0, "the near side never blurs (the near planes are behind the camera)");
        assert_eq!(d.coc(6.0), 0.0);
        assert!((d.coc(6.5) - 0.5).abs() < 1e-6, "halfway up the ramp");
        assert_eq!(d.coc(50.0), 1.0);
        let flat = skate_data::ice_camera::TakeFocus { near: 3.0, far: 3.0, ..take };
        let f = NisDof::from_camera(&flat.camera(0.0), &t);
        assert_eq!((f.focal_far, f.far_blur), (100.0, 200.0), "a range under 0.01 takes the fallback planes");
    }
}
