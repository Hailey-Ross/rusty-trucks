//! Pedestrian body data (doc 26, peds milestone M2): the ped animation bank and the look /
//! animation tables, read into the pure `skate_core::living_world::peds` types.
//!
//! - Bank: `data/anim/PedestrianSkeletonPres.abin` from the user's stock data (462 VBR clips, one
//!   50-bone rig with trajectory, 10 parts) [data]. Its clips carry only the first 6 parts (bones
//!   0..=26), so the skater's full-hierarchy loader (`animation_frames`) does not apply; this
//!   module decodes the parts a clip has with the same core VBR decoder and leaves the other
//!   bones at identity. The reference pose is the bank's `PEDESTRIAN_RIG_TPOSE` pose record.
//!   Clip attributes (`LEFTTOEDOWN`, `RIGHTTOEDOWN`, `LEFTHEELDOWN`, `RIGHTHEELDOWN`,
//!   `BODYFALLTYPE`, ...) come from `animation_metadata` (first payload word = the value).
//! - Tables (`private/living_world/tables.json`): `livingworld_entitycategories.entities`,
//!   `livingworld_entities` (`model`, the `livingworld_entity_animation` ref), `livingworld_models`
//!   (`recipe`, `voice`, `tints_a` / `tints_b`, distance pairs) and the animation sets, whose
//!   logical names are vault hashes (`attrib_hash::hash("FwdWalkCyc")` = `Hash_4AA12E0083F10739`)
//!   and whose `tAnimAttributes` entries name their clip through `anim_name` (setup resolves the
//!   string-pool offset, `living_world_anim.py`).

use crate::abin::{Bank, RecordData};
use crate::animation_metadata::AnimationMetadata;
use serde_json::Value;
use skate_core::animation::output::Sqt;
use skate_core::animation::vbr::VbrDecoder;
use skate_core::living_world::peds::anim::{ClipWindow, IDENTITY, PedAnimSet, PedClip, PedRig, RemapClip, names};
use skate_core::living_world::peds::{PedCatalog, PedEntity, PedModel};
use std::collections::BTreeMap;
use std::path::Path;

/// The bank under an asset root.
pub const PED_BANK: &str = "private/stock/data/anim/PedestrianSkeletonPres.abin";
/// The bank's reference pose record.
pub const REFERENCE_POSE: &str = "PEDESTRIAN_RIG_TPOSE";

fn sqt(words: [u32; 10]) -> Sqt {
    let [sx, sy, sz, x, y, z, w, tx, ty, tz] = words.map(f32::from_bits);
    Sqt { scale: [sx, sy, sz, 1.0], rotation: [x, y, z, w], translation: [tx, ty, tz, 1.0] }
}

/// The ped animation bank: rig, reference pose, clip decoding.
pub struct PedBank {
    pub bank: Bank,
    pub metadata: AnimationMetadata,
    pub rig: PedRig,
}

impl PedBank {
    pub fn load(asset_root: &Path) -> Result<Self, String> {
        let path = asset_root.join(PED_BANK);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(bytes)
    }

    pub fn parse(bytes: Vec<u8>) -> Result<Self, String> {
        let bank = Bank::parse(bytes).map_err(|e| e.to_string())?;
        let metadata = AnimationMetadata::from_bank(&bank, "PedestrianSkeletonPres.abin".into(), "0".repeat(64))?;
        let h = bank.hierarchy().ok_or("ped bank has no hierarchy")?.clone();
        let n = h.bone_count as usize;
        let mut rig = PedRig { names: h.bone_names.clone(), parents: h.parents.clone(), mirrors: h.mirrors.clone(), reference: vec![IDENTITY; n], animated: vec![false; n] };
        let mut this = Self { bank, metadata, rig: PedRig::default() };
        if let Some((_, pose)) = this.bank.pose(REFERENCE_POSE) {
            let parts = pose.parts.clone();
            let (frames, present) = this.decode_parts(&parts, 1)?;
            for (i, p) in present.iter().enumerate() {
                if *p {
                    rig.reference[i] = frames[0][i];
                }
            }
        } else {
            return Err(format!("ped bank has no {REFERENCE_POSE} pose"));
        }
        // Bones every clip carries: the parts all clips have.
        let parts = this.bank.records().iter().filter_map(|r| if let RecordData::Clip(c) = &r.data { Some(c.parts.len()) } else { None }).min().unwrap_or(0);
        for layout in h.parts.iter().take(parts) {
            for b in layout.sqt_offset as usize..(layout.sqt_offset as usize + layout.bone_count as usize).min(n) {
                rig.animated[b] = true;
            }
        }
        this.rig = rig;
        Ok(this)
    }

