//! ICE camera tracks from the disc (scene.big `ice_%03d.bin`, 82827EB0), little-endian.
//! Layout and gaps: docs/hails-additions/31-retail-menus.md#ice-camera-file
use skate_core::camera_track::{CameraKey, CameraSegment, CameraSpace, CameraTrack};

/// The Trick Guide's ViewScene scene hash (miscboot.big scene_000.bin record #254).
pub const VIEW_SCENE: u32 = 0x7F29_8808;
const GROUPS: usize = 37;
const VALUES: usize = 168;

/// The ICE element table in id order (sub_82903840): (name, key group, one value per segment instead of per key,
/// bits). It fixes where every element's values sit in a record, so the whole table is needed to reach ids 87+.
pub const ELEMENTS: [(&str, u8, bool, u32); 148] = [
    ("EYE_X", 0, false, 32), ("EYE_Y", 0, false, 32), ("EYE_Z", 0, false, 32), ("LOOK_X", 0, false, 32),
    ("LOOK_Y", 0, false, 32), ("LOOK_Z", 0, false, 32), ("DUTCH", 0, false, 10), ("TANGENT_EYE", 0, false, 6),
    ("TANGENT_LOOK", 0, false, 6), ("LENS_LENGTH", 0, false, 9), ("LENS_LENGTH_AUX", 1, false, 9),
    ("NEAR_CLIP", 1, false, 12), ("TANGENT_LENS", 1, false, 6), ("CLIPPING_FAR", 1, false, 12),
    ("CLIPPING_NEAR", 1, false, 12), ("FOCAL_DISTANCE", 2, false, 16), ("APERTURE", 2, false, 6),
    ("TANGENT_FOCUS", 2, false, 6), ("NEAR_FOCUS", 3, false, 16), ("FAR_FOCUS", 3, false, 16),
    ("BLUR_FALLOFF", 3, false, 7), ("BLUR_INTENSITY", 3, false, 7), ("TANGENT_RAWFOCUS", 3, false, 6),
    ("SHAKE_AMPLITUDE", 4, false, 7), ("SHAKE_FREQUENCY", 4, false, 7), ("TIME_SCALE", 5, false, 7),
    ("LETTERBOX", 8, false, 7), ("FADE", 9, false, 7), ("LIGHT_RIG_DIR_U", 10, false, 8),
    ("LIGHT_RIG_DIR_V", 10, false, 8), ("LIGHT_RIG_KEY_INTENSITY", 10, false, 8),
    ("LIGHT_RIG_SEC_INTENSITY", 10, false, 8), ("LIGHT_RIG_FILL_INTENSITY", 10, false, 8),
    ("LIGHT_RIG_RIM_INTENSITY", 10, false, 8), ("TONEMAP_VALUE", 11, false, 16), ("MOTION_BLUR_VALUE", 12, false, 8),
    ("BLOOM_INTENSITY", 13, false, 8), ("BLOOM_THRESHOLD", 13, false, 8), ("TANGENT_BLOOM_INTENSITY", 13, false, 6),
    ("TANGENT_BLOOM_THRESHOLD", 13, false, 6), ("NOISE_INTENSITY", 14, false, 8), ("NOISE_SCALEX", 14, false, 8),
    ("NOISE_SCALEY", 14, false, 8), ("NOISE_SCROLLRATE", 14, false, 8), ("NOISE_SCROLLX", 14, false, 8),
    ("NOISE_SCROLLY", 14, false, 8), ("FISHEYE_HORIZONTAL", 15, false, 8), ("FISHEYE_MULTIPLIER", 15, false, 8),
    ("FISHEYE_VERTICAL", 15, false, 8), ("INTERPOLATION_FAR", 16, false, 32), ("INTERPOLATION_NEAR", 16, false, 32),
    ("BRIGHTNESS_FAR", 17, false, 8), ("BRIGHTNESS_NEAR", 17, false, 8), ("CONTRAST_FAR", 18, false, 16),
    ("CONTRAST_FAR_MIDPOINT", 18, false, 8), ("CONTRAST_NEAR", 18, false, 16),
    ("CONTRAST_NEAR_MIDPOINT", 18, false, 8), ("SATURATE_FAR", 19, false, 8), ("SATURATE_NEAR", 19, false, 8),
    ("TINT_NEAR_COLOR_X", 20, false, 8), ("TINT_NEAR_COLOR_Y", 20, false, 8), ("TINT_NEAR_COLOR_Z", 20, false, 8),
    ("TINT_FAR_COLOR_X", 20, false, 8), ("TINT_FAR_COLOR_Y", 20, false, 8), ("TINT_FAR_COLOR_Z", 20, false, 8),
    ("CHARACTER_EFFECT_MASK_VALUE", 21, false, 8), ("FXMODE_VALUE", 22, false, 8),
    ("SCREEN_WIPE_VALUE", 23, false, 8), ("SHADOW_SCALE", 24, false, 16), ("SHADOW_Y_OFFSET", 24, false, 16),
    ("SHADOW_Z_OFFSET", 24, false, 16), ("SHADOW_X_OFFSET", 24, false, 16), ("SHADOW_BIAS", 25, false, 32),
    ("SHADOW_PRECISION", 26, false, 32), ("SCANLINE_Y_OFFSET", 27, false, 16), ("TANGENT_SCANLINE", 27, false, 6),
    ("RADIALBLUR", 28, false, 8), ("TANGENT_RADIALBLUR", 28, false, 6), ("SUN_DIRECTION_X", 29, false, 16),
    ("SUN_DIRECTION_Y", 29, false, 16), ("SUN_DIRECTION_Z", 29, false, 16), ("SHAKE_QUAT_X", 36, false, 16),
    ("SHAKE_QUAT_Y", 36, false, 16), ("SHAKE_QUAT_Z", 36, false, 16), ("SHAKE_POS_X", 36, false, 16),
    ("SHAKE_POS_Y", 36, false, 16), ("SHAKE_POS_Z", 36, false, 16), ("CUBIC_EYE", 0, true, 1),
    ("CUBIC_LOOK", 0, true, 1), ("SPACE_EYE", 0, true, 3), ("SPACE_LOOK", 0, true, 3), ("AVATAR_EYE", 0, true, 5),
    ("AVATAR_LOOK", 0, true, 5), ("AVATAR_ORBIT", 0, true, 5), ("AVOID_DYNAMIC_OBJECTS", 0, true, 1),
    ("AVOID_STATIC_OBJECTS", 0, true, 1), ("PLAYER_SPACE_LOCK_TO_RENDER_OBJECT", 0, true, 1),
    ("PLAYER_SPACE_NOROLL", 0, true, 1), ("PLAYER_SPACE_LAG", 0, true, 1), ("PLAYER_SPACE_BUNGEE", 0, true, 1),
    ("CUBIC_LENS", 1, true, 1), ("LENS_OVERRIDE", 1, true, 1), ("LENS_USE_DYNAMIC", 1, true, 1),
    ("CUBIC_FOCUS", 2, true, 1), ("CUBIC_RAWFOCUS", 3, true, 1), ("RAWFOCUS_OVERRIDE", 3, true, 1),
    ("RAWFOCUS_RELATIVE", 3, true, 1), ("SHAKE_TYPE", 4, true, 5), ("EVENT_TAG", 6, true, 32),
    ("OVERLAY", 7, true, 16), ("FADE_TO_COLOR", 9, true, 3), ("TONEMAP_OVERRIDE", 11, true, 1),
    ("MOTION_BLUR_OVERRIDE", 12, true, 1), ("MOTION_BLUR_OBJECTAPPLY", 12, true, 1), ("BLOOM_OVERRIDE", 13, true, 1),
    ("CUBIC_BLOOM_INTENSITY", 13, true, 1), ("CUBIC_BLOOM_THRESHOLD", 13, true, 1), ("FISHEYE_OVERRIDE", 15, true, 1),
    ("NEAR_FAR_OVERRIDE", 16, true, 1), ("BRIGHTNESS_OVERRIDE", 17, true, 1), ("CONTRAST_OVERRIDE", 18, true, 1),
    ("SATURATE_OVERRIDE", 19, true, 1), ("TINT_NEAR_NEGATIVE", 20, true, 1), ("TINT_FAR_NEGATIVE", 20, true, 1),
    ("TINT_OVERRIDE", 20, true, 1), ("CHARACTER_EFFECT_MASK_OVERRIDE", 21, true, 1), ("FXMODE", 22, true, 4),
    ("SHADOW_OVERRIDE", 24, true, 1), ("SHADOW_PRECISION_OVERRIDE", 26, true, 1), ("SCANLINE_OVERRIDE", 27, true, 1),
    ("CUBIC_SCANLINE", 27, true, 1), ("RADIALBLUR_OVERRIDE", 28, true, 1), ("CUBIC_RADIALBLUR", 28, true, 1),
    ("SUN_DIRECTION_OVERRIDE", 29, true, 1), ("AUDIO_EFFECT_CMD", 30, true, 2), ("AUDIO_EFFECT_ID", 30, true, 32),
    ("AUDIO_MUSIC_CMD", 31, true, 2), ("AUDIO_MUSIC_ID", 31, true, 32), ("AUDIO_SPEECH_CMD", 32, true, 2),
    ("AUDIO_SPEECH_ID", 32, true, 32), ("AUDIO_SPEECH_CHARACTER", 32, true, 8), ("AUDIO_POST_CMD", 33, true, 2),
    ("AUDIO_POST_ID", 33, true, 32), ("FREEZE_CMD", 34, true, 4), ("FREEZE_TIME", 34, true, 16),
    ("TAKE_START", 35, true, 16), ("TAKE_NUMBER", 35, true, 6), ("CONTAINS_SUBTAKE", 35, true, 1),
];

