//! Traffic cars (doc 26, milestone V3): turns the population's vehicle spawn / despawn records
//! into visible cars that drive their lanes.
//!
//! - **Entity** per spawn record: [`TrafficCar`] (stable `LivingWorldId`, entity / model keys,
//!   palette ids and colours, engine record) + `Transform` + #32's
//!   [`TrafficAudio`](crate::world_audio::TrafficAudio). [`TrafficState`] keeps the follower cars
//!   (`skate_core::living_world::traffic::follow`) and the id -> entity map.
//! - **Motion** (`FixedUpdate`, after the population step): one follower step per 60 Hz world
//!   tick, the signal clock ticked once per world tick (retail ticks its 4 controllers by 1/60 s
//!   per world tick, `sub_826B2C18`); a car spawned at tick `s` steps from tick `s + 1`. Lane state
//!   goes back to the population (`LivingWorld::update_lane`, `update_position`) so the census
//!   cull and the placement test see the real cars. A car at a dead end leaves (despawn, reason
//!   External).
//! - **Look** (`Update`): the car's GLB (`private/living_world/vehicles/<recipe>.glb`, or a mod's
//!   path from [`VehicleOverrides`]) as a scene, drawn with the glTF materials on render layers 0
//!   and 28 like mod graphics; the `vehicle_chassis` material gets a tinted copy of its base
//!   texture ([`tint_rgba8`], rule below), the `vehicle_glass` material is made see-through
//!   ([`GLASS_ALPHA`], engine value); the six wheel bones spin with the distance driven over the
//!   model's wheel radius (`wheel_hint`, `Hash_FD7A66142F16B9CC`, equal to the wheel bone height
//!   [data, V0]). Drivers are part of the body mesh [data, V0]; no vehicle lights exist [data].
//! - **Collision**: one kinematic box per car from the GLB bounds (`mesh_bounds`), infinite mass,
//!   the car's velocity, joined to the skater solve through `physics::network::Proxies` like the
//!   NPC skaters. Solid contact only; bails and roof behaviour are V5.
//! - **Audio**: `TrafficAudio { engine: <spec engine_audio record>, speed: +3412, load: +3408 }`
//!   and `AudioVelocity`, so #32's traffic engine host (nearest 4 within 40 m, Doppler) plays it.
//! - **Events** ([`TrafficEvent`]): spawned, despawned, junction answer changed, entered a
//!   junction, entered a lane; the planned `sdk.living_world` events read these.
//!
//! Tint rule (`vehicle_chassis` shader not read; most likely rule, open): the body atlases paint
//! the car body pure blue `(0, 0, b)` with the shading in `b`, and the model records' base
//! palettes are chassis `(0, 0, 1)` and secondary `(1, 0, 0)` [data, V0]. So the chassis colour
//! replaces the blue channel and the secondary colour the red channel, weighted by how pure the
//! channel is: `out = rgb + m_b (b x chassis - (0, 0, b)) + m_r (r x secondary - (r, 0, 0))`, `m_b
//! = (b - max(r, g)) / b`, `m_r = (r - max(g, b)) / r`. With the base palette the texture is
//! unchanged at gain 1 (the identity the base records imply). [`PAINT_GAIN`] scales the painted
//! value (default 2x modulate: an estimate from the data, not retail; the shader's tint is not
//! decoded; overridable through [`VehicleOverrides::paint_gain`]).
//!
//! Multiplayer (no networking): a car's motion is a function of its spawn record, the world tick,
//! the signal clock's tick count (= world ticks since the world loaded) and the other cars (which
//! are themselves spawn records); the connector choice reads the occupancy, so a client must run
//! the whole set of cars, not one. See doc 26 V3.
//!
//! Moddability: [`VehicleOverrides`] (model per entity, GLB per model, colour per palette id,
//! follower numbers per entity, connector choice); restoring `VehicleOverrides::default()` undoes
//! a mod; the spec / palette defaults come from the export (`tables.json`, `vehicles.json`).

use super::{LivingWorldDespawn, LivingWorldSettings, LivingWorldSpawn, NetRole, PopulationState};
use crate::world_audio::{AudioVelocity, TrafficAudio};
use bevy::prelude::*;
use skate_core::living_world::rng::Rng;
use skate_core::living_world::traffic::follow::{self, Car, FollowEvent, FollowParams};
use skate_core::living_world::traffic::{ConnectorChoice, Entry, LaneCursor, Place, RoadNetwork, SegmentId, SignalClock, SignalTimings};
use skate_core::living_world::{DespawnReason, Kind, LivingWorldId, SpawnChoice};
use std::collections::BTreeMap;

