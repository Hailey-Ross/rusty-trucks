//! GPU-only exposure adaptation and the retail tone curve.
//!
//! The world and character shaders emit linear radiance premultiplied by a
//! baseline exposure of 2.5. Nothing downstream of them is a no-op: the tone
//! pass divides that baseline back out, applies the retail curve, and inverts
//! sRGB so Bevy's output attachment does not encode gamma twice. Skipping it
//! clips every bright surface to white, which reads as chrome rather than as
//! an exposure fault.
//!
//! Metering is a portable 16x16 bilinear sample of the HDR target rather than
//! the console's resolve surface, evaluated with the native multiplicative
//! evaluator and centre weighting.
//!
//! While the gameplay camera draws into the window (graphics_menu.rs
//! `choose_presentation`), the tone pass writes the window itself, in place of
//! Bevy's upscaling, which would copy a toned 16-bit texture into it, and the
//! UI is drawn into the window after it (the vendored bevy_ui_render's
//! `UiToOutput`). `SKATE_TONE_DIRECT=0` keeps the copy (`choose_tone_output`).
//!
//! That pass writes the curve's result as it is, through a view of the
//! window's texture that doesn't encode sRGB, rather than invert sRGB for the
//! window's sRGB view to encode again (`ExposureNode`); `SKATE_TONE_GAMMA=0`
//! inverts it. The window's texture has to be `Rgba8Unorm`, as the Switch's
//! is; on any other the pass inverts sRGB as with 0.
use bevy::{
    asset::embedded_asset,
    camera::{CameraOutputMode, RenderTarget},
    core_pipeline::{
        FullscreenShader,
        core_3d::graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    image::BevyDefault,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::{ExtractedWindows, ViewTarget},
    },
    shader::ShaderDefVal,
    window::WindowRef,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::retail_render::{RetailScene, RetailTone, world_changed};

#[derive(Resource, Clone, ExtractResource)]
struct Settings {
    tuning: Vec4,
    timing: Vec4,
}

impl Default for Settings {
    fn default() -> Self {
        // Missing metadata retains the existing fixed exposure.
        Self {
            tuning: Vec4::new(0., 2.5, 2.5, 0.),
            timing: Vec4::ZERO,
        }
    }
}

/// On the gameplay camera while its tone pass writes the window (`choose_tone_output`), with the
/// clear the camera's output step had: the tone pass clears the window as Bevy's upscaling would.
#[derive(Component, ExtractComponent, Clone, Copy, Debug)]
struct ToneToOutput {
    clear: ClearColorConfig,
    /// `SKATE_TONE_GAMMA`: the pass writes the curve's result through a view that doesn't
    /// encode sRGB.
    gamma: bool,
}

/// Whether the tone pass can write the primary window: the window's view has the format the
/// window variant of the tone pipeline draws in, and that pipeline and the meter's are compiled.
/// The render world sets it each frame (`check_window`); `choose_tone_output` reads it, so the
/// camera changes over only when the frame can be drawn.
#[derive(Resource, Clone, Default)]
struct WindowReady(Arc<AtomicBool>);

#[derive(serde::Deserialize)]
struct Authored {
    target_luminance: f32,
    min: f32,
    max: f32,
    damping: f32,
}

pub(crate) struct RetailExposurePlugin;

impl Plugin for RetailExposurePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "retail_exposure.wgsl");
        embedded_asset!(app, "retail_tone.wgsl");
        let ready = WindowReady::default();
        app.init_resource::<Settings>()
            .insert_resource(ready.clone())
            .add_plugins((
                ExtractResourcePlugin::<Settings>::default(),
                ExtractComponentPlugin::<RetailTone>::default(),
                ExtractComponentPlugin::<ToneToOutput>::default(),
            ))
            .add_systems(Startup, load)
            .add_systems(
                PreUpdate,
                load.after(crate::map_transition::MapTransitionSet)
                    .run_if(world_changed),
            )
            .add_systems(Update, advance)
            .add_systems(
                Update,
                choose_tone_output.after(crate::graphics_menu::PresentationMode),
            );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(ready)
                .add_systems(RenderStartup, initialize)
                .add_systems(
                    Render,
                    (upload, check_window).in_set(RenderSystems::PrepareResources),
                )
                .add_systems(Render, prune_bindings.in_set(RenderSystems::PrepareBindGroups))
                .add_render_graph_node::<ViewNodeRunner<ExposureNode>>(Core3d, ExposureLabel)
                .add_render_graph_edges(
                    Core3d,
                    (
                        Node3d::Tonemapping,
                        ExposureLabel,
                        Node3d::EndMainPassPostProcessing,
                    ),
                );
        }
    }
}

