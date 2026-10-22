//! Authored `.skate` world geometry → renderable scene.
//!
//! The draw-count architecture lives here (RFC 1 §1–§6). `.skate` vertices
//! already carry a per-vertex material index, so triangles of different
//! materials can share a mesh as long as the shader indexes its material table
//! with that attribute. Geometry is therefore grouped by **space and render
//! class**, not by material, and draw count stops scaling with material count.
//!
//! Collision is a separate concern that happens to live in the same file
//! because it reads the same package. It is byte-for-byte the behaviour physics
//! was validated against and must not be "improved" here.
use bevy::{
    asset::RenderAssetUsages,
    camera::primitives::Aabb,
    mesh::{Indices, MeshVertexAttribute, PrimitiveTopology, VertexFormat},
    prelude::*,
};
use skate_core::{
    math::Vector3,
    physics::{
        board_world::{
            BoardWorld, WorldTriangle,
            query_metadata::{Bounds, QueryMesh, QueryMetadata, QueryPool},
        },
        collision::TriangleFeature,
        contact::RetailContactMaterial,
        drive_frames::RetailAffineTransform,
    },
};
use skate_data::skate_map::SkateMap;
use std::collections::HashMap;

use crate::map_render::{AssetSink, SceneCommands};
use crate::retail_render::{MaterialTable, RenderClass, WorldMaterial};

/// Per-vertex index into the world material table.
///
/// `@interpolate(flat)` in the shader. Interpolating it would silently corrupt
/// material lookup across triangle interiors, which is the one failure mode of
/// this design that does not announce itself.
pub(crate) const ATTRIBUTE_MATERIAL_INDEX: MeshVertexAttribute =
    MeshVertexAttribute::new("MaterialIndex", 0x534B_4D49, VertexFormat::Uint32);

/// Triangles per spatial leaf. The single tuning dial trading draw count against
/// culling granularity (RFC 1 §5): ~1.9M Downtown triangles at this size gives
/// ~120 leaves.
const LEAF_TRIANGLE_BUDGET: usize = 16_384;

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct SceneStats {
    pub leaves: usize,
    pub draws: usize,
    pub triangles: usize,
    pub slabs: usize,
}

// ---------------------------------------------------------------------------
// Render path
// ---------------------------------------------------------------------------

/// One triangle, reduced to what partitioning needs.
struct Triangle {
    indices: [u32; 3],
    centroid: Vec3,
    /// Slab owning this triangle's material; a leaf spanning two slabs must split.
    slab: u16,
    class: RenderClass,
}

/// Static geometry of one package, partitioned into draws; `tag` goes on
/// every draw.
#[allow(clippy::too_many_arguments)]
fn spawn_static<T: Bundle + Clone>(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
    tag: T,
) -> (SceneStats, [usize; RenderClass::ALL.len()]) {
    let mut table = MaterialTable::build(map, tuning, environment, materials, images, buffers);

    let mut triangles: Vec<Triangle> = Vec::with_capacity(map.geometry.indices.len() / 3);
    for tri in map.geometry.indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]];
        // The format guarantees all three corners share a material, and the
        // loader enforces it, so corner 0 is authoritative. `Vertex::material`
        // is one-based with zero meaning "no material", matching the collision
        // path below and `map.materials` indexing everywhere else.
        let Some(source) = (map.geometry.vertices[a as usize].material as usize).checked_sub(1)
        else {
            continue;
        };
        let Some(entry) = table.entry(source) else { continue };
        let position = |i: u32| Vec3::from_array(map.geometry.vertices[i as usize].position);
        triangles.push(Triangle {
            indices: [a, b, c],
            centroid: (position(a) + position(b) + position(c)) / 3.0,
            slab: entry.slab,
            class: entry.class,
        });
    }

    // Partition by render class first: classes cannot share a draw (RFC 1 §4),
    // so splitting here keeps every leaf single-class for free.
    let mut stats = SceneStats { slabs: table.slab_count(), triangles: triangles.len(), ..default() };
    let mut buckets: HashMap<(RenderClass, u16), Vec<usize>> = HashMap::new();
    for (index, triangle) in triangles.iter().enumerate() {
        buckets.entry((triangle.class, triangle.slab)).or_default().push(index);
    }

    // Draws per render class. The class mix is what decides whether the draw
    // budget holds on a big map: spatial subdivision is a number we choose, but
    // every two-sided or blended material forces an extra bucket we do not.
    let mut per_class = [0usize; RenderClass::ALL.len()];
    for ((class, slab), mut members) in buckets {
        let leaves = partition(&mut members, &triangles);
        stats.leaves += leaves.len();
        per_class[class as usize] += leaves.len();
        for leaf in leaves {
            let mesh = merge(map, &table, &triangles, &leaf);
            let aabb = mesh.1;
            commands.spawn((
                Name::new(format!("world {class:?} slab {slab}")),
                Mesh3d(meshes.add(mesh.0)),
                MeshMaterial3d(table.material(slab, class)),
                Transform::default(),
                // Precomputed so Bevy's CalculateBounds never walks this mesh
                // (RFC 1 D4). The extents were already computed while merging.
                aabb,
                tag.clone(),
            ));
            stats.draws += 1;
        }
    }
    (stats, per_class)
}

/// The district's global presentation model (`private/native-backdrops/<map>.skate`:
/// ocean surfaces, distant tree walls, far sea planes), drawn through the same
/// retail material path as the district. Every draw carries
/// [`crate::retail_backdrop::Backdrop`] so its visibility follows
/// `BackdropSettings` (mod-reachable). No lights, no collision.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_backdrop(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
) -> SceneStats {
    let _span = info_span!("spawn_backdrop").entered();
    let tag = crate::retail_backdrop::Backdrop;
    spawn_static(map, tuning, environment, commands, meshes, materials, images, buffers, tag).0
}

/// The unpaired far-proxy cells (`private/native-backdrops/<map>.proxy.skate`,
/// see [`crate::retail_backdrop`]): every draw carries
/// [`crate::retail_backdrop::ProxyTerrain`]. No lights, no collision.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_proxy_terrain(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
) -> SceneStats {
    let _span = info_span!("spawn_proxy_terrain").entered();
    let tag = crate::retail_backdrop::ProxyTerrain;
    spawn_static(map, tuning, environment, commands, meshes, materials, images, buffers, tag).0
}

pub(crate) fn spawn(
    map: &SkateMap,
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
) -> SceneStats {
    let _span = info_span!("spawn_world").entered();
    let (stats, per_class) =
        spawn_static(map, tuning, environment, commands, meshes, materials, images, buffers, ());
    spawn_lights(map, commands);
    eprintln!(
        "SKATE_RENDER_READY draws={} triangles={} slabs={} materials={} \
         opaque={} opaque_two_sided={} cutout={} cutout_two_sided={} \
         blended={} blended_two_sided={} blended_depth_write={} blended_depth_write_rgba={}",
        stats.draws,
        stats.triangles,
        stats.slabs,
        map.materials.len(),
        per_class[RenderClass::Opaque as usize],
        per_class[RenderClass::OpaqueTwoSided as usize],
        per_class[RenderClass::Cutout as usize],
        per_class[RenderClass::CutoutTwoSided as usize],
        per_class[RenderClass::Blended as usize],
        per_class[RenderClass::BlendedTwoSided as usize],
        per_class[RenderClass::BlendedDepthWrite as usize],
        per_class[RenderClass::BlendedDepthWriteRgba as usize],
    );
    stats
}