/// The six wheel bones of the car rig [data, V0].
pub(crate) const WHEEL_BONES: [&str; 6] = ["LeftFront_wheel", "RightFront_wheel", "LeftRear_wheel_1", "RightRear_wheel_1", "LeftRear_wheel_2", "RightRear_wheel_2"];
/// Glass opacity (engine value; the `vehicle_glass` shader is not read, open).
pub(crate) const GLASS_ALPHA: f32 = 0.55;
/// Default gain on the painted value of the tint (a 2x modulate). ESTIMATE, NOT RETAIL: the
/// `vehicle_chassis` shader's tint maths is not decoded. Chosen from the data: the atlases' paint
/// blue sits around 0.5-0.56, and only with 2x does a palette "white" (0.90) or the taxi yellow
/// read as that colour (headless renders in `.local/research/npc/v3-tint`; gain 1 gives muddy
/// half-bright paint). A mod or setting overrides it ([`VehicleOverrides::paint_gain`]).
pub(crate) const PAINT_GAIN: f32 = 2.0;
/// Solid ids of car proxies: a tag in the top bits keeps them apart from NPCs and mod bodies.
pub(crate) const PROXY_ID_TAG: u64 = 0x5643_0000_0000_0000;

/// One car model (`vehicles.json` `models.<record>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VehicleModel {
    /// Asset path relative to the asset root.
    pub glb: String,
    pub chassis: Vec<[f32; 4]>,
    pub secondary: Vec<[f32; 4]>,
    pub chassis_ids: Vec<String>,
    pub secondary_ids: Vec<String>,
    /// m (`wheel_hint`).
    pub wheel_radius: f32,
    /// Mesh bounds, model space (min, max).
    pub bounds: [[f32; 3]; 2],
}

/// What a car entity drives with (`livingworld_entities` -> spec record).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VehicleSpec {
    /// `aud_traffic_engine` record (spec `engine_audio`).
    pub engine: String,
    pub params: FollowParams,
}

/// Car data of the loaded world.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VehicleData {
    pub models: BTreeMap<String, VehicleModel>,
    /// Entity name -> spec.
    pub specs: BTreeMap<String, VehicleSpec>,
    pub timings: Option<SignalTimings>,
}

fn vec3(v: &serde_json::Value) -> Option<[f32; 3]> {
    Some([v.get(0)?.as_f64()? as f32, v.get(1)?.as_f64()? as f32, v.get(2)?.as_f64()? as f32])
}

/// Read `vehicles.json` and `tables.json` (the follower numbers and engine record from each
/// entity's spec record; missing fields keep [`FollowParams::default`]).
pub(crate) fn parse_vehicle_data(vehicles: &[u8], tables: Option<&[u8]>) -> Result<VehicleData, String> {
    let doc: serde_json::Value = serde_json::from_slice(vehicles).map_err(|e| format!("vehicles.json: {e}"))?;
    let tables: Option<serde_json::Value> = tables.and_then(|t| serde_json::from_slice(t).ok());
    let mut data = VehicleData { timings: tables.as_ref().and_then(skate_data::roads::signal_timings), ..Default::default() };
    for (key, m) in doc["models"].as_object().into_iter().flatten() {
        let Some(glb) = m["glb"].as_str() else { continue };
        let colours = |k: &str| -> Vec<[f32; 4]> {
            m[k].as_array().into_iter().flatten().filter_map(|c| Some([c.get(0)?.as_f64()? as f32, c.get(1)?.as_f64()? as f32, c.get(2)?.as_f64()? as f32, c.get(3).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32])).collect()
        };
        let ids = |k: &str| -> Vec<String> { m["palette_ids"][k].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect() };
        let bounds = (|| Some([vec3(&m["mesh_bounds"][0])?, vec3(&m["mesh_bounds"][1])?]))().unwrap_or([[-0.9, 0.0, -2.2], [0.9, 1.5, 2.2]]);
        let wheel_radius = m["wheel_hint"].as_f64().map(|x| x as f32).filter(|r| *r > 0.05).unwrap_or(0.32);
        data.models.insert(
            key.clone(),
            VehicleModel {
                glb: format!("private/living_world/{glb}"),
                chassis: colours("chassis_colours"),
                secondary: colours("secondary_colours"),
                chassis_ids: ids("chassis"),
                secondary_ids: ids("secondary"),
                wheel_radius,
                bounds,
            },
        );
    }
    let class = |c: &str, r: &str| tables.as_ref().and_then(|t| t.pointer(&format!("/classes/{c}/{r}/fields")).cloned());
    for (name, e) in doc["entities"].as_object().into_iter().flatten() {
        let spec_name = e["spec"].as_str().unwrap_or("default");
        let mut params = FollowParams::default();
        let mut engine = "default".to_string();
        if let Some(f) = class("livingworld_vehicle_characteristics", spec_name) {
            let num = |k: &str| f.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
            if let Some(v) = num("Hash_328B9F4685A14018") {
                params.accel_max = v;
            }
            if let Some(v) = num("Hash_758229215579C6D1") {
                params.plan_decel = v;
            }
            if let Some(v) = num("follow_min_speed_kmh") {
                params.follow_min_speed = v / 3.6;
            }
            if let Some(v) = num("follow_speed_margin_kmh") {
                params.follow_margin = v / 3.6;
            }
            if let Some(k) = f.pointer("/engine_audio/key").and_then(|v| v.as_str()) {
                engine = k.to_string();
            }
        }
        data.specs.insert(name.clone(), VehicleSpec { engine, params });
    }
    Ok(data)
}