fn load(
    config: Res<crate::config::Config>,
    retail: Res<RetailScene>,
    mut settings: ResMut<Settings>,
) {
    let generation = settings.timing.y + 1.;
    *settings = Settings::default();
    settings.timing.y = generation;
    if !retail.0 {
        return;
    }
    if std::env::var_os("SKATE_FIXED_EXPOSURE").is_some_and(|v| v == "1") {
        info!("RETAIL_EXPOSURE: fixed 2.5 comparison mode");
        return;
    }
    let profile = config
        .map_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|name| {
            let bytes =
                std::fs::read(config.asset_root.join("private/exposure-profiles.json")).ok()?;
            let mut profiles: std::collections::BTreeMap<String, Authored> =
                serde_json::from_slice(&bytes).ok()?;
            profiles.remove(&name.to_string_lossy().to_lowercase())
        });
    let fallback = || {
        std::fs::read(config.asset_root.join("private/exposure.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Authored>(&b).ok())
    };
    if let Some(a) = profile.or_else(fallback) {
        if [a.target_luminance, a.min, a.max, a.damping]
            .iter()
            .all(|x| x.is_finite())
            && a.min > 0.
            && a.max >= a.min
            && a.target_luminance > 0.
            && a.damping >= 0.
        {
            settings.tuning = Vec4::new(a.target_luminance, a.min, a.max, a.damping);
            info!(
                "RETAIL_EXPOSURE: authored target={} range={}..{} damping={}; portable GPU meter",
                a.target_luminance, a.min, a.max, a.damping
            );
        }
    }
}

fn advance(
    time: Res<Time>,
    mut settings: ResMut<Settings>,
) {
    settings.timing.x = time.delta_secs().clamp(0., 0.05);
}

/// While the gameplay camera draws into the primary window and `SKATE_TONE_DIRECT` isn't 0, its
/// output step, Bevy's upscaling, is skipped: the tone pass writes the window instead, and the
/// camera's UI is drawn there after it (`UiToOutput`). Only a camera whose output step replaces
/// the window's pixels, without blending, changes over; its clear goes with it and comes back.
/// `SKATE_TONE_GAMMA` goes with it too, and follows the option while the camera stays over.
fn choose_tone_output(
    mut commands: Commands,
    options: Option<Res<crate::render_options::RenderOptions>>,
    ready: Res<WindowReady>,
    mut cameras: Query<
        (Entity, &mut Camera, &RenderTarget, Has<RetailTone>, Option<&ToneToOutput>),
        With<crate::camera::GameplayCamera>,
    >,
) {
    let options = options.map_or_else(crate::render_options::RenderOptions::default, |options| {
        *options
    });
    let (enabled, gamma) = (options.tone_direct, options.tone_gamma);
    let ready = ready.0.load(Ordering::Relaxed);
    for (entity, mut camera, target, toned, direct) in &mut cameras {
        let want = enabled
            && ready
            && toned
            && matches!(target, RenderTarget::Window(WindowRef::Primary));
        match (want, direct) {
            (true, None) => {
                let CameraOutputMode::Write {
                    blend_state: None,
                    clear_color,
                } = camera.output_mode
                else {
                    continue;
                };
                camera.output_mode = CameraOutputMode::Skip;
                commands.entity(entity).insert((
                    ToneToOutput {
                        clear: clear_color,
                        gamma,
                    },
                    bevy::ui_render::UiToOutput,
                ));
                info!(
                    "[Skate] tone: the tone pass writes the window{}",
                    if gamma { ", with SKATE_TONE_GAMMA" } else { "" }
                );
            }
            (true, Some(&direct)) if direct.gamma != gamma => {
                commands.entity(entity).insert(ToneToOutput { gamma, ..direct });
                info!("[Skate] tone: SKATE_TONE_GAMMA {}", if gamma { "on" } else { "off" });
            }
            (false, Some(&ToneToOutput { clear, .. })) => {
                camera.output_mode = CameraOutputMode::Write {
                    blend_state: None,
                    clear_color: clear,
                };
                commands
                    .entity(entity)
                    .remove::<(ToneToOutput, bevy::ui_render::UiToOutput)>();
                info!("[Skate] tone: into a 16-bit texture, which upscaling copies into the window");
            }
            _ => {}
        }
    }
}

#[derive(Resource)]
struct Pipeline {
    compute_layout: BindGroupLayoutDescriptor,
    tone_layout: BindGroupLayoutDescriptor,
    compute: CachedComputePipelineId,
    tone: CachedRenderPipelineId,
    /// `tone` for the window's format, with `ToneToOutput`.
    tone_direct: CachedRenderPipelineId,
    /// `tone` for a view of the window's texture that doesn't encode sRGB, with
    /// `SKATE_TONE_GAMMA`.
    tone_gamma: CachedRenderPipelineId,
    settings: Buffer,
    state: Buffer,
    sampler: Sampler,
    // The buffers, sampler and layouts are immutable for this Pipeline's life.
    // Source views can alternate, resize or belong to different cameras. Keep a
    // bounded cache so retired targets cannot accumulate across map/size changes.
    bindings: Mutex<VecDeque<(TextureViewId, BindGroup, BindGroup)>>,
}

fn initialize(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
) {
    let compute_layout = BindGroupLayoutDescriptor::new(
        "retail exposure meter",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<[Vec4; 2]>(false),
                storage_buffer::<Vec4>(false),
            ),
        ),
    );
    let tone_layout = BindGroupLayoutDescriptor::new(
        "retail exposed tone",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                storage_buffer_read_only::<Vec4>(false),
            ),
        ),
    );
    let compute = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("retail exposure".into()),
        layout: vec![compute_layout.clone()],
        shader: assets.load(
            bevy::asset::AssetPath::from(bevy::asset::embedded_path!("retail_exposure.wgsl"))
                .with_source("embedded"),
        ),
        entry_point: Some("meter".into()),
        ..default()
    });
    let tone_shader = assets.load(
        bevy::asset::AssetPath::from(bevy::asset::embedded_path!("retail_tone.wgsl"))
            .with_source("embedded"),
    );
    let tone_pipeline = |label: &'static str, format, shader_defs: Vec<ShaderDefVal>| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(label.into()),
            layout: vec![tone_layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: tone_shader.clone(),
                shader_defs,
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    let tone = tone_pipeline("retail exposed tone", ViewTarget::TEXTURE_FORMAT_HDR, Vec::new());
    // The format Bevy views the window in when the surface offers it, as on the Switch;
    // `check_window` holds the tone pass back from any other.
    let tone_direct = tone_pipeline(
        "retail exposed tone, window",
        TextureFormat::bevy_default(),
        Vec::new(),
    );
    // The Switch's window texture, which Bevy views as `bevy_default`'s sRGB format.
    let tone_gamma = tone_pipeline(
        "retail exposed tone, window gamma",
        TextureFormat::Rgba8Unorm,
        vec!["TONE_GAMMA".into()],
    );
    let settings = device.create_buffer(&BufferDescriptor {
        label: Some("retail exposure settings"),
        size: 32,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let state = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("retail exposure state"),
        contents: &bytes([Vec4::new(2.5, 0., 0., 0.)]),
        usage: BufferUsages::STORAGE,
    });
    commands.insert_resource(Pipeline {
        compute_layout,
        tone_layout,
        compute,
        tone,
        tone_direct,
        tone_gamma,
        settings,
        state,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        bindings: default(),
    });
}