/// Element ids read here.
const DUTCH: usize = 6;
const CUBIC_EYE: usize = 87;
/// (min, max) of DUTCH, TANGENT_EYE, TANGENT_LOOK, LENS_LENGTH (ids 6..9).
const QUANTISED: [(f32, f32); 4] = [(-0.25, 0.25), (0.0, 8.0), (0.0, 8.0), (10.0, 500.0)];

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

/// The record at `s` and the offset just past its values.
fn track(b: &[u8], s: usize) -> Result<(CameraTrack, usize), String> {
    let length = f32_at(b, s + 12)?;
    let pairs: Vec<(usize, usize)> = (0..GROUPS)
        .map(|k| Ok((u16_at(b, s + 20 + 4 * k)? as usize, u16_at(b, s + 22 + 4 * k)? as usize)))
        .collect::<Result<_, String>>()?;
    let n2: usize = pairs.iter().map(|(seg, _)| seg.saturating_sub(2)).sum();
    let n1: usize = pairs.iter().map(|(seg, _)| seg.saturating_sub(1)).sum();
    let times_at = s + ((2 * n2 + VALUES + 1) & !1);
    let values_at = s + ((times_at - s + 2 * n1 + 3) & !3);
    // Where each element's values start.
    let mut offsets = [0usize; ELEMENTS.len()];
    let mut end = values_at;
    for (id, &(_, group, per_segment, bits)) in ELEMENTS.iter().enumerate() {
        offsets[id] = end;
        let (segments, keys) = pairs[group as usize];
        end += ((bits as usize * if per_segment { segments } else { keys } + 31) >> 3) & !3;
    }
    let (segments, keys) = pairs[0];
    if keys == 0 {
        return Ok((CameraTrack { length, ..Default::default() }, end));
    }
    let mut times = vec![0.0];
    for i in 0..keys.saturating_sub(2) {
        times.push(u16_at(b, times_at + 2 * i)? as f32 / 65535.0 * length);
    }
    if keys > 1 {
        times.push(length);
    }
    let channel = |c: usize, k: usize| f32_at(b, offsets[c] + 4 * k);
    let quantised = |q: usize| -> Result<Vec<f32>, String> {
        let (min, max) = QUANTISED[q];
        let bits = ELEMENTS[DUTCH + q].3;
        Ok(bit_values(b, offsets[DUTCH + q], bits, keys)?.iter()
            .map(|&r| min + r as f32 * (max - min) / (1u32 << bits) as f32).collect())
    };
    let [dutch, tangent_eye, tangent_look, lens] = [0, 1, 2, 3].map(quantised);
    let (dutch, tangent_eye, tangent_look, lens) = (dutch?, tangent_eye?, tangent_look?, lens?);
    // Group 0's per-segment ints, ids 87..92 (min 0, so the raw value).
    let per_segment = |id: usize| bit_values(b, offsets[id], ELEMENTS[id].3, segments);
    let [cubic_eye, cubic_look, space_eye, space_look, avatar_eye, avatar_look] =
        [0, 1, 2, 3, 4, 5].map(|i| per_segment(CUBIC_EYE + i));
    let (cubic_eye, cubic_look, space_eye, space_look, avatar_eye, avatar_look) =
        (cubic_eye?, cubic_look?, space_eye?, space_look?, avatar_eye?, avatar_look?);
    let mut out = Vec::with_capacity(keys);
    for k in 0..keys {
        out.push(CameraKey {
            time: times[k],
            eye: [channel(0, k)?, channel(1, k)?, channel(2, k)?],
            look: [channel(3, k)?, channel(4, k)?, channel(5, k)?],
            dutch: dutch[k],
            tangent_eye: tangent_eye[k],
            tangent_look: tangent_look[k],
            lens_mm: lens[k],
        });
    }
    let segments = (0..segments).map(|i| CameraSegment {
        cubic_eye: cubic_eye[i] != 0,
        cubic_look: cubic_look[i] != 0,
        eye_space: CameraSpace { mode: space_eye[i] as u8, avatar: avatar_eye[i] as u8 },
        look_space: CameraSpace { mode: space_look[i] as u8, avatar: avatar_look[i] as u8 },
    }).collect();
    Ok((CameraTrack { length, keys: out, segments }, end))
}

