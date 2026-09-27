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
    let table = MaterialTable::build(map, tuning, environment, materials, images, buffers);

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
            ));
            stats.draws += 1;
        }
    }

    spawn_lights(map, commands);
    eprintln!(
        "SKATE_RENDER_READY draws={} triangles={} slabs={} materials={} \
         opaque={} opaque_two_sided={} cutout={} cutout_two_sided={} \
         blended={} blended_two_sided={}",
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
    let path = asset_root
        .join("private")
        .join("native-props")
        .join(format!("{map_name}.skate"));
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
) -> usize {
    let _span = info_span!("spawn_prop_instances").entered();
    let table = MaterialTable::build(map, tuning, environment, materials, images, buffers);
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
        let children: Vec<_> = parts
            .iter()
            .map(|(class, slab, mesh, aabb)| {
                (
                    Name::new(format!("{} {class:?} slab {slab}", object.name)),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(table.material(*slab, *class)),
                    Transform::default(),
                    *aabb,
                )
            })
            .collect();
        draws += children.len();
        commands.spawn_with_children(
            (
                Name::new(object.name.clone()),
                PropInstance {
                    id: object.id,
                    template: template.to_string(),
                    name: name.to_string(),
                },
                Transform::from_matrix(prop_affine(&object.transform)),
                Visibility::default(),
            ),
            children,
        );
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
        if extension.tag == *b"MOBJ" {
            skate_data::skate_map::validate_static_objects(map, extension)?;
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
    if let Some(archive) = retail_archive(map)? {
        return retail_collision_world(archive, material);
    }
    portable_world(&map.geometry.collision, &map.materials, material)
}

/// Portable triangle world: 1 mm vertex welding, reconstructed adjacency and
/// contiguous-range broadphase metadata. Shared by the playable map and by
/// static prop instances, which supply already-placed world-space triangles.
fn portable_world(
    collision: &[skate_data::skate_map::Collision],
    materials: &[skate_data::skate_map::Material],
    material: RetailContactMaterial,
) -> Result<BoardWorld, String> {
    // Match the reference RW mesh compiler's 1 mm vertex welding and reversed
    // edge pairing. Triangle diagonals are adjacency, never authored ledges.
    let mut welded = HashMap::<[i64; 3], usize>::new();
    let mut positions = Vec::<Vec3>::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    for tri in collision {
        let ids = tri.points.map(|p| {
            let inverse = 1.0 / f64::from(0.001_f32);
            let key = p.map(|v| (f64::from(v) * inverse).round() as i64);
            *welded.entry(key).or_insert_with(|| {
                let id = positions.len();
                positions.push(Vec3::from_array(p));
                id
            })
        });
        let [a, b, c] = tri.points.map(Vec3::from_array);
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
    if collision
        .iter()
        .any(|t| t.native_edges.is_none())
    {
        let mut open = HashMap::<(usize, usize), (usize, usize)>::new();
        for (i, ids) in vertices.iter().enumerate() {
            for edge in 0..3 {
                let (a, b) = (ids[edge], ids[(edge + 1) % 3]);
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
    let mut triangles = Vec::with_capacity(vertices.len());
    let mut packed_surfaces = Vec::with_capacity(vertices.len());
    for (i, source) in collision.iter().enumerate() {
        if let Some(edges) = source.native_edges {
            (flags[i], cosines[i]) = decode_native_edges(edges)?;
        }
        let m = &materials[source.material as usize - 1];
        // Exact EncodeRwSurfaceId mapping from the reference native adapter.
        packed_surfaces.push((m.audio | (m.physics << 7) | (m.pattern << 12)) as u16);
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
    for start in (0..triangles.len()).step_by(64) {
        let end = (start + 64).min(triangles.len());
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
    let metadata = QueryMetadata {
        packed_surfaces,
        meshes,
        static_edges: vec![],
        island_flags: 0,
    };
    BoardWorld::with_query_metadata(triangles, metadata).map_err(str::to_owned)
}

/// Static collision for spawned DMO prop instances. No authored DMO collision
/// mesh is recovered, so each instance reuses its template's render triangles,
/// baked into world space with the instance transform at load. Reflections
/// flip winding to keep outward normals; degenerate render triangles are
/// skipped rather than rejecting the whole layer. Props stay out of the map
/// collision world so Phase 2 can rebuild moved instances independently.
pub(crate) fn prop_collision_world(
    map: &SkateMap,
    objects: &[skate_data::skate_map::StaticObject],
    material: RetailContactMaterial,
) -> Result<Option<BoardWorld>, String> {
    let mut collision = Vec::new();
    for object in objects {
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
        let range =
            object.first_index as usize..(object.first_index + object.index_count) as usize;
        for tri in map.geometry.indices[range].chunks_exact(3) {
            let mut points =
                [tri[0], tri[1], tri[2]].map(|i| transform(map.geometry.vertices[i as usize].position));
            if determinant < 0. {
                points.swap(1, 2);
            }
            let [a, b, c] = points.map(Vec3::from_array);
            let cross = (b - a).cross(c - a);
            if cross.length_squared() <= 0. {
                continue;
            }
            let source = &map.materials
                [map.geometry.vertices[tri[0] as usize].material as usize - 1];
            // Wheels read the packed surface nibble from the triangle tag; use
            // the same EncodeRwSurfaceId mapping as static map collision.
            let surface = source.audio | (source.physics << 7) | (source.pattern << 12);
            collision.push(skate_data::skate_map::Collision {
                points,
                surface,
                material: map.geometry.vertices[tri[0] as usize].material,
                native_edges: None,
            });
        }
    }
    if collision.is_empty() {
        return Ok(None);
    }
    Ok(Some(portable_world(&collision, &map.materials, material)?))
}

/// Load the district's prop package and build its static collision layer.
/// The package is a presentation supplement: missing or invalid files leave
/// props uncollidable rather than failing the map, matching the render path.
pub(crate) fn load_prop_collision(
    asset_root: &std::path::Path,
    map_name: &str,
    material: RetailContactMaterial,
) -> Option<BoardWorld> {
    let path = asset_root
        .join("private")
        .join("native-props")
        .join(format!("{map_name}.skate"));
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
    match prop_collision_world(&map, &objects, material) {
        Ok(world) => {
            if let Some(world) = &world {
                info!(
                    "SKATE_PROP_COLLISION: {map_name} instances={} triangles={}",
                    objects.len(),
                    world.triangles().len()
                );
            }
            world
        }
        Err(error) => {
            warn!("SKATE_PROP_COLLISION: {}: {error}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut world = prop_collision_world(&map, &objects, material()).unwrap().unwrap();
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
            body: skate_core::physics::board_step::CollisionBody::Board(
                skate_core::physics::board::BodyId::Deck,
            ),
            primitive: skate_core::physics::world_contact::ContactPrimitive::Sphere(
                skate_core::physics::collision::Sphere {
                    center: Vector3::new(10., 6.05, 0.),
                    radius: 0.1,
                },
            ),
            linear_velocity: Vector3::ZERO,
            material: material(),
        }];
        let contacts = world.query_primitives(&volumes, query, retention);
        assert!(!contacts.is_empty());
        assert!(contacts.iter().all(|c| c.contact.normal.y > 0.9));
    }
}
