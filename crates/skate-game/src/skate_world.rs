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
    mesh::{
        Indices, MeshTag, MeshVertexAttribute, PrimitiveTopology, VertexAttributeValues,
        VertexFormat,
    },
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

/// The world's normals and tangents as Snorm16x4 and its decal UVs as Float32x2: 56 bytes a
/// vertex instead of the 76 that Bevy's standard normal (Float32x3), tangent and colour
/// (Float32x4) attributes take, so a quarter less vertex data to fetch per vertex and to keep in
/// the heap. The shaders normalize the directions they read, so the packing keeps each one's
/// direction to within 1/65534 a component, and the zero tangent that means "no authored frame"
/// and the ±1 handedness come through exactly (`snorm16x4`). The decal UVs are the two floats the
/// shaders already read from the colour. `SKATE_VERTEX_PACKING=0` at launch keeps the standard
/// attributes, for comparing; `WorldMaterial::specialize` follows whichever the mesh has.
pub(crate) const ATTRIBUTE_PACKED_NORMAL: MeshVertexAttribute =
    MeshVertexAttribute::new("PackedNormal", 0x534B_4E4D, VertexFormat::Snorm16x4);
pub(crate) const ATTRIBUTE_PACKED_TANGENT: MeshVertexAttribute =
    MeshVertexAttribute::new("PackedTangent", 0x534B_544E, VertexFormat::Snorm16x4);
pub(crate) const ATTRIBUTE_DECAL_UV: MeshVertexAttribute =
    MeshVertexAttribute::new("DecalUv", 0x534B_4455, VertexFormat::Float32x2);

/// Whether `merge` packs the vertices (`ATTRIBUTE_PACKED_NORMAL`). Read once, at the first map
/// load.
fn vertex_packing() -> bool {
    static PACKING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PACKING.get_or_init(|| {
        let packing =
            std::env::var("SKATE_VERTEX_PACKING").map_or(true, |value| value.trim() != "0");
        info!("[Skate] world vertex packing: {}", if packing { "on" } else { "off" });
        packing
    })
}

/// Triangles per spatial leaf. The single tuning dial trading draw count against
/// culling granularity (RFC 1 §5): ~1.9M Downtown triangles at this size gives
/// ~120 leaves.
const LEAF_TRIANGLE_BUDGET: usize = 16_384;

/// `SKATE_WORLD_LEAF_BUDGET=<triangles>`: the leaf size to build the world with instead, from 256
/// to 1048576; read once, at the first map load. Smaller leaves let frustum culling, and
/// occlusion culling under `SKATE_OCCLUSION`, leave out more of the world's triangles, for more
/// draws.
fn leaf_budget() -> usize {
    static BUDGET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        let budget = std::env::var("SKATE_WORLD_LEAF_BUDGET")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|budget| (256..=1 << 20).contains(budget))
            .unwrap_or(LEAF_TRIANGLE_BUDGET);
        info!("[Skate] world leaf budget: {budget} triangles");
        budget
    })
}

