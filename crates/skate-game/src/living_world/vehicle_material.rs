//! Retail car materials: the `vehicle_chassis` and `vehicle_glass` programs of TU3
//! `shaders_final.big` (`vehicle_defaultPS`, `vehicle_glass_defaultPS`) as one Bevy material.
//!
//! Retail [code, shader microcode; constant tables read from the `.fpo` objects]:
//! - Constants: `g_vLightDir` c9, `m_params` c14..c19 (rows 0..5), `i_params` c20,
//!   `i_colorize_red` c21, `i_colorize_blue` c22; samplers `i_diffuse` 3, `i_normal` 4,
//!   `i_environment` 5 (glass: no diffuse, `i_normal` 3, `i_environment` 4).
//! - Chassis: `lin = diffuse^2`; a texel with `G^2 < 0.001225` takes `R^2 x i_colorize_red + B^2 x
//!   i_colorize_blue` (the ped shaders' rule, `skate_core::living_world::peds::colorize`). Light:
//!   `lin x (ambient + max(N.L, 0) x m_params[1].rgb x shadow) + (pow(sat(V.reflect(L)),
//!   m_params[2].w) x shadow x m_params[2].rgb + env^2) x (1 - sat(N.V))^m_params[1].w x
//!   diffuse.a^2`, ambient = the literal `(0.1, 0.175, 0.3)` x `m_params[5].w`, the shadow zero
//!   where `N.L < 0`; times `m_params[0].y` and the fog factor, plus the fog colour; the
//!   exposure / tone tail; output alpha `i_params.x`.
//! - Glass: the same with albedo `m_params[4].rgb` and no diffuse alpha mask; output alpha
//!   `i_params.x x m_params[4].w`. Render state [code, `sub_82CF0B90`]: alpha blend
//!   (SRC_ALPHA / INV_SRC_ALPHA), depth test on, depth write off.
//! - The rows come from the vault row named by each part's `AttribulatorMaterialName`
//!   (`character.livingworld_vehicles` / `character.livingworld_vehicles_glass` on every car)
//!   [data]; the converter writes them to `vehicles.json` `materials`.
//! - `i_params.x` = min(distance fade, fade-in ramp) (`sub_827C1188`) [code, partly read]; the
//!   engine draws 1 (no fade yet, open).
//!
//! Engine: lit with the character lighting's light (`retail_character::Lighting`), Bevy's
//! directional shadow and, like the character shader, without the vertex fog and the tone tail.
//! The environment cube is bound when the export carries one (open: not converted yet).
//!
//! Moddability: the rows are data (`vehicles.json`) and a mod replaces them per material name
//! (`VehicleOverrides::material_params`); the palette colours and their gain come from
//! `VehicleOverrides` as before; a mod GLB names its row with material extras
//! `{"attrib_material": "..."}`, else gets the row its `kind` used in retail.

use bevy::{
    asset::embedded_asset,
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

/// The ambient literal of both car pixel shaders (scaled by `m_params[5].w`) [code, shader].
pub(crate) const AMBIENT: [f32; 3] = [0.1, 0.175, 0.3];

#[derive(Clone, Debug, Default, PartialEq, ShaderType)]
pub(crate) struct VehicleParams {
    pub light: Vec4,
    pub red: Vec4,
    pub blue: Vec4,
    /// Glass program, normal map present, `i_params.x`, environment cube present.
    pub options: Vec4,
    /// Ambient literal (rgb), mask literal (w).
    pub literals: Vec4,
    pub rows: [Vec4; 9],
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct VehicleMaterial {
    #[uniform(0)]
    pub params: VehicleParams,
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub normal: Option<Handle<Image>>,
    #[texture(5, dimension = "cube")]
    #[sampler(6)]
    pub environment: Option<Handle<Image>>,
    pub alpha: AlphaMode,
}

impl Material for VehicleMaterial {
    fn fragment_shader() -> ShaderRef {
        bevy::asset::AssetPath::from(bevy::asset::embedded_path!("vehicle_material.wgsl")).with_source("embedded").into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }
}

/// Which program a car material runs, from its export kind (`extras.kind`, the GLB material name
/// prefix): `vehicle_glass` -> glass, `vehicle_chassis` -> chassis, anything else keeps its glTF
/// material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Program {
    Chassis,
    Glass,
}

impl Program {
    pub(crate) fn of(kind: &str) -> Option<Self> {
        if kind.starts_with("vehicle_glass") {
            Some(Self::Glass)
        } else if kind.starts_with("vehicle_chassis") {
            Some(Self::Chassis)
        } else {
            None
        }
    }
    /// The export's material kind of this program.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::Chassis => "vehicle_chassis",
            Self::Glass => "vehicle_glass",
        }
    }
}

/// The uniform of one car material: `rows` from the material's vault row, the palette pair
/// scaled by `gain`, `light` from the character lighting.
pub(crate) fn params(program: Program, rows: &[[f32; 4]], chassis: [f32; 4], secondary: [f32; 4], gain: f32, light: Vec4, normal_map: bool, environment: bool) -> VehicleParams {
    let scale = |c: [f32; 4]| Vec4::new(c[0] * gain, c[1] * gain, c[2] * gain, c[3]);
    VehicleParams {
        light,
        red: scale(secondary),
        blue: scale(chassis),
        options: Vec4::new(f32::from(program == Program::Glass), f32::from(normal_map), 1.0, f32::from(environment)),
        literals: Vec3::from_array(AMBIENT).extend(skate_core::living_world::peds::colorize::MASK_GREEN_SQ_MAX),
        rows: std::array::from_fn(|i| rows.get(i).map_or(Vec4::ZERO, |r| Vec4::from_array(*r))),
    }
}

/// Retail render state per program: the body is opaque, the glass alpha-blended without depth
/// write (`sub_82CF0B90`: blend[8], depth[14]).
pub(crate) fn alpha_mode(program: Program) -> AlphaMode {
    match program {
        Program::Chassis => AlphaMode::Opaque,
        Program::Glass => AlphaMode::Blend,
    }
}

/// The car material (installed with the retail render plugins, next to the character lighting
/// whose WGSL module it imports).
pub(crate) struct VehicleMaterialPlugin;
impl Plugin for VehicleMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "vehicle_material.wgsl");
        app.add_plugins(MaterialPlugin::<VehicleMaterial>::default());
    }
}
