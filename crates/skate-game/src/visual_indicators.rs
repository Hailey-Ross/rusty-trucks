//! Challenge visual indicators: the floating ribbon icon over each armed teleporter (research b532 / b533).
//! Data, placement and what is not retail yet: docs/hails-additions/38-dlc-support.md#teleporter-markers
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Our ribbon look until retail's draw code is read (b533 open items): width in metres, colour, samples per segment.
const WIDTH: f32 = 0.06;
const COLOR: [f32; 3] = [1.0, 0.85, 0.2];
const SAMPLES: usize = 6;
/// Retail draws an indicator only within 400 m of the query point (`82D4F8A8`, constant 0x820C6AF0).
pub(crate) const DRAW_DISTANCE: f32 = 400.0;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Spline {
    /// Power-basis cubics, rows c3, c2, c1, c0 (setup visual_indicators.py).
    pub segments: Vec<[[f32; 3]; 4]>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Indicator {
    /// In the anchor's frame (rows right, up, forward), b533 `827AB790`.
    pub offset: [f32; 3],
    #[serde(default)]
    pub spline: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Deserialize)]
struct File {
    format: String,
    version: u32,
    splines: HashMap<String, Spline>,
    indicators: HashMap<String, Indicator>,
}

/// A teleporter's indicator: the `ribbon_indicator` key and its anchor (the challenge's locator matrix).
#[derive(Clone, Debug, PartialEq, serde::Serialize, Deserialize)]
pub(crate) struct Placement {
    pub key: String,
    #[serde(default)]
    pub locator: String,
    pub matrix: [[f32; 4]; 4],
}

#[derive(Resource, Default)]
pub(crate) struct VisualIndicators {
    loaded: bool,
    splines: HashMap<String, Spline>,
    indicators: HashMap<String, Indicator>,
    meshes: HashMap<String, Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    shown: Vec<(String, [[f32; 4]; 4])>,
}

#[derive(Component)]
pub(crate) struct IndicatorMarker;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<VisualIndicators>().add_systems(Update, (sync, face_camera).chain());
}

fn load(assets: &Path) -> Result<File, String> {
    let path = assets.join("private/visual-indicators.json");
    let file: File = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if file.format != "skate3rust.visual-indicators" || file.version != 1 {
        return Err(format!("{}: unsupported {} v{}", path.display(), file.format, file.version));
    }
    Ok(file)
}

/// The armed teleporters' indicators, one per anchor locator (retail keeps the first entry per locator, b533).
pub(crate) fn wanted(placements: &[&Placement]) -> Vec<(String, [[f32; 4]; 4])> {
    let mut out: Vec<(String, [[f32; 4]; 4])> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in placements {
        if !p.locator.is_empty() && !seen.insert(p.locator.clone()) {
            continue;
        }
        out.push((p.key.clone(), p.matrix));
    }
    out
}

/// The icon's world position: anchor + offset in the anchor's frame (b533 `827AB790`).
pub(crate) fn position(matrix: &[[f32; 4]; 4], offset: [f32; 3]) -> Vec3 {
    let row = |i: usize| Vec3::new(matrix[i][0], matrix[i][1], matrix[i][2]);
    row(3) + row(0) * offset[0] + row(1) * offset[1] + row(2) * offset[2]
}