/// A scene's camera cuts (doc 43 "NIS scenes"): the track whose group 35 holds TAKE_START / TAKE_NUMBER (a track
/// without camera keys) lists, per segment, the scene time (TAKE_START / 65535 * length) at which the camera
/// switches to take (track) TAKE_NUMBER [data; meaning inferred from the takes' own keys]. The last segment is
/// empty (0, 0). Sorted by time; empty when the scene has no cut list.
pub fn scene_cuts(b: &[u8], scene_hash: u32) -> Result<Vec<(f32, usize)>, String> {
    let tracks = scene_tracks(b, scene_hash)?;
    let starts = raw_element(b, scene_hash, "TAKE_START")?;
    let numbers = raw_element(b, scene_hash, "TAKE_NUMBER")?;
    let Some(i) = starts.iter().position(|(_, _, v)| !v.is_empty()) else { return Ok(Vec::new()) };
    let length = tracks[i].length;
    let mut cuts: Vec<(f32, usize)> = starts[i].2.iter().zip(&numbers[i].2)
        .filter(|&(&s, &n)| s != 0 || n != 0)
        .map(|(&s, &n)| (s as f32 / 65535.0 * length, n as usize)).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(cuts)
}

/// The take shown at scene time `t`: the last cut at or before `t`, the first cut's take before it.
pub fn take_at(cuts: &[(f32, usize)], t: f32) -> Option<usize> {
    cuts.iter().rev().find(|c| c.0 <= t).or(cuts.first()).map(|c| c.1)
}