    /// Decode the parts present (clip part i = hierarchy part i); absent bones stay identity.
    fn decode_parts(&self, parts: &[crate::abin::PartEntry], frames: usize) -> Result<(Vec<Vec<Sqt>>, Vec<bool>), String> {
        let h = self.bank.hierarchy().ok_or("no hierarchy")?;
        let n = h.bone_count as usize;
        let mut out = vec![vec![IDENTITY; n]; frames];
        let mut present = vec![false; n];
        for (entry, layout) in parts.iter().zip(&h.parts) {
            let Some(part) = &entry.part else { continue };
            let mut d = VbrDecoder::new(self.bank.bytes(), part.offset, part.compression_header_relative as usize, part.compressed_data_relative as usize)?;
            if d.frame_count() != frames || d.channel_count() != layout.bone_count as usize {
                return Err(format!("{}: part frame / channel count differs from the header", layout.name));
            }
            let start = layout.sqt_offset as usize;
            for (f, frame) in out.iter_mut().enumerate() {
                for (i, w) in d.decode_frame(f)?.into_iter().enumerate() {
                    if let Some(slot) = frame.get_mut(start + i) {
                        *slot = sqt(w);
                    }
                }
            }
            for p in present.iter_mut().skip(start).take(layout.bone_count as usize) {
                *p = true;
            }
        }
        Ok((out, present))
    }

    pub fn clip_names(&self) -> Vec<String> {
        self.bank.records().iter().filter(|r| matches!(r.data, RecordData::Clip(_))).map(|r| r.header.name.clone()).collect()
    }

    /// Decode one clip with its attribute windows.
    pub fn clip(&self, name: &str) -> Result<PedClip, String> {
        let (header, clip) = self.bank.clip(name).ok_or_else(|| format!("ped clip {name} not in the bank"))?;
        let frames = f32::from_bits(clip.frame_count_bits);
        if !frames.is_finite() || frames < 1.0 || frames.fract() != 0.0 {
            return Err(format!("{name}: invalid frame count"));
        }
        let (frames, _) = self.decode_parts(&clip.parts, frames as usize).map_err(|e| format!("{name}: {e}"))?;
        let meta = self.metadata.clip(&header.name)?;
        let windows = meta
            .attributes
            .iter()
            .map(|a| ClipWindow {
                channel: a.name.clone(),
                begin: f32::from_bits(a.begin_bits),
                end: f32::from_bits(a.end_bits),
                value: a.payload_words.first().map_or(0.0, |w| f32::from_bits(*w)),
            })
            .collect();
        let speed = f32::from_bits(clip.base_speed_bits);
        Ok(PedClip {
            name: header.name.clone(),
            fps: f32::from_bits(clip.fps_bits) * if speed.is_finite() && speed > 0.0 { speed } else { 1.0 },
            frames,
            looping: clip.looping(),
            loop_rotation: clip.loop_rotation_words.map(f32::from_bits),
            loop_translation: [f32::from_bits(clip.loop_translation_words[0]), f32::from_bits(clip.loop_translation_words[1]), f32::from_bits(clip.loop_translation_words[2])],
            windows,
        })
    }
}