/// Mod and engine overrides (all keyed by the export's stable names). Empty = retail.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct VehicleOverrides {
    /// Entity name -> `livingworld_models` record.
    pub models: BTreeMap<String, String>,
    /// Model record -> GLB asset path.
    pub glbs: BTreeMap<String, String>,
    /// Palette id (`<model>/chassis/<i>`, `<model>/secondary/<i>`) -> RGBA.
    pub colours: BTreeMap<String, [f32; 4]>,
    /// Entity name -> follower numbers.
    pub params: BTreeMap<String, FollowParams>,
    /// Connector choice (retail: least loaded on every car).
    pub connector_choice: ConnectorChoice,
    /// Tint gain; `None` = [`PAINT_GAIN`] (estimate, not retail).
    pub paint_gain: Option<f32>,
}

/// One traffic car.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct TrafficCar {
    pub id: LivingWorldId,
    pub entity: String,
    pub model: String,
    pub glb: String,
    pub chassis_id: String,
    pub secondary_id: String,
    pub chassis: [f32; 4],
    pub secondary: [f32; 4],
    pub engine: String,
    /// Tint gain this car is drawn with ([`PAINT_GAIN`] unless overridden).
    pub paint_gain: f32,
    pub wheel_radius: f32,
    pub bounds: [[f32; 3]; 2],
    pub spawn_tick: u64,
}

/// Fixed-step pose of a car (render interpolates between `prev` and `curr`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct CarMotion {
    pub prev: Transform,
    pub curr: Transform,
    pub wheel_prev: f32,
    pub wheel: f32,
    pub velocity: Vec3,
}

/// What happened to a car (engine systems, the planned mod events).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) enum TrafficEvent {
    Spawned { id: LivingWorldId, entity: String, model: String, chassis: String },
    Despawned { id: LivingWorldId, reason: DespawnReason },
    /// The junction answer for the chosen connector changed (retail junction state).
    Junction { id: LivingWorldId, junction: u64, connector: u32, entry: Entry },
    EnteredJunction { id: LivingWorldId, junction: u64, connector: u32 },
    EnteredLane { id: LivingWorldId, segment: u64, lane: u8 },
}

/// The traffic of the loaded world.
#[derive(Resource)]
pub(crate) struct TrafficState {
    /// Follower cars, sorted by key (= the id serial).
    pub cars: Vec<Car>,
    /// Serial -> (entity, spawn tick).
    pub index: BTreeMap<u32, (Entity, u64)>,
    pub clock: Option<SignalClock>,
    /// World tick the cars were stepped to.
    pub last_tick: Option<u64>,
    pub rng: Rng,
    pub data: VehicleData,
    pub(crate) loaded_for: Option<(String, u64)>,
}

impl Default for TrafficState {
    fn default() -> Self {
        Self { cars: Vec::new(), index: BTreeMap::new(), clock: None, last_tick: None, rng: Rng::new(0x5452_4146), data: VehicleData::default(), loaded_for: None }
    }
}

/// Car rotation from the lane direction (+Z forward, y up).
pub(crate) fn car_rotation(forward: [f32; 3]) -> Quat {
    let f = Vec3::from_array(forward).normalize_or(Vec3::Z);
    let x = Vec3::Y.cross(f).normalize_or(Vec3::X);
    let y = f.cross(x);
    Quat::from_mat3(&Mat3::from_cols(x, y, f))
}