/// FREEZE_CMD value of a photo shot: NISMan_GetPhotoNISCount (8285F220) counts a take's points with it (research
/// b1061; the `_seq` photo scenes use 5, which it does not count).
pub const PHOTO_FREEZE: u32 = 4;

/// A take's screen color lanes (the ICE environment block the NIS writes: IceEnv_SetBrightness 82678F88, contrast
/// 82679D70, saturation 8267A250; research b1063 / b1064). Each is Some only when its override switch is on; values
/// are the lane's first key [the photo and reel takes hold one value; key interpolation not ported].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TakeColour {
    /// BRIGHTNESS (near, far): added to each band's rgb.
    pub brightness: Option<[f32; 2]>,
    /// CONTRAST (near, far) and CONTRAST_*_MIDPOINT (near, far).
    pub contrast: Option<([f32; 2], [f32; 2])>,
    /// SATURATE_NEAR (retail neutral 0; the renderer uses the near value for both bands, b380).
    pub saturation: Option<f32>,
}

/// (min, max) of the color elements as `sub_82903840` registers them through `guess_IceLiteElementDesc_InitFloat`
/// 82C8E7C0 (args default, min, max): BRIGHTNESS 0 in -1..1 (8 bits), CONTRAST 1 in 0..50 (16), MIDPOINT 0.5 in 0..1
/// (8), SATURATE 0 in -1..10 (8). The disc's photo takes hold 127 / 1311 / 23 raw: 0, 1.0 and 0 (research b1064).
const COLOUR_RANGES: [(&str, f32, f32); 4] = [("BRIGHTNESS", -1.0, 1.0), ("CONTRAST", 0.0, 50.0), ("MIDPOINT", 0.0, 1.0), ("SATURATE", -1.0, 10.0)];

