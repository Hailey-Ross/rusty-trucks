//! The NIS scene table `data/scene/scene_000.bin` (miscboot.big; doc 43 "NIS scenes"): one record per in-engine
//! cutscene (lesson demos, story scenes) with its actors. Big-endian: a 16-byte header {tag, version, record count,
//! actor count}, 104-byte records, then 180-byte actors.

const RECORD: usize = 104;
const ACTOR: usize = 180;

/// The scene id retail keys the ICE camera block by (h = 0xFFFFFFFF; h = h * 33 + byte, case-sensitive).
pub fn scene_hash(name: &str) -> u32 {
    name.bytes().fold(0xFFFF_FFFF, |h: u32, c| h.wrapping_mul(33).wrapping_add(c as u32))
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneActor {
    /// The character key (`benny_fairfax`, `current_player`, `male_adult_1`).
    pub character: String,
    /// The character's source (`marquee`, `livingworld`).
    pub kind: String,
    /// The actor's clip, `scene/anim/<name>.abin`.
    pub clip: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub name: String,
    pub hash: u32,
    pub kind: u32,
    /// Record floats +8..+32; the last is the scene's length in seconds (tut_sksc_01: 22.3).
    pub floats: [f32; 7],
    pub actors: Vec<SceneActor>,
}

impl Scene {
    pub fn length(&self) -> f32 {
        self.floats[6]
    }
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| c as char).collect()
}

