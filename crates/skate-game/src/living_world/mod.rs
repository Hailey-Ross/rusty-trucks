//! Living world, engine side (doc `docs/hails-additions/26-living-world.md`, milestone 2: the
//! population core). Runs `skate_core::living_world` around the local player at the console
//! cadence and publishes spawn / despawn decisions as messages. Consumers: the replay-tier NPC
//! skaters ([`npc_skaters`], milestone 3); peds and cars come with their milestones.
//!
//! Data: the setup group `livingworld` export (`private/living_world/`): `tables.json`
//! (census caps, ranges), `<District>.census.bin`, `skater_profiles.json`,
//! `skater_paths/<District>.bin`. Without it the world stays empty.
//!
//! Multiplayer seams (no networking here): [`NetRole`] decides who runs the population.
//! Standalone and Host run the rules; a Client never does and only mirrors records it is given
//! ([`PopulationState::apply_records`]). Records serialise ([`WireRecord`]). Retail default:
//! nothing ambient spawns in an online session (culling still runs).
//!
//! Mod surface (planned `sdk.living_world`, see doc 26): [`LivingWorldSettings`] is the one place
//! settings and mods change per-kind enable / density and the ambient skater count; restoring
//! `LivingWorldSettings::default()` undoes a mod.
// Messages and records for consumers the engine does not have yet.
#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use skate_core::living_world::skaters::SkaterData;
use skate_core::living_world::{
    CensusMap, Decision, DespawnReason, DespawnRecord, FreePlay, Kind, LivingWorld, LivingWorldId, Observer, PopulationConfig, SpawnChoice,
    SpawnRecord, TickInputs,
};
use std::path::Path;

pub(crate) mod npc_skaters;
pub(crate) mod peds;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
#[cfg(test)]
#[path = "npc_tests.rs"]
mod npc_tests;
#[cfg(test)]
#[path = "peds_tests.rs"]
mod peds_tests;

/// Who runs the population decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NetRole {
    /// Single player (and, with the retail default, every online session: nothing spawns).
    #[default]
    Standalone,
    /// Future: decides for everyone and sends the records.
    Host,
    /// Future: never decides; mirrors the host's records.
    Client,
}

/// Per-kind switch and density (1.0 = retail).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct KindSetting {
    pub enabled: bool,
    pub density: f32,
}

impl Default for KindSetting {
    fn default() -> Self {
        Self { enabled: true, density: 1.0 }
    }
}

/// Settings and mod overrides. Defaults = retail.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct LivingWorldSettings {
    pub enabled: bool,
    pub skaters: KindSetting,
    pub pedestrians: KindSetting,
    pub vehicles: KindSetting,
    /// Desired ambient NPC skaters offline (retail 3).
    pub ambient_skaters: u32,
    /// Free Play options (mode 3); `None` = career free roam (no scaling). The Free Play menu is
    /// a later milestone.
    pub free_play: Option<FreePlay>,
    /// The zombie cheat (later milestone; peds without cap, no traffic, no NPC skaters).
    pub zombie: bool,
    pub net_role: NetRole,
    /// Session seed (0 = derive from the map name).
    pub seed: u64,
    /// Log a population summary every 5 s (`SKATE_LIVING_WORLD_DEBUG=1`).
    pub debug: bool,
}

impl Default for LivingWorldSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            skaters: KindSetting::default(),
            pedestrians: KindSetting::default(),
            vehicles: KindSetting::default(),
            ambient_skaters: skate_core::living_world::config::retail::SKATER_DESIRED,
            free_play: None,
            zombie: false,
            net_role: NetRole::Standalone,
            seed: 0,
            debug: false,
        }
    }
}

impl LivingWorldSettings {
    fn from_env() -> Self {
        let flag = |k: &str| std::env::var(k).ok();
        Self {
            enabled: flag("SKATE_LIVING_WORLD").as_deref() != Some("0"),
            debug: flag("SKATE_LIVING_WORLD_DEBUG").as_deref() == Some("1"),
            ..Self::default()
        }
    }