/// The triangles in one world leaf, counted into the frame statistics for the leaves a camera
/// sees (performance.rs `world_draw_stats`).
#[derive(Component, Clone, Copy)]
pub(crate) struct LeafTriangles(pub u32);

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
    /// Water materials are drawn by their own shader variant, so a leaf holding
    /// water and other materials must split too.
    water: bool,
    /// Under `SKATE_WORLD_SHADOW_SPLIT`, the skater's shadow can fall on this opaque or cutout
    /// triangle's material and not on those of the leaves split from it (retail_render.rs
    /// `MaterialEntry::shadow_group`).
    shadow_group: bool,
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
    crate::log_memory("world texture pages built");

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
            water: entry.water,
            shadow_group: entry.shadow_group,
        });
    }

    // Partition by render class first: classes cannot share a draw (RFC 1 §4),
    // so splitting here keeps every leaf single-class for free.
    let mut stats = SceneStats { slabs: table.slab_count(), triangles: triangles.len(), ..default() };
    let mut buckets: HashMap<(RenderClass, u16, bool, bool), Vec<usize>> = HashMap::new();
    for (index, triangle) in triangles.iter().enumerate() {
        let key = (triangle.class, triangle.slab, triangle.water, triangle.shadow_group);
        buckets.entry(key).or_default().push(index);
    }

    // Draws per render class. The class mix is what decides whether the draw
    // budget holds on a big map: spatial subdivision is a number we choose, but
    // every two-sided or blended material forces an extra bucket we do not.
    let mut per_class = [0usize; RenderClass::ALL.len()];
    let (mut vertices, mut vertex_bytes) = (0usize, 0usize);
    let authored_order = std::env::var("SKATE_WORLD_ORDER").as_deref() != Ok("centroid");
    if !authored_order {
        eprintln!("[Skate] SKATE_WORLD_ORDER=centroid: world leaves keep the partition's order");
    }
    let mut water_leaves = 0usize;
    // `SKATE_VIS_CULL`'s marks: a word for each triangle of the leaves the triangle cull takes,
    // in leaf order. A leaf's start is the low bits of its tag, which the depth prepass and the
    // cull add a triangle's place in the leaf's draw to (retail_depth.wgsl `WORLD_MARK`).
    let marks = crate::render_options::world_marks();
    let mut mark_words = 0usize;
    // `SKATE_CLUSTER_CULL`'s boxes, for each leaf from the marks' word over the run length, so
    // a leaf's marks then start at a multiple of it (bevy_pbr `TriangleCullClusters`).
    let clusters = crate::render_options::world_clusters();
    let run = bevy::pbr::TRIANGLE_CULL_CLUSTER_TRIANGLES as usize;
    let mut cluster_boxes: Vec<u8> = Vec::new();
    let mut fetch = crate::render_options::world_fetch_geometry().then(|| FetchGeometry {
        triangles: Vec::with_capacity(triangles.len()),
    });
    // Diagnostic: `SKATE_FETCH_PARTS=tags` gives the leaves their `MeshTag`s but binds the
    // placeholders, `buffers` the reverse, to tell which of the two disturbs the rest of a frame.
    let parts = std::env::var("SKATE_FETCH_PARTS").ok();
    let (fetch_tags, fetch_copy) = match parts.as_deref().map(str::trim) {
        Some("tags") => (true, false),
        Some("buffers") => (false, true),
        _ => (true, true),
    };
    if fetch.is_some() && parts.is_some() {
        eprintln!("[Skate] SKATE_FETCH_PARTS: tags={fetch_tags} copy={fetch_copy}");
    }
    for ((class, slab, water, shadow_group), mut members) in buckets {
        let leaves = partition(&mut members, &triangles, authored_order, leaf_budget());
        stats.leaves += leaves.len();
        per_class[class as usize] += leaves.len();
        if water {
            water_leaves += leaves.len();
        }
        for leaf in leaves {
            let mesh = merge(map, &table, &triangles, &leaf);
            let aabb = mesh.1;
            let leaf_triangles = mesh.0.indices().map_or(0, |indices| indices.len() / 3);
            vertices += mesh.0.count_vertices();
            vertex_bytes += mesh.0.get_vertex_buffer_size();
            let bundle = (
                Name::new(format!(
                    "world {class:?} slab {slab}{}{}",
                    if water { " water" } else { "" },
                    if shadow_group { " shadowed" } else { "" }
                )),
                Mesh3d(meshes.add(mesh.0)),
                MeshMaterial3d(table.material(slab, class, water, shadow_group)),
                // The world shaders read positions, normals and tangents as world
                // space and never load this transform (retail_world.wgsl), so a
                // leaf must stay at the identity.
                Transform::default(),
                // Precomputed so Bevy's CalculateBounds never walks this mesh
                // (RFC 1 D4). The extents were already computed while merging.
                aabb,
                LeafTriangles(leaf.len() as u32),
            );
            match &mut fetch {
                Some(fetch) => {
                    let first = fetch.push_leaf(map, &table, &triangles, &leaf);
                    if fetch_tags {
                        commands.spawn((bundle, MeshTag(first)))
                    } else {
                        commands.spawn(bundle)
                    }
                }
                None => {
                    if clusters {
                        mark_words = mark_words.next_multiple_of(run);
                        // The boxes run on from the last leaf's, as its words do. Past the tag's
                        // bits a leaf has none, and the prepass draws it whole.
                        if mark_words + leaf_triangles
                            <= bevy::pbr::TRIANGLE_CULL_MARK_BASE as usize
                        {
                            debug_assert_eq!(
                                cluster_boxes.len(),
                                mark_words / run * CLUSTER_BOX_BYTES
                            );
                            push_cluster_boxes(&mut cluster_boxes, map, &triangles, &leaf);
                        }
                    }
                    // Past the tag's bits, a leaf's words start out of the marks' range, and the
                    // cull keeps its triangles.
                    let mark_base = if marks {
                        mark_words.min(bevy::pbr::TRIANGLE_CULL_MARK_BASE as usize) as u32
                    } else {
                        0
                    };
                    mark_words += leaf_triangles;
                    commands.spawn((
                        bundle,
                        // Its triangles may be culled one by one for the main pass
                        // (`SKATE_TRIANGLE_CULL`, render_options.rs).
                        MeshTag(
                            bevy::pbr::TRIANGLE_CULL_LEAF
                                | if class.two_sided() {
                                    bevy::pbr::TRIANGLE_CULL_TWO_SIDED
                                } else {
                                    0
                                }
                                | mark_base,
                        ),
                    ))
                }
            }
            stats.draws += 1;
        }
    }

    crate::log_memory(&format!(
        "world meshes merged ({vertices} vertices, {:.1} MiB of vertex data)",
        vertex_bytes as f64 / (1024.0 * 1024.0)
    ));
    // Every material binds the copy, so a map without one still gets a placeholder of a record
    // each, which nothing reads.
    let (fetch_vertices, fetch_triangles) = match fetch {
        Some(fetch) if fetch_copy => {
            let geometry = (FetchGeometry::vertex_bytes(map), fetch.triangle_bytes());
            crate::log_memory(&format!(
                "world fetch geometry built ({} vertices, {} triangles, {:.1} MiB)",
                map.geometry.vertices.len(),
                fetch.triangles.len(),
                (geometry.0.len() + geometry.1.len()) as f64 / (1024.0 * 1024.0)
            ));
            geometry
        }
        _ => (vec![0; FetchGeometry::VERTEX_BYTES], vec![0; FetchGeometry::TRIANGLE_BYTES]),
    };
    let storage = |bytes: Vec<u8>| bevy::render::storage::ShaderStorageBuffer {
        data: Some(bytes),
        asset_usage: RenderAssetUsages::RENDER_WORLD,
        ..default()
    };
    buffers.insert(&table.fetch_buffers().vertices, storage(fetch_vertices));
    buffers.insert(&table.fetch_buffers().triangles, storage(fetch_triangles));
    // The marks start at zero (wgpu clears a new buffer), and the cull clears each mark it reads.
    // Every material binds them, so a map without them still gets a word, which nothing reads.
    let words = if marks {
        mark_words.min(bevy::pbr::TRIANGLE_CULL_MARK_BASE as usize).max(1)
    } else {
        1
    };
    buffers.insert(
        table.marks(),
        bevy::render::storage::ShaderStorageBuffer::with_size(
            words * 4,
            RenderAssetUsages::RENDER_WORLD,
        ),
    );
    if marks {
        crate::log_memory(&format!(
            "world marks built ({mark_words} triangles, {:.1} MiB)",
            (words * 4) as f64 / (1024.0 * 1024.0)
        ));
        commands.insert_resource(bevy::pbr::TriangleCullMarks(table.marks().id()));
    }
    if clusters && !cluster_boxes.is_empty() {
        let boxes = cluster_boxes.len() / CLUSTER_BOX_BYTES;
        let bytes = cluster_boxes.len();
        let handle = buffers.add(storage(cluster_boxes));
        crate::log_memory(&format!(
            "world cluster boxes built ({boxes} boxes, {:.1} MiB)",
            bytes as f64 / (1024.0 * 1024.0)
        ));
        commands.insert_resource(bevy::pbr::TriangleCullClusters(handle.id()));
        // The staged handle is dropped once the buffer is published, and nothing else binds it.
        commands.spawn((Name::new("world cluster boxes"), WorldClusterBoxes { _boxes: handle }));
    }
    spawn_lights(map, commands);
    eprintln!(
        "SKATE_RENDER_READY draws={} triangles={} slabs={} materials={} \
         opaque={} opaque_two_sided={} cutout={} cutout_two_sided={} \
         blended={} blended_two_sided={} water={}",
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
        water_leaves,
    );
    stats
}