pub fn parse(b: &[u8]) -> Result<Vec<Scene>, String> {
    if b.len() < 16 {
        return Err("scene table: no header".into());
    }
    let (records, actors) = (u32_at(b, 8) as usize, u32_at(b, 12) as usize);
    let base = 16 + records * RECORD;
    if base + actors * ACTOR != b.len() {
        return Err(format!("scene table: {} bytes for {records} records and {actors} actors", b.len()));
    }
    let actor = |i: usize| {
        let a = &b[base + i * ACTOR..base + (i + 1) * ACTOR];
        SceneActor { character: text(&a[0..32]), kind: text(&a[32..48]), clip: text(&a[48..112]) }
    };
    (0..records).map(|r| {
        let o = 16 + r * RECORD;
        let name = text(&b[o + 40..o + RECORD]);
        let hash = u32_at(b, o);
        if hash != scene_hash(&name) {
            return Err(format!("scene table record {r} ({name}): hash {hash:#010x} is not the name's"));
        }
        let mut floats = [0.0; 7];
        for (i, f) in floats.iter_mut().enumerate() {
            *f = f32::from_bits(u32_at(b, o + 8 + i * 4));
        }
        let count = u16::from_be_bytes([b[o + 36], b[o + 37]]) as usize;
        let first = u16::from_be_bytes([b[o + 38], b[o + 39]]) as usize;
        if first + count > actors {
            return Err(format!("scene table record {r} ({name}): actors {first}+{count} past {actors}"));
        }
        Ok(Scene { name, hash, kind: u32_at(b, o + 4), floats, actors: (first..first + count).map(actor).collect() })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, length: f32, first: u16, count: u16) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend(scene_hash(name).to_be_bytes());
        r.extend(0x1300_0000u32.to_be_bytes());
        for f in [0.0, 0.0, 0.0, 0.0, 0.0, -1.0, length] {
            r.extend(f32::to_bits(f).to_be_bytes());
        }
        r.extend(count.to_be_bytes());
        r.extend(first.to_be_bytes());
        let mut n = name.as_bytes().to_vec();
        n.resize(64, 0);
        r.extend(n);
        r
    }

    fn field(s: &str, len: usize) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.resize(len, 0);
        v
    }

    #[test]
    fn scene_hash_matches_the_ice_ids() {
        // Laptop job 3 (2026-11-06): tut_sksc_01's ICE block and the ViewScene block.
        assert_eq!(scene_hash("tut_sksc_01"), 0x210E_780F);
        assert_eq!(scene_hash("tut_sksc_02"), 0x210E_7810);
    }

    /// Data-gated: `SKATE3_SCENE_DIR=<install>/assets/private/hud/scene` (scene_000.bin, ice_000.bin, anim/).
    #[test]
    fn the_disc_table_parses() {
        let Some(dir) = std::env::var_os("SKATE3_SCENE_DIR").map(std::path::PathBuf::from) else { return };
        let scenes = parse(&std::fs::read(dir.join("scene_000.bin")).unwrap()).unwrap();
        assert_eq!(scenes.len(), 503);
        let tut = scenes.iter().find(|s| s.name == "tut_sksc_01").unwrap();
        assert_eq!(tut.actors[0].character, "benny_fairfax");
        assert_eq!(tut.actors[0].clip, "scene/anim/tut_sksc_011.abin");
        assert!((tut.length() - 22.3).abs() < 1e-3);
        // Its camera block and clip decode with the Trick Guide's readers.
        let tracks = crate::ice_camera::scene_tracks(&std::fs::read(dir.join("ice_000.bin")).unwrap(), tut.hash).unwrap();
        assert_eq!(tracks.len(), 5);
        let ice = std::fs::read(dir.join("ice_000.bin")).unwrap();
        let cuts = crate::ice_camera::scene_cuts(&ice, tut.hash).unwrap();
        assert_eq!(cuts.iter().map(|c| c.1).collect::<Vec<_>>(), [2, 3, 0, 0, 1]);
        assert!((cuts[0].0 - 0.56).abs() < 0.01 && (cuts[4].0 - 18.79).abs() < 0.01, "{cuts:?}");
        assert_eq!(crate::ice_camera::take_at(&cuts, 0.0), Some(2));
        assert_eq!(crate::ice_camera::take_at(&cuts, 20.0), Some(1));
        let clip = crate::scene_clip::SceneClip::parse(std::fs::read(dir.join("anim/tut_sksc_011.abin")).unwrap()).unwrap();
        eprintln!("tut_sksc_01: {} bones, {:.2} s at {} fps; tracks {:?}", clip.bone_names.len(), clip.duration(), clip.fps,
            tracks.iter().map(|t| t.length).collect::<Vec<_>>());
        if std::env::var_os("SKATE3_SCENE_DUMP").is_some() {
            let ice = std::fs::read(dir.join("ice_000.bin")).unwrap();
            for name in ["TAKE_START", "TAKE_NUMBER", "CONTAINS_SUBTAKE", "FISHEYE_HORIZONTAL", "FISHEYE_VERTICAL", "FISHEYE_MULTIPLIER", "FISHEYE_OVERRIDE"] {
                for (i, (pair, times, values)) in crate::ice_camera::raw_element(&ice, tut.hash, name).unwrap().into_iter().enumerate() {
                    eprintln!("{name} track {i}: group {pair:?} times {times:?} values {values:?}");
                }
            }
            for (i, t) in tracks.iter().enumerate() {
                eprintln!("track {i}: {} keys, spaces {:?}", t.keys.len(),
                    t.segments.iter().map(|s| (s.eye_space.mode, s.eye_space.avatar, s.look_space.mode, s.look_space.avatar)).collect::<Vec<_>>());
                for k in t.keys.iter().take(3) {
                    eprintln!("   t {:.2} eye {:?} look {:?} lens {:.1} dutch {:.3}", k.time, k.eye, k.look, k.lens_mm, k.dutch);
                }
            }
        }
    }

    #[test]
    fn records_and_actors_parse() {
        let mut b = Vec::new();
        for v in [u32::from_be_bytes(*b"scen"), 1, 2, 1] {
            b.extend(v.to_be_bytes());
        }
        b.extend(record("tut_sksc_01", 22.3, 0, 1));
        b.extend(record("tut_sksc_04", 13.0, 1, 0));
        b.extend(field("benny_fairfax", 32));
        b.extend(field("marquee", 16));
        b.extend(field("scene/anim/tut_sksc_011.abin", 64));
        b.extend(field("tut_sksc_01", 64));
        b.extend([0; 4]);
        let scenes = parse(&b).unwrap();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].name, "tut_sksc_01");
        assert!((scenes[0].length() - 22.3).abs() < 1e-5);
        assert_eq!(scenes[0].actors, [SceneActor { character: "benny_fairfax".into(), kind: "marquee".into(),
            clip: "scene/anim/tut_sksc_011.abin".into() }]);
        assert!(scenes[1].actors.is_empty());
        b.pop();
        assert!(parse(&b).is_err());
    }
}
