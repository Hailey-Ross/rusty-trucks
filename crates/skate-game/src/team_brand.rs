//! Team brand billboards (research b1068, b1072): world textures whose image is the player's career photo.
//!
//! Retail: each presentation sector carries binding records {texture asset id, brand tier row}
//! (TeamBrandPresentation_BindRecords 827C9A30 -> BindAsset 827C9BE8). A binding whose tier is unlocked and of kind 2
//! (challenge photo) shows the career photo of the tier's challenge in place of the texture; the bindings are redone on
//! stream-in and after every stored photo (cMsgPresTeamBrandRefresh, 827CA900). A locked tier or a missing photo keeps
//! the authored ad. Ours: setup writes the records beside the map (`<Map>.teambrand`, tools/asset_pipeline/team_brand.py),
//! the world build keeps where each texture sits in its pages (`MapTextureLayers`), and this system writes the photo
//! into those layers (`layered_upload::replace_layer`). Bindings, tiers and slots are data, so a mod's map or tier rows
//! reach it the same way.
use crate::map_render::SceneCommands;
use crate::retail_render::{MaterialTable, TextureLayer};
use bevy::prelude::*;
use skate_core::career::team::TierRow;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Tier kind 2: a challenge's career photo (827C9BE8's switch).
const KIND_CHALLENGE_PHOTO: u32 = 2;
/// Seconds between checks of the bound photos' files (a new photo is written by the capture a few frames after the
/// picker's store; retail refreshes on the store's message).
const CHECK_SECONDS: f32 = 1.0;

#[derive(Component)]
pub(crate) struct MapTextureLayers(pub HashMap<u64, Vec<TextureLayer>>);

pub(crate) fn spawn_layers<B: Bundle>(commands: &mut SceneCommands, table: &mut MaterialTable, tag: B) {
    if !table.texture_layers.is_empty() {
        commands.spawn((MapTextureLayers(std::mem::take(&mut table.texture_layers)), tag));
    }
}

/// One binding record: the texture asset id and the tier row's key hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub texture: u64,
    pub tier: u64,
}

pub(crate) fn read_bindings(path: &Path) -> Vec<Binding> {
    let Some(doc) = std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
        return Vec::new();
    };
    let hex = |v: &serde_json::Value| v.as_str().and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok());
    doc["bindings"].as_array().into_iter().flatten()
        .filter_map(|b| Some(Binding { texture: hex(&b["texture"])?, tier: hex(&b["tier"])? }))
        .collect()
}

/// A row key as the vault stores it: `Hash_<16 hex>`, else the hash of the name.
fn key_hash(key: &str) -> u64 {
    key.strip_prefix("Hash_").and_then(|h| u64::from_str_radix(h, 16).ok())
        .unwrap_or_else(|| skate_data::attrib_hash::hash(key))
}