fn dequantise(raw: u32, bits: u32, min: f32, max: f32) -> f32 {
    min + raw as f32 * (max - min) / (1u64 << bits) as f32
}

/// Per take of a scene block, its color lanes ([`TakeColour`]).
pub fn take_colours(b: &[u8], scene_hash: u32) -> Result<Vec<TakeColour>, String> {
    let lane = |name: &str| raw_element(b, scene_hash, name);
    let first = |name: &str, range: usize| -> Result<Vec<Option<f32>>, String> {
        let bits = ELEMENTS.iter().find(|e| e.0 == name).map_or(8, |e| e.3);
        let (_, min, max) = COLOUR_RANGES[range];
        Ok(lane(name)?.into_iter().map(|(_, _, v)| v.first().map(|&r| dequantise(r, bits, min, max))).collect())
    };
    let on = |name: &str| -> Result<Vec<bool>, String> { Ok(lane(name)?.into_iter().map(|(_, _, v)| v.first() == Some(&1)).collect()) };
    let (b_on, c_on, s_on) = (on("BRIGHTNESS_OVERRIDE")?, on("CONTRAST_OVERRIDE")?, on("SATURATE_OVERRIDE")?);
    let (bn, bf) = (first("BRIGHTNESS_NEAR", 0)?, first("BRIGHTNESS_FAR", 0)?);
    let (cn, cf) = (first("CONTRAST_NEAR", 1)?, first("CONTRAST_FAR", 1)?);
    let (mn, mf) = (first("CONTRAST_NEAR_MIDPOINT", 2)?, first("CONTRAST_FAR_MIDPOINT", 2)?);
    let sn = first("SATURATE_NEAR", 3)?;
    Ok((0..b_on.len()).map(|t| {
        let pair = |a: &[Option<f32>], b: &[Option<f32>], d: f32| [a.get(t).copied().flatten().unwrap_or(d), b.get(t).copied().flatten().unwrap_or(d)];
        TakeColour {
            brightness: b_on[t].then(|| pair(&bn, &bf, 0.0)),
            contrast: c_on.get(t).copied().unwrap_or(false).then(|| (pair(&cn, &cf, 1.0), pair(&mn, &mf, 0.5))),
            saturation: s_on.get(t).copied().unwrap_or(false).then(|| sn.get(t).copied().flatten().unwrap_or(0.0)),
        }
    }).collect())
}

/// A take's depth of field (research b1071): the ICE take listener 826765C0 applies it only while RAWFOCUS_OVERRIDE is
/// on. Values are the lanes' first key, as for [`TakeColour`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TakeFocus {
    /// NEAR_FOCUS / FAR_FOCUS, 0..10000 (16 bits): the in-focus band.
    pub near: f32,
    pub far: f32,
    /// BLUR_FALLOFF / BLUR_INTENSITY, 0..1 (7 bits).
    pub falloff: f32,
    pub intensity: f32,
    /// RAWFOCUS_RELATIVE: the focus is a projected point instead of the band's middle.
    pub relative: bool,
}

/// (min, max) of the focus elements as `sub_82903840` registers them (research b1071).
const FOCUS_RANGES: [(&str, f32, f32); 2] = [("FOCUS", 0.0, 10000.0), ("BLUR", 0.0, 1.0)];