/// Median-split over triangle centroids until every leaf fits the budget.
///
/// Sorting by the longest axis of the current extent keeps leaves roughly cubic,
/// which matters because leaf AABBs are what frustum culling tests.
fn partition(members: &mut Vec<usize>, triangles: &[Triangle]) -> Vec<Vec<usize>> {
    let mut pending = vec![std::mem::take(members)];
    let mut leaves = Vec::new();
    while let Some(mut group) = pending.pop() {
        if group.len() <= LEAF_TRIANGLE_BUDGET {
            leaves.push(group);
            continue;
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &i in &group {
            min = min.min(triangles[i].centroid);
            max = max.max(triangles[i].centroid);
        }
        let extent = max - min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        group.sort_unstable_by(|&a, &b| {
            triangles[a].centroid[axis].total_cmp(&triangles[b].centroid[axis])
        });
        let half = group.split_off(group.len() / 2);
        pending.push(group);
        pending.push(half);
    }
    leaves
}

/// Concatenate a leaf's triangles into one mesh, preserving each vertex's
/// material index so the shader can still tell them apart.
fn merge(
    map: &SkateMap,
    table: &MaterialTable,
    triangles: &[Triangle],
    leaf: &[usize],
) -> (Mesh, Aabb) {
    let mut remap: HashMap<u32, u32> = HashMap::with_capacity(leaf.len() * 2);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(leaf.len() * 2);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(leaf.len() * 2);
    let mut uv: Vec<[f32; 2]> = Vec::with_capacity(leaf.len() * 2);
    let mut lightmap_uv: Vec<[f32; 2]> = Vec::with_capacity(leaf.len() * 2);
    let mut decal_uv: Vec<[f32; 4]> = Vec::with_capacity(leaf.len() * 2);
    let mut material_index: Vec<u32> = Vec::with_capacity(leaf.len() * 2);
    let mut tangents: Vec<[f32; 4]> = Vec::with_capacity(leaf.len() * 2);
    let mut indices: Vec<u32> = Vec::with_capacity(leaf.len() * 3);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);

    for &triangle in leaf {
        for source in triangles[triangle].indices {
            let next = positions.len() as u32;
            let index = *remap.entry(source).or_insert(next);
            if index == next {
                let v = &map.geometry.vertices[source as usize];
                let position = Vec3::from_array(v.position);
                min = min.min(position);
                max = max.max(position);
                positions.push(v.position);
                normals.push(v.normal);
                uv.push(v.uv);
                lightmap_uv.push(v.lightmap_uv);
                // Decal UVs ride in COLOR to match the retail shader, which
                // reads them from `color.xy`.
                let [du, dv] = v.decal_uv.unwrap_or_default();
                decal_uv.push([du, dv, 0.0, 1.0]);
                // One-based, as in the bucketing pass above. Triangles whose
                // material is absent were dropped there, so every vertex
                // reachable here has one.
                material_index
                    .push(table.slab_index(v.material.saturating_sub(1) as usize));
                tangents.push(tangent(v));
            }
            indices.push(index);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, lightmap_uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, decal_uv)
        .with_inserted_attribute(ATTRIBUTE_MATERIAL_INDEX, material_index)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    mesh.insert_indices(Indices::U32(indices));
    let aabb = Aabb::from_min_max(min, max);
    (mesh, aabb)
}

/// Decodes the authored tangent frame, which the format stores as a signed-byte
/// binormal plus a handedness byte, both scaled by 127.
///
/// A merged mesh spans many materials, so unlike a per-material mesh it cannot
/// choose between authored and generated tangents for the whole draw. Vertices
/// with no authored frame get a zero tangent instead, which both Bevy's
/// local-to-world helper and the shader read as "fall back to derivatives" —
/// a per-vertex decision rather than a per-mesh one.
fn tangent(v: &skate_data::skate_map::Vertex) -> [f32; 4] {
    let Some(frame) = v.tangent_frame else {
        return [0.0; 4];
    };
    let frame = frame.map(|b| (b as i8 as f32 / 127.).max(-1.));
    let binormal = Vec3::new(frame[0], frame[1], frame[2]);
    let tangent = binormal.cross(Vec3::from_array(v.normal)) * frame[3];
    [tangent.x, tangent.y, tangent.z, frame[3]]
}

fn spawn_lights(map: &SkateMap, commands: &mut SceneCommands) {
    for light in &map.lights {
        let position = Vec3::from_array(light.position);
        let color = Color::srgb(light.color[0], light.color[1], light.color[2]);
        match light.kind {
            0 => commands.spawn((
                PointLight {
                    color,
                    intensity: light.intensity,
                    range: light.range,
                    radius: light.radius,
                    // RFC 1 D5: no shadow casting anywhere in v1.
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_translation(position),
            )),
            1 => commands.spawn((
                SpotLight {
                    color,
                    intensity: light.intensity,
                    range: light.range,
                    radius: light.radius,
                    outer_angle: light.outer_cos.clamp(-1., 1.).acos(),
                    inner_angle: light.inner_cos.clamp(-1., 1.).acos(),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_translation(position)
                    .looking_to(Vec3::from_array(light.direction), Vec3::Y),
            )),
            // Area lights are parsed but have no Bevy equivalent; validation
            // already warned about them.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamic props (DMO instances)
//
// Ported from SK8-ENGINE PR #15 (laaledesiempre, phase 0) onto the current
// renderer: each MOBJ record becomes one root entity with its own transform,
// and its template geometry is merged per (render class, slab) like the
// static world, but in template space and shared between instances.
// ---------------------------------------------------------------------------

/// Marker on the root entity of one spawned dynamic-prop (DMO) instance.
/// Later phases move these entities; static batches never contain them.
#[derive(Component)]
pub(crate) struct PropInstance {
    pub id: u32,
    pub template: String,
    pub name: String,
}

/// The district's movable-prop package (`private/native-props/<map>.skate`)
/// and its MOBJ placements. The package is a presentation supplement: a
/// missing or invalid file leaves the map without props instead of failing it.
pub(crate) fn load_prop_package(
    asset_root: &std::path::Path,
    map_name: &str,
) -> Option<(SkateMap, Vec<skate_data::skate_map::StaticObject>)> {
    load_prop_package_at(&asset_root.join("private").join("native-props").join(format!("{map_name}.skate")), map_name)
}

/// The prop package and sidecar a loaded map uses: its skate.Park layout's movable pieces
/// (`park-layouts/<layout>.props.skate`, research b435: ordinary DMOs) when it carries a layout that has them, else the
/// district's own (`native-props/<map>.skate`). No park stream has district props (2026-10-21), so the layout's
/// package stands in for them; a stream with both would need the two merged.
pub(crate) fn prop_files(asset_root: &std::path::Path, map: &SkateMap) -> (std::path::PathBuf, std::path::PathBuf, String) {
    if let Some(layout) = crate::park_layout::layout_of(map) {
        let directory = crate::park_layout::directory(asset_root);
        let skate = directory.join(format!("{layout}.props.skate"));
        if skate.is_file() {
            return (skate, directory.join(format!("{layout}.props.json")), layout.to_owned());
        }
    }
    let directory = asset_root.join("private").join("native-props");
    (directory.join(format!("{}.skate", map.name)), directory.join(format!("{}.json", map.name)), map.name.clone())
}

/// [load_prop_package] for a loaded map ([prop_files]).
pub(crate) fn load_map_prop_package(asset_root: &std::path::Path, map: &SkateMap) -> Option<(SkateMap, Vec<skate_data::skate_map::StaticObject>)> {
    let (skate, _, name) = prop_files(asset_root, map);
    load_prop_package_at(&skate, &name)
}

fn load_prop_package_at(path: &std::path::Path, map_name: &str) -> Option<(SkateMap, Vec<skate_data::skate_map::StaticObject>)> {
    if !path.is_file() {
        return None;
    }
    let map = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|data| SkateMap::parse_render_only(&data))
    {
        Ok(map) => map,
        Err(error) => {
            error!("SKATE_PROPS: {}: {error}", path.display());
            return None;
        }
    };
    // Presentation only; never route lights, doors or rails from it.
    if map.name != map_name
        || !map.geometry.collision.is_empty()
        || !map.lights.is_empty()
        || !map.doors.is_empty()
        || !map.rails.is_empty()
    {
        error!("SKATE_PROPS: invalid render-only package {}", path.display());
        return None;
    }
    let mut objects = Vec::new();
    for extension in map.extensions.iter().filter(|e| e.tag == *b"MOBJ") {
        match skate_data::skate_map::parse_static_objects(&map, extension) {
            Ok(parsed) => objects.extend(parsed),
            Err(error) => {
                error!("SKATE_PROPS: {}: {error}", path.display());
                return None;
            }
        }
    }
    if objects.is_empty() {
        // Packages written before MOBJ schema 4 bake every prop into one
        // static mesh with no placements; setup groups maps/environment
        // rewrite them.
        warn!(
            "SKATE_PROPS: {} has no prop placements; re-run setup (maps) to get movable props",
            path.display()
        );
        return None;
    }
    Some((map, objects))
}

/// Row-vector affine (v @ basis + translation) as a column-vector matrix:
/// the basis rows become the matrix columns.
pub(crate) fn prop_affine(t: &[f32; 12]) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(t[0], t[1], t[2], 0.),
        Vec4::new(t[3], t[4], t[5], 0.),
        Vec4::new(t[6], t[7], t[8], 0.),
        Vec4::new(t[9], t[10], t[11], 1.),
    )
}

/// One template's render parts: a merged template-space mesh per
/// (render class, slab), with its bounds.
type PropPart = (RenderClass, u16, Handle<Mesh>, Aabb);

fn prop_template_parts(
    map: &SkateMap,
    table: &MaterialTable,
    object: &skate_data::skate_map::StaticObject,
    meshes: &mut impl AssetSink<Mesh>,
) -> Vec<PropPart> {
    let start = object.first_index as usize;
    let end = start + object.index_count as usize;
    let Some(range) = map.geometry.indices.get(start..end) else {
        return Vec::new();
    };
    let mut triangles = Vec::with_capacity(range.len() / 3);
    for tri in range.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]];
        let Some(source) = (map.geometry.vertices[a as usize].material as usize).checked_sub(1)
        else {
            continue;
        };
        let Some(entry) = table.entry(source) else { continue };
        let position = |i: u32| Vec3::from_array(map.geometry.vertices[i as usize].position);
        triangles.push(Triangle {
            indices: [a, b, c],
            centroid: (position(a) + position(b) + position(c)) / 3.0,
            slab: entry.slab,
            class: entry.class,
        });
    }
    // First-seen order keeps the spawn order deterministic.
    let mut buckets: Vec<((RenderClass, u16), Vec<usize>)> = Vec::new();
    for (index, triangle) in triangles.iter().enumerate() {
        let key = (triangle.class, triangle.slab);
        match buckets.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(index),
            None => buckets.push((key, vec![index])),
        }
    }
    buckets
        .into_iter()
        .map(|((class, slab), members)| {
            let (mesh, aabb) = merge(map, table, &triangles, &members);
            (class, slab, meshes.add(mesh), aabb)
        })
        .collect()
}

/// Spawn one root entity per MOBJ object. Template meshes are built once per
/// geometry range and shared by every instance that places it.
pub(crate) fn spawn_instances(
    map: &SkateMap,
    objects: &[skate_data::skate_map::StaticObject],
    tuning: &crate::retail_render::MaterialTuning,
    environment: &crate::retail_sky::SkyEnvironment,
    commands: &mut SceneCommands,
    meshes: &mut impl AssetSink<Mesh>,
    materials: &mut impl AssetSink<WorldMaterial>,
    images: &mut impl AssetSink<Image>,
    buffers: &mut impl AssetSink<bevy::render::storage::ShaderStorageBuffer>,
    // Some for a skate.Park layout's fixed pieces: their draw inputs by piece id (park_layout::load_draws).
    pieces: Option<&HashMap<u32, crate::retail_render::ParkDraw>>,
) -> usize {
    let _span = info_span!("spawn_prop_instances").entered();
    let mut table = MaterialTable::build(map, tuning, environment, materials, images, buffers);
    let mut templates: HashMap<(u32, u32), Vec<PropPart>> = HashMap::new();
    let mut draws = 0;
    for object in objects {
        let parts = templates
            .entry((object.first_index, object.index_count))
            .or_insert_with(|| prop_template_parts(map, &table, object, meshes));
        // The exporter prefixes the template ID to the authored locator name.
        let (template, name) = object
            .name
            .split_once('/')
            .unwrap_or(("", object.name.as_str()));
        let draw = pieces.and_then(|draws| draws.get(&object.id).copied()).unwrap_or_default();
        let children: Vec<_> = parts
            .iter()
            .map(|(class, slab, mesh, aabb)| {
                (
                    Name::new(format!("{} {class:?} slab {slab}", object.name)),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(table.tinted(*slab, *class, draw, materials)),
                    Transform::default(),
                    *aabb,
                )
            })
            .collect();
        draws += children.len();
        let root = (
            Name::new(object.name.clone()),
            Transform::from_matrix(prop_affine(&object.transform)),
            Visibility::default(),
        );
        if pieces.is_some() {
            // Fixed skate.Park pieces: never a PropInstance, so no prop system moves or picks them up.
            commands.spawn_with_children((root, crate::park_layout::ParkPiece { id: object.id, template: template.to_string() }), children);
        } else {
            commands.spawn_with_children(
                (root, PropInstance { id: object.id, template: template.to_string(), name: name.to_string() }),
                children,
            );
        }
    }
    eprintln!(
        "SKATE_PROP_INSTANCES count={} templates={} draws={draws}",
        objects.len(),
        templates.len()
    );
    objects.len()
}

// ---------------------------------------------------------------------------
// Validation and collision
//
// Everything below is carried over unchanged. Physics was validated against this
// exact behaviour, including the bit patterns and epsilons, and RFC 1 §10.1 puts
// it out of scope for the renderer rewrite.
// ---------------------------------------------------------------------------

pub(crate) fn validate_runtime(map: &SkateMap) -> Result<(), String> {
    let _span = info_span!("validate_map").entered();
    let archive = retail_archive(map)?;
    if map.geometry.collision.is_empty() && archive.is_none() {
        return Err("SKATE map has no collision geometry".into());
    }
    for triangle in map.geometry.collision.iter().filter(|_| archive.is_none()) {
        if let Some(edges) = triangle.native_edges {
            decode_native_edges(edges)?;
        }
    }
    if !map.doors.is_empty() {
        return Err(format!(
            "Map '{}' contains {} hinged doors. This imported game has no door body/controller adapter yet; refusing to drop their geometry or turn them into static walls.",
            map.name,
            map.doors.len()
        ));
    }
    for extension in &map.extensions {
        let tag = String::from_utf8_lossy(&extension.tag);
        if extension.tag == *b"RWCM" {
            continue;
        }
        if extension.tag == skate_data::trigger_volumes::EXTENSION_TAG {
            // Named trigger volumes (data only, never collision); see trigger_volumes.rs.
            if extension.schema != skate_data::trigger_volumes::EXTENSION_SCHEMA {
                return Err(format!("SKATE extension TVOL schema {} is not supported", extension.schema));
            }
            skate_data::trigger_volumes::parse(&extension.payload)?;
            continue;
        }
        if extension.tag == *b"MOBJ" {
            skate_data::skate_map::validate_static_objects(map, extension)?;
            continue;
        }
        if extension.tag == crate::park_layout::EXTENSION_TAG {
            // The in-memory skate.Park layout mark (park_layout::attach), never in a package file.
            continue;
        }
        if extension.tag == *b"SKYB" && extension.schema == 1 {
            eprintln!(
                "SKATE LIMITATION: SKYB retail sky retained; using the map horizon until its shader adapter is available."
            );
            continue;
        }
        if extension.tag != *b"WMET" && extension.tag != *b"WCFG" && extension.tag != *b"BMAT" {
            return Err(format!(
                "SKATE extension {tag} schema {} is decoded but its runtime adapter is not implemented. Refusing to silently omit potentially required world geometry.",
                extension.schema
            ));
        }
        eprintln!(
            "SKATE LIMITATION: {tag} extension retained; its runtime behavior is not connected."
        );
    }
    if map.textures.iter().any(|t| t.width == 0) {
        return Err(
            "SKATE contains external texture placeholders; supply a package with embedded textures"
                .into(),
        );
    }
    if !map.routes.is_empty() {
        eprintln!(
            "SKATE LIMITATION: {} NPC routes parsed; supplied game has no NPC controller.",
            map.routes.len()
        );
    }
    if map.lights.iter().any(|l| l.kind == 2) {
        eprintln!(
            "SKATE LIMITATION: area-light records retained; Bevy adapter currently renders point and spot lights only."
        );
    }
    eprintln!(
        "SKATE_MAP_LOADED name={:?} version={} render_triangles={} collision_triangles={} textures={} spawn={:?}",
        map.name,
        map.version,
        map.geometry.indices.len() / 3,
        map.geometry.collision.len(),
        map.textures.len(),
        map.spawn
    );
    Ok(())
}

/// TU3 ClusteredMesh::GetUnitVolumes (82AC8A68): fdivs then fsubs,
/// using the pi-squared word at 822F88D0. This is not acos/angle decoding.
/// Bit 7 denotes an unmatched compiler edge and is not a triangle flag.
fn decode_native_edges(edges: [u8; 3]) -> Result<(u32, [f32; 3]), String> {
    let mut flags = 1 | TriangleFeature::ONE_SIDED | TriangleFeature::USE_EDGE_COSINES;
    let mut cosines = [0.; 3];
    for (i, code) in edges.into_iter().enumerate() {
        let exponent = code & 0x1f;
        // The native signed 32-bit shift becomes negative at 28 and zero
        // above it. Reject those malformed codes instead of producing NaNs.
        if exponent >= 28 {
            return Err(format!(
                "Invalid SKATE native edge angle code {exponent} at corner {i}"
            ));
        }
        cosines[i] = 1.0 - f32::from_bits(0x411d_e9e7) / ((8_u32 << exponent) as f32);
        flags |= u32::from(code & 0x20) << i;
        flags |= u32::from(code & 0x40) << (i + 3);
    }
    Ok((flags, cosines))
}