/// The career photo slot each bound texture shows now: its tier is known, unlocked and a challenge photo tier, and
/// the tier's challenge has a slot (827C9BE8 kind 2 -> 827CA698 -> "TeamBrandTextureManager CareerPhoto <slot>").
pub(crate) fn photo_slots(bindings: &[Binding], tiers: &BTreeMap<String, TierRow>, unlocked: impl Fn(&str) -> bool,
    slots: &HashMap<u64, u32>) -> Vec<(u64, u32)> {
    let by_key: HashMap<u64, (&String, &TierRow)> = tiers.iter().map(|(k, r)| (key_hash(k), (k, r))).collect();
    let mut out: Vec<(u64, u32)> = bindings.iter().filter_map(|b| {
        let (name, row) = by_key.get(&b.tier)?;
        if row.kind != KIND_CHALLENGE_PHOTO || !unlocked(name) {
            return None;
        }
        let slot = *slots.get(&row.challenge?)?;
        (slot != 0).then_some((b.texture, slot))
    }).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Photo rows' career slots by the challenge id's key hash (the export's `photos[].photo_slot`).
fn read_slots(asset_root: &Path) -> HashMap<u64, u32> {
    let Some(doc) = std::fs::read(asset_root.join("private/challenges.json")).ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else { return HashMap::new() };
    doc["photos"].as_array().into_iter().flatten().filter_map(|p| {
        let slot = p["photo_slot"].as_u64()? as u32;
        Some((key_hash(p["id"].as_str()?), slot))
    }).collect()
}

pub(crate) fn photo_file(asset_root: &Path, slot: u32) -> PathBuf {
    asset_root.parent().unwrap_or(asset_root).join(format!("settings/profiles/local/career-photos/{slot}.png"))
}

/// The photo as one page layer: resampled to the page, rows bottom first as the map writer stores textures, with the
/// page's mip chain. Returns the bytes and the level count.
pub(crate) fn layer_bytes(rgba: &[u8], width: u32, height: u32, place: &TextureLayer) -> (Vec<u8>, u32) {
    let sized = if (width, height) == (place.width, place.height) {
        rgba.to_vec()
    } else {
        crate::retail_render::resample(rgba, width, height, place.width, place.height)
    };
    let row = place.width as usize * 4;
    let flipped: Vec<u8> = sized.chunks_exact(row).rev().flatten().copied().collect();
    if place.mips {
        crate::retail_render::mip_chain(&flipped, place.width, place.height, 1)
    } else {
        (flipped, 1)
    }
}

#[derive(Default)]
struct State {
    generation: Option<u64>,
    bindings: Vec<Binding>,
    slots: Option<HashMap<u64, u32>>,
    /// Texture -> (slot, photo file time) written into its layers.
    applied: HashMap<u64, (u32, SystemTime)>,
    since: f32,
}

fn update(
    mut state: Local<State>,
    time: Res<Time>,
    map: Option<Res<crate::map_transition::CurrentMap>>,
    config: Option<Res<crate::config::Config>>,
    career: Option<Res<crate::career::CareerState>>,
    added: Query<(), Added<MapTextureLayers>>,
    layers: Query<&MapTextureLayers>,
) {
    let (Some(map), Some(config), Some(career)) = (map, config, career) else { return };
    let asset_root = &config.asset_root;
    let mut force = !added.is_empty();
    if state.generation != Some(map.generation) {
        state.generation = Some(map.generation);
        state.bindings = map.path.as_ref().map(|p| read_bindings(&p.with_extension("teambrand"))).unwrap_or_default();
        state.applied.clear();
        force = true;
        let photos = state.bindings.len();
        info!("SKATE_TEAMBRAND {} bindings on {}", photos, map.name);
    }
    if force {
        state.applied.clear();
    }
    state.since += time.delta_secs();
    if state.bindings.is_empty() || !force && state.since < CHECK_SECONDS {
        return;
    }
    state.since = 0.0;
    let slots = state.slots.get_or_insert_with(|| read_slots(asset_root)).clone();
    let tiers = &career.awards.tiers;
    let team = &career.career.team;
    let test = test_mode();
    let wanted = photo_slots(&state.bindings, tiers, |t| test || team.tier_unlocked(t, tiers), &slots);
    for (texture, slot) in wanted {
        let path = photo_file(asset_root, slot);
        let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok()
            .or_else(|| test.then_some(SystemTime::UNIX_EPOCH));
        let Some(stamp) = stamp else { continue };
        if state.applied.get(&texture) == Some(&(slot, stamp)) {
            continue;
        }
        let places: Vec<&TextureLayer> = layers.iter().filter_map(|l| l.0.get(&texture)).flatten().collect();
        if places.is_empty() {
            continue;
        }
        let image = match image::open(&path) {
            Ok(i) => i.to_rgba8(),
            Err(_) if test => test_pattern(slot),
            Err(e) => {
                warn!("SKATE_TEAMBRAND photo {}: {e}", path.display());
                state.applied.insert(texture, (slot, stamp));
                continue;
            }
        };
        let (w, h) = image.dimensions();
        for place in &places {
            let (bytes, levels) = layer_bytes(image.as_raw(), w, h, place);
            crate::layered_upload::replace_layer(place.page.id(), place.layer, place.layers, place.width, place.height,
                levels, bytes);
        }
        info!("SKATE_TEAMBRAND texture 0x{texture:016x} shows career photo {slot} ({} placements)", places.len());
        state.applied.insert(texture, (slot, stamp));
    }
}

/// Test aid (screenshot runs): SKATE_TEAMBRAND_TEST=1 treats every photo tier as unlocked and draws a numbered test
/// pattern where no career photo is stored.
fn test_mode() -> bool {
    static TEST: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TEST.get_or_init(|| std::env::var("SKATE_TEAMBRAND_TEST").is_ok_and(|v| v == "1"))
}

/// A 256 x 128 checker whose color depends on the slot, with a white band along the top edge (shows orientation).
fn test_pattern(slot: u32) -> image::RgbaImage {
    let tint = [(slot * 53 % 256) as u8, (slot * 97 % 256) as u8, (slot * 151 % 256) as u8, 255];
    image::RgbaImage::from_fn(256, 128, |x, y| {
        if y < 16 { image::Rgba([255, 255, 255, 255]) }
        else if (x / 32 + y / 32) % 2 == 0 { image::Rgba(tint) }
        else { image::Rgba([20, 20, 20, 255]) }
    })
}

pub(crate) struct TeamBrandPlugin;

impl Plugin for TeamBrandPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(kind: u32, bit: u32, challenge: Option<u64>) -> TierRow {
        TierRow { kind, bit, challenge, ..Default::default() }
    }

    #[test]
    fn an_unlocked_photo_tier_binds_its_challenges_slot_and_the_rest_keep_the_ad() {
        let tiers = BTreeMap::from([
            ("0101_challenge_photo_anyw_11".to_owned(), tier(2, 101, Some(key_hash("photo_anyw_11")))),
            ("0102_challenge_photo_anyw_21".to_owned(), tier(2, 102, Some(key_hash("photo_anyw_21")))),
            ("Hash_4C5ACD841A00BFFE".to_owned(), tier(1, 200, None)),
        ]);
        let slots = HashMap::from([(key_hash("photo_anyw_11"), 32), (key_hash("photo_anyw_21"), 34)]);
        let bindings = [
            Binding { texture: 0xBC, tier: key_hash("0101_challenge_photo_anyw_11") },
            Binding { texture: 0xB8, tier: key_hash("0102_challenge_photo_anyw_21") },
            Binding { texture: 0x09, tier: 0x4C5ACD841A00BFFE },
            Binding { texture: 0x01, tier: 0x1234 },
        ];
        let unlocked = |t: &str| t.starts_with("0101") || t.starts_with("Hash_");
        assert_eq!(photo_slots(&bindings, &tiers, unlocked, &slots), [(0xBC, 32)],
            "the locked tier, the logo tier and the unknown tier bind no photo");
    }

    /// Where the photo bindings' textures are drawn in an installed map (SKATE3_MAP=<path to .skate>): proves the map
    /// names its textures by the bound asset ids, and gives positions for screenshot runs.
    #[test]
    #[ignore = "requires an installed map via SKATE3_MAP"]
    fn photo_bindings_name_textures_the_installed_map_draws() {
        let path = PathBuf::from(std::env::var_os("SKATE3_MAP").expect("SKATE3_MAP"));
        let map = skate_data::skate_map::SkateMap::load(&path).unwrap();
        let bindings = read_bindings(&path.with_extension("teambrand"));
        assert!(!bindings.is_empty(), "no .teambrand beside the map");
        let id = |t: &skate_data::skate_map::Texture| u64::from_str_radix(t.name.strip_prefix("0x")?, 16).ok();
        let bound: std::collections::HashSet<u64> = bindings.iter().map(|b| b.texture).collect();
        let named: std::collections::HashSet<u64> = map.textures.iter().filter_map(id).collect();
        let found = bound.iter().filter(|t| named.contains(t)).count();
        println!("{} bound textures, {found} in the map's textures", bound.len());
        assert!(found > 0);
        let diffuse = |m: &skate_data::skate_map::Material| {
            let def = m.retail_definition.as_deref().and_then(crate::retail_render::Definition::parse);
            let index = def.and_then(|d| d.bindings.get("diffuse").map(|b| b.texture)).unwrap_or(m.textures[0]);
            map.textures.get((index as usize).checked_sub(1)?).and_then(id)
        };
        let objects = map.extensions.iter().find(|e| e.tag == *b"MOBJ")
            .and_then(|e| skate_data::skate_map::parse_static_objects(&map, e).ok()).unwrap_or_default();
        for texture in [0x2c70170a000700bc_u64, 0x2c70170a001d0180, 0x2c70170a000d01a1] {
            let materials: Vec<usize> = map.materials.iter().enumerate()
                .filter(|(_, m)| diffuse(m) == Some(texture)).map(|(i, _)| i + 1).collect();
            let uses = |i: u32| materials.contains(&(map.geometry.vertices[i as usize].material as usize));
            let world: Vec<[f32; 3]> = map.geometry.indices.iter().filter(|&&i| uses(i))
                .map(|&i| map.geometry.vertices[i as usize].position).collect();
            let placed: Vec<[f32; 3]> = objects.iter()
                .filter(|o| map.geometry.indices[o.first_index as usize..][..o.index_count as usize].iter().any(|&i| uses(i)))
                .map(|o| [o.transform[9], o.transform[10], o.transform[11]]).collect();
            let center = |v: &[[f32; 3]]| (!v.is_empty()).then(|| [0, 1, 2].map(|k| v.iter().map(|p| p[k]).sum::<f32>() / v.len() as f32));
            println!("0x{texture:016x}: materials {materials:?} triangles-vertices {} center {:?} placed {placed:?}",
                world.len(), center(&world));
            println!("  corners {:?}", &world[..world.len().min(6)]);
        }
    }

    #[test]
    fn the_photo_fills_the_page_layer_bottom_row_first_with_its_mips() {
        let place = TextureLayer { page: Handle::default(), layer: 3, layers: 8, width: 2, height: 2, mips: true };
        let rgba = [[255, 0, 0, 255], [255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 255, 255]].concat();
        let (bytes, levels) = layer_bytes(&rgba, 2, 2, &place);
        assert_eq!(levels, 2);
        assert_eq!(&bytes[..4], &[0, 0, 255, 255], "the top row of the photo is the last row of the layer");
        assert_eq!(bytes.len(), 2 * 2 * 4 + 4);
    }
}
