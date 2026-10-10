//! Ped hand props (b87, b89): the `livingworld_handprops` records (offset, carry channel) and their models
//! (`living_world/hand_props.json` + `hand_props/<key>.glb`, setup `tools/asset_pipeline/hand_props.py` from the
//! retail DMO templates), and the held object on the ped.
//!
//! Retail: SpawnInteractionBasedHandProp requests the record (`82E3DDA0`, `brain+3279` 0x01); the ped update
//! creates the DMO (`82E3DE18`) and attaches it (`82E3EC60`: `ped+5920`, `brain+3278` 0x02); every frame
//! `82E3E4F0` / `82E3E1B0` put it at the hand matrix composed with the record's local offset [code].
//! NOT RETAIL YET: the hand bone is RIGHTHANDPROP (rig 26) [inferred: every carry channel is `*RH`; which bone fills
//! the hand matrix at skeleton +19504 is not decoded]; the record fields `Hash_3FE1...` = Euler rotation in degrees
//! and `Hash_DC20...` = translation in metres, applied translation then X, Y, Z [inferred from the value shapes; the
//! record loader is not read]; the object is drawn only (no physics body until it is released, which is not ported).

use std::collections::BTreeMap;
use std::path::Path;

use bevy::prelude::*;
use serde_json::Value;

/// The rig bone a hand prop follows.
pub(crate) const HAND_PROP_BONE: &str = "RIGHTHANDPROP";

const ROTATION_FIELD: &str = "Hash_3FE10F7B1115B8E6";
const OFFSET_FIELD: &str = "Hash_DC20CCEAB4B92992";
const CHANNEL_FIELD: &str = "Hash_FC1D2C4E5CCA6AED";

/// One hand prop record.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HandPropRecord {
    /// The model under the asset root (`private/living_world/hand_props/<key>.glb`); `None` when setup found no
    /// template for it (stock: `orange`).
    pub glb: Option<String>,
    pub rotation_degrees: Vec3,
    pub offset: Vec3,
    pub carry_channel: Option<String>,
}

impl HandPropRecord {
    /// The prop's frame in the hand bone's frame.
    pub fn local(&self) -> Mat4 {
        let r = self.rotation_degrees * (std::f32::consts::PI / 180.0);
        Mat4::from_translation(self.offset) * Mat4::from_quat(Quat::from_euler(EulerRot::XYZ, r.x, r.y, r.z))
    }
}

/// Every hand prop by key (`livingworld_handprops`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HandPropData {
    pub props: BTreeMap<String, HandPropRecord>,
}

fn vec3(v: Option<&Value>) -> Vec3 {
    let f = |k: &str| v.and_then(|v| v.get(k)).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Vec3::new(f("x"), f("y"), f("z"))
}

impl HandPropData {
    /// From `private/living_world/tables.json` (inheritance already merged by the exporter) and `hand_props.json`.
    pub fn load(root: &Path) -> Result<Self, String> {
        let read = |p: &str| -> Result<Value, String> { serde_json::from_slice(&std::fs::read(root.join(p)).map_err(|e| format!("{p}: {e}"))?).map_err(|e| format!("{p}: {e}")) };
        let tables = read("private/living_world/tables.json")?;
        let records = tables.pointer("/classes/livingworld_handprops").and_then(Value::as_object).ok_or("tables.json has no livingworld_handprops")?;
        let models = read("private/living_world/hand_props.json").ok();
        let mut props = BTreeMap::new();
        for (key, record) in records {
            let fields = record.get("fields");
            let file = models.as_ref().and_then(|m| m.pointer(&format!("/props/{key}/file"))).and_then(Value::as_str);
            props.insert(
                key.clone(),
                HandPropRecord {
                    glb: file.map(|f| format!("private/living_world/{f}")),
                    rotation_degrees: vec3(fields.and_then(|f| f.get(ROTATION_FIELD))),
                    offset: vec3(fields.and_then(|f| f.get(OFFSET_FIELD))),
                    carry_channel: fields.and_then(|f| f.get(CHANNEL_FIELD)).and_then(Value::as_str).map(str::to_string),
                },
            );
        }
        Ok(Self { props })
    }
}

/// The object a ped holds (the created DMO, `ped+5920`).
#[derive(Component, Clone, Debug)]
pub(crate) struct HeldHandProp {
    pub key: String,
    pub entity: Entity,
    pub local: Mat4,
}

/// Create the requested object (then `holding`, `82E3EC60`) and remove it when the brain dropped the prop.
pub(crate) fn sync_hand_props(
    mut commands: Commands,
    server: Res<AssetServer>,
    data: Res<super::peds::PedData>,
    state: Res<super::PopulationState>,
    mut peds: Query<(Entity, &super::peds::Pedestrian, &mut super::peds::PedMind, Option<&super::peds::PedPuppet>, Option<&HeldHandProp>)>,
) {
    let tick = state.world.tick();
    for (e, ped, mut mind, puppet, held) in &mut peds {
        let want = mind.brain.hand_prop.has().then(|| mind.brain.hand_prop.key.clone()).flatten();
        if let Some(h) = held
            && want.as_deref() != Some(h.key.as_str())
        {
            commands.entity(h.entity).despawn();
            commands.entity(e).remove::<HeldHandProp>();
            mind.brain.hand_prop.holding = false;
            info!("PED_HAND_PROP ped=#{} {} removed tick={tick}", ped.id.serial, h.key);
            continue;
        }
        let (Some(key), None, Some(scene)) = (want, held, puppet.and_then(|p| p.scene)) else { continue };
        let Some(record) = data.hand_props.props.get(&key) else { continue };
        let Some(glb) = record.glb.clone() else { continue }; // no model: stays requested (retail: the create fails)
        let entity = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(glb))), Transform::default(), Visibility::Inherited, ChildOf(scene))).id();
        commands.entity(e).insert(HeldHandProp { key: key.clone(), entity, local: record.local() });
        mind.brain.hand_prop.holding = true;
        info!("PED_HAND_PROP ped=#{} {key} created tick={tick}", ped.id.serial);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_offset_is_translation_then_rotation() {
        let r = HandPropRecord { rotation_degrees: Vec3::new(90.0, 0.0, 0.0), offset: Vec3::new(0.01, 0.0, 0.01), ..Default::default() };
        let m = r.local();
        assert!(m.w_axis.truncate().distance(Vec3::new(0.01, 0.0, 0.01)) < 1e-6);
        // +Y of the prop turns to +Z of the hand under 90 degrees about X.
        assert!(m.transform_vector3(Vec3::Y).distance(Vec3::Z) < 1e-6);
    }

    #[test]
    fn stock_hand_props_load_with_models() {
        let Some(raw) = std::env::var_os("SKATE3_ASSET_ROOT") else { return };
        let Some(root) = std::env::split_paths(&raw).find(|r| r.join("private/living_world/tables.json").exists()) else { return };
        let d = HandPropData::load(&root).unwrap();
        let pop = &d.props["pop"];
        assert_eq!((pop.rotation_degrees, pop.offset), (Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.01, 0.0, 0.01)));
        assert_eq!(pop.carry_channel.as_deref(), Some("CarrySmallRHChannel"));
        eprintln!("hand prop models: {} of {}", d.props.values().filter(|p| p.glb.is_some()).count(), d.props.len());
    }
}