fn car_transform(net: &RoadNetwork, cursor: &LaneCursor) -> Transform {
    let frame = cursor.frame(net);
    Transform::from_translation(Vec3::from_array(frame.position)).with_rotation(car_rotation(frame.forward))
}

/// Tint an RGBA8 (sRGB) pixel buffer with the chassis and secondary colours (rule in the module
/// doc). Alpha is kept.
pub(crate) fn tint_rgba8(pixels: &mut [u8], chassis: [f32; 4], secondary: [f32; 4], gain: f32) {
    for px in pixels.chunks_exact_mut(4) {
        let [r, g, b] = [px[0], px[1], px[2]].map(|c| c as f32 / 255.0);
        let mb = if b > 0.0 { ((b - r.max(g)) / b).clamp(0.0, 1.0) } else { 0.0 };
        let mr = if r > 0.0 { ((r - g.max(b)) / r).clamp(0.0, 1.0) } else { 0.0 };
        let (pb, pr) = ((b * gain).min(1.0), (r * gain).min(1.0));
        let out = [
            r + mb * (pb * chassis[0]) + mr * (pr * secondary[0] - r),
            g + mb * (pb * chassis[1]) + mr * (pr * secondary[1]),
            b + mb * (pb * chassis[2] - b) + mr * (pr * secondary[2]),
        ];
        for i in 0..3 {
            px[i] = (out[i].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Load the car data when the world changes.
pub(crate) fn load_traffic_data(mut commands: Commands, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>, mut traffic: ResMut<TrafficState>) {
    let key = (map.name.clone(), map.generation);
    if traffic.loaded_for.as_ref() == Some(&key) {
        return;
    }
    // A new world: cars of the old one go (the population's reset records normally did this).
    for (_, (e, _)) in std::mem::take(&mut traffic.index) {
        commands.entity(e).despawn();
    }
    traffic.cars.clear();
    let dir = config.asset_root.join("private/living_world");
    let tables = std::fs::read(dir.join("tables.json")).ok();
    traffic.data = std::fs::read(dir.join("vehicles.json"))
        .ok()
        .and_then(|b| parse_vehicle_data(&b, tables.as_deref()).map_err(|e| warn!("LIVING_WORLD traffic: {e}")).ok())
        .unwrap_or_default();
    traffic.clock = traffic.data.timings.map(SignalClock::new);
    traffic.last_tick = None;
    traffic.loaded_for = Some(key);
    info!("LIVING_WORLD traffic models {} specs {} lights {}", traffic.data.models.len(), traffic.data.specs.len(), traffic.clock.is_some());
}

/// Build a car from a spawn record (pure; the engine and the tests use it).
pub(crate) fn car_from_record(
    net: &RoadNetwork,
    data: &VehicleData,
    overrides: &VehicleOverrides,
    record: &skate_core::living_world::SpawnRecord,
    length: f32,
) -> Option<(TrafficCar, Car, TrafficAudio)> {
    let SpawnChoice::Vehicle { entity, model, chassis, secondary, segment, lane, distance, .. } = &record.choice else { return None };
    let model_key = overrides.models.get(entity).unwrap_or(model);
    let m = data.models.get(model_key).cloned().unwrap_or_default();
    let pick = |ids: &[String], colours: &[[f32; 4]], i: u32, base: [f32; 4]| -> (String, [f32; 4]) {
        let id = ids.get(i as usize).cloned().unwrap_or_else(|| format!("{model_key}/{i}"));
        let c = overrides.colours.get(&id).copied().or_else(|| colours.get(i as usize).copied()).unwrap_or(base);
        (id, c)
    };
    let (chassis_id, chassis_c) = pick(&m.chassis_ids, &m.chassis, *chassis, [0.0, 0.0, 1.0, 1.0]);
    let (secondary_id, secondary_c) = pick(&m.secondary_ids, &m.secondary, *secondary, [1.0, 0.0, 0.0, 1.0]);
    let spec = data.specs.get(entity).cloned().unwrap_or(VehicleSpec { engine: "default".into(), params: FollowParams::default() });
    let params = overrides.params.get(entity).copied().unwrap_or(spec.params);
    let si = net.segment_index(SegmentId(*segment))?;
    // The connector is chosen on the first step from the live occupancy (cursor next = None here
    // is replaced at once by the engine with the loads of that tick).
    let cursor = LaneCursor { place: Place::Lane { segment: si, lane: (*lane).min(net.segments[si].lanes - 1) }, distance: distance.clamp(0.0, net.segments[si].length), next: None, lane_shift: 0.0 };
    let length = if length > 0.5 { length } else { (m.bounds[1][2] - m.bounds[0][2]).max(3.0) };
    let car = Car::new(record.id.serial, cursor, length, params);
    let audio = TrafficAudio { engine: spec.engine.clone(), speed: Some(0.0), load: Some(0.0), ..TrafficAudio::new(spec.engine.clone()) };
    let glb = overrides.glbs.get(model_key).cloned().unwrap_or(m.glb.clone());
    Some((
        TrafficCar {
            id: record.id,
            entity: entity.clone(),
            model: model_key.clone(),
            glb,
            chassis_id,
            secondary_id,
            chassis: chassis_c,
            secondary: secondary_c,
            engine: spec.engine,
            paint_gain: overrides.paint_gain.filter(|g| g.is_finite() && *g > 0.0).unwrap_or(PAINT_GAIN),
            wheel_radius: m.wheel_radius.max(0.05),
            bounds: m.bounds,
            spawn_tick: record.tick,
        },
        car,
        audio,
    ))
}

/// Pick the first connector of a freshly spawned car with the live loads (retail chooses on
/// entering the lane, `sub_82C376E8`).
fn choose_first(net: &RoadNetwork, cars: &[Car], car: &mut Car, choice: ConnectorChoice, rng: &mut Rng) {
    if let Place::Lane { segment, lane } = car.cursor.place {
        let occ = follow::occupancy(cars);
        car.cursor.next = skate_core::living_world::traffic::choose_connector(net, segment, lane, choice, &|s, l| occ.lane_load(s, l), rng);
    }
}

/// Spawn and despawn car entities from the population's records.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_vehicle_records(
    mut commands: Commands,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    state: Res<PopulationState>,
    overrides: Res<VehicleOverrides>,
    mut traffic: ResMut<TrafficState>,
    mut events: MessageWriter<TrafficEvent>,
) {
    let traffic = &mut *traffic;
    for LivingWorldDespawn(r) in despawns.read() {
        if r.id.kind != Kind::Vehicle {
            continue;
        }
        if let Some((e, _)) = traffic.index.remove(&r.id.serial) {
            commands.entity(e).despawn();
            traffic.cars.retain(|c| c.key != r.id.serial);
            events.write(TrafficEvent::Despawned { id: r.id, reason: r.reason });
        }
    }
    let Some(net) = state.roads.as_ref() else {
        spawns.clear();
        return;
    };
    for LivingWorldSpawn(s) in spawns.read() {
        if s.id.kind != Kind::Vehicle || traffic.index.contains_key(&s.id.serial) {
            continue;
        }
        let length = state.world.live(Kind::Vehicle).find(|l| l.id == s.id).and_then(|l| l.lane).map_or(0.0, |l| l.length);
        let Some((meta, mut car, audio)) = car_from_record(net, &traffic.data, &overrides, s, length) else {
            warn!("LIVING_WORLD traffic: car #{} has no lane on this road network", s.id.serial);
            continue;
        };
        choose_first(net, &traffic.cars, &mut car, overrides.connector_choice, &mut traffic.rng);
        let t = car_transform(net, &car.cursor);
        events.write(TrafficEvent::Spawned { id: s.id, entity: meta.entity.clone(), model: meta.model.clone(), chassis: meta.chassis_id.clone() });
        let e = commands
            .spawn((
                Name::new(format!("Traffic car {} ({})", s.id.serial, meta.entity)),
                t,
                Visibility::Inherited,
                CarMotion { prev: t, curr: t, wheel_prev: 0.0, wheel: 0.0, velocity: Vec3::ZERO },
                audio,
                AudioVelocity(Vec3::ZERO),
                meta,
            ))
            .id();
        traffic.index.insert(s.id.serial, (e, s.tick));
        let at = traffic.cars.partition_point(|c| c.key < car.key);
        traffic.cars.insert(at, car);
    }
}

/// Step the cars to the population's tick, publish pose, lane state and audio.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_traffic(
    mut commands: Commands,
    settings: Res<LivingWorldSettings>,
    overrides: Res<VehicleOverrides>,
    mut state: ResMut<PopulationState>,
    mut traffic: ResMut<TrafficState>,
    mut cars_q: Query<(&TrafficCar, &mut CarMotion, &mut TrafficAudio, &mut AudioVelocity)>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
    mut events: MessageWriter<TrafficEvent>,
) {
    let st = &mut *state;
    let traffic = &mut *traffic;
    let Some(net) = st.roads.as_ref() else { return };
    let now = st.world.tick();
    let from = traffic.last_tick.unwrap_or(now);
    traffic.last_tick = Some(now);
    let mut travelled: BTreeMap<u32, f32> = BTreeMap::new();
    let mut prev_pose: BTreeMap<u32, Transform> = BTreeMap::new();
    for c in &traffic.cars {
        prev_pose.insert(c.key, car_transform(net, &c.cursor));
    }
    let mut dead = Vec::new();
    let dt = (1.0 / skate_core::living_world::clock::RETAIL_TICK_HZ) as f32;
    for tick in from + 1..=now {
        let changes = traffic.clock.as_mut().map(|c| {
            let mut v = Vec::new();
            c.tick(&mut v);
            v
        });
        drop(changes);
        let Some(clock) = traffic.clock.as_ref() else { break };
        // Cars decided before this tick drive in it.
        let active: Vec<usize> = (0..traffic.cars.len()).filter(|&i| traffic.index.get(&traffic.cars[i].key).is_some_and(|(_, s)| *s < tick)).collect();
        let mut batch: Vec<Car> = active.iter().map(|&i| traffic.cars[i]).collect();
        let before: Vec<f32> = batch.iter().map(|c| c.cursor.distance).collect();
        let before_place: Vec<Place> = batch.iter().map(|c| c.cursor.place).collect();
        let out = follow::step(net, clock, &mut batch, dt, overrides.connector_choice, &mut traffic.rng);
        for (k, c) in batch.iter().enumerate() {
            let ds = if c.cursor.place == before_place[k] { (c.cursor.distance - before[k]).max(0.0) } else { c.speed * dt };
            *travelled.entry(c.key).or_default() += ds;
        }
        for (k, &i) in active.iter().enumerate() {
            traffic.cars[i] = batch[k];
        }
        for e in out {
            let id = |key: u32| LivingWorldId { kind: Kind::Vehicle, serial: key };
            match e {
                FollowEvent::Junction { key, connector, entry } => {
                    let c = &net.connectors[connector];
                    events.write(TrafficEvent::Junction { id: id(key), junction: c.id.junction.0, connector: c.id.index, entry });
                }
                FollowEvent::EnteredJunction { key, connector } => {
                    let c = &net.connectors[connector];
                    events.write(TrafficEvent::EnteredJunction { id: id(key), junction: c.id.junction.0, connector: c.id.index });
                }
                FollowEvent::EnteredLane { key, segment, lane } => {
                    events.write(TrafficEvent::EnteredLane { id: id(key), segment: net.segments[segment].id.0, lane });
                }
                FollowEvent::DeadEnd { key } => dead.push(key),
            }
        }
    }
    // Lane state and position back to the population; pose, audio.
    for c in &traffic.cars {
        let id = LivingWorldId { kind: Kind::Vehicle, serial: c.key };
        let frame = c.cursor.frame(net);
        st.world.update_position(id, frame.position);
        let (segment, lane, distance) = match c.cursor.place {
            Place::Lane { segment, lane } => (segment, lane, c.cursor.distance),
            Place::Connector { connector } => {
                let k = &net.connectors[connector];
                (net.connector_exit(connector).unwrap_or(0), k.to_lane, 0.0)
            }
        };
        st.world.update_lane(id, net.segments[segment].id, lane, distance, c.speed);
        let Some((e, _)) = traffic.index.get(&c.key) else { continue };
        let Ok((meta, mut motion, mut audio, mut velocity)) = cars_q.get_mut(*e) else { continue };
        let t = car_transform(net, &c.cursor);
        motion.prev = prev_pose.get(&c.key).copied().unwrap_or(t);
        motion.curr = t;
        motion.wheel_prev = motion.wheel;
        motion.wheel = (motion.wheel + travelled.get(&c.key).copied().unwrap_or(0.0) / meta.wheel_radius) % std::f32::consts::TAU;
        motion.velocity = Vec3::from_array(frame.forward) * c.speed;
        velocity.0 = motion.velocity;
        audio.speed = Some(c.speed);
        audio.load = Some(c.accel);
    }
    // Dead ends: the car leaves (hosts decide; a client waits for the host's record).
    if settings.net_role != NetRole::Client {
        for key in dead {
            let id = LivingWorldId { kind: Kind::Vehicle, serial: key };
            if let Some(skate_core::living_world::Decision::Despawn(r)) = st.world.despawn(id, DespawnReason::External) {
                st.despawned += 1;
                despawns.write(LivingWorldDespawn(r));
            }
            if let Some((e, _)) = traffic.index.remove(&key) {
                commands.entity(e).despawn();
                traffic.cars.retain(|c| c.key != key);
                events.write(TrafficEvent::Despawned { id, reason: DespawnReason::External });
            }
        }
    }
}

/// The kinematic box of one car, world space.
pub(crate) fn proxy(car: &TrafficCar, pose: &Transform, velocity: Vec3) -> skate_dynamics::SolidBody {
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};
    let [lo, hi] = car.bounds;
    let half = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
    let centre = pose.translation + pose.rotation * Vec3::new((hi[0] + lo[0]) * 0.5, (hi[1] + lo[1]) * 0.5, (hi[2] + lo[2]) * 0.5);
    let q = pose.rotation;
    let rotation = Rotation::from_xyzw(q.x, q.y, q.z, q.w).normalize();
    let p = |v: Vec3| Vector::new(v.x, v.y, v.z);
    skate_dynamics::SolidBody {
        id: PROXY_ID_TAG | car.id.to_u64(),
        pose: Pose::from_parts(p(pose.translation), rotation),
        center_of_mass: p(centre),
        inertia_rotation: rotation,
        inverse_mass: 0.0,
        inverse_inertia: Vector::new(0.0, 0.0, 0.0),
        linvel: p(velocity),
        angvel: Vector::new(0.0, 0.0, 0.0),
        contact_group: 0,
        colliders: vec![skate_dynamics::SolidCollider {
            shape: SharedShape::cuboid(half[0].max(0.1), half[1].max(0.1), half[2].max(0.1)),
            pose: Pose::from_parts(p(centre), rotation),
            friction: 0.5,
        }],
    }
}

/// Add the car proxies to the skater solve (after the network proxies were rebuilt).
pub(crate) fn push_vehicle_proxies(
    cars: Query<(&TrafficCar, &CarMotion)>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    replay: Res<crate::replay::Replay>,
) {
    if replay.active || cars.is_empty() {
        return;
    }
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    let mut list: Vec<_> = cars.iter().collect();
    list.sort_by_key(|(c, _)| c.id);
    for (car, motion) in list {
        proxies.append_solid(proxy(car, &motion.curr, motion.velocity), &physics, &skater, false);
    }
    physics.network_proxies = proxies;
}

/// The render side of one car.
#[derive(Component, Default)]
pub(crate) struct CarLook {
    scene: Option<Entity>,
    /// Wheel joint entity and its bind transform.
    wheels: Vec<(Entity, Transform)>,
    ready: bool,
}

/// Tinted / glass materials per (source material, colours).
#[derive(Resource, Default)]
pub(crate) struct CarMaterials(BTreeMap<(AssetId<StandardMaterial>, [u32; 8]), Handle<StandardMaterial>>);

/// Load the GLB, then tint the chassis, clear the glass and find the wheel bones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_car_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut cars: Query<(Entity, &TrafficCar, Option<&mut CarLook>)>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
    named: Query<(&Name, &Transform)>,
    meshes: Query<(&MeshMaterial3d<StandardMaterial>, Option<&bevy::gltf::GltfMaterialName>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<CarMaterials>,
) {
    for (e, car, look) in &mut cars {
        let Some(mut look) = look else {
            let scene = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(car.glb.clone()))), Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            commands.entity(e).insert(CarLook { scene: Some(scene), wheels: Vec::new(), ready: false });
            continue;
        };
        let Some(scene) = look.scene else { continue };
        if look.ready || !instances.get(scene).is_ok_and(|i| spawner.instance_is_ready(**i)) {
            continue;
        }
        let Ok(instance) = instances.get(scene) else { continue };
        let entities: Vec<Entity> = spawner.iter_instance_entities(**instance).collect();
        let mut pending = false;
        let mut wheels = Vec::new();
        for ent in entities {
            if let Ok((name, t)) = named.get(ent) {
                if WHEEL_BONES.contains(&name.as_str()) {
                    wheels.push((ent, *t));
                }
            }
            let Ok((handle, name)) = meshes.get(ent) else { continue };
            let kind = name.map(|n| n.0.as_str()).unwrap_or("");
            commands.entity(ent).insert((bevy::camera::visibility::RenderLayers::from_layers(&[0, 28]), bevy::camera::visibility::NoFrustumCulling));
            let bits = |c: [f32; 4]| c.map(f32::to_bits);
            let ch = bits(car.chassis);
            let se = bits(car.secondary);
            let key = (handle.0.id(), [ch[0], ch[1], ch[2], ch[3], se[0], se[1], se[2], if kind.starts_with("vehicle_glass") { 1 } else { car.paint_gain.to_bits() | 2 }]);
            if let Some(m) = cache.0.get(&key) {
                commands.entity(ent).insert(MeshMaterial3d(m.clone()));
                continue;
            }
            let Some(source) = materials.get(&handle.0).cloned() else {
                pending = true;
                continue;
            };
            let replacement = if kind.starts_with("vehicle_chassis") {
                let Some(tex) = source.base_color_texture.clone() else { continue };
                let Some(image) = images.get(&tex) else {
                    pending = true;
                    continue;
                };
                let mut tinted = image.clone();
                match tinted.data.as_mut() {
                    Some(px) if tinted.texture_descriptor.format.block_copy_size(None) == Some(4) => tint_rgba8(px, car.chassis, car.secondary, car.paint_gain),
                    _ => {
                        warn!("LIVING_WORLD traffic: {} base texture is not RGBA8; drawn untinted", car.glb);
                    }
                }
                let mut m = source.clone();
                m.base_color_texture = Some(images.add(tinted));
                m
            } else if kind.starts_with("vehicle_glass") {
                let mut m = source.clone();
                m.base_color = m.base_color.with_alpha(GLASS_ALPHA);
                m.alpha_mode = AlphaMode::Blend;
                m.perceptual_roughness = 0.1;
                m.reflectance = 0.8;
                m
            } else {
                continue;
            };
            let h = materials.add(replacement);
            cache.0.insert(key, h.clone());
            commands.entity(ent).insert(MeshMaterial3d(h));
        }
        if pending {
            continue;
        }
        look.wheels = wheels;
        look.ready = true;
        commands.entity(scene).insert(Visibility::Inherited);
    }
}