/// The look and animation tables M2 reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PedTables {
    pub catalog: PedCatalog,
    /// `livingworld_entity_animation` sets by record name.
    pub anim_sets: BTreeMap<String, PedAnimSet>,
    /// Remap entries that name a clip only by a string-pool offset (tables exported before the
    /// clip-name resolver): re-run setup to fill them.
    pub unresolved: usize,
}

fn f32s(v: &Value) -> Option<[f32; 4]> {
    Some([v["x"].as_f64()? as f32, v["y"].as_f64()? as f32, v["z"].as_f64()? as f32, v["w"].as_f64().unwrap_or(1.0) as f32])
}

fn pair(v: &Value) -> Option<[f32; 2]> {
    Some([v["f32_0"].as_f64()? as f32, v["f32_4"].as_f64()? as f32])
}

/// One remap value: a clip name, a list of names, a `tAnimAttributes` struct or a list of them.
fn remap(v: &Value, unresolved: &mut usize) -> Vec<RemapClip> {
    match v {
        Value::String(s) if !s.is_empty() && !s.contains('/') => vec![RemapClip { clip: s.clone(), windows: vec![] }],
        Value::Array(items) => items.iter().flat_map(|i| remap(i, unresolved)).collect(),
        Value::Object(o) if o.contains_key("anim") => match o.get("anim_name").and_then(Value::as_str) {
            Some(name) => {
                let windows = ["window_0", "window_1", "window_2"]
                    .iter()
                    .filter_map(|k| {
                        let w = o.get(*k)?.as_array()?;
                        let tag = w.get(2)?.as_i64()? as i32;
                        (tag != 0).then_some((w.first()?.as_f64()? as f32, w.get(1)?.as_f64()? as f32, tag))
                    })
                    .collect();
                vec![RemapClip { clip: name.to_string(), windows }]
            }
            None => {
                *unresolved += 1;
                vec![]
            }
        },
        _ => vec![],
    }
}

impl PedTables {
    pub fn parse(json: &[u8]) -> Result<Self, String> {
        let doc: Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
        Self::from_value(&doc)
    }