/// Median-split over triangle centroids until every leaf fits the budget.
///
/// Sorting by the longest axis of the current extent keeps leaves roughly cubic,
/// which matters because leaf AABBs are what frustum culling tests.
///
/// Each leaf is returned in authored order. The sorts leave a split leaf ordered
/// by centroid along one axis, which scatters every vertex's triangles across
/// the leaf, so the GPU shades each vertex again in batch after batch: 2.55
/// vertices per triangle by the Switch's own counters, 2.58 in a model of
/// University. In the model the retail exporter's own order was already
/// optimised for vertex reuse (1.38, against 1.36 for Forsyth's algorithm and a
/// floor of 1.28 unique vertices per triangle), so restoring it costs one
/// integer sort per leaf (findings doc 20). `authored_order` is false only
/// under `SKATE_WORLD_ORDER=centroid`, kept to measure the difference.
fn partition(
    members: &mut Vec<usize>,
    triangles: &[Triangle],
    authored_order: bool,
    budget: usize,
) -> Vec<Vec<usize>> {
    let mut pending = vec![std::mem::take(members)];
    let mut leaves = Vec::new();
    while let Some(mut group) = pending.pop() {
        if group.len() <= budget {
            if authored_order {
                group.sort_unstable();
            }
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

    // The capacities above guess two vertices per triangle; a leaf that needs
    // more doubles every list, and the slack would ride along to the renderer.
    positions.shrink_to_fit();
    normals.shrink_to_fit();
    uv.shrink_to_fit();
    lightmap_uv.shrink_to_fit();
    decal_uv.shrink_to_fit();
    material_index.shrink_to_fit();
    tangents.shrink_to_fit();
    // Nothing reads these vertices back on the CPU, and the AABB is computed
    // here. On the Switch the GPU's memory is the game's own heap (bevy-horizon
    // docs/platform-notes.md), so a main-world copy beside the GPU one would
    // hold the world's geometry in the heap twice, and three times while Bevy
    // uploads it, as extraction clones a mesh it keeps. `RENDER_WORLD` moves
    // the vertices to the renderer instead, which frees them once they are on
    // the GPU (bevy_render `extract_render_asset`).
    let usage = if cfg!(target_os = "horizon") {
        RenderAssetUsages::RENDER_WORLD
    } else {
        RenderAssetUsages::default()
    };
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, lightmap_uv)
        .with_inserted_attribute(ATTRIBUTE_MATERIAL_INDEX, material_index);
    if vertex_packing() {
        let normals = normals.iter().map(|n| snorm16x4(*n, 0.)).collect();
        let tangents = tangents.iter().map(|t| snorm16x4([t[0], t[1], t[2]], t[3])).collect();
        let decal_uv: Vec<[f32; 2]> = decal_uv.iter().map(|c| [c[0], c[1]]).collect();
        mesh.insert_attribute(ATTRIBUTE_PACKED_NORMAL, VertexAttributeValues::Snorm16x4(normals));
        mesh.insert_attribute(ATTRIBUTE_PACKED_TANGENT, VertexAttributeValues::Snorm16x4(tangents));
        mesh.insert_attribute(ATTRIBUTE_DECAL_UV, decal_uv);
    } else {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, decal_uv);
    }
    mesh.insert_indices(Indices::U32(indices));
    let aabb = Aabb::from_min_max(min, max);
    (mesh, aabb)
}

