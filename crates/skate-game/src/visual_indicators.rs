//! Challenge visual indicators: the floating ribbon icon over each armed teleporter (research b532 / b533).
//! Data, placement and what is not retail yet: docs/hails-additions/38-dlc-support.md#teleporter-markers
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// NOT RETAIL: retail's colour / texture come from a shader or texture not read yet (b534 section 3).
const COLOR: [f32; 3] = [1.0, 0.85, 0.2];
/// Retail caps the samples per spline segment at 10 (`827B0BA8`, b534).
const MAX_SAMPLES: u16 = 10;
/// Retail draws an indicator only within 400 m of the query point (`82D4F8A8`, constant 0x820C6AF0).
pub(crate) const DRAW_DISTANCE: f32 = 400.0;

/// A `visualindicators` row as the draw reads it (setup visual_indicators.py `styles`, research b534).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Style {
    /// Width terms A, B: full ribbon width 0.5 * (A + B).
    pub width: [f32; 2],
    pub samples: u16,
    /// Three crossed strips (local z, y, x) instead of one diagonal strip.
    pub three_strips: bool,
    /// Fade graph x points: fade in from [0] to [1], out from [2] to [3], hidden beyond [3].
    pub fade: [f32; 4],
    /// Shader `i_intensity` (b536): front pass brightness, background pass opacity.
    #[serde(default = "retail_intensity")]
    pub intensity: [f32; 2],
}

fn retail_intensity() -> [f32; 2] {
    [1.0, 0.7]
}

impl Default for Style {
    /// Retail's `default` row, for installs that predate the style export.
    fn default() -> Self {
        Self { width: [0.4, 0.3], samples: 10, three_strips: true, fade: [0.0, 8.306_189, 94.299_67, 100.0], intensity: retail_intensity() }
    }
}

impl Style {
    /// Opacity at `distance` from the camera (`827B1B70` fade window, linear ramps assumed, b534); 0 = hidden.
    pub(crate) fn alpha(&self, distance: f32) -> f32 {
        let [x0, x1, x2, x3] = self.fade;
        if distance > x3 {
            return 0.0;
        }
        let ramp = |from: f32, to: f32| if to > from { ((distance - from) / (to - from)).clamp(0.0, 1.0) } else if distance >= from { 1.0 } else { 0.0 };
        ramp(x0, x1).min(1.0 - if x3 > x2 { ramp(x2, x3) } else { 0.0 })
    }
}

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
    #[serde(default)]
    pub style: Option<String>,
}

#[derive(Deserialize)]
struct File {
    format: String,
    version: u32,
    splines: HashMap<String, Spline>,
    indicators: HashMap<String, Indicator>,
    #[serde(default)]
    styles: HashMap<String, Style>,
    #[serde(default)]
    colours: HashMap<String, Colour>,
    /// Folder under `private/` holding the textures as PNGs.
    #[serde(default)]
    textures: Option<String>,
}

/// A `ribbon_spline_colour` row (b535 `827AEF08`): the front and background pass textures. Its tint (E4F3) is not an
/// input of the ribbon's pixel shaders (b536).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Colour {
    pub front: String,
    pub background: String,
}

/// One draw pass of an icon (b536 `82CE9080`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pass {
    /// `spline_darkenPS`, alpha blended: rgb = tex * I.y, alpha = sqrt(tex.a * I.y * fade) (setup stores sqrt(tex.a)).
    Background,
    /// `spline_defaultPS`, additive: rgb = tex * I.x * fade.
    Front,
    /// No textures installed: our flat colour, alpha = fade.
    Flat,
}

impl Pass {
    /// The material's linear base colour for intensity `i` (front, background) and `fade`.
    pub(crate) fn colour(self, i: [f32; 2], fade: f32) -> [f32; 4] {
        match self {
            Pass::Background => [i[1], i[1], i[1], (i[1] * fade).max(0.0).sqrt()],
            Pass::Front => [i[0] * fade, i[0] * fade, i[0] * fade, 1.0],
            Pass::Flat => [COLOR[0], COLOR[1], COLOR[2], fade],
        }
    }
}