    /// Write the settings into the core config (code defaults and data ranges stay).
    pub(crate) fn apply(&self, config: &mut PopulationConfig) {
        config.skaters.enabled = self.enabled && self.skaters.enabled;
        config.skaters.desired = (self.ambient_skaters as f32 * self.skaters.density.max(0.0)).round() as u32;
        config.pedestrians.enabled = self.enabled && self.pedestrians.enabled;
        config.pedestrians.density = self.pedestrians.density;
        config.vehicles.enabled = self.enabled && self.vehicles.enabled;
        config.vehicles.density = self.vehicles.density;
    }
}

/// A spawn decision for the engine (bodies, rendering, audio publishers consume it).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) struct LivingWorldSpawn(pub SpawnRecord);

/// A despawn decision.
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) struct LivingWorldDespawn(pub DespawnRecord);

/// Serialisable form of a decision (what a host would send; mod events use the same fields).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum WireRecord {
    Spawn { kind: String, id: u64, tick: u64, position: [f32; 3], heading: f32, seed: u64, initial: bool, choice: WireChoice },
    Despawn { kind: String, id: u64, tick: u64, reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum WireChoice {
    Census { record: String, category: String },
    Skater { line: String, character: String, slot: u8 },
    /// Census car (milestone V2): stable names plus the lane (retail segment id, lane, distance).
    Vehicle { record: String, category: String, entity: String, model: String, chassis: u32, secondary: u32, segment: u64, lane: u8, distance: f32 },
}

fn reason_name(r: DespawnReason) -> &'static str {
    match r {
        DespawnReason::Distance => "distance",
        DespawnReason::Excess => "excess",
        DespawnReason::FreePlayOff => "free_play_off",
        DespawnReason::Disabled => "disabled",
        DespawnReason::External => "external",
    }
}

impl WireRecord {
    pub(crate) fn from_decision(d: &Decision) -> Self {
        match d {
            Decision::Spawn(s) => WireRecord::Spawn {
                kind: s.id.kind.name().into(),
                id: s.id.to_u64(),
                tick: s.tick,
                position: s.position,
                heading: s.heading,
                seed: s.seed,
                initial: s.initial,
                choice: match &s.choice {
                    SpawnChoice::Census { record, category } => WireChoice::Census { record: record.clone(), category: category.clone() },
                    SpawnChoice::Vehicle { record, category, entity, model, chassis, secondary, segment, lane, distance } => WireChoice::Vehicle {
                        record: record.clone(),
                        category: category.clone(),
                        entity: entity.clone(),
                        model: model.clone(),
                        chassis: *chassis,
                        secondary: *secondary,
                        segment: *segment,
                        lane: *lane,
                        distance: *distance,
                    },
                    SpawnChoice::Skater { line, character, slot } => {
                        WireChoice::Skater { line: line.iter().map(|b| format!("{b:02x}")).collect(), character: character.clone(), slot: *slot }
                    }
                },
            },
            Decision::Despawn(r) => WireRecord::Despawn { kind: r.id.kind.name().into(), id: r.id.to_u64(), tick: r.tick, reason: reason_name(r.reason).into() },
        }
    }