fn bytes<const N: usize>(values: [Vec4; N]) -> Vec<u8> {
    values
        .into_iter()
        .flat_map(|v| v.to_array())
        .flat_map(f32::to_le_bytes)
        .collect()
}

fn upload(settings: Res<Settings>, pipeline: Res<Pipeline>, queue: Res<RenderQueue>) {
    let mut data = [0u8; 32];
    for (chunk, value) in data.chunks_exact_mut(4).zip(
        [settings.tuning, settings.timing]
            .into_iter()
            .flat_map(|v| v.to_array()),
    ) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    queue.write_buffer(&pipeline.settings, 0, &data);
}

fn check_window(
    ready: Res<WindowReady>,
    windows: Res<ExtractedWindows>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    mut logged: Local<Option<(TextureFormat, Option<TextureFormat>)>>,
) {
    let window = windows
        .primary
        .and_then(|primary| windows.windows.get(&primary));
    let format = window.and_then(|window| window.swap_chain_texture_view_format);
    // Once for each format the window's texture comes in: whether `SKATE_TONE_GAMMA` can write it.
    if let Some(texture) = window.and_then(|window| window.swap_chain_texture_format)
        && *logged != Some((texture, format))
    {
        *logged = Some((texture, format));
        info!(
            "[Skate] tone: the window's texture is {texture:?}, viewed as {format:?}, so \
             SKATE_TONE_GAMMA {}",
            if texture == TextureFormat::Rgba8Unorm {
                "can write it through an Rgba8Unorm view"
            } else {
                "would write as it does unset"
            }
        );
    }
    ready.0.store(
        format == Some(TextureFormat::bevy_default())
            && cache.get_render_pipeline(pipeline.tone_direct).is_some()
            && cache.get_compute_pipeline(pipeline.compute).is_some(),
        Ordering::Relaxed,
    );
}