    pub fn from_value(doc: &Value) -> Result<Self, String> {
        let classes = doc.get("classes").ok_or("tables.json has no classes")?;
        let class = |n: &str| classes.get(n).and_then(Value::as_object);
        let mut t = PedTables::default();
        for (k, row) in class("livingworld_entitycategories").into_iter().flatten() {
            let list = row["fields"]["entities"].as_array().map(|a| a.iter().filter_map(|r| r["key"].as_str().map(String::from)).collect()).unwrap_or_default();
            t.catalog.categories.insert(k.clone(), list);
        }
        for (k, row) in class("livingworld_entities").into_iter().flatten() {
            let fields = row["fields"].as_object();
            let model = row["fields"]["model"]["key"].as_str().map(String::from);
            // The animation set is the one field referencing that class (its key is a hash).
            let anim_set = fields.and_then(|f| f.values().find(|v| v["class"] == "livingworld_entity_animation")).and_then(|v| v["key"].as_str()).map(String::from);
            t.catalog.entities.insert(k.clone(), PedEntity { model, anim_set });
        }
        for (k, row) in class("livingworld_models").into_iter().flatten() {
            let f = &row["fields"];
            let tints = |name: &str| f[name].as_array().map(|a| a.iter().filter_map(f32s).collect()).unwrap_or_default();
            t.catalog.models.insert(
                k.clone(),
                PedModel {
                    parent: row["parent"].as_str().map(String::from),
                    recipe: f["recipe"].as_str().unwrap_or("").to_string(),
                    voice: f["voice"].as_u64().map(|v| v as u32),
                    tints_a: tints("tints_a"),
                    tints_b: tints("tints_b"),
                    lod_near: pair(&f["Hash_73B6874C7B46C7C6"]),
                    lod_far: pair(&f["Hash_9FCFDBEA56BA4733"]),
                },
            );
        }
        let keys: Vec<(String, String)> = names::ALL.iter().map(|n| (crate::attrib_hash::numeric_name(n), n.to_string())).collect();
        for (k, row) in class("livingworld_entity_animation").into_iter().flatten() {
            let mut set = PedAnimSet::default();
            for (hash, name) in &keys {
                if let Some(v) = row["fields"].get(hash) {
                    let list = remap(v, &mut t.unresolved);
                    if !list.is_empty() {
                        set.entries.insert(name.clone(), list);
                    }
                }
            }
            t.anim_sets.insert(k.clone(), set);
        }
        if t.catalog.categories.is_empty() || t.catalog.models.is_empty() {
            return Err("tables.json has no ped categories or models".into());
        }
        Ok(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_names_hash_like_the_export() {
        // Field keys seen in the exported tables [data].
        assert_eq!(crate::attrib_hash::numeric_name("FwdWalkCyc"), "Hash_4AA12E0083F10739");
        assert_eq!(crate::attrib_hash::numeric_name("IdleBasicCyc"), "Hash_ECC7EF02769736DB");
        assert_eq!(crate::attrib_hash::numeric_name("Stand2Walk"), "Hash_8A7544FD90ECFFF7");
    }

    #[test]
    fn tables_resolve_categories_models_and_remaps() {
        let doc = serde_json::json!({"classes": {
            "livingworld_entitycategories": {"aletown": {"parent": null, "fields": {"entities": [{"class": "livingworld_entities", "key": "jock02"}]}}},
            "livingworld_entities": {"jock02": {"parent": "jock", "fields": {"model": {"class": "livingworld_models", "key": "jock02"},
                "Hash_00367B8F33E79C43": {"class": "livingworld_entity_animation", "key": "jock"}}}},
            "livingworld_models": {"jock02": {"parent": "jock", "fields": {"recipe": "male_jock_2", "voice": 55,
                "tints_a": [{"x": 1.0, "y": 0.5, "z": 0.25, "w": 1.0}], "tints_b": [],
                "Hash_73B6874C7B46C7C6": {"f32_0": 45.0, "f32_4": 55.0, "f32_8": 0.0}}}},
            "livingworld_entity_animation": {"jock": {"parent": null, "fields": {
                "Hash_4AA12E0083F10739": {"anim": 52075, "anim_name": "NPC_WNDR_WLK_N_0_CYC", "window_0": [0.0, 0.051, 1], "window_1": [0.4166, 0.584, -1], "window_2": [0.0, 0.0, 0]},
                "Hash_ECC7EF02769736DB": [{"anim": 1, "anim_name": "IDLE_A"}, {"anim": 2}],
                "Hash_8A7544FD90ECFFF7": "NPC_WNDR_STND2WLK_N_0_N"}}}}});
        let t = PedTables::from_value(&doc).unwrap();
        assert_eq!(t.catalog.categories["aletown"], vec!["jock02".to_string()]);
        assert_eq!(t.catalog.entities["jock02"].anim_set.as_deref(), Some("jock"));
        let m = &t.catalog.models["jock02"];
        assert_eq!((m.recipe.as_str(), m.voice, m.tints_a.len(), m.lod_near), ("male_jock_2", Some(55), 1, Some([45.0, 55.0])));
        let set = &t.anim_sets["jock"];
        assert_eq!(set.entries[names::WALK][0].clip, "NPC_WNDR_WLK_N_0_CYC");
        assert_eq!(set.entries[names::WALK][0].windows, vec![(0.0, 0.051, 1), (0.4166, 0.584, -1)]);
        assert_eq!(set.entries[names::IDLE].len(), 1);
        assert_eq!(set.entries[names::START][0].clip, "NPC_WNDR_STND2WLK_N_0_N");
        assert_eq!(t.unresolved, 1);
    }
}