    pub(crate) fn to_decision(&self) -> Option<Decision> {
        Some(match self {
            WireRecord::Spawn { id, tick, position, heading, seed, initial, choice, .. } => Decision::Spawn(SpawnRecord {
                id: LivingWorldId::from_u64(*id)?,
                tick: *tick,
                position: *position,
                heading: *heading,
                seed: *seed,
                initial: *initial,
                choice: match choice {
                    WireChoice::Census { record, category } => SpawnChoice::Census { record: record.clone(), category: category.clone() },
                    WireChoice::Vehicle { record, category, entity, model, chassis, secondary, segment, lane, distance } => SpawnChoice::Vehicle {
                        record: record.clone(),
                        category: category.clone(),
                        entity: entity.clone(),
                        model: model.clone(),
                        chassis: *chassis,
                        secondary: *secondary,
                        segment: *segment,
                        lane: *lane,
                        distance: *distance,
                    },
                    WireChoice::Skater { line, character, slot } => {
                        let mut id = [0u8; 16];
                        for (i, b) in id.iter_mut().enumerate() {
                            *b = u8::from_str_radix(line.get(2 * i..2 * i + 2)?, 16).ok()?;
                        }
                        SpawnChoice::Skater { line: id, character: character.clone(), slot: *slot }
                    }
                },
            }),
            WireRecord::Despawn { id, tick, reason, .. } => Decision::Despawn(DespawnRecord {
                id: LivingWorldId::from_u64(*id)?,
                tick: *tick,
                reason: match reason.as_str() {
                    "distance" => DespawnReason::Distance,
                    "excess" => DespawnReason::Excess,
                    "free_play_off" => DespawnReason::FreePlayOff,
                    "disabled" => DespawnReason::Disabled,
                    _ => DespawnReason::External,
                },
            }),
        })
    }
}

/// The population of the current world plus the loaded data.
#[derive(Resource)]
pub(crate) struct PopulationState {
    pub world: LivingWorld,
    pub census: Option<CensusMap>,
    pub skaters: Option<SkaterData>,
    /// Road network and vehicle entities of the loaded world (milestone V2: cars are placed on
    /// lanes; none = no cars).
    pub roads: Option<skate_core::living_world::traffic::RoadNetwork>,
    pub vehicles: Option<skate_core::living_world::VehicleCatalog>,
    /// Replay-tier lines and voices of the loaded district (milestone 3).
    pub npc: npc_skaters::NpcData,
    /// The ranges from `tables.json` (re-applied after settings changes).
    data_config: PopulationConfig,
    /// (map name, generation) the data was loaded for.
    loaded_for: Option<(String, u64)>,
    pub status: String,
    pub spawned: u64,
    pub despawned: u64,
    last_report: u64,
}

impl Default for PopulationState {
    fn default() -> Self {
        Self {
            world: LivingWorld::new(PopulationConfig::retail(), 0),
            census: None,
            skaters: None,
            roads: None,
            vehicles: None,
            npc: npc_skaters::NpcData::default(),
            data_config: PopulationConfig::retail(),
            loaded_for: None,
            status: "no living-world data".into(),
            spawned: 0,
            despawned: 0,
            last_report: 0,
        }
    }
}

impl PopulationState {
    /// A new world: fresh population (seeded), new data.
    pub(crate) fn install(&mut self, map: &str, generation: u64, settings: &LivingWorldSettings, data: LoadedData) {
        let seed = if settings.seed != 0 { settings.seed } else { skate_core::living_world::rng::derive(0x5345_4544, &[name_hash(map), generation]) };
        self.data_config = data.config;
        self.world = LivingWorld::new(self.data_config.clone(), seed);
        self.census = data.census;
        self.skaters = data.skaters;
        self.roads = data.roads;
        self.vehicles = data.vehicles;
        self.npc = data.npc;
        self.status = data.status;
        self.loaded_for = Some((map.to_string(), generation));
    }

    /// Client side: mirror records from a host (no rules, no RNG). Transport is not built.
    pub(crate) fn apply_records(&mut self, records: &[WireRecord]) -> Vec<Decision> {
        let decisions: Vec<Decision> = records.iter().filter_map(WireRecord::to_decision).collect();
        for d in &decisions {
            self.world.apply(d);
        }
        decisions
    }
}