fn retail_archive(map: &SkateMap) -> Result<Option<&[u8]>, String> {
    let mut archives = map.extensions.iter().filter(|e| e.tag == *b"RWCM");
    let Some(archive) = archives.next() else {
        return Ok(None);
    };
    if archive.schema != 1 || archives.next().is_some() {
        return Err("SKATE requires one RWCM extension with schema 1".into());
    }
    Ok(Some(&archive.payload))
}

fn retail_collision_world(
    archive: &[u8],
    material: RetailContactMaterial,
    pieces: Option<&crate::park_layout::Pieces>,
) -> Result<BoardWorld, String> {
    let mut triangles = Vec::new();
    let mut packed_surfaces = Vec::new();
    let mut meshes = Vec::new();
    let count = skate_data::retail_collision::visit_clusters(archive, |_, cluster| {
        // Preserve cluster order and partition further only for group filters.
        let mut cursor = 0;
        while cursor < cluster.len() {
            let group = cluster[cursor].group;
            let start = triangles.len();
            while cursor < cluster.len() && cluster[cursor].group == group {
                let source = cluster[cursor];
                let (flags, cosines) = match source.edges {
                    Some(edges) => {
                        let (mut flags, cosines) = decode_native_edges(edges)?;
                        flags &= !TriangleFeature::ONE_SIDED;
                        if source.one_sided {
                            flags |= TriangleFeature::ONE_SIDED;
                        }
                        (flags, cosines)
                    }
                    // TriangleVolume ctor82AC7770 retains these defaults when
                    // the unit has no edge data; the mesh sidedness is not read.
                    None => (0x1e1, [-1.; 3]),
                };
                triangles.push(
                    WorldTriangle::from_vertices(
                        source.points.map(|p| Vector3::new(p[0], p[1], p[2])),
                        material,
                        u32::from(source.surface),
                        flags,
                        cosines,
                        0.,
                    )
                    .ok_or("Invalid RWCM collision triangle")?,
                );
                packed_surfaces.push(source.surface);
                cursor += 1;
            }
            let range = start..triangles.len();
            let bounds = Bounds::from_points(
                triangles[range.clone()]
                    .iter()
                    .flat_map(|t| t.triangle.vertices),
            )
            .ok_or("Invalid RWCM cluster bounds")?;
            meshes.push(QueryMesh {
                geometry: 0, rejection_flags: 0,
                triangle_range: range,
                local_to_world: RetailAffineTransform::IDENTITY,
                world_to_local: RetailAffineTransform::IDENTITY,
                local_bounds: bounds,
                // Native static registration uses matchingID -1; the unit group
                // only splits clusters and must not filter actor queries.
                matching_group: -1,
                pool: QueryPool::Ground,
            });
        }
        Ok(())
    })?;
    eprintln!(
        "SKATE_RWCM_READY triangles={count} query_clusters={} source=embedded",
        meshes.len()
    );
    if let Some(pieces) = pieces {
        append_pieces(&mut triangles, &mut packed_surfaces, &mut meshes, pieces, material)?;
    }
    BoardWorld::with_query_metadata(
        triangles,
        QueryMetadata {
            packed_surfaces,
            meshes,
            static_edges: vec![],
            island_flags: 0,
        },
    )
    .map_err(str::to_owned)
}

pub(crate) fn collision_world(
    map: &SkateMap,
    material: RetailContactMaterial,
) -> Result<BoardWorld, String> {
    collision_world_with(map, material, None)
}

/// [`collision_world`] plus a skate.Park layout's placed pieces (crate::park_layout::Pieces): their posed triangles
/// after the map's own, one query range per piece (retail registers each piece in the static set, b431).
pub(crate) fn collision_world_with(
    map: &SkateMap,
    material: RetailContactMaterial,
    pieces: Option<&crate::park_layout::Pieces>,
) -> Result<BoardWorld, String> {
    let pieces = pieces.filter(|p| !p.collision.is_empty());
    if let Some(archive) = retail_archive(map)? {
        return retail_collision_world(archive, material, pieces);
    }
    let Some(pieces) = pieces else {
        return portable_world(&map.geometry.collision, &map.materials, material, None);
    };
    let (mut triangles, mut packed_surfaces, mut meshes) = portable_parts(&map.geometry.collision, &map.materials, material, None, false)?;
    append_pieces(&mut triangles, &mut packed_surfaces, &mut meshes, pieces, material)?;
    BoardWorld::with_query_metadata(triangles, QueryMetadata { packed_surfaces, meshes, static_edges: vec![], island_flags: 0 })
        .map_err(str::to_owned)
}

fn append_pieces(
    triangles: &mut Vec<WorldTriangle>,
    packed_surfaces: &mut Vec<u16>,
    meshes: &mut Vec<QueryMesh>,
    pieces: &crate::park_layout::Pieces,
    material: RetailContactMaterial,
) -> Result<(), String> {
    // Retail fuses the static pieces into one mesh and rebuilds its edge codes (b478: the SkateParkView fuserJob,
    // 828728B8 / 82876188), so flush seams between pieces pair up instead of keeping each piece's open borders.
    let (more, surfaces, piece_meshes) =
        portable_parts_fused(&pieces.collision, &pieces.materials, material, Some(&pieces.ranges), true, Some(pieces.weld))?;
    info!("SKATE_PARK_FUSE weld={} m triangles={} kept={} (collapsed and back-to-back faces dropped, seams paired, edge codes rebuilt)",
        pieces.weld, pieces.collision.len(), more.len());
    let offset = triangles.len();
    triangles.extend(more);
    packed_surfaces.extend(surfaces);
    meshes.extend(piece_meshes.into_iter().map(|mut mesh| {
        mesh.triangle_range = mesh.triangle_range.start + offset..mesh.triangle_range.end + offset;
        mesh
    }));
    Ok(())
}

/// Welded vertices and reconstructed edge features (flags, cosines) of a
/// triangle soup: the 1 mm welding and edge pairing shared by the portable map
/// world and prop instances created mid-game. `reconstruct` = false keeps the
/// default features (fully native maps decode theirs per triangle).
fn welded_edge_features(
    triangles: &[[[f32; 3]; 3]],
    reconstruct: bool,
) -> Result<(Vec<Vec3>, Vec<[usize; 3]>, Vec<u32>, Vec<[f32; 3]>), String> {
    welded_edge_features_within(triangles, reconstruct, None)
}

/// [`welded_edge_features`] with `merge` = a distance weld (a vertex joins the first earlier vertex within `merge`
/// metres, in triangle order, so the result is deterministic): the fused park pieces (b478). A triangle whose corners
/// merge together has repeated vertex ids; its edges pair with nothing and the caller drops it.
fn welded_edge_features_within(
    triangles: &[[[f32; 3]; 3]],
    reconstruct: bool,
    merge: Option<f32>,
) -> Result<(Vec<Vec3>, Vec<[usize; 3]>, Vec<u32>, Vec<[f32; 3]>), String> {
    // Match the reference RW mesh compiler's 1 mm vertex welding and reversed
    // edge pairing. Triangle diagonals are adjacency, never authored ledges.
    let mut welded = HashMap::<[i64; 3], usize>::new();
    let mut cells = HashMap::<[i64; 3], Vec<usize>>::new();
    let mut positions = Vec::<Vec3>::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    for points in triangles {
        let ids = points.map(|p| {
            if let Some(merge) = merge {
                let cell = p.map(|v| (f64::from(v) / f64::from(merge)).floor() as i64);
                let point = Vec3::from_array(p);
                let near = (-1..=1).flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| [x, y, z])))
                    .filter_map(|d| cells.get(&[cell[0] + d[0], cell[1] + d[1], cell[2] + d[2]]))
                    .flatten()
                    .copied()
                    .filter(|&id| positions[id].distance(point) <= merge)
                    .min();
                return near.unwrap_or_else(|| {
                    let id = positions.len();
                    positions.push(point);
                    cells.entry(cell).or_default().push(id);
                    id
                });
            }
            let inverse = 1.0 / f64::from(0.001_f32);
            let key = p.map(|v| (f64::from(v) * inverse).round() as i64);
            *welded.entry(key).or_insert_with(|| {
                let id = positions.len();
                positions.push(Vec3::from_array(p));
                id
            })
        });
        let [a, b, c] = points.map(Vec3::from_array);
        let normal = (b - a)
            .cross(c - a)
            .try_normalize()
            .ok_or("Invalid SKATE collision triangle normal")?;
        vertices.push(ids);
        normals.push(normal);
    }
    let mut cosines = vec![[1.; 3]; vertices.len()];
    let mut flags =
        vec![TriangleFeature::ONE_SIDED | TriangleFeature::USE_EDGE_COSINES | 0xe0; vertices.len()];
    // Fully native maps need no reconstructed adjacency. Mixed maps still
    // include every face when finding neighbors for their authored geometry.
    if reconstruct {
        let mut open = HashMap::<(usize, usize), (usize, usize)>::new();
        for (i, ids) in vertices.iter().enumerate() {
            for edge in 0..3 {
                let (a, b) = (ids[edge], ids[(edge + 1) % 3]);
                // A collapsed edge of a fused triangle (dropped by the caller); the 1 mm path keeps its old pairing.
                if a == b && merge.is_some() {
                    continue;
                }
                if let Some((other, oe)) = open.remove(&(b, a)) {
                    let cosine = normals[i].dot(normals[other]).clamp(-1., 1.);
                    let orientation =
                        (positions[b] - positions[a]).dot(normals[i].cross(normals[other]));
                    // ExtendedEdgeCosine / MakeEdgeCode in rw_collision_mesh.cpp:
                    // orientation >= -1e-6 is convex; flat edges have no convex bit.
                    for (ti, e) in [(i, edge), (other, oe)] {
                        cosines[ti][e] = cosine;
                        if orientation <= -1.0e-6 || cosine >= 1. {
                            flags[ti] &= !(0x20 << e);
                        }
                    }
                } else {
                    open.entry((a, b)).or_insert((i, edge));
                }
            }
        }
        let mut adjacent = vec![Vec::new(); positions.len()];
        for (i, ids) in vertices.iter().enumerate() {
            for &v in ids {
                adjacent[v].push(i);
            }
        }
        for (v, faces) in adjacent.iter().enumerate() {
            let reference = normals[faces[0]];
            if faces
                .iter()
                .all(|&i| (reference.dot(normals[i]) - 1.).abs() <= 0.01)
            {
                for &i in faces {
                    for corner in 0..3 {
                        if vertices[i][corner] == v {
                            flags[i] |= 0x200 << corner;
                        }
                    }
                }
            }
        }
    }
    Ok((positions, vertices, flags, cosines))
}

/// Retail's fused-mesh edge score (`82AC00A0`): d = n1.n2; a convex or flat edge (edge . (n1 x n2) > -1e-6) scores
/// max(d, -1) in [-1, 1], a concave one min(2 - d, 3) in [1, 3]. Faces folded flat onto each other score 3 (concave)
/// or -1 (a convex knife edge).
fn fused_edge_score(n1: Vec3, n2: Vec3, edge: Vec3) -> f32 {
    let d = n2.z * n1.z + n2.x * n1.x + n2.y * n1.y;
    if edge.dot(n1.cross(n2)) > -1.0e-6 { d.max(-1.0) } else { (2.0 - d).min(3.0) }
}

/// Retail's back-to-back cull thresholds (`828755B8`: 2.99 `822F90E8`, -0.99 `822F8E60`).
const FUSED_FOLD_ABOVE: f32 = 2.99;
const FUSED_FOLD_BELOW: f32 = -0.99;