/// Per take of a scene block, its depth of field when RAWFOCUS_OVERRIDE is on.
pub fn take_focus(b: &[u8], scene_hash: u32) -> Result<Vec<Option<TakeFocus>>, String> {
    let lane = |name: &str| raw_element(b, scene_hash, name);
    let first = |name: &str, range: usize| -> Result<Vec<f32>, String> {
        let bits = ELEMENTS.iter().find(|e| e.0 == name).map_or(8, |e| e.3);
        let (_, min, max) = FOCUS_RANGES[range];
        Ok(lane(name)?.into_iter().map(|(_, _, v)| v.first().map_or(min, |&r| dequantise(r, bits, min, max))).collect())
    };
    let on = |name: &str| -> Result<Vec<bool>, String> { Ok(lane(name)?.into_iter().map(|(_, _, v)| v.first() == Some(&1)).collect()) };
    let (enabled, relative) = (on("RAWFOCUS_OVERRIDE")?, on("RAWFOCUS_RELATIVE")?);
    let (near, far) = (first("NEAR_FOCUS", 0)?, first("FAR_FOCUS", 0)?);
    let (falloff, intensity) = (first("BLUR_FALLOFF", 1)?, first("BLUR_INTENSITY", 1)?);
    Ok((0..enabled.len()).map(|t| enabled[t].then(|| TakeFocus {
        near: near.get(t).copied().unwrap_or(0.0), far: far.get(t).copied().unwrap_or(0.0),
        falloff: falloff.get(t).copied().unwrap_or(0.0), intensity: intensity.get(t).copied().unwrap_or(0.0),
        relative: relative.get(t).copied().unwrap_or(false),
    })).collect())
}

/// The camera's DOF inputs from a take (826765C0, recomp skate3_recomp.25.cpp:35017): focus = (near + far) * 0.5,
/// range = far - near, falloff * 2, clamp(intensity + photo edit, 0, 1) * 3. The scales are retail's constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFocus {
    pub focus: f32,
    pub range: f32,
    pub falloff: f32,
    pub blur: f32,
}

impl TakeFocus {
    pub fn camera(&self, photo_edit: f32) -> CameraFocus {
        CameraFocus { focus: (self.near + self.far) * 0.5, range: self.far - self.near, falloff: self.falloff * 2.0,
            blur: (self.intensity + photo_edit).clamp(0.0, 1.0) * 3.0 }
    }
}

/// One FREEZE_CMD point of a take (research b1061): `NIS_ApplyFreezeCmd` (8267F168) holds the NIS clock `frames` / 30 s
/// for commands 1, 3 and 4 (4 = a photo shot, which also tells the front end), stops it for 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreezePoint {
    /// Normalized take time, 0..1.
    pub at: f32,
    pub command: u32,
    /// FREEZE_TIME, in 30ths of a second (float 8232BA4C).
    pub frames: u32,
}

impl FreezePoint {
    /// Seconds the NIS clock holds at this point (None: no hold).
    pub fn hold_seconds(&self) -> Option<f32> {
        matches!(self.command, 1 | 3 | PHOTO_FREEZE).then(|| self.frames as f32 / 30.0).filter(|s| *s > 0.0)
    }
}

/// Per take of a scene block, its FREEZE_CMD points with a command (0 = none). Point 0 is the take's start, the last
/// its end, the rest the group's interior times [inferred from the layout: one value per point, interior times /
/// 65535 like the camera keys].
pub fn freeze_points(b: &[u8], scene_hash: u32) -> Result<Vec<Vec<FreezePoint>>, String> {
    let frames = raw_element(b, scene_hash, "FREEZE_TIME")?;
    Ok(raw_element(b, scene_hash, "FREEZE_CMD")?.into_iter().zip(frames).map(|(((points, _), times, values), (_, _, time))| {
        values.iter().enumerate().filter(|&(_, &v)| v != 0).map(|(i, &command)| {
            let at = if i == 0 { 0.0 } else if i + 1 >= points { 1.0 } else { f32::from(times.get(i - 1).copied().unwrap_or(0)) / 65535.0 };
            FreezePoint { at, command, frames: time.get(i).copied().unwrap_or(0) }
        }).collect()
    }).collect())
}

