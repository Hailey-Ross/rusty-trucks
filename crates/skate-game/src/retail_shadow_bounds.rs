//! Where the baked world's shadow map can hold anything (`FrameStateData::casters`),
//! so the world fragment looks it up only there (retail_material_bindings.wgsl
//! `near_casters`), and which of the scene sun's cascades anything drawn can look
//! up, and where, so the sun draws only those, each only within the texels read
//! (`sun_receivers`).
use crate::{
    render_options::{RenderOptions, SceneSun},
    retail_character::BakedWorldShadows,
    retail_render::{FrameStateData, WorldMaterial},
    retail_sky::SkyMaterial,
};
use bevy::{
    camera::{
        primitives::Aabb,
        visibility::{CascadesVisibleEntities, VisibleEntities},
    },
    core_pipeline::prepass::DeferredPrepass,
    ecs::entity::{EntityHashMap, EntityHashSet},
    light::{
        cascade::Cascade, CascadeShadowConfig, Cascades, DirectionalLightShadowMap, TransmittedShadowReceiver,
        VolumetricLight,
    },
    math::{Affine3A, Mat3A, URect, Vec3A},
    mesh::{
        morph::MeshMorphWeights,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        VertexAttributeValues,
    },
    pbr::{DistanceFog, ScreenSpaceReflections, ShadowScissors},
    prelude::*,
    render::{render_resource::TextureFormat, sync_world::MainEntity, view::RetainedViewEntity},
};
use std::{
    any::TypeId,
    collections::{hash_map::Entry, HashMap},
    f32::consts::SQRT_2,
};

/// Metres a skinned caster may be drawn beyond its bound: float error in the
/// skin matrices and in the bound itself.
const SKIN_SLACK: f32 = 0.1;

/// Texels from a lookup to the farthest texel centre its filter reads (bevy_pbr
/// shadow_sampling.wgsl). Castano's 13 taps read centres under 3 texels away
/// along each axis, so under 3√2 ≈ 4.243 across; the hardware 2×2 under √2.
const FILTER_TEXELS: f32 = 4.25;

/// Metres on top of `FILTER_TEXELS`. Jimenez's 14 taps sample within
/// 0.015 + 1.0503·(texel − 0.0039) m of the lookup, each reading centres under
/// √2 texels from its sample: under 0.0109 m + 2.47 texels, which the two cover
/// at any texel size.
const FILTER_METRES: f32 = 0.02;

/// How far each joint of a skinned mesh carries the vertices it moves, from the
/// mesh's own data.
struct SkinReach {
    /// Each joint some vertex follows, with where the IBⱼ·p of those vertices lie: where the
    /// joint's transform starts them.
    joints: Vec<(usize, Carried)>,
    /// A row per joint above: the farthest each morph target moves one of its
    /// vertices, through IBⱼ.
    morph: Vec<f32>,
    targets: usize,
    /// The most any vertex's weights miss summing to one by.
    weight_error: f32,
    /// The morph target image read, so a change to it drops this.
    image: Option<AssetId<Image>>,
}

enum Reach {
    Bounded(SkinReach),
    Unbounded,
}

/// Where a joint's transform starts the vertices it moves, in the joint's own frame: within
/// `reach` of the joint, and in the box `min`..`max`.
#[derive(Clone, Copy, Debug)]
struct Carried {
    reach: f32,
    min: Vec3A,
    max: Vec3A,
}

impl Carried {
    /// Where these lie once each is moved up to `moved` metres.
    fn grown(self, moved: f32) -> Self {
        Carried { reach: self.reach + moved, min: self.min - moved, max: self.max + moved }
    }

