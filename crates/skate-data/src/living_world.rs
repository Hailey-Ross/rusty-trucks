//! Readers for the living-world export the population core needs (setup group `livingworld`,
//! formats in `docs/hails-additions/living-world/{peds-data,skaters-data}.md`). Bytes in, core
//! types out; no file I/O.
//!
//! - `<District>.census.bin` → [`CensusGrid`] (`tools/asset_pipeline/living_world.py`,
//!   `write_census_grid`);
//! - `tables.json` → [`LivingWorldTables`]: `livingworld_census` records resolved through their
//!   category group, and the `livingworld_census_ranges` circles;
//! - `skater_profiles.json` → [`SkaterCharacter`]s of the free-roam pool;
//! - decoded AIPATH lines (`aipath`) → [`SkaterLine`]s.

use crate::aipath::AiPath;
use serde_json::Value;
use skate_core::living_world::census::CensusCategory;
use skate_core::living_world::{CensusCircle, CensusGrid, CensusMap, CensusRange, CensusRecord, PopulationConfig, SkaterCharacter, SkaterLine};
use std::collections::BTreeMap;
use std::fmt;

pub const CENSUS_MAGIC: &[u8; 8] = b"LWCENSUS";
pub const CENSUS_VERSION: u32 = 1;
/// Range records the census kinds use (`livingworld_census_ranges`, census `+104` / `+112`).
pub const RANGE_PEDESTRIANS: &str = "pedestrians";
pub const RANGE_VEHICLES: &str = "vehicles";

#[derive(Debug, Clone, PartialEq)]
pub enum LivingWorldError {
    Census(String),
    Tables(String),
    Profiles(String),
}

impl fmt::Display for LivingWorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Census(m) => write!(f, "living-world census grid: {m}"),
            Self::Tables(m) => write!(f, "living-world tables: {m}"),
            Self::Profiles(m) => write!(f, "living-world skater profiles: {m}"),
        }
    }
}
impl std::error::Error for LivingWorldError {}

fn u32_at(d: &[u8], at: usize) -> Result<u32, LivingWorldError> {
    d.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| LivingWorldError::Census(format!("truncated at {at}")))
}
fn f32_at(d: &[u8], at: usize) -> Result<f32, LivingWorldError> {
    u32_at(d, at).map(f32::from_bits)
}