/// The fused park set's edge features, as retail's fuser finalise `82876188` builds them (b478, USER-ASKS 221):
/// vertices merge within `merge` ([`welded_edge_features_within`]); triangles the merge collapses or flattens are
/// dropped (`82AC2010`); faces of DIFFERENT pieces lying back to back on each other (the side walls of two abutting
/// pieces, under their tops) are removed in pairs, or as two quads (`828755B8` with `82875438`); then each edge pairs
/// with the neighbour of the highest [`fused_edge_score`], a better pair displacing a worse one (`82875B20` /
/// `82875CA8`). `pieces[i]` = triangle i's piece. Returns the features plus `dropped[i]` (collapsed or removed).
#[allow(clippy::type_complexity)]
fn fused_edge_features(
    triangles: &[[[f32; 3]; 3]],
    merge: f32,
    pieces: &[usize],
) -> Result<(Vec<Vec3>, Vec<[usize; 3]>, Vec<u32>, Vec<[f32; 3]>, Vec<bool>), String> {
    let (positions, vertices, mut flags, mut cosines) = welded_edge_features_within(triangles, false, Some(merge))?;
    let n = vertices.len();
    let normals: Vec<Option<Vec3>> = vertices
        .iter()
        .map(|v| {
            let [a, b, c] = v.map(|id| positions[id]);
            (v[0] != v[1] && v[1] != v[2] && v[0] != v[2]).then(|| (b - a).cross(c - a).try_normalize()).flatten()
        })
        .collect();
    let mut dropped: Vec<bool> = normals.iter().map(Option::is_none).collect();
    // Retail's vertex -> triangle table, in vertex then triangle order.
    let mut users = vec![Vec::new(); positions.len()];
    for (i, v) in vertices.iter().enumerate() {
        if !dropped[i] {
            for &id in v {
                if users[id].last() != Some(&i) {
                    users[id].push(i);
                }
            }
        }
    }
    let normal = |i: usize| normals[i].unwrap_or(Vec3::ZERO);
    // 82875438: the live triangle of `t`'s piece across `t`'s edge (from, to), and its third vertex.
    let partner = |dropped: &[bool], t: usize, from: usize, to: usize| {
        users[from].iter().copied().filter(|&u| u != t && !dropped[u] && pieces[u] == pieces[t]).find_map(|u| {
            (0..3).find(|&j| vertices[u][j] == from && vertices[u][(j + 2) % 3] == to).map(|j| (u, vertices[u][(j + 1) % 3]))
        })
    };
    // 828755B8: back-to-back faces of different pieces.
    for t in 0..n {
        if dropped[t] {
            continue;
        }
        for e in 0..3 {
            let (a, b, c) = (vertices[t][e], vertices[t][(e + 1) % 3], vertices[t][(e + 2) % 3]);
            for &u in &users[a] {
                if u <= t || dropped[u] || pieces[u] == pieces[t] {
                    continue;
                }
                for k in 0..3 {
                    if vertices[u][k] != a || vertices[u][(k + 2) % 3] != b {
                        continue;
                    }
                    let score = fused_edge_score(normal(t), normal(u), positions[b] - positions[a]);
                    if !(score > FUSED_FOLD_ABOVE || score < FUSED_FOLD_BELOW) {
                        continue;
                    }
                    let x = vertices[u][(k + 1) % 3];
                    if c == x {
                        dropped[t] = true;
                        dropped[u] = true;
                    } else if let (Some((t2, t2_third)), Some((u2, u2_third))) =
                        (partner(&dropped, t, b, c), partner(&dropped, u, a, x))
                    {
                        if t2_third == x && u2_third == c {
                            for i in [t, t2, u, u2] {
                                dropped[i] = true;
                            }
                        }
                    }
                }
            }
        }
    }
    // 82875B20 / 82875CA8: best-score pairing; unpaired edges keep score 1.
    let mut neighbour = vec![[usize::MAX; 3]; n];
    let mut best = vec![[1.0f32; 3]; n];
    let unlink = |neighbour: &mut Vec<[usize; 3]>, best: &mut Vec<[f32; 3]>, x: usize, from: usize| {
        if let Some(j) = (0..3).find(|&j| neighbour[x][j] == from) {
            neighbour[x][j] = usize::MAX;
            best[x][j] = 1.0;
        }
    };
    for t in 0..n {
        if dropped[t] {
            continue;
        }
        for e in 0..3 {
            let (a, b) = (vertices[t][e], vertices[t][(e + 1) % 3]);
            for ui in 0..users[a].len() {
                let u = users[a][ui];
                if u >= t || dropped[u] {
                    continue;
                }
                let Some(ue) = (0..3).find(|&k| vertices[u][k] == b && vertices[u][(k + 1) % 3] == a) else { continue };
                let score = fused_edge_score(normal(t), normal(u), positions[b] - positions[a]);
                let (tn, un) = (neighbour[t][e], neighbour[u][ue]);
                let replace = match (tn == usize::MAX, un == usize::MAX) {
                    (true, true) => true,
                    (true, false) => best[u][ue] < score,
                    (false, true) => best[t][e] < score,
                    (false, false) => tn != u && un != t && best[t][e] < score && best[u][ue] < score,
                };
                if !replace {
                    continue;
                }
                if tn != usize::MAX {
                    unlink(&mut neighbour, &mut best, tn, t);
                }
                if un != usize::MAX {
                    unlink(&mut neighbour, &mut best, un, u);
                }
                neighbour[t][e] = u;
                neighbour[u][ue] = t;
                best[t][e] = score;
                best[u][ue] = score;
            }
        }
    }
    // Edge codes from the chosen pairs, as the map path writes them (ExtendedEdgeCosine / MakeEdgeCode).
    for t in 0..n {
        if dropped[t] {
            continue;
        }
        for e in 0..3 {
            let u = neighbour[t][e];
            if u == usize::MAX {
                continue;
            }
            let (a, b) = (vertices[t][e], vertices[t][(e + 1) % 3]);
            let cosine = normal(t).dot(normal(u)).clamp(-1., 1.);
            let orientation = (positions[b] - positions[a]).dot(normal(t).cross(normal(u)));
            cosines[t][e] = cosine;
            if orientation <= -1.0e-6 || cosine >= 1. {
                flags[t] &= !(0x20 << e);
            }
        }
    }
    for (v, faces) in users.iter().enumerate() {
        let faces: Vec<usize> = faces.iter().copied().filter(|&i| !dropped[i]).collect();
        let Some(&first) = faces.first() else { continue };
        if faces.iter().all(|&i| (normal(first).dot(normal(i)) - 1.).abs() <= 0.01) {
            for &i in &faces {
                for corner in 0..3 {
                    if vertices[i][corner] == v {
                        flags[i] |= 0x200 << corner;
                    }
                }
            }
        }
    }
    Ok((positions, vertices, flags, cosines, dropped))
}

/// Portable triangle world: 1 mm vertex welding, reconstructed adjacency and
/// contiguous-range broadphase metadata. Shared by the playable map and by
/// static prop instances, which supply already-placed world-space triangles.
fn portable_world(
    collision: &[skate_data::skate_map::Collision],
    materials: &[skate_data::skate_map::Material],
    material: RetailContactMaterial,
    // Ranges the query meshes must not cross (one prop instance each, so each has its own collision type); `None`
    // chunks the whole list.
    splits: Option<&[std::ops::Range<usize>]>,
) -> Result<BoardWorld, String> {
    let (triangles, packed_surfaces, meshes) = portable_parts(collision, materials, material, splits, false)?;
    let metadata = QueryMetadata {
        packed_surfaces,
        meshes,
        static_edges: vec![],
        island_flags: 0,
    };
    BoardWorld::with_query_metadata(triangles, metadata).map_err(str::to_owned)
}

/// [`portable_world`]'s triangles, packed surfaces and query meshes. `native_surfaces`: a triangle with native edge
/// codes packs its own surface id (as an RWCM cluster unit does) instead of its package material's; the park pieces
/// use it, the props keep their established packing.
fn portable_parts(
    collision: &[skate_data::skate_map::Collision],
    materials: &[skate_data::skate_map::Material],
    material: RetailContactMaterial,
    splits: Option<&[std::ops::Range<usize>]>,
    native_surfaces: bool,
) -> Result<(Vec<WorldTriangle>, Vec<u16>, Vec<QueryMesh>), String> {
    portable_parts_fused(collision, materials, material, splits, native_surfaces, None)
}

/// [`portable_parts`]; `fuse` = the distance (m) within which vertices merge across the whole set, with every edge
/// code rebuilt over the fused set and the native edge codes ignored (surfaces kept): retail's fused park pieces
/// (b478, `82872D70` copies only the corners, piece and surface of each triangle). Triangles that collapse are dropped
/// (retail `82AC2010`) and `splits` follow the kept triangles.
fn portable_parts_fused(
    collision: &[skate_data::skate_map::Collision],
    materials: &[skate_data::skate_map::Material],
    material: RetailContactMaterial,
    splits: Option<&[std::ops::Range<usize>]>,
    native_surfaces: bool,
    fuse: Option<f32>,
) -> Result<(Vec<WorldTriangle>, Vec<u16>, Vec<QueryMesh>), String> {
    let points: Vec<_> = collision.iter().map(|t| t.points).collect();
    let (positions, vertices, mut flags, mut cosines, kept) = if let Some(merge) = fuse {
        // The piece of each triangle: one split range per piece (append_pieces), else one piece.
        let mut pieces = vec![0; points.len()];
        for (piece, range) in splits.unwrap_or(&[]).iter().enumerate() {
            pieces[range.clone()].fill(piece);
        }
        let (positions, vertices, flags, cosines, dropped) = fused_edge_features(&points, merge, &pieces)?;
        (positions, vertices, flags, cosines, dropped.into_iter().map(|d| !d).collect::<Vec<bool>>())
    } else {
        let reconstruct = collision.iter().any(|t| t.native_edges.is_none());
        let (positions, vertices, flags, cosines) = welded_edge_features_within(&points, reconstruct, None)?;
        let kept = vec![true; vertices.len()];
        (positions, vertices, flags, cosines, kept)
    };
    let fused_splits: Option<Vec<std::ops::Range<usize>>> = (fuse.is_some()).then(|| {
        let before: Vec<usize> = std::iter::once(0).chain(kept.iter().scan(0, |n, &k| { *n += usize::from(k); Some(*n) })).collect();
        splits.map_or_else(|| vec![0..before[kept.len()]], |s| s.iter().map(|r| before[r.start]..before[r.end]).filter(|r| !r.is_empty()).collect())
    });
    let splits = fused_splits.as_deref().or(splits);
    let mut triangles = Vec::with_capacity(vertices.len());
    let mut packed_surfaces = Vec::with_capacity(vertices.len());
    for (i, source) in collision.iter().enumerate() {
        if !kept[i] {
            continue;
        }
        if let (Some(edges), None) = (source.native_edges, fuse) {
            (flags[i], cosines[i]) = decode_native_edges(edges)?;
        }
        if native_surfaces && source.native_edges.is_some() {
            packed_surfaces.push(source.surface as u16);
        } else {
            let m = &materials[source.material as usize - 1];
            // Exact EncodeRwSurfaceId mapping from the reference native adapter.
            packed_surfaces.push((m.audio | (m.physics << 7) | (m.pattern << 12)) as u16);
        }
        let points = vertices[i].map(|id| {
            let p = positions[id];
            Vector3::new(p.x, p.y, p.z)
        });
        // Keep the supplied game's original static-world contact combine values.
        // The native map bridge supplies packed surfaces, not a guessed split of
        // the package's single friction scalar into static/dynamic coefficients.
        triangles.push(
            WorldTriangle::from_vertices(
                points,
                material,
                source.surface,
                flags[i],
                cosines[i],
                0.,
            )
            .ok_or("Invalid SKATE collision volume")?,
        );
    }
    // Portable maps have no native cluster hierarchy. Bound contiguous ranges
    // once at load time so the existing BVH can reject distant geometry. Keep
    // triangle order and mesh identity/filter values: contact tie-breaking,
    // packed surfaces and adjacency must not change with this acceleration.
    let mut meshes = Vec::new();
    let whole = [0..triangles.len()];
    for (start, end) in splits.unwrap_or(&whole).iter().flat_map(|r| (r.start..r.end).step_by(64).map(move |s| (s, (s + 64).min(r.end)))) {
        let bounds = Bounds::from_points(triangles[start..end].iter().flat_map(|t| t.triangle.vertices))
            .ok_or("SKATE collision bounds empty")?;
        meshes.push(QueryMesh {
            geometry: 0, rejection_flags: 0,
            triangle_range: start..end,
            local_to_world: RetailAffineTransform::IDENTITY,
            world_to_local: RetailAffineTransform::IDENTITY,
            local_bounds: bounds,
            matching_group: -1,
            pool: QueryPool::Ground,
        });
    }
    Ok((triangles, packed_surfaces, meshes))
}

/// One prop instance's share of the prop collision world: its world triangle
/// range plus the winding-fixed template-space source triangles. Rigid-motion
/// invariance of adjacency flags and edge cosines lets `rebake` skip welding.
#[derive(Clone)]
pub(crate) struct PropCollisionInstance {
    pub id: u32,
    /// Index of the originating MOBJ record; `usize::MAX` for an instance
    /// added mid-game ([`PropCollisionLayer::add_instance`]).
    pub object: usize,
    pub range: std::ops::Range<usize>,
    local: Vec<[Vector3; 3]>,
}

impl PropCollisionInstance {
    /// Winding-fixed template-space triangles with per-axis scale folded in.
    pub fn local_points(&self) -> &[[Vector3; 3]] {
        &self.local
    }
}

/// Static collision for spawned DMO prop instances, kept out of the map
/// collision world so dynamic instances can be rebaked independently.
pub(crate) struct PropCollisionLayer {
    /// Shared with NPC skater workers ([`Self::share`]); changed only while no worker holds it.
    world: crate::physics::WorldHandle,
    /// Shared with NPC skater workers ([`Self::share`]): copying the template triangles per NPC per tick cost ~0.5 ms.
    instances: std::sync::Arc<Vec<PropCollisionInstance>>,
    contact_material: RetailContactMaterial,
    /// Instances added mid-game and retired since; their parked triangle
    /// ranges are reused by the next instance of the same size.
    retired: Vec<usize>,
    /// The collision type last written to each instance's meshes (`u32::MAX` = none yet), so an unchanged type
    /// costs no mesh scan.
    collision_types: Vec<u32>,
}