/// The spline as a flat ribbon in its own x / y plane (z = 0), double-sided through the material.
pub(crate) fn ribbon(spline: &Spline, width: f32) -> Mesh {
    let mut points: Vec<Vec2> = Vec::new();
    for [c3, c2, c1, c0] in &spline.segments {
        for i in 0..=SAMPLES {
            let t = i as f32 / SAMPLES as f32;
            let p = |k: usize| c3[k] * t * t * t + c2[k] * t * t + c1[k] * t + c0[k];
            let point = Vec2::new(p(0), p(1));
            if points.last().is_none_or(|last| last.distance(point) > 1e-4) {
                points.push(point);
            }
        }
    }
    let (mut positions, mut indices) = (Vec::new(), Vec::new());
    for (i, &p) in points.iter().enumerate() {
        let before = points[i.saturating_sub(1)];
        let after = points[(i + 1).min(points.len() - 1)];
        let d = (after - before).normalize_or_zero();
        let n = Vec2::new(-d.y, d.x) * width * 0.5;
        positions.push([p.x + n.x, p.y + n.y, 0.0]);
        positions.push([p.x - n.x, p.y - n.y, 0.0]);
        if i > 0 {
            let b = (i as u32) * 2;
            indices.extend_from_slice(&[b - 2, b - 1, b, b - 1, b + 1, b]);
        }
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(bevy::mesh::Indices::U32(indices))
}

/// Respawn the icons when the armed set changes (map switch, layout, mods adding or removing teleporters).
fn sync(
    mut commands: Commands,
    mut state: ResMut<VisualIndicators>,
    teleporters: Res<crate::teleporters::Teleporters>,
    map: Res<crate::map_transition::CurrentMap>,
    config: Res<crate::config::Config>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    markers: Query<Entity, With<IndicatorMarker>>,
) {
    if !state.loaded {
        state.loaded = true;
        match load(&config.asset_root) {
            Ok(file) => {
                state.splines = file.splines;
                state.indicators = file.indicators;
            }
            Err(e) => info!("SKATE_VISUAL_INDICATORS none ({e})"),
        }
    }
    let armed = teleporters.armed(map.path.as_deref());
    let wanted = wanted(&armed.iter().filter_map(|t| t.indicator.as_ref()).collect::<Vec<_>>());
    if wanted == state.shown {
        return;
    }
    for e in &markers {
        commands.entity(e).despawn();
    }
    let material = state.material.get_or_insert_with(|| materials.add(StandardMaterial {
        base_color: Color::srgb(COLOR[0], COLOR[1], COLOR[2]),
        unlit: true,
        cull_mode: None,
        ..default()
    })).clone();
    let mut spawned = 0;
    for (key, matrix) in &wanted {
        let Some(indicator) = state.indicators.get(key).filter(|i| i.enabled).cloned() else { continue };
        let Some(spline_id) = indicator.spline.clone() else { continue };
        let mesh = match state.meshes.get(&spline_id) {
            Some(mesh) => mesh.clone(),
            None => {
                let Some(spline) = state.splines.get(&spline_id) else { continue };
                let mesh = meshes.add(ribbon(spline, WIDTH));
                state.meshes.insert(spline_id.clone(), mesh.clone());
                mesh
            }
        };
        commands.spawn((IndicatorMarker, Mesh3d(mesh), MeshMaterial3d(material.clone()),
            Transform::from_translation(position(matrix, indicator.offset)), Visibility::Visible));
        spawned += 1;
    }
    info!("SKATE_VISUAL_INDICATORS map={} shown={spawned}", map.name);
    state.shown = wanted;
}

/// The draw distance (retail), and ours until the draw code is read (b533): the flat icon turns about the vertical to
/// face the camera.
fn face_camera(
    cameras: Query<&GlobalTransform, With<crate::camera::GameplayCamera>>,
    mut markers: Query<(&mut Transform, &mut Visibility), With<IndicatorMarker>>,
) {
    let Some(camera) = cameras.iter().next().map(|c| c.translation()) else { return };
    for (mut t, mut visibility) in &mut markers {
        let to = camera - t.translation;
        visibility.set_if_neq(if to.length_squared() < DRAW_DISTANCE * DRAW_DISTANCE { Visibility::Visible } else { Visibility::Hidden });
        if to.x.abs() + to.z.abs() > 1e-3 {
            t.rotation = Quat::from_rotation_y(to.x.atan2(to.z));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(locator: &str, x: f32) -> Placement {
        let mut matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        matrix[3] = [x, 0.0, 0.0, 1.0];
        Placement { key: "teleport".into(), locator: locator.into(), matrix }
    }

    #[test]
    fn one_icon_per_locator() {
        let (a, b, c) = (placement("loc_a", 1.0), placement("loc_a", 2.0), placement("loc_b", 3.0));
        let wanted = wanted(&[&a, &b, &c]);
        assert_eq!(wanted.len(), 2);
        assert_eq!(wanted[0].1[3][0], 1.0);
    }

    #[test]
    fn offset_is_in_the_anchor_frame() {
        // Rows right, up, forward: an anchor lying on its side moves the (0, 1, 0) offset along world x.
        let matrix = [[0.0, -1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [5.0, 2.0, 3.0, 1.0]];
        assert_eq!(position(&matrix, [0.0, 1.0, 0.0]), Vec3::new(6.0, 2.0, 3.0));
    }

    #[test]
    fn ribbon_follows_the_spline() {
        // One straight segment from (0, 0) to (0, 1): c1 = (0, 1), c0 = 0.
        let spline = Spline { segments: vec![[[0.0; 3], [0.0; 3], [0.0, 1.0, 0.0], [0.0; 3]]] };
        let mesh = ribbon(&spline, 0.1);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert_eq!(p.len(), (SAMPLES + 1) * 2);
        assert!((p[0][0].abs() - 0.05).abs() < 1e-6 && p.last().unwrap()[1] == 1.0);
    }
}