fn prune_bindings(pipeline: Res<Pipeline>, views: Query<&ViewTarget, With<RetailTone>>) {
    // Bind groups retain their textures. Release retired camera/resize targets
    // before rendering, including when there are no exposure views left.
    pipeline.bindings.lock().unwrap().retain(|(id, _, _)| {
        views.iter().any(|view| {
            *id == view.main_texture_view().id() || *id == view.main_texture_other_view().id()
        })
    });
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ExposureLabel;

#[derive(Default)]
struct ExposureNode;

impl ViewNode for ExposureNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static RetailTone,
        Option<&'static ToneToOutput>,
        Option<&'static ExtractedCamera>,
    );

    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext,
        (view, _, direct, camera): QueryItem<Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let p = world.resource::<Pipeline>();
        let cache = world.resource::<PipelineCache>();
        // `SKATE_TONE_GAMMA`: a view of the window's texture in its own format, which doesn't
        // encode sRGB, made each frame as Bevy makes its sRGB one (bevy_render
        // `ExtractedWindow::set_swapchain_texture`). Only an `Rgba8Unorm` texture, the format the
        // pipeline draws in; any other is written through Bevy's view, as without it.
        let gamma_view = match (direct, cache.get_render_pipeline(p.tone_gamma)) {
            (Some(ToneToOutput { gamma: true, .. }), Some(pipeline)) => {
                let windows = world.resource::<ExtractedWindows>();
                windows
                    .primary
                    .and_then(|primary| windows.windows.get(&primary))
                    .and_then(|window| window.swap_chain_texture.as_ref())
                    .filter(|surface| surface.texture.format() == TextureFormat::Rgba8Unorm)
                    .map(|surface| {
                        let view = surface.texture.create_view(&TextureViewDescriptor {
                            label: Some("retail exposed tone, window gamma"),
                            format: Some(TextureFormat::Rgba8Unorm),
                            ..default()
                        });
                        (view, pipeline)
                    })
            }
            _ => None,
        };
        let (Some(compute), Some(tone)) = (
            cache.get_compute_pipeline(p.compute),
            match (&gamma_view, direct) {
                (Some((_, pipeline)), _) => Some(*pipeline),
                (None, Some(_)) => cache.get_render_pipeline(p.tone_direct),
                (None, None) => cache.get_render_pipeline(p.tone),
            },
        ) else {
            return Ok(());
        };
        // With `ToneToOutput` the camera's output step is skipped, so this pass is the one that
        // writes the window, from the main texture and with the clear upscaling would have used.
        let (source, attachment) = match direct {
            Some(&ToneToOutput { clear, .. }) => {
                if view.out_texture_view_format() != TextureFormat::bevy_default() {
                    return Ok(());
                }
                let clear = match clear {
                    ClearColorConfig::Default => Some(world.resource::<ClearColor>().0),
                    ClearColorConfig::Custom(color) => Some(color),
                    ClearColorConfig::None => None,
                };
                let attachment = match &gamma_view {
                    // The view doesn't encode sRGB, so the clear goes in encoded already. Bevy's
                    // attachment still decides whether to clear or load, as for its own view.
                    Some((gamma_view, _)) => {
                        let attachment = view.out_texture_color_attachment(clear.map(|color| {
                            let encoded = color.to_srgba();
                            LinearRgba::new(
                                encoded.red,
                                encoded.green,
                                encoded.blue,
                                encoded.alpha,
                            )
                        }));
                        RenderPassColorAttachment {
                            view: gamma_view,
                            ..attachment
                        }
                    }
                    None => view.out_texture_color_attachment(clear.map(Into::into)),
                };
                (view.main_texture_view(), attachment)
            }
            None => {
                let post = view.post_process_write();
                (
                    post.source,
                    RenderPassColorAttachment {
                        view: post.destination,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations::default(),
                    },
                )
            }
        };
        use bevy::render::diagnostic::RecordDiagnostics;
        let diagnostics = context.diagnostic_recorder();
        let mut bindings = p.bindings.lock().unwrap();
        let index = if let Some(index) = bindings
            .iter()
            .position(|(id, _, _)| *id == source.id())
        {
            index
        } else {
            let meter = context.render_device().create_bind_group(
                "retail meter",
                &cache.get_bind_group_layout(&p.compute_layout),
                &BindGroupEntries::sequential((
                    source,
                    &p.sampler,
                    p.settings.as_entire_binding(),
                    p.state.as_entire_binding(),
                )),
            );
            let output = context.render_device().create_bind_group(
                "retail exposed tone",
                &cache.get_bind_group_layout(&p.tone_layout),
                &BindGroupEntries::sequential((
                    source,
                    &p.sampler,
                    p.state.as_entire_binding(),
                )),
            );
            if bindings.len() == 8 {
                bindings.pop_front();
            }
            bindings.push_back((source.id(), meter, output));
            bindings.len() - 1
        };
        let (_, meter, output) = &bindings[index];
        {
            let mut pass = context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("retail exposure meter"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(compute);
            let span = diagnostics.pass_span(&mut pass, "retail_exposure_meter");
            pass.set_bind_group(0, meter, &[]);
            pass.dispatch_workgroups(1, 1, 1);
            span.end(&mut pass);
        }
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("retail exposed tone"),
            color_attachments: &[Some(attachment)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        // As upscaling: a camera with a viewport writes only its own part of the window.
        if direct.is_some()
            && let Some(viewport) = camera.and_then(|camera| camera.viewport.as_ref())
        {
            let (position, size) = (viewport.physical_position, viewport.physical_size);
            pass.set_scissor_rect(position.x, position.y, size.x, size.y);
        }
        pass.set_render_pipeline(tone);
        let span = diagnostics.pass_span(&mut pass, "retail_exposed_tone");
        pass.set_bind_group(0, output, &[]);
        pass.draw(0..3, 0..1);
        span.end(&mut pass);
        Ok(())
    }
}