impl PropCollisionLayer {
    /// A copy for an NPC skater worker: the same (shared) prop world with its own query scratch, and the instance list.
    pub(crate) fn share(&self) -> Self {
        Self {
            world: self.world.share(),
            instances: std::sync::Arc::clone(&self.instances),
            contact_material: self.contact_material,
            retired: self.retired.clone(),
            collision_types: self.collision_types.clone(),
        }
    }

    /// The world handle for queries (its own scratch).
    pub(crate) fn world_handle_mut(&mut self) -> &mut crate::physics::WorldHandle {
        &mut self.world
    }

    pub fn world(&self) -> &BoardWorld {
        &*self.world
    }
    pub fn world_mut(&mut self) -> &mut BoardWorld {
        &mut *self.world
    }
    /// Gives an instance's query meshes the retail collision type's rejection flag (`1 << type`; DMOPool rebuild
    /// `827778C0`). Returns whether anything changed.
    pub fn set_collision_type(&mut self, instance: usize, collision_type: u32) -> bool {
        let Some(range) = self.instances.get(instance).map(|i| i.range.clone()) else { return false };
        if self.collision_types.len() < self.instances.len() {
            self.collision_types.resize(self.instances.len(), u32::MAX);
        }
        if self.collision_types[instance] == collision_type {
            return false;
        }
        self.collision_types[instance] = collision_type;
        let flags = 1u32.checked_shl(collision_type).unwrap_or(0);
        self.world_mut().set_rejection_flags(range, flags)
    }

    pub fn instances(&self) -> &[PropCollisionInstance] {
        &self.instances
    }

    /// Re-bake one instance at a new rigid pose (rotation basis columns plus
    /// translation of the template origin). Scale, if any, is baked into the
    /// local triangles at load and is not reapplied here.
    pub fn rebake(
        &mut self,
        instance: usize,
        basis: [[f32; 3]; 3],
        translation: Vector3,
    ) -> Result<(), String> {
        let triangles = self.bake(instance, basis, translation)?;
        self.world
            .replace_triangles(self.instances[instance].range.clone(), &triangles)
            .map_err(str::to_owned)
    }

    /// [`Self::rebake`] for several instances in order with one query index rebuild
    /// (`BoardWorld::replace_triangles_batch`: identical result). An instance whose triangles
    /// do not bake is skipped and reported, as its own `rebake` call failed before.
    pub fn rebake_many(&mut self, poses: &[(usize, [[f32; 3]; 3], Vector3)]) -> Vec<(usize, String)> {
        let mut errors = Vec::new();
        let mut baked = Vec::with_capacity(poses.len());
        for &(instance, basis, translation) in poses {
            match self.bake(instance, basis, translation) {
                Ok(triangles) => baked.push((self.instances[instance].range.clone(), triangles)),
                Err(error) => errors.push((instance, error)),
            }
        }
        let edits: Vec<_> = baked.iter().map(|(range, t)| (range.clone(), t.as_slice())).collect();
        if let Err(error) = self.world.replace_triangles_batch(&edits) {
            errors.extend(poses.iter().map(|p| (p.0, error.to_owned())));
        }
        errors
    }

    /// One instance's triangles at a rigid pose (tag, flags and edge cosines kept).
    fn bake(&self, instance: usize, basis: [[f32; 3]; 3], translation: Vector3) -> Result<Vec<WorldTriangle>, String> {
        let entry = &self.instances[instance];
        let transform = |p: Vector3| {
            Vector3::new(
                p.x * basis[0][0] + p.y * basis[1][0] + p.z * basis[2][0] + translation.x,
                p.x * basis[0][1] + p.y * basis[1][1] + p.z * basis[2][1] + translation.y,
                p.x * basis[0][2] + p.y * basis[1][2] + p.z * basis[2][2] + translation.z,
            )
        };
        let mut triangles = Vec::with_capacity(entry.local.len());
        for (i, &local) in entry.local.iter().enumerate() {
            let source = self.world.triangles()[entry.range.start + i];
            let points = local.map(transform);
            triangles.push(
                WorldTriangle::from_vertices(
                    points,
                    self.contact_material,
                    source.tag,
                    source.triangle.feature.flags,
                    source.triangle.feature.edge_cosines,
                    0.,
                )
                .ok_or("Rebaked prop collision triangle is invalid")?,
            );
        }
        Ok(triangles)
    }

    /// A layer with no instances yet (a map without placed props, which can
    /// still get props created mid-game).
    pub fn empty(material: RetailContactMaterial) -> Result<Self, String> {
        let metadata = QueryMetadata {
            packed_surfaces: vec![],
            meshes: vec![],
            static_edges: vec![],
            island_flags: 0,
        };
        Ok(Self {
            world: BoardWorld::with_query_metadata(vec![], metadata).map_err(str::to_owned)?.into(),
            instances: std::sync::Arc::new(Vec::new()),
            contact_material: material,
            retired: Vec::new(),
            collision_types: Vec::new(),
        })
    }

    /// Add one instance mid-game (a released hand prop, a mod's prop): `local`
    /// are template-space triangles with scale folded in, baked at `basis` /
    /// `translation` like [`Self::rebake`]; `surface` is the packed surface code
    /// (tag) of every triangle. Edge features come from the instance's own
    /// triangles (load-time instances also pair edges with touching neighbours).
    /// A retired slot with the same triangle count is reused, so repeated
    /// spawns of one template do not grow the world. Returns the instance index.
    pub fn add_instance(
        &mut self,
        id: u32,
        local: Vec<[Vector3; 3]>,
        surface: u32,
        basis: [[f32; 3]; 3],
        translation: Vector3,
    ) -> Result<usize, String> {
        let local: Vec<[Vector3; 3]> = local
            .into_iter()
            .filter(|t| {
                let [a, b, c] = t.map(|p| Vec3::new(p.x, p.y, p.z));
                (b - a).cross(c - a).length_squared() > 0.
            })
            .collect();
        if local.is_empty() {
            return Err("Prop instance has no valid triangles".into());
        }
        let transform = |p: Vector3| {
            [
                p.x * basis[0][0] + p.y * basis[1][0] + p.z * basis[2][0] + translation.x,
                p.x * basis[0][1] + p.y * basis[1][1] + p.z * basis[2][1] + translation.y,
                p.x * basis[0][2] + p.y * basis[1][2] + p.z * basis[2][2] + translation.z,
            ]
        };
        let points: Vec<[[f32; 3]; 3]> = local.iter().map(|t| t.map(transform)).collect();
        let (_, _, flags, cosines) = welded_edge_features(&points, true)?;
        let mut triangles = Vec::with_capacity(points.len());
        for (i, p) in points.iter().enumerate() {
            triangles.push(
                WorldTriangle::from_vertices(
                    p.map(|p| Vector3::new(p[0], p[1], p[2])),
                    self.contact_material,
                    surface,
                    flags[i],
                    cosines[i],
                    0.,
                )
                .ok_or("Prop instance triangle is invalid")?,
            );
        }
        let reuse = self
            .retired
            .iter()
            .position(|&slot| self.instances[slot].range.len() == triangles.len());
        let index = match reuse {
            Some(position) => {
                let slot = self.retired.remove(position);
                self.world
                    .replace_triangles(self.instances[slot].range.clone(), &triangles)
                    .map_err(str::to_owned)?;
                slot
            }
            None => {
                let range = self
                    .world
                    .append_triangles(&triangles, &vec![surface as u16; triangles.len()])
                    .map_err(str::to_owned)?;
                std::sync::Arc::make_mut(&mut self.instances).push(PropCollisionInstance { id, object: usize::MAX, range, local: Vec::new() });
                self.instances.len() - 1
            }
        };
        let entry = &mut std::sync::Arc::make_mut(&mut self.instances)[index];
        entry.id = id;
        entry.local = local;
        Ok(index)
    }

    /// Retire an instance added with [`Self::add_instance`] (the released prop is
    /// removed): its triangles are parked far below the world and the slot is
    /// kept for the next instance of the same size.
    pub fn retire_instance(&mut self, instance: usize, park: Vector3) -> Result<(), String> {
        if instance >= self.instances.len() || self.retired.contains(&instance) {
            return Err("Unknown or already retired prop instance".into());
        }
        if self.instances[instance].object != usize::MAX {
            return Err("Map prop instances are not retired".into());
        }
        self.rebake(instance, [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], park)?;
        self.retired.push(instance);
        Ok(())
    }
}

/// Static collision for spawned DMO prop instances. No authored DMO collision
/// mesh is recovered, so each instance reuses its template's render triangles,
/// baked into world space with the instance transform at load. Reflections
/// flip winding to keep outward normals; degenerate render triangles are
/// skipped rather than rejecting the whole layer.
/// One DMO template's retail physics collision (`native-props/<map>.json` `collision`, setup
/// `dynamic_props.template_collision`): the template's ClusteredMesh triangles in model space, with their RW edge codes
/// and surface ids (research dmo-physics-collision.md). Keyed by template id, so a mod can supply or replace one.
#[derive(Clone, Debug, Default)]
pub(crate) struct PropShape {
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub edges: Vec<Option<[u8; 3]>>,
    pub surfaces: Vec<u32>,
}

/// The retail collision shapes per DMO template id; empty when setup exported none (older install).
pub(crate) fn load_dmo_collision(asset_root: &std::path::Path, map_name: &str) -> std::collections::BTreeMap<String, PropShape> {
    load_collision_shapes(&asset_root.join("private").join("native-props").join(format!("{map_name}.json")))
}

/// The `collision` map of one placement sidecar (`dynamic_props.write_placements`: district props, park layouts).
pub(crate) fn load_collision_shapes(path: &std::path::Path) -> std::collections::BTreeMap<String, PropShape> {
    let Some(sidecar) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else { return Default::default() };
    let Some(map) = sidecar.get("collision").and_then(|v| v.as_object()) else { return Default::default() };
    let mut out = std::collections::BTreeMap::new();
    for (key, v) in map {
        let point = |p: &serde_json::Value| -> Option<[f32; 3]> {
            let a = p.as_array()?;
            Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
        };
        let triangles: Option<Vec<[[f32; 3]; 3]>> = v["triangles"].as_array().map(|ts| {
            ts.iter().filter_map(|t| { let t = t.as_array()?; Some([point(t.first()?)?, point(t.get(1)?)?, point(t.get(2)?)?]) }).collect()
        });
        let Some(triangles) = triangles.filter(|t| !t.is_empty()) else { continue };
        let edges: Vec<Option<[u8; 3]>> = v["edges"].as_array().map(|es| es.iter().map(|e| {
            let a = e.as_array()?;
            Some([a.first()?.as_u64()? as u8, a.get(1)?.as_u64()? as u8, a.get(2)?.as_u64()? as u8])
        }).collect()).unwrap_or_default();
        let surfaces: Vec<u32> = v["surfaces"].as_array().map(|ss| ss.iter().map(|x| x.as_u64().unwrap_or(0) as u32).collect()).unwrap_or_default();
        if edges.len() != triangles.len() || surfaces.len() != triangles.len() {
            warn!("SKATE_PROP_COLLISION: {}: template {key}: triangle, edge and surface counts differ; render mesh kept", path.display());
            continue;
        }
        out.insert(key.clone(), PropShape { triangles, edges, surfaces });
    }
    out
}

pub(crate) fn build_prop_layer(
    map: &SkateMap,
    objects: &[skate_data::skate_map::StaticObject],
    material: RetailContactMaterial,
) -> Result<Option<PropCollisionLayer>, String> {
    build_prop_layer_with(map, objects, material, &Default::default())
}

/// [`build_prop_layer`] with retail collision shapes per template: a prop whose template has one collides with that
/// mesh and its native edge codes (retail's DMO physics collision); others with their render triangles (the welded
/// render mesh left seams between a bench's seat pieces as catching edges, 2026-10-15: 10 of 12 grind bails on one).
pub(crate) fn build_prop_layer_with(
    map: &SkateMap,
    objects: &[skate_data::skate_map::StaticObject],
    material: RetailContactMaterial,
    shapes: &std::collections::BTreeMap<String, PropShape>,
) -> Result<Option<PropCollisionLayer>, String> {
    let (collision, instances) = posed_collision(map, objects, shapes);
    if collision.is_empty() {
        return Ok(None);
    }
    Ok(Some(PropCollisionLayer {
        world: portable_world(&collision, &map.materials, material, Some(&instances.iter().map(|i| i.range.clone()).collect::<Vec<_>>()))?.into(),
        instances: std::sync::Arc::new(instances),
        contact_material: material,
        retired: Vec::new(),
        collision_types: Vec::new(),
    }))
}