/// Per take, the times (seconds of a `length`-second scene) of its photo shots: the points NISMan_GetPhotoNISCount
/// counts ([`PHOTO_FREEZE`]).
pub fn photo_shots(b: &[u8], scene_hash: u32, length: f32) -> Result<Vec<Vec<f32>>, String> {
    Ok(freeze_points(b, scene_hash)?.into_iter()
        .map(|take| take.iter().filter(|p| p.command == PHOTO_FREEZE).map(|p| p.at * length).collect())
        .collect())
}

/// Raw values of one element per track of a scene block (research aid): per track the element's group
/// (segments, keys) pair, the group's interior time words (u16) and the element's raw bit values.
pub fn raw_element(b: &[u8], scene_hash: u32, name: &str) -> Result<Vec<((usize, usize), Vec<u16>, Vec<u32>)>, String> {
    let id = ELEMENTS.iter().position(|e| e.0 == name).ok_or_else(|| format!("no element {name}"))?;
    let (_, group, per_segment, bits) = ELEMENTS[id];
    let block = (0..b.len().saturating_sub(16)).step_by(4)
        .find(|&o| u32_at(b, o) == Ok(1) && u32_at(b, o + 8) == Ok(scene_hash))
        .ok_or_else(|| format!("no ICE scene block {scene_hash:#010x}"))?;
    let count = u32_at(b, block + 12)? as usize;
    let mut at = block + 16;
    let mut out = Vec::new();
    for _ in 0..count {
        let s = at;
        let pairs: Vec<(usize, usize)> = (0..GROUPS)
            .map(|k| Ok((u16_at(b, s + 20 + 4 * k)? as usize, u16_at(b, s + 22 + 4 * k)? as usize)))
            .collect::<Result<_, String>>()?;
        let n2: usize = pairs.iter().map(|(seg, _)| seg.saturating_sub(2)).sum();
        let n1: usize = pairs.iter().map(|(seg, _)| seg.saturating_sub(1)).sum();
        let times_at = s + ((2 * n2 + VALUES + 1) & !1);
        let values_at = s + ((times_at - s + 2 * n1 + 3) & !3);
        let before: usize = pairs[..group as usize].iter().map(|(seg, _)| seg.saturating_sub(2)).sum();
        let (seg, _) = pairs[group as usize];
        let times = (0..seg.saturating_sub(2)).map(|i| u16_at(b, times_at + 2 * (before + i))).collect::<Result<_, _>>()?;
        let mut offset = values_at;
        let mut values = Vec::new();
        for (i, &(_, g, per_seg, n)) in ELEMENTS.iter().enumerate() {
            let (segments, keys) = pairs[g as usize];
            if i == id {
                values = bit_values(b, offset, bits, if per_segment { segments } else { keys })?;
            }
            offset += ((n as usize * if per_seg { segments } else { keys } + 31) >> 3) & !3;
        }
        out.push((pairs[group as usize], times, values));
        at = track(b, s)?.1;
    }
    Ok(out)
}

/// The tracks of the scene block with `scene_hash`, in file order (track index = position).
pub fn scene_tracks(b: &[u8], scene_hash: u32) -> Result<Vec<CameraTrack>, String> {
    let block = (0..b.len().saturating_sub(16)).step_by(4)
        .find(|&o| u32_at(b, o) == Ok(1) && u32_at(b, o + 8) == Ok(scene_hash))
        .ok_or_else(|| format!("no ICE scene block {scene_hash:#010x}"))?;
    let count = u32_at(b, block + 12)? as usize;
    let mut tracks = Vec::with_capacity(count);
    let mut at = block + 16;
    // Records sit back to back: the next one starts where the element table says this one's values end.
    for _ in 0..count {
        let (track, end) = track(b, at)?;
        tracks.push(track);
        at = end;
    }
    if at > b.len() {
        return Err(format!("ICE scene block {scene_hash:#010x} runs past the file ({at:#x})"));
    }
    Ok(tracks)
}