    /// Where these and `other` lie.
    fn merge(self, other: Self) -> Self {
        Carried {
            reach: self.reach.max(other.reach),
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    fn is_finite(&self) -> bool {
        self.reach.is_finite() && self.min.is_finite() && self.max.is_finite()
    }
}

type ReachMap = HashMap<(AssetId<Mesh>, AssetId<SkinnedMeshInverseBindposes>), Reach>;

#[derive(Default)]
pub(crate) struct CasterCache {
    reach: ReachMap,
    seen: EntityHashSet,
    joints: EntityHashMap<Carried>,
    /// This frame's joints, placed.
    placed: Vec<(Affine3A, Carried)>,
    state: Option<&'static str>,
    changes: u32,
    /// `SKATE_SHADOW_BOUNDS_CHECK`'s checks, the least any left its farthest vertex inside the
    /// sphere by, and how many found one outside.
    checks: u32,
    least: Option<f32>,
    outside: u32,
}

/// Changes of `caster_bounds`' state logged in a launch.
const LOGGED_CHANGES: u32 = 32;

/// Frames between the logged checks of `SKATE_SHADOW_BOUNDS_CHECK`.
const CHECK_EVERY: u32 = 60;

/// What `caster_bounds`' sphere was made of, logged each time it becomes bounded, so a run
/// shows what sets its radius. Each box is a `(min, max)` pair.
#[derive(Default)]
struct BoundParts {
    rigid: Option<(Vec3A, Vec3A)>,
    /// Round the skinned casters' joints themselves.
    skeleton: Option<(Vec3A, Vec3A)>,
    /// Round the balls each joint carries its vertices in (`joint_ball`).
    balls: Option<(Vec3A, Vec3A)>,
    /// `balls` once `widen` has allowed for the weight error.
    widened: Option<(Vec3A, Vec3A)>,
    weight_error: f32,
    /// Round each joint's ball cut to its own box (`joint_box`).
    boxes: Option<(Vec3A, Vec3A)>,
    /// The joint with the largest ball, and that ball's radius.
    largest: Option<(Entity, f32)>,
    margin: f32,
    /// The radius of the sphere round the box of it all, and of `tight_sphere`'s, each with the
    /// margin, and whether the tight one was taken.
    box_radius: f32,
    tight: Option<f32>,
    tight_used: bool,
}

impl BoundParts {
    fn log(&self, casters: Vec4, names: &Query<&Name>) {
        let half =
            |part: Option<(Vec3A, Vec3A)>| part.map_or(0., |(min, max)| (max - min).length() / 2.);
        let (joint, ball) = self
            .largest
            .map_or(("none".to_owned(), 0.), |(joint, radius)| {
                let name = names
                    .get(joint)
                    .map_or_else(|_| joint.to_string(), |name| name.to_string());
                (name, radius)
            });
        let tight = self.tight.map_or("none".to_owned(), |radius| format!("{radius:.3}"));
        info!(
            "SKATE_SHADOW_BOUNDS parts: half-diagonals rigid {:.3} m, skeleton {:.3}, \
             joint balls {:.3} (largest {joint}, {ball:.3}), widened {:.3} for weight error \
             {:.2e}, joint boxes {:.3}; margin {:.3}, box radius {:.3}, tight radius {tight}, \
             radius {:.3}",
            half(self.rigid),
            half(self.skeleton),
            half(self.balls),
            half(self.widened),
            self.weight_error,
            half(self.boxes),
            self.margin,
            self.box_radius,
            casters.w,
        );
    }
}

/// Bounds what the baked world's shadow light draws, as a sphere whose radius is
/// 0 where it cannot be bounded and negative where it draws nothing.
///
/// Bevy draws into a view's map exactly the entities it lists for that view
/// (`CascadesVisibleEntities`). An unmorphed rigid one is bounded by its `Aabb`. A skinned
/// one is drawn at Σ wₖ·Gₖ·IBₖ·p′ (bevy_pbr skinning.wgsl `skin_model`), p′ being
/// p plus its morph targets' weighted moves (prepass.wgsl `morph_vertex`). Each
/// IBₖ·p′ is within the joint's reach of the origin, so within `stretch` of that
/// of the joint, Gₖ moving it; for weights summing to one the vertex lies in the
/// box round those balls, and a weight error scales that box. Only the views
/// that draw the world count. A lookup reads up to a margin from the point
/// shaded: its filter's reach, the normal bias, which Bevy scales by √2 and
/// applies in texels (bevy_pbr light.rs), and the depth bias (shadows.wgsl).
/// Under `SKATE_SHADOW_BOUNDS_TIGHT` it takes instead, where smaller, the sphere
/// `tight_sphere` fits round each joint's ball and its own box.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn caster_bounds(
    options: Res<RenderOptions>,
    lights: Query<(&DirectionalLight, &Cascades, &CascadesVisibleEntities, Has<BakedWorldShadows>)>,
    views: Query<(), With<Camera3d>>,
    casters: Query<(&GlobalTransform, &Mesh3d, Option<&Aabb>, Option<&SkinnedMesh>, Option<&MeshMorphWeights>)>,
    joints: Query<&GlobalTransform>,
    names: Query<&Name>,
    meshes: Res<Assets<Mesh>>,
    poses: Res<Assets<SkinnedMeshInverseBindposes>>,
    images: Res<Assets<Image>>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut pose_events: MessageReader<AssetEvent<SkinnedMeshInverseBindposes>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    mut cache: Local<CasterCache>,
    mut frame: ResMut<FrameStateData>,
) {
    let cache = &mut *cache;
    forget_changed(&mut cache.reach, &mut mesh_events, &mut pose_events, &mut image_events);
    let mut parts = BoundParts::default();
    let mut bound = || -> Result<Vec4, &'static str> {
        if !options.shadow_bounds {
            return Err("off");
        }
        // The world fragment takes the first light that is shadowed and lights
        // lightmapped meshes (directional flags 1 and 4), so it must be this one.
        let mut receivers = lights
            .iter()
            .filter(|(light, ..)| light.shadows_enabled && light.affects_lightmapped_mesh_diffuse);
        let (Some((light, cascades, visible, true)), None) = (receivers.next(), receivers.next()) else {
            return Err("the world's shadow light is not the marked one alone");
        };
        let mut bounds = None;
        let mut margin = None::<f32>;
        let mut weight_error = 0f32;
        cache.seen.clear();
        cache.joints.clear();
        for (view, lists) in &visible.entities {
            if !views.contains(*view) || lists.iter().all(|list| list.entities.is_empty()) {
                continue;
            }
            let texel = cascades
                .cascades
                .get(view)
                .and_then(|view| view.iter().map(|c| c.texel_size).reduce(f32::max))
                .ok_or("a view has no cascade")?;
            let view_margin = texel * (FILTER_TEXELS + light.shadow_normal_bias.abs() * SQRT_2)
                + light.shadow_depth_bias.abs()
                + FILTER_METRES;
            margin = Some(margin.map_or(view_margin, |m| m.max(view_margin)));
            for &entity in lists.iter().flat_map(|list| &list.entities) {
                if !cache.seen.insert(entity) {
                    continue;
                }
                let (transform, mesh, aabb, skin, morph) = casters.get(entity).map_err(|_| "a caster is not a mesh")?;
                let Some(skin) = skin else {
                    // Its `Aabb` leaves out morph targets, which Bevy applies
                    // only to an entity with weights (bevy_pbr mesh.rs
                    // `SetMeshBindGroup`).
                    if morph.is_some() {
                        return Err("a rigid caster morphs");
                    }
                    let (min, max) = world_box(transform, aabb.ok_or("a rigid caster has no Aabb")?);
                    grow(&mut bounds, min, max);
                    continue;
                };
                let reaches = &mut cache.joints;
                let merge = |joint, carried: Carried| {
                    let known = reaches.entry(joint).or_insert(carried);
                    *known = known.merge(carried);
                };
                let error = skin_joints(
                    &mut cache.reach, mesh, skin, morph, &meshes, &poses, &images, merge,
                )?;
                weight_error = weight_error.max(error);
            }
        }
        let mut skinned = None;
        let mut boxes = None;
        parts.rigid = bounds;
        cache.placed.clear();
        for (&joint, carried) in &cache.joints {
            let transform = joints.get(joint).map_err(|_| "a joint has no transform")?;
            let (min, max) = joint_ball(transform, carried.reach);
            grow(&mut skinned, min, max);
            let at = transform.translation_vec3a();
            grow(&mut parts.skeleton, at, at);
            let radius = (max.x - min.x) / 2.;
            if parts.largest.is_none_or(|(_, largest)| radius > largest) {
                parts.largest = Some((joint, radius));
            }
            let affine = transform.affine();
            let (min, max) = joint_box(&affine, carried);
            grow(&mut boxes, min, max);
            cache.placed.push((affine, *carried));
        }
        parts.balls = skinned;
        parts.boxes = boxes;
        parts.weight_error = weight_error;
        let rigid = bounds;
        let mut around = bounds;
        if let Some((min, max)) = skinned {
            let (min, max) = widen(min, max, weight_error);
            parts.widened = Some((min, max));
            grow(&mut bounds, min, max);
        }
        if let Some((min, max)) = boxes {
            let (min, max) = widen(min, max, weight_error);
            grow(&mut around, min, max);
        }
        let (Some((min, max)), Some(margin)) = (bounds, margin) else {
            return Ok(Vec4::new(0., 0., 0., -1.));
        };
        parts.margin = margin;
        let casters = Vec3::from((min + max) / 2.).extend((max - min).length() / 2. + margin);
        parts.box_radius = casters.w;
        let tight = around
            .and_then(|around| tight_sphere(around, rigid, &cache.placed, weight_error))
            .map(|(center, radius)| Vec3::from(center).extend(radius + margin));
        parts.tight = tight.map(|tight| tight.w);
        let casters = match tight {
            Some(tight) if options.shadow_bounds_tight && tight.w < casters.w => {
                parts.tight_used = true;
                tight
            }
            _ => casters,
        };
        if casters.is_finite() { Ok(casters) } else { Err("the bound is not finite") }
    };
    // Under `SKATE_SHADOW_LOOKUPS=0` the world fragment is told there are no casters, so it
    // makes no lookup at all, for measuring what they cost.
    let outcome = if options.shadow_lookups { bound() } else { Ok(Vec4::new(0., 0., 0., -1.)) };
    frame.casters = outcome.unwrap_or(Vec4::ZERO);
    // What the world fragment was told, each time it changes, for a run on the
    // Switch to show whether it skips lookups and, if not, why.
    let state = match outcome {
        _ if !options.shadow_lookups => "lookups off",
        Ok(casters) if casters.w < 0. => "no casters",
        Ok(_) if parts.tight_used => "bounded tight",
        Ok(_) => "bounded",
        Err(why) => why,
    };
    if cache.state != Some(state) {
        cache.state = Some(state);
        cache.changes += 1;
        if cache.changes <= LOGGED_CHANGES {
            let kind = if outcome.is_err() && state != "off" { "unbounded: " } else { "" };
            info!("SKATE_SHADOW_BOUNDS {kind}{state}, casters {}", frame.casters);
            if state.starts_with("bounded") {
                parts.log(frame.casters, &names);
            }
        } else if cache.changes == LOGGED_CHANGES + 1 {
            info!("SKATE_SHADOW_BOUNDS keeps changing; later changes are not logged");
        }
    }
    // How far inside the sphere, less the lookup margin, the farthest vertex the GPU draws
    // lies, each skinned caster skinned here as the GPU skins it.
    if options.shadow_bounds_check && frame.casters.w > 0. {
        let center = Vec3A::from(frame.casters.truncate());
        let inside = frame.casters.w - parts.margin;
        let (mut farthest, mut unread) = (0f32, 0);
        for &entity in &cache.seen {
            let Ok((_, mesh, _, Some(skin), morph)) = casters.get(entity) else {
                continue;
            };
            let far = meshes.get(mesh.0.id()).zip(poses.get(skin.inverse_bindposes.id()));
            let far = far.and_then(|(mesh, poses)| {
                farthest_vertex(mesh, poses, &images, skin, morph, &joints, center)
            });
            match far {
                Some(far) => farthest = farthest.max(far),
                None => unread += 1,
            }
        }
        let slack = inside - farthest;
        let out = slack.is_nan() || slack < 0. || unread > 0;
        cache.checks += 1;
        cache.least = Some(cache.least.map_or(slack, |least| least.min(slack)));
        cache.outside += u32::from(out);
        let logged = cache.checks == 1 || cache.checks % CHECK_EVERY == 0;
        if logged || (out && cache.outside <= LOGGED_CHANGES) {
            info!(
                "SKATE_SHADOW_BOUNDS check {}: the farthest vertex {farthest:.3} m from the \
                 centre, {slack:.3} m inside the radius less the margin, {inside:.3}, {unread} \
                 casters unread{}; the least inside {:.3} m, {} checks out",
                cache.checks,
                if out { " OUT" } else { "" },
                cache.least.unwrap_or(slack),
                cache.outside,
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SunState {
    Off,
    Unbounded(&'static str),
    /// A bit for each of the `of` cascades some view still draws.
    Drawn { cascades: u32, of: usize },
}

/// Whether the sun's cascades draw only within the texels read (`SKATE_SUN_SCISSOR`).
#[derive(Clone, Copy, PartialEq)]
enum ScissorState {
    Off,
    On,
    Unbounded(&'static str),
}

#[derive(Default)]
pub(crate) struct ReceiverCache {
    reach: ReachMap,
    joints: Vec<(Entity, f32)>,
    views: Vec<(Entity, u32)>,
    /// For each cascade of the view being marked, the texels its receivers read.
    texels: Vec<Option<(Vec2, Vec2)>>,
    /// Each view's cascades' texels read.
    rects: Vec<(Entity, usize, (Vec2, Vec2))>,
    state: Option<SunState>,
    changes: u32,
    scissor: Option<ScissorState>,
    scissor_changes: u32,
}

/// Empties each of the scene sun's cascades that nothing a view draws looks
/// up, so Bevy draws no casters into it (`CascadesVisibleEntities`), and keeps
/// the rest to the texels something can read (`ShadowScissors`).
///
/// A fragment looks the sun up in the first cascade whose far bound is beyond
/// its depth −z, and in the next as well over that one's last
/// `overlap_proportion` (bevy_pbr shadows.wgsl `fetch_directional_shadow`),
/// both the sun's `CascadeShadowConfig` (light.rs `prepare_lights`); so
/// cascade i serves depths from (1 − overlap)·boundᵢ₋₁ to boundᵢ. Of what a
/// view draws, the world looks up only a light that lights lightmapped meshes
/// (retail_world.wgsl), and the sky and an unlit `StandardMaterial` (pbr.wgsl)
/// no light. The rest take z from their interpolated world position, all four
/// components (pbr_functions.wgsl `apply_pbr_lighting`,
/// retail_character_common.wgsl). A rigid mesh's vertices lie in its world box
/// with w = 1. A skinned one's lie at s·(q, 1), q in the box round its joints'
/// balls (`caster_bounds`) and s the sum of their weights, within the weight
/// error of 1, which scales z by s. Single-sampled, a fragment's interpolated
/// values are a mix of its triangle's vertices'. Multisampling, fog that
/// scatters sunlight, deferred lighting and volumetric light break that or
/// take z otherwise, so they leave every cascade drawn.
///
/// Mixed the same way, a fragment lies at S·(Q, 1), Q in the box and S within
/// the error of 1, and looks up where Q plus its biases over S falls on the
/// cascade (shadows.wgsl `sample_directional_cascade`), whose filter reads round
/// that (`cascade_texels`). A receiver of transmitted shadows looks up behind
/// itself as well (pbr_functions.wgsl), so it leaves every layer whole.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn sun_receivers(
    options: Res<RenderOptions>,
    mut suns: Query<
        (
            Entity,
            &DirectionalLight,
            &CascadeShadowConfig,
            &Cascades,
            &mut CascadesVisibleEntities,
            Has<VolumetricLight>,
        ),
        With<SceneSun>,
    >,
    views: Query<
        (
            &GlobalTransform,
            &VisibleEntities,
            Option<&Msaa>,
            Option<&DistanceFog>,
            Has<DeferredPrepass>,
            Has<ScreenSpaceReflections>,
        ),
        With<Camera3d>,
    >,
    receivers: Query<(
        &GlobalTransform,
        &Mesh3d,
        Option<&Aabb>,
        Option<&SkinnedMesh>,
        Option<&MeshMorphWeights>,
        Option<&MeshMaterial3d<StandardMaterial>>,
        Has<MeshMaterial3d<WorldMaterial>>,
        Has<MeshMaterial3d<SkyMaterial>>,
        Has<TransmittedShadowReceiver>,
    )>,
    joints: Query<&GlobalTransform>,
    meshes: Res<Assets<Mesh>>,
    poses: Res<Assets<SkinnedMeshInverseBindposes>>,
    images: Res<Assets<Image>>,
    standard: Res<Assets<StandardMaterial>>,
    shadow_map: Option<Res<DirectionalLightShadowMap>>,
    mut scissors: Option<ResMut<ShadowScissors>>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut pose_events: MessageReader<AssetEvent<SkinnedMeshInverseBindposes>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    mut cache: Local<ReceiverCache>,
) {
    if let Some(scissors) = scissors.as_mut() {
        scissors.rects.clear();
    }
    let cache = &mut *cache;
    forget_changed(&mut cache.reach, &mut mesh_events, &mut pose_events, &mut image_events);
    let Ok((sun, light, config, maps, mut visible, volumetric)) = suns.single_mut() else {
        return;
    };
    if !light.shadows_enabled {
        return;
    }
    let bounds = &config.bounds;
    let overlap = config.overlap_proportion;
    let size = shadow_map.and_then(|map| u32::try_from(map.size).ok()).filter(|size| *size > 0);
    let mut scissor = match (&scissors, size) {
        (None, _) => Err("nothing takes the rectangles"),
        (_, None) => Err("the shadow map has no size"),
        _ => Ok(()),
    };
    cache.rects.clear();
    let mut depths = None::<(f32, f32)>;
    let mut mark = || -> Result<(), &'static str> {
        if bounds.len() > 32
            || !(0. ..=1.).contains(&overlap)
            || !bounds.first().is_some_and(|first| *first >= 0.)
            || !bounds.iter().all(|bound| bound.is_finite())
            || !bounds.windows(2).all(|pair| pair[0] < pair[1])
        {
            return Err("the sun's cascade bounds are not increasing ones");
        }
        if volumetric {
            return Err("the sun lights volumetric fog");
        }
        cache.views.clear();
        for (&view, lists) in &visible.entities {
            let Ok((transform, drawn, msaa, fog, deferred, reflections)) = views.get(view) else {
                continue;
            };
            if lists.len() != bounds.len() {
                return Err("a view's cascades are not the sun's");
            }
            if msaa != Some(&Msaa::Off) {
                return Err("a view is multisampled");
            }
            if fog.is_some_and(|fog| fog.directional_light_color.to_linear().alpha > 0.) {
                return Err("a view's fog scatters sunlight");
            }
            if deferred || reflections {
                return Err("a view lights deferred");
            }
            let view_from_world = transform.affine().inverse();
            let m = view_from_world.matrix3;
            let row = Vec3A::new(m.x_axis.z, m.y_axis.z, m.z_axis.z);
            let tz = view_from_world.translation.z;
            let layers = maps.cascades.get(&view).filter(|layers| layers.len() == bounds.len());
            if layers.is_none() {
                scissor = scissor.and(Err("a view's cascades are not the sun's"));
            }
            cache.texels.clear();
            cache.texels.resize(bounds.len(), None);
            let mut cascades = 0u32;
            for &entity in drawn.get(TypeId::of::<Mesh3d>()) {
                let (transform, mesh, aabb, skin, morph, material, world, sky, transmitted) =
                    receivers.get(entity).map_err(|_| "a drawn mesh has no transform")?;
                let lit = material.is_some_and(|m| standard.get(&m.0).is_none_or(|m| !m.unlit));
                let blind = (world && !light.affects_lightmapped_mesh_diffuse) || sky || material.is_some();
                if blind && !lit {
                    continue;
                }
                let (min, max, error) = match skin {
                    None => {
                        // Its `Aabb` leaves out morph targets (`caster_bounds`).
                        if morph.is_some() {
                            return Err("a rigid receiver morphs");
                        }
                        let (min, max) = world_box(transform, aabb.ok_or("a rigid receiver has no Aabb")?);
                        (min, max, 0.)
                    }
                    Some(skin) => {
                        cache.joints.clear();
                        let found = &mut cache.joints;
                        let push = |joint, carried: Carried| found.push((joint, carried.reach));
                        let error = skin_joints(
                            &mut cache.reach, mesh, skin, morph, &meshes, &poses, &images, push,
                        )?;
                        let mut hull = None;
                        for &(joint, reach) in &cache.joints {
                            let (min, max) = joint_ball(joints.get(joint).map_err(|_| "a joint has no transform")?, reach);
                            grow(&mut hull, min, max);
                        }
                        let Some((min, max)) = hull else { continue };
                        (min, max, error)
                    }
                };
                let center = (min + max) / 2.;
                let half = (max - min) / 2.;
                let depth = -(row.dot(center) + tz);
                let spread = row.abs().dot(half);
                // The GPU's rounding in z, the bounds and the interpolation.
                let slack = 0.05 + 1e-5 * (row.length() * (center.length() + half.length()) + tz.abs());
                let (near, far) = (depth - spread, depth + spread);
                let near = near - error * near.abs() - slack;
                let far = far + error * far.abs() + slack;
                if !(near.is_finite() && far.is_finite()) {
                    return Err("a receiver's depth is not finite");
                }
                let mut own = 0u32;
                for (i, &bound) in bounds.iter().enumerate() {
                    let from = if i == 0 { f32::NEG_INFINITY } else { (1. - overlap) * bounds[i - 1] };
                    if near < bound && far >= from {
                        own |= 1 << i;
                    }
                }
                cascades |= own;
                depths = Some(depths.map_or((near, far), |(low, high)| (low.min(near), high.max(far))));
                if transmitted {
                    scissor = scissor.and(Err("a receiver takes transmitted shadows"));
                }
                if let (Ok(()), Some(layers), Some(size)) = (scissor, layers, size) {
                    for (i, layer) in layers.iter().enumerate().filter(|&(i, _)| own & 1 << i != 0) {
                        let Some((low, high)) = cascade_texels(layer, size as f32, light, min, max, error) else {
                            scissor = Err("a receiver's lookups cannot be bounded");
                            break;
                        };
                        let texels = &mut cache.texels[i];
                        *texels = Some(texels.map_or((low, high), |(l, h)| (l.min(low), h.max(high))));
                    }
                }
            }
            cache.views.push((view, cascades));
            for (i, texels) in cache.texels.iter().enumerate() {
                if let Some(texels) = *texels {
                    cache.rects.push((view, i, texels));
                }
            }
        }
        Ok(())
    };
    let state = if !options.sun_receivers {
        SunState::Off
    } else if let Err(why) = mark() {
        SunState::Unbounded(why)
    } else {
        let mut all = 0;
        for (view, lists) in visible.entities.iter_mut() {
            let Some(&(_, cascades)) = cache.views.iter().find(|(drawn, _)| drawn == view) else {
                continue;
            };
            all |= cascades;
            for (i, list) in lists.iter_mut().enumerate() {
                if cascades & 1 << i == 0 {
                    list.entities.clear();
                }
            }
        }
        SunState::Drawn { cascades: all, of: bounds.len() }
    };
    // Which cascades the sun draws, each time that changes, for a run on the
    // Switch to show whether it skips them and, if not, why.
    if cache.state != Some(state) {
        cache.state = Some(state);
        cache.changes += 1;
        if cache.changes <= LOGGED_CHANGES {
            match state {
                SunState::Off => info!("SKATE_SUN_RECEIVERS off"),
                SunState::Unbounded(why) => info!("SKATE_SUN_RECEIVERS unbounded: {why}; every cascade drawn"),
                SunState::Drawn { cascades, of } => {
                    let drawn: Vec<usize> = (0..of).filter(|&i| cascades & 1 << i != 0).collect();
                    match depths {
                        Some((near, far)) => {
                            info!("SKATE_SUN_RECEIVERS drawn {drawn:?} of {of}, receivers {near:.1} to {far:.1} m deep")
                        }
                        None => info!("SKATE_SUN_RECEIVERS drawn {drawn:?} of {of}, no receivers"),
                    }
                }
            }
        } else if cache.changes == LOGGED_CHANGES + 1 {
            info!("SKATE_SUN_RECEIVERS keeps changing; later changes are not logged");
        }
    }
    let scissor = if !options.sun_scissor {
        ScissorState::Off
    } else {
        match (state, scissor) {
            (SunState::Drawn { .. }, Ok(())) => ScissorState::On,
            (SunState::Drawn { .. }, Err(why)) => ScissorState::Unbounded(why),
            (SunState::Off, _) => ScissorState::Unbounded("SKATE_SUN_RECEIVERS is off"),
            (SunState::Unbounded(_), _) => ScissorState::Unbounded("the sun's receivers are unbounded"),
        }
    };
    if let (ScissorState::On, Some(scissors), Some(size)) = (scissor, scissors.as_mut(), size) {
        scissors.size = size;
        let edge = Vec2::splat(size as f32);
        for &(view, i, (low, high)) in &cache.rects {
            // A texel more each side than the filter reads, for the GPU's rounding.
            let low = (low.floor() - 1.).clamp(Vec2::ZERO, edge);
            let high = (high.ceil() + 1.).max(low).min(edge);
            let key = cascade_key(sun, view, i);
            scissors.rects.insert(key, URect::from_corners(low.as_uvec2(), high.as_uvec2()));
        }
    }
    // Whether the sun's cascades are scissored, each time that changes.
    if cache.scissor != Some(scissor) {
        cache.scissor = Some(scissor);
        cache.scissor_changes += 1;
        if cache.scissor_changes <= LOGGED_CHANGES {
            match scissor {
                ScissorState::Off => info!("SKATE_SUN_SCISSOR off"),
                ScissorState::On => info!("SKATE_SUN_SCISSOR on"),
                ScissorState::Unbounded(why) => info!("SKATE_SUN_SCISSOR unbounded: {why}; whole layers drawn"),
            }
        } else if cache.scissor_changes == LOGGED_CHANGES + 1 {
            info!("SKATE_SUN_SCISSOR keeps changing; later changes are not logged");
        }
    }
}

/// The corners, in texels from the top left of a map `size` texels square, of
/// what of `cascade` a fragment of a receiver in the box `min`..`max` can read,
/// its vertices' weights within `error` of summing to one; `None` where that
/// can't be told.
///
/// The cascade maps world to clip space orthographically, keeping w, its texels
/// `texel_size` metres square (bevy_light cascade.rs `calculate_cascade`), and
/// a lookup's texel is (x + 1, 1 − y)/2 of clip space times the size
/// (shadows.wgsl `world_to_directional_light_local`). A fragment at S·(Q, 1)
/// looks up Q plus its biases over S: the normal bias, which Bevy scales by √2
/// (light.rs), in texels along a normal no longer than 1, and the depth bias
/// along the light (`sample_directional_cascade`). Its filter reads round that.
fn cascade_texels(
    cascade: &Cascade,
    size: f32,
    light: &DirectionalLight,
    min: Vec3A,
    max: Vec3A,
    error: f32,
) -> Option<(Vec2, Vec2)> {
    let m = cascade.clip_from_world;
    let texel = cascade.texel_size;
    if m.row(3) != Vec4::W || !(error < 1.) || !(texel > 0.) {
        return None;
    }
    let offset = (texel * light.shadow_normal_bias.abs() * SQRT_2 + light.shadow_depth_bias.abs()) / (1. - error);
    let reach = (offset + FILTER_METRES) / texel + FILTER_TEXELS;
    let (center, half) = ((min + max) / 2., (max - min) / 2.);
    let (x, y) = (m.row(0), m.row(1));
    let (xs, ys) = (Vec3A::from(x.truncate()), Vec3A::from(y.truncate()));
    let clip = Vec2::new(xs.dot(center) + x.w, ys.dot(center) + y.w);
    let spread = Vec2::new(xs.abs().dot(half), ys.abs().dot(half));
    let mid = Vec2::new(clip.x + 1., 1. - clip.y) * (size / 2.);
    let spread = spread * (size / 2.) + reach;
    let (low, high) = (mid - spread, mid + spread);
    (low.is_finite() && high.is_finite()).then_some((low, high))
}

/// What `ShadowScissors` keys the rectangle of the sun's cascade `index` for `view` by: the
/// cascade's shadow view, as Bevy keys it (bevy_pbr light.rs `prepare_lights`).
pub(crate) fn cascade_key(sun: Entity, view: Entity, index: usize) -> RetainedViewEntity {
    RetainedViewEntity::new(MainEntity::from(sun), Some(MainEntity::from(view)), index as u32)
}

fn grow(bounds: &mut Option<(Vec3A, Vec3A)>, min: Vec3A, max: Vec3A) {
    *bounds = Some(match *bounds {
        Some((low, high)) => (low.min(min), high.max(max)),
        None => (min, max),
    });
}

/// Forgets the reaches read from a mesh, bind poses or morph target image that
/// has changed since.
fn forget_changed(
    reach: &mut ReachMap,
    meshes: &mut MessageReader<AssetEvent<Mesh>>,
    poses: &mut MessageReader<AssetEvent<SkinnedMeshInverseBindposes>>,
    images: &mut MessageReader<AssetEvent<Image>>,
) {
    fn changed<A: Asset>(event: &AssetEvent<A>) -> Option<AssetId<A>> {
        match *event {
            AssetEvent::Modified { id } | AssetEvent::Removed { id } | AssetEvent::Unused { id } => Some(id),
            _ => None,
        }
    }
    for id in meshes.read().filter_map(changed) {
        reach.retain(|(mesh, _), _| *mesh != id);
    }
    for id in poses.read().filter_map(changed) {
        reach.retain(|(_, poses), _| *poses != id);
    }
    for id in images.read().filter_map(changed) {
        reach.retain(|_, reach| !matches!(reach, Reach::Bounded(r) if r.image == Some(id)));
    }
}

/// Calls `each` with every joint a skinned mesh's vertices follow and where the joint's
/// transform starts them under the mesh's morph weights; returns the most its weights miss
/// summing to one by.
#[allow(clippy::too_many_arguments)]
fn skin_joints(
    reach: &mut ReachMap,
    mesh: &Mesh3d,
    skin: &SkinnedMesh,
    morph: Option<&MeshMorphWeights>,
    meshes: &Assets<Mesh>,
    poses: &Assets<SkinnedMeshInverseBindposes>,
    images: &Assets<Image>,
    mut each: impl FnMut(Entity, Carried),
) -> Result<f32, &'static str> {
    let reach = match reach.entry((mesh.0.id(), skin.inverse_bindposes.id())) {
        Entry::Occupied(known) => known.into_mut(),
        Entry::Vacant(new) => {
            let mesh = meshes.get(new.key().0).ok_or("a skinned mesh is loading")?;
            let poses = poses.get(new.key().1).ok_or("a skin's bind poses are loading")?;
            new.insert(skin_reach(mesh, poses, images).ok_or("a morph target image is loading")?)
        }
    };
    let Reach::Bounded(reach) = &*reach else {
        return Err("a skinned mesh's data cannot be bounded");
    };
    // Bevy pads the weights with zeros (bevy_pbr morph.rs `extract_morphs`),
    // and binds the targets only with them.
    let weights = match morph {
        Some(morph) => &morph.weights()[..reach.targets.min(morph.weights().len())],
        None if reach.targets == 0 => &[][..],
        None => return Err("a morphed skin has no weights"),
    };
    for (row, &(joint, carried)) in reach.joints.iter().enumerate() {
        let joint = *skin.joints.get(joint).ok_or("a skin lacks a joint its vertices use")?;
        let moved = reach.morph[row * reach.targets..][..reach.targets]
            .iter()
            .zip(weights)
            .map(|(far, weight)| far * weight.abs())
            .sum::<f32>();
        each(joint, carried.grown(moved));
    }
    Ok(reach.weight_error)
}

/// The world box round `aabb` placed by `transform`.
fn world_box(transform: &GlobalTransform, aabb: &Aabb) -> (Vec3A, Vec3A) {
    let affine = transform.affine();
    let m = affine.matrix3;
    let center = affine.transform_point3a(aabb.center);
    let half = m.x_axis.abs() * aabb.half_extents.x
        + m.y_axis.abs() * aabb.half_extents.y
        + m.z_axis.abs() * aabb.half_extents.z;
    (center - half, center + half)
}

/// The world box round the ball a joint at `transform` carries its vertices
/// in, given its reach.
fn joint_ball(transform: &GlobalTransform, reach: f32) -> (Vec3A, Vec3A) {
    let affine = transform.affine();
    let radius = Vec3A::splat(reach * stretch(affine.matrix3) + SKIN_SLACK);
    (affine.translation - radius, affine.translation + radius)
}

/// The world box round where a joint placed by `affine` draws what it `carried`: the box round
/// its ball (`joint_ball`) cut to the box round its own box, placed.
fn joint_box(affine: &Affine3A, carried: &Carried) -> (Vec3A, Vec3A) {
    let m = affine.matrix3;
    let radius = carried.reach * stretch(m) + SKIN_SLACK;
    let center = affine.transform_point3a((carried.min + carried.max) / 2.);
    let half = (carried.max - carried.min) / 2.;
    let half = m.x_axis.abs() * half.x + m.y_axis.abs() * half.y + m.z_axis.abs() * half.z
        + SKIN_SLACK;
    (
        (affine.translation - radius).max(center - half),
        (affine.translation + radius).min(center + half),
    )
}

/// The farthest from `center` a joint placed by `affine` draws what it `carried`: the nearer of
/// its ball's far side and its own box's farthest corner, placed, with `SKIN_SLACK`. Distance
/// is convex, so over the placed box, a parallelepiped, it is greatest at a corner.
fn joint_far(affine: &Affine3A, carried: &Carried, center: Vec3A) -> f32 {
    let ball = affine.translation.distance(center) + carried.reach * stretch(affine.matrix3);
    let corner = (0..8u32)
        .map(|i| {
            let pick = |bit: u32, low: f32, high: f32| if i & bit == 0 { low } else { high };
            let corner = Vec3A::new(
                pick(1, carried.min.x, carried.max.x),
                pick(2, carried.min.y, carried.max.y),
                pick(4, carried.min.z, carried.max.z),
            );
            affine.transform_point3a(corner).distance(center)
        })
        .fold(0., f32::max);
    ball.min(corner) + SKIN_SLACK
}

/// The farthest from `center` a point of the box `min`..`max` lies.
fn box_far(min: Vec3A, max: Vec3A, center: Vec3A) -> f32 {
    (center - min).abs().max((max - center).abs()).length()
}

/// A sphere round every rigid caster, inside the box `rigid`, and every skinned one, whose
/// `joints` are each placed by an affine and carry what their `Carried` says, centred on
/// `around`'s middle. A vertex whose weights sum to one is a weighted mean of points its
/// joints carry, so lies within the farthest of them from the centre (`joint_far`); weights
/// that miss one by up to `weight_error` scale it about the origin. `None` where something is
/// not finite.
fn tight_sphere(
    around: (Vec3A, Vec3A),
    rigid: Option<(Vec3A, Vec3A)>,
    joints: &[(Affine3A, Carried)],
    weight_error: f32,
) -> Option<(Vec3A, f32)> {
    let center = (around.0 + around.1) / 2.;
    let mut skinned = 0f32;
    for (affine, carried) in joints {
        if !(affine.is_finite() && carried.is_finite()) {
            return None;
        }
        skinned = skinned.max(joint_far(affine, carried, center));
    }
    let skinned = skinned + weight_error * (center.length() + skinned);
    let radius = rigid.map_or(0., |(min, max)| box_far(min, max, center)).max(skinned);
    (center.is_finite() && radius.is_finite()).then_some((center, radius))
}

/// The farthest from `center` the GPU draws a vertex of a skinned mesh, skinning it here as
/// bevy_pbr morph.wgsl `morph_vertex` and skinning.wgsl `skin_model` do, for
/// `SKATE_SHADOW_BOUNDS_CHECK`; `None` where its data cannot be read.
fn farthest_vertex(
    mesh: &Mesh,
    poses: &[Mat4],
    images: &Assets<Image>,
    skin: &SkinnedMesh,
    morph: Option<&MeshMorphWeights>,
    joints: &Query<&GlobalTransform>,
    center: Vec3A,
) -> Option<f32> {
    let (
        Ok(Some(positions)),
        Ok(VertexAttributeValues::Uint16x4(indices)),
        Ok(VertexAttributeValues::Float32x4(weights)),
    ) = (
        mesh.try_attribute(Mesh::ATTRIBUTE_POSITION).map(VertexAttributeValues::as_float3),
        mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_INDEX),
        mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT),
    ) else {
        return None;
    };
    // Read as `skin_reach` reads them.
    let targets = match mesh.try_has_morph_targets() {
        Ok(false) => None,
        Ok(true) => {
            let image = images.get(mesh.try_morph_targets().ok()?)?;
            if image.texture_descriptor.format != TextureFormat::R32Float {
                return None;
            }
            let size = image.texture_descriptor.size;
            let layer = (size.width * size.height) as usize;
            Some((image.data.as_deref()?, layer, size.depth_or_array_layers as usize))
        }
        Err(_) => return None,
    };
    let target_weights = morph.map_or(&[][..], |morph| morph.weights());
    let skins = skin
        .joints
        .iter()
        .zip(poses)
        .map(|(&joint, pose)| Some(Mat4::from(joints.get(joint).ok()?.affine()) * *pose))
        .collect::<Option<Vec<Mat4>>>()?;
    let mut farthest = 0f32;
    let vertices = positions.iter().zip(indices).zip(weights).enumerate();
    for (vertex, ((position, joint), weight)) in vertices {
        let mut p = Vec3::from(*position);
        if let Some((data, layer, count)) = targets {
            for (target, &w) in target_weights.iter().take(count).enumerate() {
                if w == 0. {
                    continue;
                }
                let start = (target * layer + vertex * 9) * 4;
                let read = |i: usize| -> Option<f32> {
                    let bytes = data.get(start + i * 4..)?.get(..4)?;
                    Some(f32::from_le_bytes(bytes.try_into().ok()?))
                };
                p += w * Vec3::new(read(0)?, read(1)?, read(2)?);
            }
        }
        let mut m = Mat4::ZERO;
        for (&j, &w) in joint.iter().zip(weight) {
            if w != 0. {
                m += *skins.get(j as usize)? * w;
            }
        }
        farthest = farthest.max(Vec3A::from(m.transform_point3(p)).distance(center));
    }
    Some(farthest)
}

/// The box round where weights missing one by up to `error` draw what lies in
/// `min`..`max`: s times it, |s − 1| ≤ `error`.
fn widen(min: Vec3A, max: Vec3A, error: f32) -> (Vec3A, Vec3A) {
    let far = Vec3A::splat(min.abs().max(max.abs()).length() * error);
    (min - far, max + far)
}

/// At least the most `m` lengthens a vector, its largest singular value: a
/// Gershgorin bound on the eigenvalues of mᵀm. Exact where the columns are
/// orthogonal, as for any rotation and scale.
fn stretch(m: Mat3A) -> f32 {
    let columns = [m.x_axis, m.y_axis, m.z_axis];
    columns
        .iter()
        .map(|a| columns.iter().map(|b| a.dot(*b).abs()).sum::<f32>())
        .fold(0., f32::max)
        .sqrt()
}

/// The mesh's `SkinReach` under `poses`, `Unbounded` where its data cannot be
/// read or bounded, and `None` while its morph targets are still loading.
fn skin_reach(mesh: &Mesh, poses: &[Mat4], images: &Assets<Image>) -> Option<Reach> {
    let (Ok(Some(positions)), Ok(VertexAttributeValues::Uint16x4(indices)), Ok(VertexAttributeValues::Float32x4(weights))) = (
        mesh.try_attribute(Mesh::ATTRIBUTE_POSITION).map(VertexAttributeValues::as_float3),
        mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_INDEX),
        mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT),
    ) else {
        return Some(Reach::Unbounded);
    };
    // IB·p is only a point for an affine IB, as glTF requires.
    if indices.len() != positions.len()
        || weights.len() != positions.len()
        || poses.iter().any(|pose| !pose.row(3).abs_diff_eq(Vec4::W, 1e-6))
    {
        return Some(Reach::Unbounded);
    }
    // Each target is a layer of R32Float texels, nine per vertex, position first
    // (bevy_mesh morph.rs `MorphTargetImage`).
    let morph = match mesh.try_has_morph_targets() {
        Ok(false) => None,
        Ok(true) => {
            let handle = mesh.try_morph_targets().ok()?;
            let image = images.get(handle)?;
            let size = image.texture_descriptor.size;
            let layer = (size.width * size.height) as usize;
            let targets = size.depth_or_array_layers as usize;
            match image.data.as_deref() {
                Some(data)
                    if image.texture_descriptor.format == TextureFormat::R32Float
                        && layer >= positions.len() * 9
                        && data.len() >= targets * layer * 4 =>
                {
                    Some((handle.id(), data, layer, targets))
                }
                _ => return Some(Reach::Unbounded),
            }
        }
        Err(_) => return Some(Reach::Unbounded),
    };
    let targets = morph.map_or(0, |(_, _, _, targets)| targets);
    let mut reach = vec![-1f32; poses.len()];
    let mut boxes = vec![(Vec3A::INFINITY, Vec3A::NEG_INFINITY); poses.len()];
    let mut moved = vec![0f32; poses.len() * targets];
    let mut moves = vec![0f32; targets];
    let mut weight_error = 0f32;
    for (vertex, ((position, joints), weights)) in positions.iter().zip(indices).zip(weights).enumerate() {
        // Weights summing to s draw the vertex at s times a point in its
        // joints' hull; with none it is drawn at the origin.
        let sum = weights.iter().sum::<f32>();
        if sum.is_nan() || sum <= 0. {
            return Some(Reach::Unbounded);
        }
        weight_error = weight_error.max((sum - 1.).abs());
        if let Some((_, data, layer, _)) = morph {
            for (target, distance) in moves.iter_mut().enumerate() {
                let start = (target * layer + vertex * 9) * 4;
                let component = |i: usize| f32::from_le_bytes(data[start + i * 4..][..4].try_into().unwrap());
                *distance = Vec3::new(component(0), component(1), component(2)).length();
            }
        }
        for (&joint, &weight) in joints.iter().zip(weights) {
            if weight == 0. {
                continue;
            }
            let joint = joint as usize;
            if weight.is_nan() || weight < 0. || joint >= poses.len() {
                return Some(Reach::Unbounded);
            }
            let local = poses[joint].transform_point3(Vec3::from(*position));
            let distance = local.length();
            reach[joint] = reach[joint].max(distance);
            let (low, high) = &mut boxes[joint];
            *low = low.min(Vec3A::from(local));
            *high = high.max(Vec3A::from(local));
            for (far, &distance) in moved[joint * targets..][..targets].iter_mut().zip(&moves) {
                *far = far.max(distance);
            }
        }
    }
    let mut bound = SkinReach {
        joints: Vec::new(),
        morph: Vec::new(),
        targets,
        weight_error,
        image: morph.map(|(id, ..)| id),
    };
    for (joint, &distance) in reach.iter().enumerate() {
        if distance < 0. {
            continue;
        }
        let (min, max) = boxes[joint];
        bound.joints.push((joint, Carried { reach: distance, min, max }));
        let scale = stretch(Mat3A::from_mat4(poses[joint]));
        bound.morph.extend(moved[joint * targets..][..targets].iter().map(|far| far * scale));
    }
    Some(Reach::Bounded(bound))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        camera::visibility::VisibleMeshEntities,
        ecs::system::{RunSystemOnce, SystemId},
        light::cascade::Cascade,
        math::Affine3A,
        mesh::{
            morph::{MorphAttributes, MorphTargetImage},
            PrimitiveTopology,
        },
    };

    const TEXEL: f32 = 0.05;
    const NORMAL_BIAS: f32 = 0.6;
    const DEPTH_BIAS: f32 = 0.002;

    fn margin(texel: f32) -> f32 {
        texel * (FILTER_TEXELS + NORMAL_BIAS * SQRT_2) + DEPTH_BIAS + FILTER_METRES
    }

    /// Deterministic values in [-1, 1).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 23) as f32 - 1.
        }
        fn vec3(&mut self) -> Vec3 {
            Vec3::new(self.next(), self.next(), self.next())
        }
        fn rotation(&mut self) -> Quat {
            Quat::from_euler(EulerRot::XYZ, self.next() * 3., self.next() * 3., self.next() * 3.)
        }
    }

    /// A gameplay view and the marked light, with the asset stores and the
    /// option on. Nothing is listed yet.
    fn scene() -> (World, Entity, Entity) {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<SkinnedMeshInverseBindposes>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Messages<AssetEvent<Mesh>>>();
        world.init_resource::<Messages<AssetEvent<SkinnedMeshInverseBindposes>>>();
        world.init_resource::<Messages<AssetEvent<Image>>>();
        world.insert_resource(RenderOptions { shadow_bounds: true, ..default() });
        world.insert_resource(FrameStateData::default());
        let view = world.spawn(Camera3d::default()).id();
        let light = world
            .spawn((
                DirectionalLight {
                    shadows_enabled: true,
                    shadow_normal_bias: NORMAL_BIAS,
                    shadow_depth_bias: DEPTH_BIAS,
                    ..default()
                },
                BakedWorldShadows,
            ))
            .id();
        (world, view, light)
    }

    /// Lists `entities` for `view` in one cascade of `texel`-metre texels.
    fn list(world: &mut World, light: Entity, view: Entity, texel: f32, entities: Vec<Entity>) {
        let mut light = world.entity_mut(light);
        light
            .get_mut::<Cascades>()
            .unwrap()
            .cascades
            .insert(view, vec![Cascade { texel_size: texel, ..default() }]);
        let visible = VisibleMeshEntities { entities };
        light.get_mut::<CascadesVisibleEntities>().unwrap().entities.insert(view, vec![visible]);
    }

    fn bounds(world: &mut World) -> Vec4 {
        world.run_system_once(caster_bounds).unwrap();
        world.resource::<FrameStateData>().casters
    }

    fn rigid(world: &mut World, transform: Transform, half: Vec3) -> Entity {
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::from_size(half * 2.));
        world
            .spawn((Mesh3d(mesh), GlobalTransform::from(transform), Aabb::from_min_max(-half, half)))
            .id()
    }

    fn assert_inside(bounds: Vec4, points: impl IntoIterator<Item = Vec3>, margin: f32) {
        assert!(bounds.w > margin, "no bound: {bounds}");
        for point in points {
            let distance = point.distance(bounds.truncate());
            assert!(distance <= bounds.w - margin + 1e-4, "{point} is {distance} m out, bound {bounds}");
        }
    }

    #[test]
    fn a_rigid_caster_lies_inside_with_the_lookup_margin() {
        let (mut world, view, light) = scene();
        let transform = Transform::from_xyz(30., 2., -7.)
            .with_rotation(Quat::from_euler(EulerRot::XYZ, 0.4, 1.1, -0.3))
            .with_scale(Vec3::new(1., 2.5, 0.5));
        let half = Vec3::new(0.4, 0.1, 1.2);
        let caster = rigid(&mut world, transform, half);
        list(&mut world, light, view, TEXEL, vec![caster]);
        let corners = (0..8).map(|i| {
            let sign = Vec3::new([-1., 1.][i & 1], [-1., 1.][i >> 1 & 1], [-1., 1.][i >> 2 & 1]);
            transform.transform_point(sign * half)
        });
        let bounds = bounds(&mut world);
        assert_inside(bounds, corners, margin(TEXEL));
        // Nor loose: the world box of a box is within √3 of its circumsphere.
        assert!(bounds.w - margin(TEXEL) <= 3f32.sqrt() * (transform.scale * half).length() + 1e-4);
    }

    /// Skins a figure on a three-joint spine with Bevy's arithmetic, morphed
    /// first: the vertex shader sums wₖ·Gₖ·IBₖ, applies it to p′ and drops the
    /// result's w (bevy_pbr skinning.wgsl, prepass.wgsl), so weights that miss
    /// one by a little scale the vertex about the origin.
    #[test]
    fn skinned_and_morphed_vertices_lie_inside_with_the_lookup_margin() {
        let (mut world, view, light) = scene();
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        const VERTICES: usize = 300;
        const JOINTS: usize = 3;
        let positions: Vec<Vec3> = (0..VERTICES)
            .map(|_| rng.vec3() * Vec3::new(0.3, 0.9, 0.2) + Vec3::Y * 0.9)
            .collect();
        let binds: Vec<Affine3A> = (0..JOINTS)
            .map(|j| Affine3A::from_rotation_translation(rng.rotation(), Vec3::Y * (0.3 + 0.6 * j as f32)))
            .collect();
        let inverse: Vec<Mat4> = binds.iter().map(|bind| Mat4::from(bind.inverse())).collect();
        // Posed: each joint bends (and one stretches) about its bind origin,
        // then the figure is placed in the world.
        let placed = Affine3A::from_rotation_translation(Quat::from_rotation_y(0.7), Vec3::new(12., 1., -4.));
        let globals: Vec<Affine3A> = binds
            .iter()
            .enumerate()
            .map(|(j, bind)| {
                let scale = if j == 1 { Vec3::new(1.2, 0.8, 1.) } else { Vec3::ONE };
                let bend = Quat::from_euler(EulerRot::XYZ, rng.next() * 0.6, rng.next() * 0.6, rng.next() * 0.6);
                let origin = Vec3::from(bind.translation);
                placed
                    * Affine3A::from_translation(origin)
                    * Affine3A::from_scale_rotation_translation(scale, bend, Vec3::ZERO)
                    * Affine3A::from_translation(-origin)
                    * *bind
            })
            .collect();
        // Each vertex on its nearest joint and the next, a seventh of them with
        // weights that miss one.
        let mut indices = Vec::new();
        let mut weights = Vec::new();
        for (v, position) in positions.iter().enumerate() {
            let near = ((position.y / 0.6) as usize).min(JOINTS - 1);
            let next = if near + 1 < JOINTS { near + 1 } else { near - 1 };
            let share = rng.next().abs() * 0.5;
            let error = if v % 7 == 0 { 1. + rng.next() * 1e-3 } else { 1. };
            indices.push([near as u16, next as u16, 0, 0]);
            weights.push([(1. - share) * error, share * error, 0., 0.]);
        }
        let targets: Vec<Vec<Vec3>> =
            (0..4).map(|_| (0..VERTICES).map(|_| rng.vec3() * 0.15).collect()).collect();
        let morph_weights = vec![0.5, -0.3, 0.25, 0.];
        let image = MorphTargetImage::new(
            targets.iter().map(|target| target.iter().map(|&d| MorphAttributes::new(d, Vec3::ZERO, Vec3::ZERO))),
            VERTICES,
            RenderAssetUsages::default(),
        )
        .unwrap()
        .0;
        let image = world.resource_mut::<Assets<Image>>().add(image);
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(indices.clone()))
            .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights.clone())
            .with_morph_targets(image);
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let poses = world
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(SkinnedMeshInverseBindposes::from(inverse.clone()));
        let joints: Vec<Entity> = globals.iter().map(|&g| world.spawn(GlobalTransform::from(g)).id()).collect();
        let caster = world
            .spawn((
                Mesh3d(mesh),
                GlobalTransform::from_xyz(-50., 0., 0.),
                SkinnedMesh { inverse_bindposes: poses, joints },
                MeshMorphWeights::new(morph_weights.clone()).unwrap(),
            ))
            .id();
        list(&mut world, light, view, TEXEL, vec![caster]);

        let drawn: Vec<Vec3> = (0..VERTICES)
            .map(|v| {
                let morphed = positions[v]
                    + targets.iter().zip(&morph_weights).map(|(target, w)| target[v] * *w).sum::<Vec3>();
                let skin = (0..4)
                    .map(|k| {
                        let joint = indices[v][k] as usize;
                        Mat4::from(globals[joint]) * inverse[joint] * weights[v][k]
                    })
                    .fold(Mat4::ZERO, |sum, m| sum + m);
                (skin * morphed.extend(1.)).truncate()
            })
            .collect();
        let bounds = bounds(&mut world);
        assert_inside(bounds, drawn.iter().copied(), margin(TEXEL));
        // The figure is 1.8 m tall; the joint balls overlap it by about a bone.
        let spread = drawn.iter().map(|p| p.distance(bounds.truncate())).fold(0., f32::max);
        eprintln!("skinned bound {bounds}, farthest vertex {spread} m from its centre");
        assert!(bounds.w < 4., "{bounds}");

        // `SKATE_SHADOW_BOUNDS_TIGHT`'s sphere holds them too, and is smaller.
        world.resource_mut::<RenderOptions>().shadow_bounds_tight = true;
        let tight = self::bounds(&mut world);
        assert_inside(tight, drawn.iter().copied(), margin(TEXEL));
        eprintln!("tight bound {tight}");
        assert!(tight.w < bounds.w, "{tight} against {bounds}");
    }

    #[test]
    fn nothing_listed_is_no_casters_and_an_unboundable_caster_is_no_bound() {
        let (mut world, view, light) = scene();
        list(&mut world, light, view, TEXEL, Vec::new());
        assert_eq!(bounds(&mut world), Vec4::new(0., 0., 0., -1.));

        let caster = rigid(&mut world, Transform::IDENTITY, Vec3::ONE);
        world.entity_mut(caster).remove::<Aabb>();
        list(&mut world, light, view, TEXEL, vec![caster]);
        assert_eq!(bounds(&mut world), Vec4::ZERO);

        // A morphed rigid one leaves its `Aabb`.
        let morphed = rigid(&mut world, Transform::IDENTITY, Vec3::ONE);
        world.entity_mut(morphed).insert(MeshMorphWeights::new(vec![1.]).unwrap());
        list(&mut world, light, view, TEXEL, vec![morphed]);
        assert_eq!(bounds(&mut world), Vec4::ZERO);

        // Skinned, a negative weight takes the vertex out of its joints' hull
        // and none at all puts it at the origin.
        let joints = vec![world.spawn(GlobalTransform::IDENTITY).id(), world.spawn(GlobalTransform::IDENTITY).id()];
        for weights in [[1.5f32, -0.5, 0., 0.], [0.; 4]] {
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0f32; 3]])
                .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[0, 1, 0, 0]]))
                .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![weights]);
            let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
            let poses = world
                .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
                .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY; 2]));
            let skinned = world
                .spawn((
                    Mesh3d(mesh),
                    GlobalTransform::IDENTITY,
                    SkinnedMesh { inverse_bindposes: poses, joints: joints.clone() },
                ))
                .id();
            list(&mut world, light, view, TEXEL, vec![skinned]);
            assert_eq!(bounds(&mut world), Vec4::ZERO, "{weights:?}");
        }
    }

    #[test]
    fn the_option_and_a_second_receiving_light_turn_it_off() {
        let (mut world, view, light) = scene();
        let caster = rigid(&mut world, Transform::IDENTITY, Vec3::ONE);
        list(&mut world, light, view, TEXEL, vec![caster]);
        assert!(bounds(&mut world).w > 0.);

        world.resource_mut::<RenderOptions>().shadow_bounds = false;
        assert_eq!(bounds(&mut world), Vec4::ZERO);
        world.resource_mut::<RenderOptions>().shadow_bounds = true;

        // `SKATE_SHADOW_LOOKUPS=0` tells the world there are no casters, bounded or not.
        world.resource_mut::<RenderOptions>().shadow_lookups = false;
        assert_eq!(bounds(&mut world), Vec4::new(0., 0., 0., -1.));
        world.resource_mut::<RenderOptions>().shadow_bounds = false;
        assert_eq!(bounds(&mut world), Vec4::new(0., 0., 0., -1.));
        world.resource_mut::<RenderOptions>().shadow_bounds = true;
        world.resource_mut::<RenderOptions>().shadow_lookups = true;
        assert!(bounds(&mut world).w > 0.);

        // A light that does not light lightmapped meshes is not a receiver...
        world.spawn(DirectionalLight {
            shadows_enabled: true,
            affects_lightmapped_mesh_diffuse: false,
            ..default()
        });
        assert!(bounds(&mut world).w > 0.);
        // ...but one that does might be the one the world fragment takes.
        world.spawn(DirectionalLight { shadows_enabled: true, ..default() });
        assert_eq!(bounds(&mut world), Vec4::ZERO);
    }

    #[test]
    fn only_views_of_the_world_count() {
        let (mut world, view, light) = scene();
        let caster = rigid(&mut world, Transform::IDENTITY, Vec3::ONE);
        list(&mut world, light, view, TEXEL, vec![caster]);
        let expected = bounds(&mut world);

        let overlay = world.spawn(Camera2d).id();
        let far = rigid(&mut world, Transform::from_xyz(500., 0., 0.), Vec3::ONE);
        list(&mut world, light, overlay, 100., vec![far]);
        assert_eq!(bounds(&mut world), expected);
    }

    /// A loading morph image leaves the frame unbounded without caching that,
    /// and a mesh modified afterwards is read again.
    #[test]
    fn loading_and_modified_assets_are_read_again() {
        let (mut world, view, light) = scene();
        let image = world.resource::<Assets<Image>>().reserve_handle();
        let skinned = |offset: f32| {
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[offset, 0., 0.]])
                .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[0; 4]]))
                .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1f32, 0., 0., 0.]])
                .with_morph_targets(image.clone())
        };
        let mesh = world.resource_mut::<Assets<Mesh>>().add(skinned(0.));
        let poses = world
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]));
        let joints = vec![world.spawn(GlobalTransform::IDENTITY).id()];
        let caster = world
            .spawn((
                Mesh3d(mesh.clone()),
                GlobalTransform::IDENTITY,
                SkinnedMesh { inverse_bindposes: poses, joints },
                MeshMorphWeights::new(vec![1.]).unwrap(),
            ))
            .id();
        list(&mut world, light, view, TEXEL, vec![caster]);
        let system: SystemId = world.register_system(caster_bounds);
        let run = |world: &mut World| {
            world.run_system(system).unwrap();
            world.resource::<FrameStateData>().casters
        };
        assert_eq!(run(&mut world), Vec4::ZERO);

        let moved = MorphTargetImage::new(
            std::iter::once(std::iter::once(MorphAttributes::new(Vec3::X * 2., Vec3::ZERO, Vec3::ZERO))),
            1,
            RenderAssetUsages::default(),
        )
        .unwrap()
        .0;
        world.resource_mut::<Assets<Image>>().insert(&image, moved).unwrap();
        let first = run(&mut world);
        assert_inside(first, [Vec3::X * 2.], margin(TEXEL));

        world.resource_mut::<Assets<Mesh>>().insert(&mesh, skinned(-9.)).unwrap();
        world.resource_mut::<Messages<AssetEvent<Mesh>>>().write(AssetEvent::Modified { id: mesh.id() });
        assert_inside(run(&mut world), [Vec3::X * -7.], margin(TEXEL));
    }

    /// The scene sun as the game makes it and a single-sampled view, with the
    /// asset stores. Its cascades end 10, 21.5, 46.4 and 100 m deep, each
    /// blending into the next over its last fifth: from 8, 17.2 and 37.1 m.
    fn sun_scene() -> (World, Entity, Entity) {
        let (mut world, view, _) = scene();
        world.init_resource::<Assets<StandardMaterial>>();
        world.entity_mut(view).insert(Msaa::Off);
        let sun = world
            .spawn((
                DirectionalLight { shadows_enabled: true, affects_lightmapped_mesh_diffuse: false, ..default() },
                SceneSun,
                RenderOptions::default().sun_cascade_config(),
            ))
            .id();
        (world, view, sun)
    }

    /// Lists `entities` as the meshes `view` draws.
    fn see(world: &mut World, view: Entity, entities: &[Entity]) {
        let mut visible = world.get_mut::<VisibleEntities>(view).unwrap();
        visible.entities.insert(TypeId::of::<Mesh3d>(), entities.to_vec());
    }

    /// Lists a caster in each of the sun's cascades for `view`, and returns
    /// the cascades still listed after `sun_receivers`.
    fn drawn(world: &mut World, sun: Entity, view: Entity) -> Vec<usize> {
        let lists = vec![VisibleMeshEntities { entities: vec![Entity::PLACEHOLDER] }; 4];
        world.get_mut::<CascadesVisibleEntities>(sun).unwrap().entities.insert(view, lists);
        world.run_system_once(sun_receivers).unwrap();
        let lists = world.get::<CascadesVisibleEntities>(sun).unwrap().entities.get(&view).unwrap();
        (0..lists.len()).filter(|&i| !lists[i].entities.is_empty()).collect()
    }

    /// The cascades a fragment `depth` metres deep looks up (shadows.wgsl
    /// `fetch_directional_shadow`).
    fn looked_up(config: &CascadeShadowConfig, depth: f32) -> Vec<usize> {
        let bounds = &config.bounds;
        let Some(first) = bounds.iter().position(|bound| depth < *bound) else {
            return Vec::new();
        };
        if first + 1 < bounds.len() && depth >= (1. - config.overlap_proportion) * bounds[first] {
            vec![first, first + 1]
        } else {
            vec![first]
        }
    }

    #[test]
    fn the_sun_draws_the_cascades_at_its_receivers_depths() {
        let (mut world, view, sun) = sun_scene();
        let at = |world: &mut World, depth: f32| rigid(world, Transform::from_xyz(0.3, -1., -depth), Vec3::splat(0.1));
        for (depth, cascades) in
            [(5., vec![0]), (9., vec![0, 1]), (13.6, vec![1]), (30., vec![2]), (40., vec![2, 3]), (105., vec![])]
        {
            let receiver = at(&mut world, depth);
            see(&mut world, view, &[receiver]);
            assert_eq!(drawn(&mut world, sun, view), cascades, "{depth} m deep");
        }
        let (near, far) = (at(&mut world, 5.), at(&mut world, 30.));
        see(&mut world, view, &[near, far]);
        assert_eq!(drawn(&mut world, sun, view), [0, 2]);
        see(&mut world, view, &[]);
        assert!(drawn(&mut world, sun, view).is_empty());
    }

    /// Wherever a box and the view are, each cascade a point of the box looks
    /// up is drawn.
    #[test]
    fn every_cascade_a_rigid_receiver_looks_up_is_drawn() {
        let (mut world, view, sun) = sun_scene();
        let config = world.get::<CascadeShadowConfig>(sun).unwrap().clone();
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut looks = [0; 4];
        let mut skips = 0;
        for _ in 0..300 {
            let camera = Transform::from_translation(rng.vec3() * 500.).with_rotation(rng.rotation());
            *world.get_mut::<GlobalTransform>(view).unwrap() = GlobalTransform::from(camera);
            let ahead = rng.vec3() * Vec3::new(20., 20., 60.) - Vec3::Z * 55.;
            let placed = Transform::from_translation(camera.transform_point(ahead))
                .with_rotation(rng.rotation())
                .with_scale(rng.vec3().abs() + Vec3::splat(0.5));
            let half = (rng.vec3().abs() + Vec3::splat(0.05)) * 3.;
            let receiver = rigid(&mut world, placed, half);
            see(&mut world, view, &[receiver]);
            let drawn = drawn(&mut world, sun, view);
            skips += usize::from(drawn.len() < 4);
            let view_from_world = camera.compute_affine().inverse();
            for i in 0..64 {
                let corner = Vec3::new([-1., 1.][i & 1], [-1., 1.][i >> 1 & 1], [-1., 1.][i >> 2 & 1]);
                let inside = if i < 8 { corner } else { rng.vec3() };
                let depth = -view_from_world.transform_point3(placed.transform_point(inside * half)).z;
                for cascade in looked_up(&config, depth) {
                    assert!(drawn.contains(&cascade), "cascade {cascade} at {depth} m, drawn {drawn:?}");
                    looks[cascade] += 1;
                }
            }
            world.despawn(receiver);
        }
        eprintln!("lookups per cascade {looks:?}, {skips} of 300 skipped some");
        assert!(looks.iter().all(|&n| n > 0) && skips > 0, "{looks:?}, {skips}");
    }

    /// Two vertices half a metre either side of their one joint along z, the
    /// second weighted `weight`, the mesh itself placed far off.
    fn skinned(world: &mut World, joint: Entity, weight: f32) -> Entity {
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0f32, 0., 0.5], [0., 0., -0.5]])
            .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[0; 4]; 2]))
            .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1f32, 0., 0., 0.], [weight, 0., 0., 0.]]);
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let poses = world
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]));
        world
            .spawn((
                Mesh3d(mesh),
                GlobalTransform::from_xyz(0., 0., -50.),
                SkinnedMesh { inverse_bindposes: poses, joints: vec![joint] },
            ))
            .id()
    }

    /// A skinned receiver is where its joints are. Weights that miss one scale
    /// its drawn 4-vector position, and the depth taken from it.
    #[test]
    fn a_skinned_receiver_is_where_its_joints_carry_it() {
        let (mut world, view, sun) = sun_scene();
        let config = world.get::<CascadeShadowConfig>(sun).unwrap().clone();
        let joint = world.spawn(GlobalTransform::from_xyz(0., 0., -5.)).id();
        let even = skinned(&mut world, joint, 1.);
        see(&mut world, view, &[even]);
        assert_eq!(drawn(&mut world, sun, view), [0]);
        *world.get_mut::<GlobalTransform>(joint).unwrap() = GlobalTransform::from_xyz(0., 0., -30.);
        assert_eq!(drawn(&mut world, sun, view), [2]);

        *world.get_mut::<GlobalTransform>(joint).unwrap() = GlobalTransform::from_xyz(0., 0., -16.4);
        assert_eq!(drawn(&mut world, sun, view), [1]);
        // Its second vertex is drawn at 1.05·(0, 0, −16.9, 1), so looked up
        // 17.745 m deep, past where the second cascade blends into the third.
        let heavy = skinned(&mut world, joint, 1.05);
        see(&mut world, view, &[heavy]);
        assert_eq!(looked_up(&config, 1.05 * 16.9), [1, 2]);
        assert_eq!(drawn(&mut world, sun, view), [1, 2]);
    }

    #[test]
    fn the_world_the_sky_and_unlit_meshes_look_up_no_cascade() {
        let (mut world, view, sun) = sun_scene();
        let at_60 = |world: &mut World| rigid(world, Transform::from_xyz(0., 0., -60.), Vec3::ONE);
        let land = at_60(&mut world);
        world.entity_mut(land).insert(MeshMaterial3d::<WorldMaterial>(Handle::default()));
        let sky = at_60(&mut world);
        world.entity_mut(sky).insert(MeshMaterial3d::<SkyMaterial>(Handle::default()));
        let unlit = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { unlit: true, ..default() });
        let glow = at_60(&mut world);
        world.entity_mut(glow).insert(MeshMaterial3d(unlit));
        see(&mut world, view, &[land, sky, glow]);
        assert!(drawn(&mut world, sun, view).is_empty());

        let lit = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let prop = at_60(&mut world);
        world.entity_mut(prop).insert(MeshMaterial3d(lit));
        see(&mut world, view, &[land, sky, glow, prop]);
        assert_eq!(drawn(&mut world, sun, view), [3]);

        // A sun that lit lightmapped meshes would be the one the world takes.
        world.get_mut::<DirectionalLight>(sun).unwrap().affects_lightmapped_mesh_diffuse = true;
        see(&mut world, view, &[land]);
        assert_eq!(drawn(&mut world, sun, view), [3]);
    }

    #[test]
    fn what_cannot_be_bounded_leaves_every_cascade_drawn() {
        let (mut world, view, sun) = sun_scene();
        let every = [0usize, 1, 2, 3];
        let receiver = rigid(&mut world, Transform::from_xyz(0., 0., -5.), Vec3::ONE);
        see(&mut world, view, &[receiver]);
        assert_eq!(drawn(&mut world, sun, view), [0]);

        world.entity_mut(receiver).remove::<Aabb>();
        assert_eq!(drawn(&mut world, sun, view), every);
        world
            .entity_mut(receiver)
            .insert((Aabb::from_min_max(-Vec3::ONE, Vec3::ONE), MeshMorphWeights::new(vec![1.]).unwrap()));
        assert_eq!(drawn(&mut world, sun, view), every);
        world.entity_mut(receiver).remove::<MeshMorphWeights>();
        assert_eq!(drawn(&mut world, sun, view), [0]);

        world.entity_mut(view).insert(Msaa::Sample4);
        assert_eq!(drawn(&mut world, sun, view), every);
        world.entity_mut(view).insert(Msaa::Off);
        world.entity_mut(view).insert(DistanceFog { directional_light_color: Color::WHITE, ..default() });
        assert_eq!(drawn(&mut world, sun, view), every);
        world.entity_mut(view).insert(DistanceFog::default());
        assert_eq!(drawn(&mut world, sun, view), [0]);
        world.entity_mut(view).insert(DeferredPrepass);
        assert_eq!(drawn(&mut world, sun, view), every);
        world.entity_mut(view).remove::<DeferredPrepass>();
        world.entity_mut(sun).insert(VolumetricLight);
        assert_eq!(drawn(&mut world, sun, view), every);
        world.entity_mut(sun).remove::<VolumetricLight>();
        assert_eq!(drawn(&mut world, sun, view), [0]);

        world.resource_mut::<RenderOptions>().sun_receivers = false;
        assert_eq!(drawn(&mut world, sun, view), every);
    }

    #[test]
    fn only_3d_views_lose_cascades() {
        let (mut world, view, sun) = sun_scene();
        let receiver = rigid(&mut world, Transform::from_xyz(0., 0., -5.), Vec3::ONE);
        see(&mut world, view, &[receiver]);
        let overlay = world.spawn((Camera2d, Msaa::Off)).id();
        let lists = vec![VisibleMeshEntities { entities: vec![Entity::PLACEHOLDER] }; 4];
        world.get_mut::<CascadesVisibleEntities>(sun).unwrap().entities.insert(overlay, lists);
        assert_eq!(drawn(&mut world, sun, view), [0]);
        let lists = world.get::<CascadesVisibleEntities>(sun).unwrap().entities.get(&overlay).unwrap();
        assert!(lists.iter().all(|list| !list.entities.is_empty()));
    }

    const MAP: u32 = 2048;

    /// `sun_scene` with the sun's map and somewhere to put the rectangles.
    fn scissor_scene() -> (World, Entity, Entity) {
        let (mut world, view, sun) = sun_scene();
        world.insert_resource(DirectionalLightShadowMap { size: MAP as usize });
        world.init_resource::<ShadowScissors>();
        (world, view, sun)
    }

    /// Builds the sun's cascades for `view` as Bevy does each frame.
    fn cascades(world: &mut World, sun: Entity, view: Entity) -> Vec<Cascade> {
        world.run_system_once(bevy::light::cascade::build_directional_light_cascades).unwrap();
        world.get::<Cascades>(sun).unwrap().cascades.get(&view).unwrap().clone()
    }

    /// The first and last texels along each axis that a lookup `at` texels from the top left of
    /// the map reads, its texels `metres` wide, with any of the filters: the hardware's 2×2,
    /// Castano's 13 and Jimenez's 14 turned any way (bevy_pbr shadow_sampling.wgsl). Each tap
    /// reads the 2×2 texels round it, clamped to the map's edge.
    fn read(at: Vec2, metres: f32) -> (IVec2, IVec2) {
        let mut taps = vec![at];
        let corner = (at + 0.5).floor();
        let split = |s: f32| [(3. - 2. * s) / (4. - 3. * s) - 2., (3. + s) / 7., s / (1. + 3. * s) + 2.];
        for u in split(at.x + 0.5 - corner.x) {
            for v in split(at.y + 0.5 - corner.y) {
                taps.push(corner - 0.5 + Vec2::new(u, v));
            }
        }
        let f = 0.015 + (metres - 0.00390625) * (0.035 - 0.015) / (0.022949219 - 0.00390625);
        let spiral = [
            (-0.7071f32, 0.7071f32),
            (0., -0.875),
            (0.5303, 0.5303),
            (-0.625, 0.),
            (0.3536, -0.3536),
            (0., 0.375),
            (-0.1768, -0.1768),
            (0.125, 0.),
        ];
        for (x, y) in spiral {
            // Turned any way, a tap reaches its length along each axis.
            let reach = Vec2::new(x, y).length() * f / metres;
            taps.extend([Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y].map(|axis| at + axis * reach));
        }
        let (mut first, mut last) = (IVec2::MAX, IVec2::MIN);
        for tap in taps {
            let texel = (tap - 0.5).floor().as_ivec2();
            first = first.min(texel);
            last = last.max(texel + 1);
        }
        let edge = IVec2::splat(MAP as i32 - 1);
        (first.clamp(IVec2::ZERO, edge), last.clamp(IVec2::ZERO, edge))
    }

    /// Whether a lookup at the world point `at` reads `cascade` (shadows.wgsl
    /// `world_to_directional_light_local`), asserting that `rect` then holds every texel read.
    fn reads_inside(cascade: &Cascade, at: Vec3, rect: Option<&URect>, what: &str) -> bool {
        let ndc = cascade.clip_from_world.project_point3(at);
        if ndc.x.abs() > 1. || ndc.y.abs() > 1. || !(0. ..=1.).contains(&ndc.z) {
            return false;
        }
        let (first, last) = read(Vec2::new(ndc.x + 1., 1. - ndc.y) * (MAP as f32 / 2.), cascade.texel_size);
        let rect = rect.unwrap_or_else(|| panic!("{what}: read, but no rectangle"));
        let (min, max) = (rect.min.as_ivec2(), rect.max.as_ivec2());
        assert!(
            min.cmple(first).all() && last.cmplt(max).all(),
            "{what}: reads texels {first} to {last}, rectangle {rect:?}"
        );
        true
    }

    /// Where a fragment at S·(Q, 1), its normal `normal`, looks the sun up in `cascade`
    /// (shadows.wgsl `sample_directional_cascade`).
    fn lookup(light: &DirectionalLight, to_light: Vec3, cascade: &Cascade, fragment: Vec4, normal: Vec3) -> Vec3 {
        let bias = light.shadow_normal_bias * SQRT_2 * cascade.texel_size * normal + light.shadow_depth_bias * to_light;
        (fragment.truncate() + bias) / fragment.w
    }

    /// Wherever the view, the sun and a receiver are, every texel that a lookup on the receiver
    /// reads, with its biases and any filter, lies in the rectangle its cascade draws within.
    #[test]
    fn every_texel_a_rigid_receiver_reads_is_in_its_rectangle() {
        let (mut world, view, sun) = scissor_scene();
        let config = world.get::<CascadeShadowConfig>(sun).unwrap().clone();
        let light = world.get::<DirectionalLight>(sun).unwrap().clone();
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let (mut reads, mut rects, mut small) = ([0; 4], 0, 0);
        for round in 0..300 {
            let camera = Transform::from_translation(rng.vec3() * 500.).with_rotation(rng.rotation());
            *world.get_mut::<GlobalTransform>(view).unwrap() = GlobalTransform::from(camera);
            let towards = Transform::from_rotation(rng.rotation());
            *world.get_mut::<GlobalTransform>(sun).unwrap() = GlobalTransform::from(towards);
            let layers = cascades(&mut world, sun, view);
            let ahead = rng.vec3() * Vec3::new(20., 20., 60.) - Vec3::Z * 55.;
            let placed = Transform::from_translation(camera.transform_point(ahead))
                .with_rotation(rng.rotation())
                .with_scale(rng.vec3().abs() + Vec3::splat(0.5));
            let size = if round % 2 == 0 { 0.5 } else { 3. };
            let half = (rng.vec3().abs() + Vec3::splat(0.05)) * size;
            let receiver = rigid(&mut world, placed, half);
            see(&mut world, view, &[receiver]);
            let drawn = drawn(&mut world, sun, view);
            let scissors = world.resource::<ShadowScissors>();
            assert_eq!(scissors.size, MAP);
            for i in 0..4 {
                let rect = scissors.rects.get(&cascade_key(sun, view, i));
                assert_eq!(rect.is_some(), drawn.contains(&i), "cascade {i}, drawn {drawn:?}");
                if let Some(rect) = rect {
                    assert!(rect.max.cmple(UVec2::splat(MAP)).all(), "{rect:?}");
                    rects += 1;
                    small += usize::from(rect.size().element_product() < MAP * MAP / 16);
                }
            }
            let view_from_world = camera.compute_affine().inverse();
            for i in 0..64 {
                let corner = Vec3::new([-1., 1.][i & 1], [-1., 1.][i >> 1 & 1], [-1., 1.][i >> 2 & 1]);
                let inside = if i < 8 { corner } else { rng.vec3() };
                let point = placed.transform_point(inside * half);
                let depth = -view_from_world.transform_point3(point).z;
                let normal = rng.vec3().normalize_or(Vec3::Y) * if i % 4 == 3 { rng.next().abs() } else { 1. };
                for cascade in looked_up(&config, depth) {
                    let layer = &layers[cascade];
                    let at = lookup(&light, *towards.back(), layer, point.extend(1.), normal);
                    let rect = scissors.rects.get(&cascade_key(sun, view, cascade));
                    reads[cascade] += usize::from(reads_inside(layer, at, rect, &format!("round {round} cascade {cascade}")));
                }
            }
            world.despawn(receiver);
        }
        eprintln!("reads per cascade {reads:?}; {small} of {rects} rectangles under a sixteenth of the map");
        assert!(reads.iter().all(|&n| n > 100), "{reads:?}");
        assert!(small * 2 > rects, "{small} of {rects}");
    }

    /// A skinned receiver's fragments lie at S·(Q, 1), its weights scaling w, and look up Q plus
    /// their biases over S: inside its cascade's rectangle too.
    #[test]
    fn every_texel_a_skinned_receiver_reads_is_in_its_rectangle() {
        let (mut world, view, sun) = scissor_scene();
        let config = world.get::<CascadeShadowConfig>(sun).unwrap().clone();
        let light = world.get::<DirectionalLight>(sun).unwrap().clone();
        let towards = Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.1, 0.4, 0.));
        *world.get_mut::<GlobalTransform>(sun).unwrap() = GlobalTransform::from(towards);
        let layers = cascades(&mut world, sun, view);
        let view_from_world = Mat4::from(world.get::<GlobalTransform>(view).unwrap().affine().inverse());
        let joint = world.spawn(GlobalTransform::IDENTITY).id();
        let mut rng = Rng(0x0123_4567_89ab_cdef);
        let mut reads = 0;
        for weight in [1., 1.05, 0.95] {
            let receiver = skinned(&mut world, joint, weight);
            see(&mut world, view, &[receiver]);
            for depth in [5., 9., 16.4, 30., 45.] {
                let at = Transform::from_xyz(0.3, -1., -depth).with_rotation(rng.rotation());
                *world.get_mut::<GlobalTransform>(joint).unwrap() = GlobalTransform::from(at);
                let drawn = drawn(&mut world, sun, view);
                let scissors = world.resource::<ShadowScissors>();
                // The vertices as drawn: Σ w·G·IB·(p, 1), the one joint's bind pose the identity.
                let near = at.transform_point(Vec3::Z * 0.5).extend(1.);
                let far = at.transform_point(Vec3::Z * -0.5).extend(1.) * weight;
                for k in 0..=64 {
                    let fragment = near.lerp(far, k as f32 / 64.);
                    let depth = -(view_from_world * fragment).z;
                    let normal = rng.vec3().normalize_or(Vec3::Y);
                    for cascade in looked_up(&config, depth) {
                        assert!(drawn.contains(&cascade), "{weight} at {depth} m: cascade {cascade}");
                        let layer = &layers[cascade];
                        let at = lookup(&light, *towards.back(), layer, fragment, normal);
                        let rect = scissors.rects.get(&cascade_key(sun, view, cascade));
                        reads += usize::from(reads_inside(layer, at, rect, &format!("{weight} at {depth} m")));
                    }
                }
            }
            world.despawn(receiver);
        }
        assert!(reads > 500, "{reads}");
    }

    /// The rectangles go when the option is off, the sun's receivers aren't bounded, one of them
    /// takes transmitted shadows, or the cascades or the map aren't known.
    #[test]
    fn layers_are_drawn_whole_where_the_reads_cannot_be_bounded() {
        let (mut world, view, sun) = scissor_scene();
        let receiver = rigid(&mut world, Transform::from_xyz(0.3, -1., -5.), Vec3::splat(0.5));
        see(&mut world, view, &[receiver]);
        cascades(&mut world, sun, view);
        let rects = |world: &mut World| {
            assert_eq!(drawn(world, sun, view), [0]);
            world.resource::<ShadowScissors>().rects.keys().copied().collect::<Vec<_>>()
        };
        let set = |world: &mut World, change: fn(&mut RenderOptions)| {
            change(&mut world.resource_mut::<RenderOptions>());
            rects(world)
        };
        let ours = [cascade_key(sun, view, 0)];
        assert_eq!(rects(&mut world), ours);

        assert!(set(&mut world, |options| options.sun_scissor = false).is_empty());
        assert_eq!(set(&mut world, |options| options.sun_scissor = true), ours);
        world.resource_mut::<RenderOptions>().sun_receivers = false;
        world.run_system_once(sun_receivers).unwrap();
        assert!(world.resource::<ShadowScissors>().rects.is_empty());
        assert_eq!(set(&mut world, |options| options.sun_receivers = true), ours);

        world.entity_mut(receiver).insert(TransmittedShadowReceiver);
        assert!(rects(&mut world).is_empty());
        world.entity_mut(receiver).remove::<TransmittedShadowReceiver>();
        assert_eq!(rects(&mut world), ours);

        world.get_mut::<Cascades>(sun).unwrap().cascades.clear();
        assert!(rects(&mut world).is_empty());
        cascades(&mut world, sun, view);
        assert_eq!(rects(&mut world), ours);

        world.remove_resource::<DirectionalLightShadowMap>();
        assert!(rects(&mut world).is_empty());
    }
}