/// Bytes in one of `SKATE_CLUSTER_CULL`'s boxes: its world-space minimum and maximum, each
/// padded to four floats (`ClusterBox` in bevy_pbr's triangle_cull_clusters.wgsl).
const CLUSTER_BOX_BYTES: usize = 32;

/// Holds `SKATE_CLUSTER_CULL`'s boxes for as long as the map: the render world finds them by
/// `TriangleCullClusters`' id alone, and the staged handle is dropped once it is published.
#[derive(Component)]
struct WorldClusterBoxes {
    _boxes: Handle<bevy::render::storage::ShaderStorageBuffer>,
}

/// Appends a box for each run of `TRIANGLE_CULL_CLUSTER_TRIANGLES` of a leaf's triangles, in the
/// order `merge` draws them. Each box is taken from the very positions the leaf's vertices
/// carry, so it holds its triangles exactly.
fn push_cluster_boxes(boxes: &mut Vec<u8>, map: &SkateMap, triangles: &[Triangle], leaf: &[usize]) {
    for run in leaf.chunks(bevy::pbr::TRIANGLE_CULL_CLUSTER_TRIANGLES as usize) {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &triangle in run {
            for source in triangles[triangle].indices {
                let position = Vec3::from_array(map.geometry.vertices[source as usize].position);
                min = min.min(position);
                max = max.max(position);
            }
        }
        for value in [min.x, min.y, min.z, 0., max.x, max.y, max.z, 0.] {
            boxes.extend_from_slice(&f32::to_le_bytes(value));
        }
    }
}

