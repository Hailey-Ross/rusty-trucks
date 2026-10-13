//! ICE camera tracks from the player's disc (scene.big `data/scene/ice_%03d.bin`, loaded by TU3 sub_82827EB0;
//! doc 31 "Trick Guide camera"). The file is little-endian (sub_828EDD80 swaps it in place on the console).
//! A scene block is `{u32 1, u32 size, u32 scene hash, u32 track count}` followed by the track records back to
//! back (sub_828F51E8). A record: +12 f32 length (s), +20 37 key groups `{u16 segments, u16 keys}`, then from +168
//! `sum(max(segments-2, 0))` u16, aligned to 2, `sum(max(segments-1, 0))` u16 interior key times, aligned to 4,
//! then every element's values in element order (sub_828EE070). Camera elements 0..9 all sit in group 0, so they
//! come first: EYE_X..LOOK_Z as `keys` f32 each (channel-major), then DUTCH (10 bits, -0.25..0.25), TANGENT_EYE and
//! TANGENT_LOOK (6 bits, 0..8) and LENS_LENGTH (9 bits, 10..500 mm), each a MSB-first bit stream padded to
//! `((bits*keys+31)>>3) & !3` bytes (element table built by sub_82903840).
//! NOT RETAIL YET (inferred from the data): interior key time = u16/65535 * length, dequantisation
//! `min + raw*(max-min)/2^bits`, and the record walk (the full element table past id 17 is not decoded, so the
//! next record is found by its constant +4 word and a positive length instead of the record size).
use skate_core::camera_track::{CameraKey, CameraTrack};

/// The Trick Guide's ViewScene scene hash (miscboot.big scene_000.bin record #254).
pub const VIEW_SCENE: u32 = 0x7F29_8808;
const GROUPS: usize = 37;
const VALUES: usize = 168;

/// (bits, min, max) of DUTCH, TANGENT_EYE, TANGENT_LOOK, LENS_LENGTH.
const QUANTISED: [(u32, f32, f32); 4] = [(10, -0.25, 0.25), (6, 0.0, 8.0), (6, 0.0, 8.0), (9, 10.0, 500.0)];

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| format!("truncated at {at:#x}"))
}
fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("truncated at {at:#x}"))
}
fn f32_at(b: &[u8], at: usize) -> Result<f32, String> {
    u32_at(b, at).map(f32::from_bits)
}

/// `count` values of `bits` each, MSB first.
fn bit_values(b: &[u8], at: usize, bits: u32, count: usize) -> Result<Vec<u32>, String> {
    (0..count).map(|i| {
        let mut v = 0u32;
        for bit in 0..bits as usize {
            let n = i * bits as usize + bit;
            let byte = *b.get(at + n / 8).ok_or_else(|| format!("truncated bit stream at {at:#x}"))?;
            v = (v << 1) | u32::from(byte >> (7 - n % 8) & 1);
        }
        Ok(v)
    }).collect()
}

fn track(b: &[u8], s: usize) -> Result<CameraTrack, String> {
    let length = f32_at(b, s + 12)?;
    let pairs: Vec<(usize, usize)> = (0..GROUPS)
        .map(|k| Ok((u16_at(b, s + 20 + 4 * k)? as usize, u16_at(b, s + 22 + 4 * k)? as usize)))
        .collect::<Result<_, String>>()?;
    let n2: usize = pairs.iter().map(|(seg, _)| seg.saturating_sub(2)).sum();
    let n1: usize = pairs.iter().map(|(seg, _)| seg.saturating_sub(1)).sum();
    let times_at = s + ((2 * n2 + VALUES + 1) & !1);
    let values_at = s + ((times_at - s + 2 * n1 + 3) & !3);
    let keys = pairs[0].1;
    if keys == 0 {
        return Ok(CameraTrack { length, keys: Vec::new() });
    }
    let mut times = vec![0.0];
    for i in 0..keys.saturating_sub(2) {
        times.push(u16_at(b, times_at + 2 * i)? as f32 / 65535.0 * length);
    }
    if keys > 1 {
        times.push(length);
    }
    let floats: Vec<f32> = (0..6 * keys).map(|i| f32_at(b, values_at + 4 * i)).collect::<Result<_, _>>()?;
    let mut at = values_at + 24 * keys;
    let mut quantised = Vec::new();
    for (bits, min, max) in QUANTISED {
        let raw = bit_values(b, at, bits, keys)?;
        quantised.push(raw.iter().map(|&r| min + r as f32 * (max - min) / (1u32 << bits) as f32).collect::<Vec<_>>());
        at += ((bits as usize * keys + 31) >> 3) & !3;
    }
    let channel = |c: usize, k: usize| floats[c * keys + k];
    Ok(CameraTrack {
        length,
        keys: (0..keys).map(|k| CameraKey {
            time: times[k],
            eye: [channel(0, k), channel(1, k), channel(2, k)],
            look: [channel(3, k), channel(4, k), channel(5, k)],
            dutch: quantised[0][k],
            tangent_eye: quantised[1][k],
            tangent_look: quantised[2][k],
            lens_mm: quantised[3][k],
        }).collect(),
    })
}

/// The tracks of the scene block with `scene_hash`, in file order (track index = position).
pub fn scene_tracks(b: &[u8], scene_hash: u32) -> Result<Vec<CameraTrack>, String> {
    let block = (0..b.len().saturating_sub(16)).step_by(4)
        .find(|&o| u32_at(b, o) == Ok(1) && u32_at(b, o + 8) == Ok(scene_hash))
        .ok_or_else(|| format!("no ICE scene block {scene_hash:#010x}"))?;
    let end = (block + u32_at(b, block + 4)? as usize).min(b.len());
    let count = u32_at(b, block + 12)? as usize;
    let first = block + 16;
    let marker = u32_at(b, first + 4)?;
    let mut tracks = Vec::with_capacity(count);
    let mut at = first;
    while tracks.len() < count {
        tracks.push(track(b, at)?);
        if tracks.len() == count {
            break;
        }
        at += VALUES;
        while at + 16 <= end && !(u32_at(b, at + 4)? == marker && f32_at(b, at + 12)? > 0.0) {
            at += 4;
        }
        if at + 16 > end {
            return Err(format!("found {} of {count} tracks", tracks.len()));
        }
    }
    Ok(tracks)
}