/// Teleporter requests name no colour (-1, b533), which retail resolves to this row.
const DEFAULT_COLOUR: &str = "default";

/// A teleporter's indicator: the `ribbon_indicator` key and its anchor (the challenge's locator matrix).
#[derive(Clone, Debug, PartialEq, serde::Serialize, Deserialize)]
pub(crate) struct Placement {
    pub key: String,
    #[serde(default)]
    pub locator: String,
    pub matrix: [[f32; 4]; 4],
    /// A `visualindicators` row in place of the indicator row's own (mods).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// A `ribbon_spline_colour` row (mods; retail teleporters use `default`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
}

#[derive(Resource, Default)]
pub(crate) struct VisualIndicators {
    loaded: bool,
    splines: HashMap<String, Spline>,
    indicators: HashMap<String, Indicator>,
    styles: HashMap<String, Style>,
    colours: HashMap<String, Colour>,
    textures: Option<String>,
    meshes: HashMap<(String, String), Handle<Mesh>>,
    shown: Vec<Placement>,
}

/// One icon: the billboard root; its pass children carry [`IndicatorPass`].
#[derive(Component)]
pub(crate) struct IndicatorMarker {
    style: Style,
    passes: Vec<(Handle<StandardMaterial>, Pass)>,
}

#[derive(Component)]
pub(crate) struct IndicatorPass;

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
pub(crate) fn wanted(placements: &[&Placement]) -> Vec<Placement> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in placements {
        if !p.locator.is_empty() && !seen.insert(p.locator.clone()) {
            continue;
        }
        out.push((*p).clone());
    }
    out
}

/// The icon's world position: anchor + offset in the anchor's frame (b533 `827AB790`).
pub(crate) fn position(matrix: &[[f32; 4]; 4], offset: [f32; 3]) -> Vec3 {
    let row = |i: usize| Vec3::new(matrix[i][0], matrix[i][1], matrix[i][2]);
    row(3) + row(0) * offset[0] + row(1) * offset[1] + row(2) * offset[2]
}