/// Every placed object's collision in world space (its template's retail shape when it has one, else its render
/// triangles) with one contiguous range per object: the props' layer and the skate.Park pieces' share of the static
/// world (crate::park_layout).
pub(crate) fn posed_collision(
    map: &SkateMap,
    objects: &[skate_data::skate_map::StaticObject],
    shapes: &std::collections::BTreeMap<String, PropShape>,
) -> (Vec<skate_data::skate_map::Collision>, Vec<PropCollisionInstance>) {
    let mut collision = Vec::new();
    let mut mismatched = std::collections::BTreeSet::new();
    let mut instances = Vec::new();
    for (object_index, object) in objects.iter().enumerate() {
        let t = &object.transform;
        // Row-vector affine (v @ basis + translation), as in spawn_instances.
        let transform = |p: [f32; 3]| -> [f32; 3] {
            [
                p[0] * t[0] + p[1] * t[3] + p[2] * t[6] + t[9],
                p[0] * t[1] + p[1] * t[4] + p[2] * t[7] + t[10],
                p[0] * t[2] + p[1] * t[5] + p[2] * t[8] + t[11],
            ]
        };
        let determinant = t[0] * (t[4] * t[8] - t[5] * t[7])
            - t[1] * (t[3] * t[8] - t[5] * t[6])
            + t[2] * (t[3] * t[7] - t[4] * t[6]);
        // Row-vector rows are the world images of the local axes. Their lengths
        // are the per-axis scale; rebake applies rotation only, so local source
        // triangles carry the scale and stay exact for diagonal-scale placements.
        let scale = [
            (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt(),
            (t[3] * t[3] + t[4] * t[4] + t[5] * t[5]).sqrt(),
            (t[6] * t[6] + t[7] * t[7] + t[8] * t[8]).sqrt(),
        ];
        let range =
            object.first_index as usize..(object.first_index + object.index_count) as usize;
        let start = collision.len();
        let mut local = Vec::new();
        let template = object.name.split('/').next().unwrap_or_default();
        // Both meshes should be in the template's model space: a retail shape that does not line up with the render
        // mesh (the garbage bags sit 0.38 m lower: anchored elsewhere, not decoded) keeps the render mesh, logged once.
        let shape = shapes.get(template).filter(|shape| {
            let bounds = |points: &mut dyn Iterator<Item = [f32; 3]>| points.fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
                (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i])))
            });
            let (rl, rh) = bounds(&mut map.geometry.indices[range.clone()].iter().map(|&i| map.geometry.vertices[i as usize].position));
            let (sl, sh) = bounds(&mut shape.triangles.iter().flatten().copied());
            let gap = (0..3).map(|i| (rl[i] - sl[i]).abs().max((rh[i] - sh[i]).abs())).fold(0.0f32, f32::max);
            if gap > 0.25 && mismatched.insert(template.to_owned()) {
                warn!("SKATE_PROP_COLLISION: template {template} retail shape bounds {sl:?}..{sh:?} differ from the render mesh {rl:?}..{rh:?} by {gap:.2} m; render mesh kept");
            }
            gap <= 0.25
        });
        if let Some(shape) = shape {
            for ((tri, edges), surface) in shape.triangles.iter().zip(&shape.edges).zip(&shape.surfaces) {
                let mut points = *tri;
                let mut edges = *edges;
                if determinant < 0. {
                    points.swap(1, 2);
                    // Edge i runs from vertex i to i + 1: swapping b and c reverses the loop, so edge 0 (a-b) becomes
                    // a-c (old edge 2) and edge 2 becomes b-a (old edge 0).
                    edges = edges.map(|e| [e[2], e[1], e[0]]);
                }
                let [a, b, c] = points.map(Vec3::from_array);
                if (b - a).cross(c - a).length_squared() <= 0. {
                    continue;
                }
                local.push(points.map(|p| Vector3::new(p[0] * scale[0], p[1] * scale[1], p[2] * scale[2])));
                collision.push(skate_data::skate_map::Collision {
                    points: points.map(transform),
                    surface: *surface,
                    material: 1,
                    native_edges: edges,
                });
            }
            if collision.len() > start {
                instances.push(PropCollisionInstance { id: object.id, object: object_index, range: start..collision.len(), local });
            }
            continue;
        }
        for tri in map.geometry.indices[range].chunks_exact(3) {
            let mut points =
                [tri[0], tri[1], tri[2]].map(|i| map.geometry.vertices[i as usize].position);
            if determinant < 0. {
                points.swap(1, 2);
            }
            let [a, b, c] = points.map(Vec3::from_array);
            let cross = (b - a).cross(c - a);
            if cross.length_squared() <= 0. {
                continue;
            }
            local.push(points.map(|p| {
                Vector3::new(p[0] * scale[0], p[1] * scale[1], p[2] * scale[2])
            }));
            let source = &map.materials
                [map.geometry.vertices[tri[0] as usize].material as usize - 1];
            // Wheels read the packed surface nibble from the triangle tag; use
            // the same EncodeRwSurfaceId mapping as static map collision.
            let surface = source.audio | (source.physics << 7) | (source.pattern << 12);
            collision.push(skate_data::skate_map::Collision {
                points: points.map(transform),
                surface,
                material: map.geometry.vertices[tri[0] as usize].material,
                native_edges: None,
            });
        }
        if collision.len() > start {
            instances.push(PropCollisionInstance {
                id: object.id,
                object: object_index,
                range: start..collision.len(),
                local,
            });
        }
    }
    (collision, instances)
}

/// Retail per-type DMO data for the district's props: setup writes the map
/// from template id to the type's vault record (`types` in
/// `private/native-props/<map>.json`, from the template's EB000D +120); the
/// values come from the installation's stock vault
/// (`livingworld_dynamicobject_characteristics`,
/// [`crate::physics::prop_dynamics::dmo_type_blocks`]). `None` when the
/// sidecar has no type map (setup older than the type data): the props keep
/// their authored material (NOT RETAIL YET there; re-run setup group maps).
/// The authored grab splines per DMO template id (`native-props/<map>.json` `grab_splines`, doc 26i "Move Object
/// step 1"); `None` when the export has none.
pub(crate) fn load_dmo_grab_splines(asset_root: &std::path::Path, map_name: &str) -> Option<std::collections::BTreeMap<String, Vec<crate::living_world::vehicles::CarGrabSpline>>> {
    load_grab_splines(&asset_root.join("private").join("native-props").join(format!("{map_name}.json")))
}

/// The grab_splines map of one placement sidecar.
pub(crate) fn load_grab_splines(path: &std::path::Path) -> Option<std::collections::BTreeMap<String, Vec<crate::living_world::vehicles::CarGrabSpline>>> {
    let sidecar: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let map = sidecar.get("grab_splines")?.as_object()?;
    let splines: std::collections::BTreeMap<_, _> = map.iter().map(|(k, v)| (k.clone(), crate::living_world::vehicles::parse_grab_splines(v))).filter(|(_, v): &(String, Vec<_>)| !v.is_empty()).collect();
    (!splines.is_empty()).then_some(splines)
}

/// The grind rails per DMO template id (`native-props/<map>.json` `grind_splines`, template space; retail setup
/// 82C535C0, doc 26 "Grinding on props"); `None` when the export has none. A malformed list is an error.
pub(crate) fn load_dmo_grind_rails(asset_root: &std::path::Path, map_name: &str) -> Result<Option<std::collections::BTreeMap<String, Vec<crate::grind_world::props::PropRail>>>, String> {
    load_grind_rails(&asset_root.join("private").join("native-props").join(format!("{map_name}.json")))
}

/// The `grind_splines` map of one placement sidecar, per template id (template space).
pub(crate) fn load_grind_rails(path: &std::path::Path) -> Result<Option<std::collections::BTreeMap<String, Vec<crate::grind_world::props::PropRail>>>, String> {
    let Ok(bytes) = std::fs::read(path) else { return Ok(None) };
    let sidecar: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(map) = sidecar.get("grind_splines").and_then(|v| v.as_object()) else { return Ok(None) };
    let rails = map.iter().map(|(k, v)| Ok((k.clone(), crate::grind_world::props::parse_rails(v).map_err(|e| format!("{}: {k}: {e}", path.display()))?)))
        .collect::<Result<std::collections::BTreeMap<_, _>, String>>()?;
    Ok((!rails.is_empty()).then_some(rails))
}

pub(crate) fn load_dmo_types(
    asset_root: &std::path::Path,
    map_name: &str,
) -> Option<std::collections::BTreeMap<String, crate::physics::prop_dynamics::DmoType>> {
    load_types(asset_root, &asset_root.join("private").join("native-props").join(format!("{map_name}.json")), map_name)
}

/// The 	ypes map of one placement sidecar, resolved against the installation's stock vault.
pub(crate) fn load_types(
    asset_root: &std::path::Path,
    path: &std::path::Path,
    map_name: &str,
) -> Option<std::collections::BTreeMap<String, crate::physics::prop_dynamics::DmoType>> {
    use crate::physics::prop_dynamics::{dmo_type_blocks, DmoType, DMO_TYPE_CLASS};
    let sidecar: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let map = sidecar.get("types")?.as_object()?;
    if map.is_empty() {
        return None;
    }
    let collections = match skate_data::collections::Collections::load(asset_root) {
        Ok(c) => c,
        Err(error) => {
            warn!("SKATE_PROP_TYPES: {map_name}: {error}");
            return None;
        }
    };
    let names: std::collections::HashMap<String, &str> = collections
        .entries()
        .iter()
        .filter(|c| c.class_name == DMO_TYPE_CLASS)
        .map(|c| (skate_data::attrib_hash::numeric_name(&c.key), c.key.as_str()))
        .collect();
    let mut types = std::collections::BTreeMap::new();
    for (template, record) in map {
        let Some(record) = record.as_str() else { continue };
        let id = skate_data::attrib_hash::numeric_name(record);
        let key = names.get(&id).map_or(record, |name| *name);
        match dmo_type_blocks(&collections, key) {
            Ok(blocks) => {
                let priority = crate::physics::prop_dynamics::dmo_type_priority(&collections, key).unwrap_or_else(|error| {
                    warn!("SKATE_PROP_TYPES: {map_name} template {template}: priority: {error}");
                    Some(100)
                });
                types.insert(template.clone(), DmoType { key: key.to_owned(), blocks, priority });
            }
            Err(error) => warn!("SKATE_PROP_TYPES: {map_name} template {template}: {error}"),
        }
    }
    Some(types)
}

/// Load the district's prop package and build its collision layer plus the
/// dynamic bodies for every instance. The package is a presentation
/// supplement: missing or invalid files leave props uncollidable rather than
/// failing the map, matching the render path.
pub(crate) fn load_prop_layer(
    asset_root: &std::path::Path,
    map_name: &str,
    material: RetailContactMaterial,
    simulation: skate_core::physics::rigid_body::RetailSimulationStep,
) -> Option<(PropCollisionLayer, crate::physics::prop_dynamics::PropDynamics)> {
    let directory = asset_root.join("private").join("native-props");
    load_prop_layer_at(asset_root, &directory.join(format!("{map_name}.skate")), &directory.join(format!("{map_name}.json")), map_name, material, simulation)
}

/// [load_prop_layer] for a loaded map ([prop_files]).
pub(crate) fn load_map_prop_layer(
    asset_root: &std::path::Path,
    map: &SkateMap,
    material: RetailContactMaterial,
    simulation: skate_core::physics::rigid_body::RetailSimulationStep,
) -> Option<(PropCollisionLayer, crate::physics::prop_dynamics::PropDynamics)> {
    let (skate, json, name) = prop_files(asset_root, map);
    load_prop_layer_at(asset_root, &skate, &json, &name, material, simulation)
}