fn name_hash(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Data for one world.
pub(crate) struct LoadedData {
    pub config: PopulationConfig,
    pub census: Option<CensusMap>,
    pub skaters: Option<SkaterData>,
    pub roads: Option<skate_core::living_world::traffic::RoadNetwork>,
    pub vehicles: Option<skate_core::living_world::VehicleCatalog>,
    pub npc: npc_skaters::NpcData,
    pub status: String,
}

/// Read the export for a district (map name = district name for the retail cities).
pub(crate) fn load_data(asset_root: &Path, district: &str) -> LoadedData {
    let dir = asset_root.join("private/living_world");
    let mut config = PopulationConfig::retail();
    let mut status = Vec::new();
    let tables = std::fs::read(dir.join("tables.json")).ok().and_then(|b| match skate_data::living_world::LivingWorldTables::parse(&b) {
        Ok(t) => Some(t),
        Err(e) => {
            status.push(e.to_string());
            None
        }
    });
    let census = tables.as_ref().and_then(|t| {
        t.apply_to(&mut config);
        let bytes = std::fs::read(dir.join(format!("{district}.census.bin"))).ok()?;
        match skate_data::living_world::parse_census_grid(&bytes) {
            Ok(g) => Some(t.census_map(vec![g])),
            Err(e) => {
                status.push(e.to_string());
                None
            }
        }
    });
    let mut npc = npc_skaters::NpcData::default();
    let skaters = (|| {
        let profiles = std::fs::read(dir.join("skater_profiles.json")).ok()?;
        let characters = skate_data::living_world::skater_characters(&profiles, &[]).ok()?;
        npc.voices = npc_voices(&profiles);
        let pack = std::fs::read(dir.join("skater_paths").join(format!("{district}.bin"))).ok()?;
        let tiles = skate_data::aipath::parse_pack(&pack).ok()?;
        let (paths, _) = skate_data::aipath::district_paths(&tiles).ok()?;
        let lines = skate_data::living_world::skater_lines(paths.iter().map(|p| &p.path));
        npc.lines = std::sync::Arc::new(paths.iter().filter(|p| p.path.id.is_ambient()).map(|p| (p.path.id.0, skate_data::living_world::replay_line(&p.path))).collect());
        Some(SkaterData { lines, characters })
    })();
    status.insert(
        0,
        format!(
            "{district}: census {}, skater lines {}",
            census.as_ref().map_or("none".to_string(), |c| format!("{} records", c.records.len())),
            skaters.as_ref().map_or(0, |s| s.lines.len())
        ),
    );
    // Roads and vehicle entities (milestone V2). The graph holds every district; the census
    // places cars only where the district's vehicle layer is painted.
    let roads = std::fs::read(dir.join("roads.bin")).ok().and_then(|b| {
        let built = skate_data::roads::RoadGraph::parse(&b)
            .map_err(|e| e.to_string())
            .and_then(|g| skate_core::living_world::traffic::RoadNetwork::build(&g.traffic_input()).map_err(|e| e.to_string()));
        built.map_err(|e| status.push(format!("roads: {e}"))).ok()
    });
    let vehicles = std::fs::read(dir.join("vehicles.json")).ok().and_then(|b| skate_data::living_world::vehicle_catalog(&b).map_err(|e| status.push(e.to_string())).ok());
    if let Some(first) = status.first_mut() {
        first.push_str(&format!(", roads {}, vehicle entities {}", roads.as_ref().map_or(0, |r| r.segments.len()), vehicles.as_ref().map_or(0, |v| v.entities.len())));
    }
    LoadedData { config, census, skaters, roads, vehicles, npc, status: status.join("; ") }
}

/// `characters_marquee` voice ids by character key (`skater_profiles.json` `characters.*.voice`).
pub(crate) fn npc_voices(profiles: &[u8]) -> std::collections::BTreeMap<String, u32> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(profiles) else { return Default::default() };
    v["characters"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, c)| c["voice"].as_u64().map(|x| (k.clone(), x as u32)))
        .collect()
}

/// The observers this frame: the local skater (deck position and velocity). Remote players join
/// here once the host role exists.
#[derive(Resource, Default, Clone)]
pub(crate) struct LivingWorldObservers {
    pub observers: Vec<Observer>,
    /// Skater slots held by players (local + remote).
    pub player_slots: u32,
    pub online: bool,
}