/// Parse a `<District>.census.bin` grid.
pub fn parse_census_grid(data: &[u8]) -> Result<CensusGrid, LivingWorldError> {
    let err = |m: String| LivingWorldError::Census(m);
    if data.get(..8) != Some(&CENSUS_MAGIC[..]) {
        return Err(err("not a living-world census grid".into()));
    }
    let version = u32_at(data, 8)?;
    if version != CENSUS_VERSION {
        return Err(err(format!("unsupported version {version}")));
    }
    let cell = f32_at(data, 12)?;
    let origin = [f32_at(data, 16)?, f32_at(data, 20)?];
    let (width, height) = (u32_at(data, 24)?, u32_at(data, 28)?);
    let (layer_count, name_count) = (u32_at(data, 32)? as usize, u32_at(data, 36)? as usize);
    let cells = width as usize * height as usize;
    if !(cell > 0.0) || cells > 64 << 20 || layer_count > 64 {
        return Err(err(format!("bad header (cell {cell}, {width} x {height}, {layer_count} layers)")));
    }
    let mut cursor = 40;
    let mut table = Vec::new();
    for _ in 0..layer_count {
        let raw = data.get(cursor..cursor + 32).ok_or_else(|| err("truncated layer table".into()))?;
        let name = String::from_utf8_lossy(raw.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
        table.push((name, u32_at(data, cursor + 32)? as usize));
        cursor += 36;
    }
    let mut names = Vec::with_capacity(name_count);
    for _ in 0..name_count {
        let len = data.get(cursor..cursor + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(|| err("truncated names".into()))?;
        let raw = data.get(cursor + 2..cursor + 2 + len).ok_or_else(|| err("truncated name".into()))?;
        names.push(String::from_utf8(raw.to_vec()).map_err(|_| err("name is not text".into()))?);
        cursor += 2 + len;
    }
    let mut layers = BTreeMap::new();
    for (name, offset) in table {
        let raw = data.get(offset..offset + 2 * cells).ok_or_else(|| err(format!("layer {name} out of range")))?;
        let values: Vec<u16> = raw.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        if let Some(bad) = values.iter().find(|&&v| v as usize > names.len()) {
            return Err(err(format!("layer {name} names record {bad} of {}", names.len())));
        }
        layers.insert(name, values);
    }
    Ok(CensusGrid { cell, origin, width, height, names, layers })
}

/// The census part of `tables.json`, resolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LivingWorldTables {
    /// `livingworld_census` records by name: max population and the group's categories.
    pub census: BTreeMap<String, CensusRecord>,
    /// `livingworld_census_ranges` records by name.
    pub ranges: BTreeMap<String, CensusRange>,
}

fn circle(v: &Value) -> Option<CensusCircle> {
    let f = |k: &str| v.get(k).and_then(Value::as_f64).map(|x| x as f32);
    Some(CensusCircle {
        spawn_inner: f("spawn_inner")?,
        spawn_outer: f("spawn_outer")?,
        cull: f("cull")?,
        forward_offset: f("forward_offset")?,
        speed_kmh: f("speed_kmh")?,
    })
}

impl LivingWorldTables {
    pub fn parse(json: &[u8]) -> Result<Self, LivingWorldError> {
        let doc: Value = serde_json::from_slice(json).map_err(|e| LivingWorldError::Tables(e.to_string()))?;
        Self::from_value(&doc)
    }

    pub fn from_value(doc: &Value) -> Result<Self, LivingWorldError> {
        let classes = doc.get("classes").ok_or_else(|| LivingWorldError::Tables("no classes".into()))?;
        let class = |name: &str| classes.get(name).and_then(Value::as_object);
        let groups = class("livingworld_categorygroups");
        let mut census = BTreeMap::new();
        for (key, row) in class("livingworld_census").into_iter().flatten() {
            let entry = &row["fields"]["entry"];
            let Some(max) = entry.get("max_population").and_then(Value::as_u64) else { continue };
            let group = entry["group"]["key"].as_str();
            let categories = group
                .and_then(|g| groups.and_then(|gs| gs.get(g)))
                .and_then(|g| g["fields"]["categories"].as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| Some(CensusCategory { name: i["category"]["key"].as_str()?.to_string(), weight: i["weight"].as_f64()? as f32 }))
                        .collect()
                })
                .unwrap_or_default();
            census.insert(key.clone(), CensusRecord { max_population: max as u32, categories });
        }
        let mut ranges = BTreeMap::new();
        for (key, row) in class("livingworld_census_ranges").into_iter().flatten() {
            let f = &row["fields"];
            if let (Some(slow), Some(fast)) = (circle(&f["circle_slow"]), circle(&f["circle_fast"])) {
                ranges.insert(key.clone(), CensusRange { slow, fast });
            }
        }
        if census.is_empty() || ranges.is_empty() {
            return Err(LivingWorldError::Tables("no census records or ranges".into()));
        }
        Ok(Self { census, ranges })
    }

    /// Fill the data side of a population config (ranges); code defaults stay.
    pub fn apply_to(&self, config: &mut PopulationConfig) {
        config.pedestrians.range = self.ranges.get(RANGE_PEDESTRIANS).copied();
        config.vehicles.range = self.ranges.get(RANGE_VEHICLES).copied();
    }

    /// A census map over the given district grids.
    pub fn census_map(&self, grids: Vec<CensusGrid>) -> CensusMap {
        CensusMap { grids, records: self.census.clone() }
    }
}

/// Free-roam pool characters from `skater_profiles.json`. Characters that need a recruited
/// teammate binding (`needs = recruited_save_slot`) are left out unless `bound` names them.
pub fn skater_characters(json: &[u8], bound: &[&str]) -> Result<Vec<SkaterCharacter>, LivingWorldError> {
    let doc: Value = serde_json::from_slice(json).map_err(|e| LivingWorldError::Profiles(e.to_string()))?;
    let pool = doc["free_roam_pool"].as_array().ok_or_else(|| LivingWorldError::Profiles("no free_roam_pool".into()))?;
    let chars = &doc["characters"];
    let mut out = Vec::new();
    for key in pool.iter().filter_map(Value::as_str) {
        let c = &chars[key];
        if c["needs"].as_str() == Some("recruited_save_slot") && !bound.contains(&key) {
            continue;
        }
        out.push(SkaterCharacter {
            key: key.to_string(),
            pro_index: c["pro_index"].as_u64().map(|p| p as u32),
            capabilities: [false; 3],
            community: c["community"].as_bool().unwrap_or(false),
        });
    }
    Ok(out)
}