/// A direction and a fourth component as Snorm16, which the GPU reads back as
/// `max(c / 32767, -1)` (Vulkan's SNORM conversion). Both shaders normalize every direction
/// they read, so only the direction is kept: it is scaled until its largest component is ±1.
/// That spends all 16 bits on it, leaves no component for the clamp to turn, and keeps a very
/// short tangent (a binormal nearly along its normal) from rounding to a zero `xyz` beside a
/// nonzero `w`, which the shader would normalize into NaN. A zero direction stays zero, and a
/// `w` of 0 or ±1 comes through exactly.
fn snorm16x4(xyz: [f32; 3], w: f32) -> [i16; 4] {
    let largest = xyz.iter().fold(0_f32, |largest, c| largest.max(c.abs()));
    let snorm = |c: f32| (c.clamp(-1., 1.) * 32767.).round() as i16;
    let direction = |c: f32| snorm(if largest > 0. { c / largest } else { 0. });
    [direction(xyz[0]), direction(xyz[1]), direction(xyz[2]), snorm(w)]
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

/// `SKATE_WORLD_FETCH`'s copy of the world (retail_render.rs `FetchBuffers`), which
/// retail_world.wgsl `fetch_triangle` reads in place of the vertex stage's outputs. Each map
/// vertex is four `vec4<u32>`: its position's bits and a spare word; its UV and lightmap UV; its
/// decal UV and its normal; and its tangent and two spare words. The normal and tangent are
/// `snorm16x4` as the packed meshes carry them, two components a word, the first in the low
/// half, which is how WGSL's `unpack2x16snorm` reads them. Each drawn triangle is its corners'
/// indices into those and its material's place in its slab, in the order its leaf draws it. A
/// leaf's `MeshTag` is where its triangles start, and the fragment adds `primitive_index`, which
/// counts a draw's triangles from zero.
struct FetchGeometry {
    triangles: Vec<[u32; 4]>,
}

impl FetchGeometry {
    const VERTEX_BYTES: usize = 64;
    const TRIANGLE_BYTES: usize = 16;

    /// Appends a leaf's triangles in `merge`'s order and returns where they start.
    fn push_leaf(
        &mut self,
        map: &SkateMap,
        table: &MaterialTable,
        triangles: &[Triangle],
        leaf: &[usize],
    ) -> u32 {
        let first = self.triangles.len() as u32;
        for &triangle in leaf {
            let [a, b, c] = triangles[triangle].indices;
            // The material of the first corner, the one flat interpolation takes (Vulkan's
            // provoking vertex), numbered as `merge` numbers it.
            let source = map.geometry.vertices[a as usize].material.saturating_sub(1) as usize;
            self.triangles.push([a, b, c, table.slab_index(source)]);
        }
        first
    }

    fn triangle_bytes(&self) -> Vec<u8> {
        self.triangles.iter().flatten().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn vertex_bytes(map: &SkateMap) -> Vec<u8> {
        let pair = |low: i16, high: i16| u32::from(low as u16) | (u32::from(high as u16) << 16);
        let mut bytes = Vec::with_capacity(map.geometry.vertices.len() * Self::VERTEX_BYTES);
        for v in &map.geometry.vertices {
            let normal = snorm16x4(v.normal, 0.);
            let t = tangent(v);
            let tangent = snorm16x4([t[0], t[1], t[2]], t[3]);
            let [du, dv] = v.decal_uv.unwrap_or_default();
            let words: [u32; 16] = [
                v.position[0].to_bits(),
                v.position[1].to_bits(),
                v.position[2].to_bits(),
                0,
                v.uv[0].to_bits(),
                v.uv[1].to_bits(),
                v.lightmap_uv[0].to_bits(),
                v.lightmap_uv[1].to_bits(),
                du.to_bits(),
                dv.to_bits(),
                pair(normal[0], normal[1]),
                pair(normal[2], normal[3]),
                pair(tangent[0], tangent[1]),
                pair(tangent[2], tangent[3]),
                0,
                0,
            ];
            for word in words {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        bytes
    }
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
    // Reserved once from the mesh headers: pushing Downtown's 1.5 million 128-byte
    // triangles one at a time doubles the list to 256 MiB while the 128 MiB copy is
    // still alive (findings 58).
    let capacity = skate_data::retail_collision::triangle_capacity(archive)?;
    let mut triangles = Vec::new();
    let mut packed_surfaces = Vec::new();
    triangles.try_reserve_exact(capacity)
        .and_then(|()| packed_surfaces.try_reserve_exact(capacity))
        .map_err(|_| format!("RWCM collision of {capacity} triangles cannot be reserved"))?;
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
    // Match the reference RW mesh compiler's 1 mm vertex welding and reversed
    // edge pairing. Triangle diagonals are adjacency, never authored ledges.
    let mut welded = HashMap::<[i64; 3], usize>::new();
    let mut positions = Vec::<Vec3>::new();
    let mut vertices = Vec::with_capacity(map.geometry.collision.len());
    let mut normals = Vec::with_capacity(map.geometry.collision.len());
    for tri in &map.geometry.collision {
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
    if map
        .geometry
        .collision
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
    for (i, source) in map.geometry.collision.iter().enumerate() {
        if let Some(edges) = source.native_edges {
            (flags[i], cosines[i]) = decode_native_edges(edges)?;
        }
        let m = &map.materials[source.material as usize - 1];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snorm16x4_keeps_zero_handedness_and_direction() {
        // The zero tangent is the shader's "no authored frame".
        assert_eq!(snorm16x4([0., 0., 0.], 0.), [0; 4]);
        assert_eq!(snorm16x4([1., -1., 0.], -1.), [32767, -32767, 0, -32767]);
        // A component past ±1 scales the direction down whole rather than clamping one axis.
        assert_eq!(snorm16x4([2., 1., 0.], 1.), [32767, 16384, 0, 32767]);
        // A very short direction is scaled up rather than rounded to zero.
        assert_eq!(snorm16x4([1e-6, 0., -1e-6], 1.), [32767, 0, -32767, 32767]);
        let direction = Vec3::new(0.3, -0.7, 0.648_074);
        let [x, y, z, _] = snorm16x4(direction.to_array(), 0.);
        let unpacked = Vec3::new(x.into(), y.into(), z.into()) / 32767.;
        let expected = direction / 0.7;
        assert!((unpacked - expected).abs().max_element() <= 0.5 / 32767.);
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
        // The package keeps its texture payloads compressed; the table decompresses each
        // one per page layer (findings 58 §3.6).
        let map = skate_data::skate_map::SkateMap::load(&path).unwrap();
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<crate::retail_render::WorldMaterial>>();
        world.init_resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>();
        world.init_resource::<Assets<crate::retail_sky::SkyMaterial>>();
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
        Triangle {
            indices: [0, 1, 2],
            centroid,
            slab: 0,
            class: RenderClass::Opaque,
            water: false,
            shadow_group: false,
        }
    }

    #[test]
    fn partition_respects_the_leaf_budget() {
        let triangles: Vec<Triangle> = (0..LEAF_TRIANGLE_BUDGET * 3 + 7)
            .map(|i| triangle(Vec3::new(i as f32, 0., 0.)))
            .collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        let leaves = partition(&mut members, &triangles, true, LEAF_TRIANGLE_BUDGET);
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
        for authored_order in [true, false] {
            let mut members: Vec<usize> = (0..triangles.len()).collect();
            let mut seen: Vec<usize> =
                partition(&mut members, &triangles, authored_order, LEAF_TRIANGLE_BUDGET)
                    .into_iter()
                    .flatten()
                    .collect();
            seen.sort_unstable();
            assert_eq!(seen, (0..triangles.len()).collect::<Vec<_>>());
        }
    }

    #[test]
    fn split_leaves_return_to_authored_order() {
        // Centroids descend while indices ascend, so every split reverses the
        // order within its halves unless the leaf is put back.
        let triangles: Vec<Triangle> = (0..LEAF_TRIANGLE_BUDGET * 2 + 1)
            .map(|i| triangle(Vec3::new(-(i as f32), 0., 0.)))
            .collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        let leaves = partition(&mut members, &triangles, true, LEAF_TRIANGLE_BUDGET);
        assert!(leaves.len() >= 2);
        for leaf in &leaves {
            assert!(leaf.windows(2).all(|w| w[0] < w[1]), "leaf is not in authored order");
        }
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        let sorted = partition(&mut members, &triangles, false, LEAF_TRIANGLE_BUDGET);
        assert!(sorted.iter().any(|leaf| leaf.windows(2).any(|w| w[0] > w[1])));
    }

    #[test]
    fn a_single_leaf_is_not_split() {
        let triangles: Vec<Triangle> = (0..64).map(|i| triangle(Vec3::splat(i as f32))).collect();
        let mut members: Vec<usize> = (0..triangles.len()).collect();
        assert_eq!(partition(&mut members, &triangles, true, LEAF_TRIANGLE_BUDGET).len(), 1);
    }
}