/// The spline as retail's strips (`827B0BA8` / `827B2200`, b534): per strip the sampled points with a fixed edge
/// offset each side in the icon's frame, half width h = 0.25 * (A + B). Three strips: offsets along z, y and x;
/// one strip: the second edge shifted by (2h, 2h, 0). Double-sided through the material.
pub(crate) fn ribbon(spline: &Spline, style: &Style) -> Mesh {
    let samples = style.samples.clamp(1, MAX_SAMPLES) as usize;
    // With each point its spline parameter in segment units (b534's vertex t), the texture's u.
    let mut points: Vec<(Vec3, f32)> = Vec::new();
    for (s, [c3, c2, c1, c0]) in spline.segments.iter().enumerate() {
        for i in 0..=samples {
            let t = i as f32 / samples as f32;
            let p = |k: usize| c3[k] * t * t * t + c2[k] * t * t + c1[k] * t + c0[k];
            let point = Vec3::new(p(0), p(1), p(2));
            if points.last().is_none_or(|(last, _)| last.distance(point) > 1e-4) {
                points.push((point, s as f32 + t));
            }
        }
    }
    let h = 0.25 * (style.width[0] + style.width[1]);
    let edges: Vec<(Vec3, Vec3)> = if style.three_strips {
        vec![(Vec3::new(0.0, 0.0, -h), Vec3::new(0.0, 0.0, h)), (Vec3::new(0.0, -h, 0.0), Vec3::new(0.0, h, 0.0)), (Vec3::new(-h, 0.0, 0.0), Vec3::new(h, 0.0, 0.0))]
    } else {
        vec![(Vec3::ZERO, Vec3::new(2.0 * h, 2.0 * h, 0.0))]
    };
    let (mut positions, mut uvs, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for (a, b) in edges {
        let first = positions.len() as u32;
        for (i, &(p, t)) in points.iter().enumerate() {
            positions.push((p + a).to_array());
            positions.push((p + b).to_array());
            // v across the strip: the textures are bands, dark at both edges (b535).
            uvs.extend_from_slice(&[[t, 0.0], [t, 1.0]]);
            if i > 0 {
                let v = first + (i as u32) * 2;
                indices.extend_from_slice(&[v - 2, v - 1, v, v - 1, v + 1, v]);
            }
        }
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(bevy::mesh::Indices::U32(indices))
}

/// Respawn the icons when the armed set changes (map switch, layout, mods adding or removing teleporters).
fn sync(
    mut commands: Commands,
    mut state: ResMut<VisualIndicators>,
    teleporters: Res<crate::teleporters::Teleporters>,
    map: Res<crate::map_transition::CurrentMap>,
    config: Res<crate::config::Config>,
    server: Res<AssetServer>,
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
                state.styles = file.styles;
                state.colours = file.colours;
                state.textures = file.textures;
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
    let mut spawned = 0;
    for placement in &wanted {
        let Some(indicator) = state.indicators.get(&placement.key).filter(|i| i.enabled).cloned() else { continue };
        let Some(spline_id) = indicator.spline.clone() else { continue };
        let style_key = placement.style.clone().or(indicator.style.clone()).unwrap_or_default();
        let style = state.styles.get(&style_key).cloned().unwrap_or_default();
        let mesh = match state.meshes.get(&(spline_id.clone(), style_key.clone())) {
            Some(mesh) => mesh.clone(),
            None => {
                let Some(spline) = state.splines.get(&spline_id) else { continue };
                let mesh = meshes.add(ribbon(spline, &style));
                state.meshes.insert((spline_id.clone(), style_key.clone()), mesh.clone());
                mesh
            }
        };
        // Retail's two passes (b535 `827AFC80`, b536 `82CE9080`): the background texture alpha blended, then the
        // front one additive; depth test, no depth write, no cull. Materials per icon as the fade is per icon.
        let colour = state.colours.get(placement.colour.as_deref().unwrap_or(DEFAULT_COLOUR)).cloned();
        let layers: Vec<(Option<String>, Pass)> = match (colour, state.textures.as_deref()) {
            (Some(c), Some(folder)) => vec![
                (Some(format!("private/{folder}/{}.png", c.background)), Pass::Background),
                (Some(format!("private/{folder}/{}.png", c.front)), Pass::Front),
            ],
            _ => vec![(None, Pass::Flat)],
        };
        let passes: Vec<(Handle<StandardMaterial>, Pass)> = layers.into_iter().map(|(texture, pass)| {
            let material = materials.add(StandardMaterial {
                base_color: Color::linear_rgba(0.0, 0.0, 0.0, 0.0),
                base_color_texture: texture.map(|path| server.load(path)),
                alpha_mode: if pass == Pass::Front { AlphaMode::Add } else { AlphaMode::Blend },
                // The front pass sorts after the background one at the same distance.
                depth_bias: if pass == Pass::Front { 1.0 } else { 0.0 },
                unlit: true,
                cull_mode: None,
                ..default()
            });
            (material, pass)
        }).collect();
        let root = commands.spawn((IndicatorMarker { style, passes: passes.clone() },
            Transform::from_translation(position(&placement.matrix, indicator.offset)), Visibility::Hidden)).id();
        for (material, ..) in passes {
            commands.spawn((IndicatorPass, Mesh3d(mesh.clone()), MeshMaterial3d(material), Transform::default(), ChildOf(root)));
        }
        spawned += 1;
    }
    info!("SKATE_VISUAL_INDICATORS map={} shown={spawned}", map.name);
    state.shown = wanted;
}

/// Per viewing camera (`827B1A00`, b534): a yaw-only billboard whose local z is the camera's flattened view axis
/// reversed (screen-parallel), then the style's distance fade; hidden beyond the request cull or the fade's end.
fn face_camera(
    cameras: Query<&GlobalTransform, With<crate::camera::GameplayCamera>>,
    mut markers: Query<(&IndicatorMarker, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(camera) = cameras.iter().next() else { return };
    let back = -camera.forward().as_vec3();
    for (marker, mut t, mut visibility) in &mut markers {
        if back.x.abs() + back.z.abs() > 1e-6 {
            t.rotation = Quat::from_rotation_y(back.x.atan2(back.z));
        }
        // The fade runs on view depth (the vertex shader's clip z, b536), the request cull on distance.
        let to = t.translation - camera.translation();
        let depth = to.dot(camera.forward().as_vec3());
        let alpha = if to.length() < DRAW_DISTANCE && depth > 0.0 { marker.style.alpha(depth) } else { 0.0 };
        visibility.set_if_neq(if alpha > 0.0 { Visibility::Visible } else { Visibility::Hidden });
        for (material, pass) in &marker.passes {
            let [r, g, b, a] = pass.colour(marker.style.intensity, alpha);
            let want = Color::linear_rgba(r, g, b, a);
            if materials.get(material).is_some_and(|m| m.base_color != want)
                && let Some(m) = materials.get_mut(material)
            {
                m.base_color = want;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(locator: &str, x: f32) -> Placement {
        let mut matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        matrix[3] = [x, 0.0, 0.0, 1.0];
        Placement { key: "teleport".into(), locator: locator.into(), matrix, style: None, colour: None }
    }

    #[test]
    fn one_icon_per_locator() {
        let (a, b, c) = (placement("loc_a", 1.0), placement("loc_a", 2.0), placement("loc_b", 3.0));
        let wanted = wanted(&[&a, &b, &c]);
        assert_eq!(wanted.len(), 2);
        assert_eq!(wanted[0].matrix[3][0], 1.0);
    }

    /// b536 pixel shaders with smallarrow's intensity (1.0, 0.7): the front glows by the fade, the background keeps
    /// its colour and fades its opacity as sqrt.
    #[test]
    fn pass_colours_follow_the_pixel_shaders() {
        assert_eq!(Pass::Front.colour([1.0, 0.7], 0.5), [0.5, 0.5, 0.5, 1.0]);
        let b = Pass::Background.colour([1.0, 0.7], 1.0);
        assert!((b[0] - 0.7).abs() < 1e-6 && (b[3] - 0.7f32.sqrt()).abs() < 1e-6);
        assert_eq!(Pass::Background.colour([1.0, 0.7], 0.0)[3], 0.0);
    }

    /// smallarrow (teleporters): fades in from 4.23 m to 13.36 m, gone beyond 100 m.
    #[test]
    fn fade_window() {
        let s = Style { width: [0.2, 0.15], samples: 10, three_strips: true, fade: [4.234_528, 13.355_05, 100.0, 100.0], intensity: [1.0, 0.7] };
        assert_eq!(s.alpha(2.0), 0.0);
        assert!((s.alpha(8.795) - 0.5).abs() < 1e-3);
        assert_eq!(s.alpha(50.0), 1.0);
        assert_eq!(s.alpha(100.0), 1.0);
        assert_eq!(s.alpha(100.5), 0.0);
        let d = Style::default();
        assert!((d.alpha(97.15) - 0.5).abs() < 1e-3);
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
        let style = Style { width: [0.2, 0.15], samples: 10, three_strips: true, fade: [0.0, 0.0, 100.0, 100.0], intensity: [1.0, 0.7] };
        let mesh = ribbon(&spline, &style);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        // Three strips of 11 point pairs; full width 0.5 * (A + B) = 0.175 along z, then y, then x.
        assert_eq!(p.len(), 3 * 11 * 2);
        assert!((p[0][2] + 0.0875).abs() < 1e-6 && (p[1][2] - 0.0875).abs() < 1e-6);
        assert!((p[22][1] + 0.0875).abs() < 1e-6 && (p[44][0] + 0.0875).abs() < 1e-6);
        assert_eq!(p[21][1], 1.0);
        let one = ribbon(&spline, &Style { three_strips: false, samples: 40, ..style });
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(q)) = one.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        // One strip, samples capped at 10, second edge shifted by (2h, 2h, 0).
        assert_eq!(q.len(), 11 * 2);
        assert!((q[1][0] - 0.175).abs() < 1e-6 && (q[1][1] - 0.175).abs() < 1e-6);
    }
}