/// Population view of decoded lines: ambient lines only, start = node 0, heading from node 0
/// towards the next node that is apart from it (the node orientation's component order is not
/// confirmed yet).
pub fn skater_lines<'a>(paths: impl IntoIterator<Item = &'a AiPath>) -> Vec<SkaterLine> {
    paths
        .into_iter()
        .filter(|p| p.id.is_ambient())
        .filter_map(|p| {
            let start = p.start()?;
            let next = p.nodes.iter().skip(1).map(|n| n.position).find(|q| (q[0] - start[0]).hypot(q[2] - start[2]) > 0.05);
            let heading = next.map_or(0.0, |q| (q[0] - start[0]).atan2(q[2] - start[2]));
            Some(SkaterLine { id: p.id.0, start, heading, valid: true, allowed_skaters: p.allowed_skaters, flags: p.flags })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic grid in the exporter's layout (mirrors `write_census_grid`).
    pub(crate) fn synth_grid(names: &[&str], layers: &[(&str, Vec<u16>)], w: u32, h: u32) -> Vec<u8> {
        let mut head = Vec::new();
        head.extend_from_slice(CENSUS_MAGIC);
        for v in [CENSUS_VERSION.to_le_bytes(), 4.0f32.to_le_bytes(), (-8.0f32).to_le_bytes(), 0.0f32.to_le_bytes(), w.to_le_bytes(), h.to_le_bytes(), (layers.len() as u32).to_le_bytes(), (names.len() as u32).to_le_bytes()] {
            head.extend_from_slice(&v);
        }
        let mut name_bytes = Vec::new();
        for n in names {
            name_bytes.extend_from_slice(&(n.len() as u16).to_le_bytes());
            name_bytes.extend_from_slice(n.as_bytes());
        }
        let header = 40 + 36 * layers.len();
        let mut cursor = header + name_bytes.len();
        cursor += (4 - cursor % 4) % 4;
        let mut table = Vec::new();
        let mut body = Vec::new();
        for (name, cells) in layers {
            let mut n = name.as_bytes().to_vec();
            n.resize(32, 0);
            table.extend_from_slice(&n);
            table.extend_from_slice(&((cursor + body.len()) as u32).to_le_bytes());
            for c in cells {
                body.extend_from_slice(&c.to_le_bytes());
            }
            while body.len() % 4 != 0 {
                body.push(0);
            }
        }
        let mut blob = [head, table, name_bytes].concat();
        while blob.len() % 4 != 0 {
            blob.push(0);
        }
        blob.extend_from_slice(&body);
        blob
    }

    #[test]
    fn census_grid_round_trip_and_lookup() {
        // 3 x 2 cells of 4 m from (-8, 0): x in [-8, 4), z in [0, 8).
        let data = synth_grid(&["aletown", "dwntwn"], &[("livingworld_npc_census", vec![0, 1, 1, 1, 0, 0]), ("livingworld_vehicle_census", vec![2; 6])], 3, 2);
        let g = parse_census_grid(&data).unwrap();
        assert_eq!((g.width, g.height, g.cell, g.origin), (3, 2, 4.0, [-8.0, 0.0]));
        assert_eq!(g.record_at("livingworld_npc_census", -7.0, 1.0), None);
        assert_eq!(g.record_at("livingworld_npc_census", -3.0, 1.0), Some("aletown"));
        assert_eq!(g.record_at("livingworld_npc_census", -7.0, 5.0), Some("aletown"));
        assert_eq!(g.record_at("livingworld_npc_census", 1.0, 5.0), None);
        assert_eq!(g.record_at("livingworld_vehicle_census", 3.9, 7.9), Some("dwntwn"));
        assert_eq!(g.record_at("livingworld_vehicle_census", 4.0, 1.0), None);
        assert!(parse_census_grid(&data[..50]).is_err());
        assert!(parse_census_grid(b"NOTCENSUS0000000").is_err());
    }

    #[test]
    fn tables_resolve_census_records_and_ranges() {
        let json = br#"{"version":1,"classes":{
            "livingworld_census":{"aletown":{"parent":"downtown","fields":{"entry":{"group":{"class":"livingworld_categorygroups","key":"aletown"},"max_population":15}}},
                                  "broken":{"parent":null,"fields":{}}},
            "livingworld_categorygroups":{"aletown":{"parent":null,"fields":{"categories":[{"category":{"class":"livingworld_entitycategories","key":"aletown"},"weight":1.0}]}}},
            "livingworld_census_ranges":{"pedestrians":{"parent":null,"fields":{
                "circle_slow":{"spawn_inner":50.0,"spawn_outer":60.0,"cull":70.0,"forward_offset":0.0,"speed_kmh":45.0},
                "circle_fast":{"spawn_inner":50.0,"spawn_outer":80.0,"cull":90.0,"forward_offset":20.0,"speed_kmh":80.0}}}}}}"#;
        let t = LivingWorldTables::parse(json).unwrap();
        assert_eq!(t.census["aletown"].max_population, 15);
        assert_eq!(t.census["aletown"].categories[0].name, "aletown");
        assert!(!t.census.contains_key("broken"));
        let mut cfg = PopulationConfig::retail();
        t.apply_to(&mut cfg);
        assert_eq!(cfg.pedestrians.range.unwrap().slow.cull, 70.0);
        assert!(cfg.vehicles.range.is_none());
    }
}