fn gather_observers(
    physics: Res<crate::physics::GamePhysics>,
    multiplayer: Option<Res<crate::multiplayer::Multiplayer>>,
    mut out: ResMut<LivingWorldObservers>,
) {
    use skate_core::physics::board::BodyId;
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates;
    let v = |x: skate_core::math::Vector3| [x.x, x.y, x.z];
    out.observers = vec![Observer { position: v(deck.position), velocity: v(deck.linear_velocity) }];
    let online = multiplayer.as_ref().is_some_and(|m| m.active());
    out.online = online;
    out.player_slots = if online { multiplayer.map_or(1, |m| m.player_ids().len().max(1) as u32) } else { 1 };
}

fn load_for_map(
    config: Res<crate::config::Config>,
    map: Res<crate::map_transition::CurrentMap>,
    settings: Res<LivingWorldSettings>,
    mut state: ResMut<PopulationState>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
) {
    let key = (map.name.clone(), map.generation);
    if state.loaded_for.as_ref() == Some(&key) {
        return;
    }
    for d in state.world.reset_world() {
        if let Decision::Despawn(r) = d {
            despawns.write(LivingWorldDespawn(r));
        }
    }
    let data = load_data(&config.asset_root, &map.name);
    info!("LIVING_WORLD data {}", data.status);
    // NPC skaters will publish board audio and speech here: let the world banks decode early.
    if let Some(mut audio) = audio {
        audio.expected = settings.enabled && !data.npc.lines.is_empty();
    }
    state.install(&map.name, map.generation, &settings, data);
}

/// One fixed step: settings into the config, then the console ticks that are due.
pub(crate) fn step_population(
    time: Res<Time>,
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    mut state: ResMut<PopulationState>,
    mut spawns: MessageWriter<LivingWorldSpawn>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
) {
    if settings.net_role == NetRole::Client {
        return;
    }
    let state = &mut *state;
    let mut config = state.data_config.clone();
    settings.apply(&mut config);
    state.world.config = config;
    let inputs = TickInputs {
        observers: &observers.observers,
        census: state.census.as_ref(),
        skater_world: state.skaters.as_ref().map(|s| s as &dyn skate_core::living_world::SkaterWorld),
        roads: state.roads.as_ref(),
        vehicles: state.vehicles.as_ref(),
        online: observers.online,
        zombie: settings.zombie,
        free_play: settings.free_play,
        player_slots: observers.player_slots.max(1),
        scripted_skaters: 0,
        ambient_skater_override: None,
        world_ready: true,
    };
    let decisions = state.world.advance(time.delta_secs_f64(), &inputs);
    for d in decisions {
        match d {
            Decision::Spawn(s) => {
                state.spawned += 1;
                spawns.write(LivingWorldSpawn(s));
            }
            Decision::Despawn(r) => {
                state.despawned += 1;
                despawns.write(LivingWorldDespawn(r));
            }
        }
    }
    // Every 5 s of the 60 Hz world tick.
    if settings.debug && state.world.tick() >= state.last_report + 300 {
        state.last_report = state.world.tick();
        info!("{}", readout(state));
    }
}

/// One-line population summary (debug log; the overlay milestone can show the same text).
pub(crate) fn readout(state: &PopulationState) -> String {
    let w = &state.world;
    format!(
        "LIVING_WORLD tick {} skaters {} peds {} vehicles {} pool {:?} spawned {} despawned {} ({})",
        w.tick(),
        w.count(Kind::Skater),
        w.count(Kind::Pedestrian),
        w.count(Kind::Vehicle),
        w.skater_pool(),
        state.spawned,
        state.despawned,
        state.status
    )
}

pub(crate) struct LivingWorldPlugin;

impl Plugin for LivingWorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LivingWorldSettings::from_env())
            .init_resource::<PopulationState>()
            .init_resource::<LivingWorldObservers>()
            .add_message::<LivingWorldSpawn>()
            .add_message::<LivingWorldDespawn>()
            .add_systems(
                FixedUpdate,
                (load_for_map, gather_observers, step_population).chain().after(crate::app::SimulationSet::Physics),
            );
        npc_skaters::install(app);
        peds::install(app);
    }
}