/// Interpolate the car between fixed steps and spin the wheels.
pub(crate) fn present_car_pose(fixed: Res<Time<Fixed>>, mut cars: Query<(&CarMotion, &CarLook, &mut Transform)>, mut joints: Query<&mut Transform, Without<CarLook>>) {
    let a = fixed.overstep_fraction();
    for (motion, look, mut t) in &mut cars {
        t.translation = motion.prev.translation.lerp(motion.curr.translation, a);
        t.rotation = motion.prev.rotation.slerp(motion.curr.rotation, a);
        let mut d = motion.wheel - motion.wheel_prev;
        if d < 0.0 {
            d += std::f32::consts::TAU;
        }
        let angle = motion.wheel_prev + d * a;
        for (joint, bind) in &look.wheels {
            if let Ok(mut j) = joints.get_mut(*joint) {
                j.rotation = Quat::from_rotation_x(angle) * bind.rotation;
            }
        }
    }
}

/// One-line traffic summary for the debug readout (`SKATE_LIVING_WORLD_DEBUG=1`).
pub(crate) fn traffic_readout(traffic: &TrafficState) -> String {
    let moving = traffic.cars.iter().filter(|c| c.speed > 0.5).count();
    let waiting = traffic.cars.iter().filter(|c| matches!(c.entry, Some(Entry::Signal | Entry::Yield | Entry::Blocked)) && c.speed < 0.5).count();
    let mean = if traffic.cars.is_empty() { 0.0 } else { traffic.cars.iter().map(|c| c.speed).sum::<f32>() / traffic.cars.len() as f32 };
    format!("traffic cars {} moving {} waiting {} mean speed {:.1} m/s signal ticks {}", traffic.cars.len(), moving, waiting, mean, traffic.clock.as_ref().map_or(0, |c| c.ticks()))
}

fn log_traffic(settings: Res<LivingWorldSettings>, state: Res<PopulationState>, traffic: Res<TrafficState>, mut last: Local<u64>) {
    if !settings.debug || state.world.tick() < *last + 300 {
        return;
    }
    *last = state.world.tick();
    info!("LIVING_WORLD {}", traffic_readout(&traffic));
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<TrafficState>()
        .init_resource::<VehicleOverrides>()
        .init_resource::<CarMaterials>()
        .add_message::<TrafficEvent>()
        .add_systems(FixedUpdate, (load_traffic_data, apply_vehicle_records, drive_traffic, log_traffic).chain().after(super::step_population))
        .add_systems(
            FixedUpdate,
            push_vehicle_proxies.after(crate::multiplayer::prepare).after(crate::app::SimulationSet::Controls).before(crate::app::SimulationSet::Physics),
        )
        .add_systems(Update, (present_car_looks, present_car_pose).chain().after(crate::app::FrameSet::Animation));
}