fn load_prop_layer_at(
    asset_root: &std::path::Path,
    path: &std::path::Path,
    sidecar: &std::path::Path,
    map_name: &str,
    material: RetailContactMaterial,
    simulation: skate_core::physics::rigid_body::RetailSimulationStep,
) -> Option<(PropCollisionLayer, crate::physics::prop_dynamics::PropDynamics)> {
    if !path.is_file() {
        return None;
    }
    let loaded = std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|data| SkateMap::parse_render_only(&data));
    let map = match loaded {
        Ok(map) => map,
        Err(error) => {
            warn!("SKATE_PROP_COLLISION: {}: {error}", path.display());
            return None;
        }
    };
    let mut objects = Vec::new();
    for extension in map.extensions.iter().filter(|e| e.tag == *b"MOBJ") {
        match skate_data::skate_map::parse_static_objects(&map, extension) {
            Ok(parsed) => objects.extend(parsed),
            Err(error) => {
                warn!("SKATE_PROP_COLLISION: {}: {error}", path.display());
                return None;
            }
        }
    }
    let shapes = load_collision_shapes(sidecar);
    let retail_shaped = objects.iter().filter(|o| shapes.contains_key(o.name.split('/').next().unwrap_or_default())).count();
    info!("SKATE_PROP_COLLISION: {map_name} retail collision shapes {} templates, {retail_shaped}/{} props", shapes.len(), objects.len());
    match build_prop_layer_with(&map, &objects, material, &shapes) {
        Ok(Some(layer)) => {
            info!(
                "SKATE_PROP_COLLISION: {map_name} instances={} triangles={}",
                objects.len(),
                layer.world().triangles().len()
            );
            let mut dynamics = crate::physics::prop_dynamics::PropDynamics::new(
                &objects,
                layer.instances(),
                simulation,
            );
            if let Some(types) = load_types(asset_root, sidecar, map_name) {
                let typed = dynamics.set_type_data(&types);
                info!("SKATE_PROP_TYPES: {map_name} types={} props_with_type_data={typed}/{}", types.len(), objects.len());
            }
            if let Some(splines) = load_grab_splines(sidecar) {
                let with = dynamics.set_grab_splines(&splines);
                info!("SKATE_PROP_GRAB: {map_name} templates={} props_with_grab_splines={with}/{}", splines.len(), objects.len());
            }
            match load_grind_rails(sidecar) {
                Ok(Some(rails)) => {
                    let templates = rails.len();
                    let with = dynamics.set_grind_rails(rails);
                    info!("SKATE_PROP_RAILS: {map_name} templates={templates} props_with_rails={with}/{}", objects.len());
                }
                Ok(None) => {}
                Err(error) => warn!("SKATE_PROP_RAILS: {map_name} not loaded: {error}"),
            }
            Some((layer, dynamics))
        }
        Ok(None) => None,
        Err(error) => {
            warn!("SKATE_PROP_COLLISION: {}: {error}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every prop type in a set-up map's type map resolves to retail type
    /// data from the stock vault, and the values are the retail record's
    /// (the default record: restitution 0.5, free pair 0.8 / 0.6).
    /// `SKATE3_ASSET_ROOT` = set-up assets (setup group maps run with the
    /// type map), `SKATE3_PROP_MAP` = map name (default DownTown).
    #[test]
    #[ignore = "Requires private installed assets"]
    fn installed_map_props_resolve_retail_type_data() {
        let root = std::path::PathBuf::from(std::env::var("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let map = std::env::var("SKATE3_PROP_MAP").unwrap_or_else(|_| "DownTown".into());
        let sidecar: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("private/native-props").join(format!("{map}.json"))).unwrap(),
        )
        .unwrap();
        let expected = sidecar["types"].as_object().expect("type map (re-run setup group maps)").len();
        let types = load_dmo_types(&root, &map).expect("type data");
        assert!(expected > 0);
        assert_eq!(types.len(), expected, "every template's record resolved");
        for (template, t) in &types {
            let b = t.blocks;
            for v in [b.restitution.unwrap(), b.free.unwrap()[0], b.free.unwrap()[1]] {
                assert!((0.0..=1.0).contains(&v), "{template} {} {v}", t.key);
            }
            assert_eq!(b.upright_pair, b.record_272);
            for v in [b.linear_drag.unwrap(), b.angular_drag.unwrap()] {
                assert!((0.0..=1.0).contains(&v), "{template} {} drag {v}", t.key);
            }
            // Body data of 82C4E568: mass +304, caps +292 / +296, inertia box +16 / +32.
            assert!(b.mass.unwrap() > 0.0, "{template} {} mass", t.key);
            assert!(b.maximum_linear_velocity.unwrap() > 0.0 && b.maximum_angular_velocity.unwrap() > 0.0);
            assert!(b.inertia_scale.is_some() && b.inertia_offset.is_some());
        }
        let collections = skate_data::collections::Collections::load(&root).unwrap();
        let default = crate::physics::prop_dynamics::dmo_type_blocks(&collections, "default").unwrap();
        assert_eq!((default.restitution, default.free, default.upright_pair), (Some(0.5), Some([0.8, 0.6]), Some(false)));
        assert_eq!((default.linear_drag, default.angular_drag), (Some(0.0), Some(0.0)));
        assert_eq!((default.mass, default.maximum_linear_velocity, default.maximum_angular_velocity), (Some(100.0), Some(100.0), Some(100.0)));
        assert_eq!((default.inertia_scale, default.inertia_offset), (Some([1.2, 1.2, 1.2]), Some([0.0; 3])));
        for (template, t) in &types {
            let b = t.blocks;
            eprintln!("{template} {} mass={:?} caps={:?}/{:?} box={:?}+{:?}", t.key, b.mass, b.maximum_linear_velocity, b.maximum_angular_velocity, b.inertia_scale, b.inertia_offset);
        }
        let mut keys: Vec<_> = types.values().map(|t| t.key.as_str()).collect();
        keys.dedup();
        eprintln!("{map}: {} templates, types {:?}", types.len(), keys);
    }

    /// Measures the static draw budget on a real installed map. This is the
    /// check that the whole architecture exists to pass, so it reports the class
    /// mix rather than only the total: if the budget is ever missed, the mix says
    /// whether to subdivide less or to stop splitting on a material property.
    ///
    /// `SKATE_BUDGET_MAP` names the `.skate` file; `SKATE_TRANSITION_TEST_ASSETS`
    /// gives the asset root.
    #[test]
    #[ignore = "Requires private installed assets; CPU only, no GPU or window"]
    fn static_draw_budget_holds_on_an_installed_map() {
        let root = std::path::PathBuf::from(
            std::env::var("SKATE_TRANSITION_TEST_ASSETS")
                .expect("SKATE_TRANSITION_TEST_ASSETS"),
        );
        let path = std::path::PathBuf::from(
            std::env::var("SKATE_BUDGET_MAP").expect("SKATE_BUDGET_MAP"),
        );
        let map = skate_data::skate_map::SkateMap::load(&path).unwrap();
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<crate::retail_render::WorldMaterial>>();
        world.init_resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>();
        let mut scene = crate::map_render::PreparedScene::new(&world);
        let started = std::time::Instant::now();
        scene.prepare(Some(&map), &root);
        let elapsed = started.elapsed();
        let stats = scene.stats;
        eprintln!(
            "SKATE_BUDGET map={} draws={} triangles={} slabs={} materials={} prepare_ms={}",
            path.file_stem().unwrap_or_default().to_string_lossy(),
            stats.draws,
            stats.triangles,
            stats.slabs,
            map.materials.len(),
            elapsed.as_millis()
        );
        assert!(
            stats.draws < 300,
            "static draw budget is under 300; got {} draws",
            stats.draws
        );
        assert!(stats.triangles > 0, "map produced no geometry");
    }

    fn triangle(centroid: Vec3) -> Triangle {
        Triangle { indices: [0, 1, 2], centroid, slab: 0, class: RenderClass::Opaque }
    }

    #[test]
    fn partition_respects_the_leaf_budget() {
        let triangles: Vec<Triangle> = (0..LEAF_TRIANGLE_BUDGET * 3 + 7)
            .map(|i| triangle(Vec3::new(i as f32, 0., 0.)))
            .collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        let leaves = partition(&mut members, &triangles);
        assert!(leaves.len() >= 4, "expected several leaves, got {}", leaves.len());
        for leaf in &leaves {
            assert!(leaf.len() <= LEAF_TRIANGLE_BUDGET, "leaf of {} triangles", leaf.len());
        }
    }

    #[test]
    fn partition_preserves_every_triangle_exactly_once() {
        let triangles: Vec<Triangle> = (0..5_000)
            .map(|i| triangle(Vec3::new((i % 71) as f32, (i % 13) as f32, i as f32)))
            .collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        let mut seen: Vec<usize> = partition(&mut members, &triangles)
            .into_iter()
            .flatten()
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..triangles.len()).collect::<Vec<_>>());
    }

    #[test]
    fn a_single_leaf_is_not_split() {
        let triangles: Vec<Triangle> = (0..64).map(|i| triangle(Vec3::splat(i as f32))).collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        assert_eq!(partition(&mut members, &triangles).len(), 1);
    }

    fn material() -> RetailContactMaterial {
        RetailContactMaterial {
            static_friction: 0.,
            dynamic_friction: 0.,
            restitution: 1.,
        }
    }

    /// A 4x1x1 slab template (x ±2, y 0..1, z ±0.5) with two placed instances:
    /// one translated, one rotated 90° about Y and translated.
    fn prop_fixture() -> (SkateMap, Vec<skate_data::skate_map::StaticObject>) {
        let corners = [
            [-2., 0., -0.5], [2., 0., -0.5], [2., 0., 0.5], [-2., 0., 0.5],
            [-2., 1., -0.5], [2., 1., -0.5], [2., 1., 0.5], [-2., 1., 0.5],
        ];
        let faces = [
            [4, 7, 6], [4, 6, 5], // +Y top
            [0, 1, 2], [0, 2, 3], // -Y bottom
            [1, 5, 6], [1, 6, 2], // +X
            [0, 7, 4], [0, 3, 7], // -X
            [3, 2, 6], [3, 6, 7], // +Z
            [0, 5, 1], [0, 4, 5], // -Z
        ];
        let vertex = |position| skate_data::skate_map::Vertex {
            position,
            normal: [0., 1., 0.],
            uv: [0.; 2],
            lightmap_uv: [0.; 2],
            material: 1,
            decal_uv: None,
            tangent_frame: None,
        };
        let map = SkateMap {
            version: 14,
            name: "props".into(),
            spawn: [0.; 3],
            heading: 0.,
            environment: vec![0.; 45],
            materials: vec![skate_data::skate_map::Material {
                name: "prop".into(),
                flags: 0,
                friction: 0.5,
                restitution: 0.1,
                color: [1.; 3],
                roughness: 0.5,
                emissive: 0.,
                textures: [0; 5],
                indirect_strength: 0.,
                alpha_mode: 0,
                alpha_cutoff: 0.5,
                audio: 3,
                physics: 1,
                pattern: 0,
                depth_layer: None,
                retail_definition: None,
            }],
            textures: vec![],
            geometry: skate_data::skate_map::Geometry {
                vertices: corners.into_iter().map(vertex).collect(),
                indices: faces.into_iter().flatten().collect(),
                collision: vec![],
            },
            rails: vec![],
            doors: vec![],
            lights: vec![],
            routes: vec![],
            extensions: vec![],
        };
        let object = |id, transform| skate_data::skate_map::StaticObject {
            id,
            name: format!("template/prop{id}"),
            transform,
            first_index: 0,
            index_count: 36,
            first_collision: 0,
            collision_count: 0,
            rails: vec![],
            physics: Default::default(),
        };
        let objects = vec![
            object(7, [1., 0., 0., 0., 1., 0., 0., 0., 1., 10., 5., 0.]),
            // 90° about Y (row-vector): x' = -z, z' = x; then z -= 10.
            object(8, [0., 0., 1., 0., 1., 0., -1., 0., 0., 0., 0., -10.]),
        ];
        (map, objects)
    }

    #[test]
    fn prop_instances_collide_as_placed_static_triangles() {
        let (map, objects) = prop_fixture();
        let layer = build_prop_layer(&map, &objects, material()).unwrap().unwrap();
        let mut world = layer.world;
        assert_eq!(world.triangles().len(), 24);
        // Translated instance: top face at y=6 between x 8..12.
        let hit = world
            .query_thin_line(Vector3::new(10., 10., 0.), Vector3::new(10., 0., 0.))
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - 6.).abs() < 1e-4);
        assert!(hit.geometry.normal.y > 0.99);
        // The packed surface tag follows the EncodeRwSurfaceId mapping.
        assert_eq!(hit.tag, 3 | (1 << 7));
        // Rotated instance: slab now spans z -12..-8, x ±0.5.
        let hit = world
            .query_thin_line(Vector3::new(0., 5., -10.), Vector3::new(0., -1., -10.))
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - 1.).abs() < 1e-4);
        // Outside the rotated footprint: unrotated, this line would hit.
        assert!(world
            .query_thin_line(Vector3::new(1.5, 5., -10.), Vector3::new(1.5, -1., -10.))
            .unwrap()
            .is_none());
        // A wheel sphere resting on the translated instance reports a contact.
        let (query, retention) = crate::physics::ground::query_settings();
        let volumes = [skate_core::physics::board_world::BoardWorldVolume {
            collision_group: 4,
            body: skate_core::physics::board_step::CollisionBody::Board(
                skate_core::physics::board::BodyId::Deck,
            ),
            primitive: skate_core::physics::world_contact::ContactPrimitive::Sphere(
                skate_core::physics::collision::Sphere {
                    center: Vector3::new(10., 6.05, 0.),
                    radius: 0.1,
                },
            ),
            motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: Vector3::ZERO, ..Default::default() },
            material: material(),
        }];
        let contacts = world.query_primitives(&volumes, query, retention);
        assert!(!contacts.is_empty());
        assert!(contacts.iter().all(|c| c.contact.normal.y > 0.9));
    }

    #[test]
    fn prop_instances_spawn_one_placed_root_per_record_sharing_template_meshes() {
        use crate::map_render::{MapEntity, StagedAssets};
        let (map, objects) = prop_fixture();
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<WorldMaterial>>();
        world.init_resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>();
        let mut meshes = StagedAssets::<Mesh>::new(&world);
        let mut images = StagedAssets::<Image>::new(&world);
        let mut materials = StagedAssets::<WorldMaterial>::new(&world);
        let mut buffers = StagedAssets::<bevy::render::storage::ShaderStorageBuffer>::new(&world);
        let mut commands = SceneCommands::default();
        let spawned = spawn_instances(
            &map,
            &objects,
            &crate::retail_render::MaterialTuning::default(),
            &crate::retail_sky::SkyEnvironment::default(),
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut buffers,
            None,
        );
        assert_eq!(spawned, 2);
        meshes.publish(&mut world);
        images.publish(&mut world);
        materials.publish(&mut world);
        buffers.publish(&mut world);
        commands.apply(&mut world);
        // Both records place the same range: one template mesh, built once.
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 1);
        let mut roots: Vec<(u32, Transform, Vec<Entity>)> = world
            .query::<(&PropInstance, &Transform, &Children)>()
            .iter(&world)
            .map(|(prop, transform, children)| (prop.id, *transform, children.to_vec()))
            .collect();
        roots.sort_by_key(|(id, ..)| *id);
        assert_eq!(roots.iter().map(|(id, ..)| *id).collect::<Vec<_>>(), [7, 8]);
        // Only the roots carry MapEntity; children go with their parent.
        assert_eq!(world.query_filtered::<Entity, With<MapEntity>>().iter(&world).count(), 2);
        assert!((roots[0].1.translation - Vec3::new(10., 5., 0.)).length() < 1e-5);
        // Row-vector basis row 0 is (0, 0, 1): template +X maps to world +Z.
        let tip = roots[1].1.transform_point(Vec3::X);
        assert!((tip - Vec3::new(0., 0., -9.)).length() < 1e-5, "{tip}");
        let mesh = |entity: Entity| world.get::<Mesh3d>(entity).unwrap().0.id();
        assert_eq!(roots[0].2.len(), 1);
        assert_eq!(mesh(roots[0].2[0]), mesh(roots[1].2[0]));
    }

    /// Two flat 1 m plates (two triangles each, normal up) meeting at x = 1 with piece B moved by `offset`.
    fn plates(offset: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        let a = |x: f32, z: f32| [x, 0.0, z];
        let b = |x: f32, z: f32| [x + offset[0], offset[1], z + offset[2]];
        vec![
            [a(0., 0.), a(0., 1.), a(1., 0.)],
            [a(1., 0.), a(0., 1.), a(1., 1.)],
            [b(1., 0.), b(1., 1.), b(2., 0.)],
            [b(2., 0.), b(1., 1.), b(2., 1.)],
        ]
    }

    /// b477 / b478: flush park pieces (laptop session 2026-10-21: 1 to 2 mm apart) fuse, so the seam edges pair and
    /// lose their convex bit; the 1 mm map path leaves them open; a real 5 cm gap stays open when fused.
    #[test]
    fn fused_park_pieces_pair_their_flush_seams() {
        let seam = |features: &(Vec<Vec3>, Vec<[usize; 3]>, Vec<u32>, Vec<[f32; 3]>)| {
            let (_, _, flags, cosines) = features;
            // Triangle 1 edge 2 (x = 1, z 1 -> 0) and triangle 2 edge 0 (x = 1, z 0 -> 1).
            [(1, 2), (2, 0)].map(|(t, e)| (flags[t] & (0x20 << e) != 0, cosines[t][e]))
        };
        let fuse = |points: &[[[f32; 3]; 3]]| {
            let (p, v, f, c, _) = fused_edge_features(points, crate::park_layout::FUSE_WELD, &[0, 0, 1, 1]).unwrap();
            (p, v, f, c)
        };
        let flush = plates([0.0015, 0.001, 0.0]);
        let fused = fuse(&flush);
        // Normals come from the merged corners (retail), so plate B's 1.5 mm pull tilts it by a hair.
        assert!(seam(&fused).iter().all(|&(convex, cosine)| !convex && (cosine - 1.0).abs() < 1e-5), "paired: flat, not convex");
        let mapped = welded_edge_features_within(&flush, true, None).unwrap();
        assert!(seam(&mapped).iter().all(|&(convex, _)| convex), "the 1 mm grid keeps them open");
        let gap = fuse(&plates([0.05, 0.0, 0.0]));
        assert!(seam(&gap).iter().all(|&(convex, _)| convex), "a real gap still catches");
        // Deterministic: the same input gives the same features.
        assert_eq!(fused, fuse(&flush));
    }

    /// The real "Back Lot Park - Full" layout (laptop session 2026-10-21 21:38: pops crossing the aa_PlazaPad seam at
    /// x -151.77, z 74.5): after fusing, every top edge on that seam is paired flat and the pads' back-to-back walls
    /// are gone. Needs SKATE3_ASSET_ROOT with private/park-layouts (the layout's own render package carries the mark).
    #[test]
    #[ignore = "requires private converted assets and the DLC park layouts"]
    fn backlot_plaza_pad_seam_is_flat_after_fusing() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT"));
        let data = std::fs::read(crate::park_layout::directory(&root).join("backlot_full.skate")).unwrap();
        let mut map = SkateMap::parse_render_only(&data).unwrap();
        map.extensions.push(skate_data::skate_map::Extension {
            tag: crate::park_layout::EXTENSION_TAG,
            schema: 1,
            payload: b"backlot_full".to_vec(),
        });
        let pieces = crate::park_layout::load_pieces(&root, &map).expect("backlot_full pieces");
        let points: Vec<_> = pieces.collision.iter().map(|t| t.points).collect();
        let mut piece_of = vec![0; points.len()];
        for (piece, range) in pieces.ranges.iter().enumerate() {
            piece_of[range.clone()].fill(piece);
        }
        let (positions, vertices, flags, cosines, dropped) = fused_edge_features(&points, pieces.weld, &piece_of).unwrap();
        let on_seam = |p: Vec3| (p.x + 151.77).abs() < 0.02 && (p.y - 0.749).abs() < 0.02 && (73.0..76.0).contains(&p.z);
        let (mut found, mut open, mut bent) = (0, 0, 0);
        for t in (0..points.len()).filter(|&i| !dropped[i]) {
            for e in 0..3 {
                let [a, b] = [vertices[t][e], vertices[t][(e + 1) % 3]].map(|v| positions[v]);
                if on_seam(a) && on_seam(b) && (a - b).length() > 0.05 {
                    found += 1;
                    // An unpaired edge keeps the defaults: convex bit and cosine exactly 1.
                    open += usize::from(flags[t] & (0x20 << e) != 0 && cosines[t][e] == 1.0);
                    // Paired with the other pad's top (merged corners: within 0.02 degrees), not with a side wall.
                    bent += usize::from(cosines[t][e] < 0.9999);
                }
            }
        }
        let removed = dropped.iter().filter(|&&d| d).count();
        println!("backlot_full: {} triangles, {removed} dropped, seam top edges {found}, open {open}, bent {bent}", points.len());
        assert!(found >= 2, "the seam's top edges were found");
        assert_eq!((open, bent), (0, 0), "every seam top edge pairs flat with the other pad's top");
    }

    /// Two raised pads side by side (laptop session 2026-10-21 21:38, BackLot's aa_PlazaPad seams at x -151.77 and
    /// -139.77: the board popped crossing them). Each pad's side wall at the shared face lies back to back on the
    /// other's, so the top seam edge had four triangles. Retail `828755B8` removes such faces between different
    /// pieces, as a pair (same diagonal) or as two quads (crossed diagonals); the tops then pair flat.
    #[test]
    fn fused_pads_drop_their_back_to_back_walls_and_pair_the_top_seam() {
        let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3], out: Vec3| {
            let n = (Vec3::from_array(b) - Vec3::from_array(a)).cross(Vec3::from_array(c) - Vec3::from_array(a));
            if n.dot(out) < 0. { [a, c, b] } else { [a, b, c] }
        };
        // A closed box from x0 to x1 (y 0..0.75, z 0..1); `cross` splits the x = x1 face on the other diagonal.
        let pad = |x0: f32, x1: f32, cross: bool| {
            let (y, h) = (0.0, 0.75);
            let centre = Vec3::new((x0 + x1) / 2., h / 2., 0.5);
            let mut out = Vec::new();
            let mut face = |q: [[f32; 3]; 4], other: bool| {
                let n = q.iter().map(|p| Vec3::from_array(*p)).sum::<Vec3>() / 4. - centre;
                if other {
                    out.push(tri(q[1], q[2], q[3], n));
                    out.push(tri(q[1], q[3], q[0], n));
                } else {
                    out.push(tri(q[0], q[1], q[2], n));
                    out.push(tri(q[0], q[2], q[3], n));
                }
            };
            face([[x0, h, 0.], [x1, h, 0.], [x1, h, 1.], [x0, h, 1.]], false);
            face([[x0, y, 0.], [x1, y, 0.], [x1, y, 1.], [x0, y, 1.]], false);
            face([[x0, y, 0.], [x1, y, 0.], [x1, h, 0.], [x0, h, 0.]], false);
            face([[x0, y, 1.], [x1, y, 1.], [x1, h, 1.], [x0, h, 1.]], false);
            face([[x0, y, 0.], [x0, h, 0.], [x0, h, 1.], [x0, y, 1.]], false);
            face([[x1, y, 0.], [x1, h, 0.], [x1, h, 1.], [x1, y, 1.]], cross);
            out
        };
        // Pad B is built mirrored (x0 = 2, x1 = 1) so its x = 1 face is the `cross` one.
        for cross in [false, true] {
            let mut points = pad(0., 1., false);
            points.extend(pad(2., 1., cross));
            let pieces: Vec<usize> = (0..points.len()).map(|i| i / 12).collect();
            let (positions, vertices, flags, cosines, dropped) =
                fused_edge_features(&points, crate::park_layout::FUSE_WELD, &pieces).unwrap();
            let walls: Vec<usize> = (0..points.len())
                .filter(|&i| points[i].iter().all(|p| (p[0] - 1.).abs() < 1e-6))
                .collect();
            assert_eq!(walls.len(), 4);
            assert_eq!((0..points.len()).filter(|&i| dropped[i]).collect::<Vec<_>>(), walls, "cross {cross}: the walls go");
            // Every top edge on the seam (both ends at x = 1, y = 0.75) is paired flat: no convex bit, cosine 1.
            let mut seam_edges = 0;
            for t in (0..points.len()).filter(|&i| !dropped[i]) {
                for e in 0..3 {
                    let [a, b] = [vertices[t][e], vertices[t][(e + 1) % 3]].map(|v| positions[v]);
                    if [a, b].iter().all(|p| (p.x - 1.).abs() < 1e-6 && (p.y - 0.75).abs() < 1e-6) {
                        seam_edges += 1;
                        assert_eq!((flags[t] & (0x20 << e), cosines[t][e]), (0, 1.0), "cross {cross}: triangle {t} edge {e}");
                    }
                }
            }
            assert!(seam_edges >= 2, "both tops have a seam edge");
            // The same pads inside ONE piece keep their walls (retail culls only between different pieces).
            let (_, _, _, _, one) = fused_edge_features(&points, crate::park_layout::FUSE_WELD, &vec![0; points.len()]).unwrap();
            assert!(one.iter().all(|d| !d));
        }
    }

    /// b478: the fused set ignores the pieces' own edge codes (keeps their surfaces), drops triangles the merge
    /// collapses (retail 82AC2010) and keeps each piece's range over the kept triangles.
    #[test]
    fn fused_pieces_drop_collapsed_triangles_and_keep_their_ranges() {
        let mut points = plates([0.0015, 0.0, 0.0]);
        // A 4 mm sliver on piece A's far edge collapses under the 1 cm merge.
        points.insert(2, [[0.0, 0.0, 0.0], [0.004, 0.0, 0.0], [0.0, 0.0, 0.004]]);
        let collision: Vec<_> = points
            .iter()
            .map(|&points| skate_data::skate_map::Collision { points, surface: 7, material: 1, native_edges: Some([0x60; 3]) })
            .collect();
        let ranges = [0..3, 3..5];
        let (triangles, surfaces, meshes) =
            portable_parts_fused(&collision, &[], material(), Some(&ranges), true, Some(crate::park_layout::FUSE_WELD)).unwrap();
        assert_eq!(triangles.len(), 4, "the sliver is dropped");
        assert_eq!(surfaces, [7; 4], "the pieces keep their surfaces");
        assert_eq!(meshes.iter().map(|m| m.triangle_range.clone()).collect::<Vec<_>>(), [0..2, 2..4]);
        // Without fusing, the same pieces keep every triangle and their native codes.
        let (kept, _, _) = portable_parts(&collision[..2], &[], material(), None, true).unwrap();
        assert_eq!(kept.len(), 2);
    }
}
