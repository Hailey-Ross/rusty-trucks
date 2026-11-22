//! Create-a-Skater (FE 15, template CreateASkater, movie cas/createskater): the `Game.CAC_*` natives over the menu
//! table the setup extracts (`menu-tables.json` cas_menus; research b1030). Details: docs/hails-additions/43-career.md#create-a-skater
use crate::apt_vm::{Value, Vm};
use skate_data::menu_tables::CasMenu;

/// The top menu (EditSkaterMain).
pub(crate) const TOP_MENU: usize = 15;
/// Edit Style (gesture slots, Stance, Style, Posture).
const EDIT_STYLE: usize = 16;

/// Who wants FE 15 open (the boot flow's Customize row); `done` once it closed.
#[derive(bevy::prelude::Resource, Default, Debug)]
pub(crate) struct CasScreen {
    pub open: bool,
    pub done: bool,
}

/// The exit confirmation's callback (82603290 -> 82604748).
pub(crate) const EXIT_POPUP: &str = "engine:cas.exit";
/// The tattoo replace warning's callback (82604618 / 82604658).
pub(crate) const TATTOO_REPLACE_POPUP: &str = "engine:cas.tattoo_replace";

/// A on a tattoo region whose location holds a tattoo (825FE3C0 menus 4 / 5): Yes (0) / No (1).
fn tattoo_replace_popup(upper: bool) -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_COMMON_WARNING_TITLE".into(),
        description: if upper { "ID_CAC_UPPER_TATTOO_CHANGE_CONFIRMATION" } else { "ID_CAC_LOWER_TATTOO_CHANGE_CONFIRMATION" }.into(),
        options: vec!["ID_COMMON_YES".into(), "ID_COMMON_NO".into()],
        callback: TATTOO_REPLACE_POPUP.into(),
        ..Default::default()
    }
}

/// B on the top menu (82603290, research b1033): Yes (0) / No (1).
pub(crate) fn exit_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_COMMON_WARNING_TITLE".into(),
        description: "ID_CAC_EXIT_CAC_CONFIRMATION".into(),
        options: vec!["ID_COMMON_YES".into(), "ID_COMMON_NO".into()],
        callback: EXIT_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// Skate 3's own Create-a-Skater screen, on by default (2026-11-20); `SKATE_RETAIL_CAS=0` keeps our customizer.
pub(crate) fn retail_enabled() -> bool {
    std::env::var("SKATE_RETAIL_CAS").map_or(true, |v| v != "0")
}

/// One option (the constructor's option records, S+0x40 + id * 0x30): text id, description id, target menu (None =
/// a leaf), type. Empty until the setup extracts them (research b1031).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CasOption {
    pub text: String,
    pub description: String,
    pub target: Option<usize>,
    pub kind: String,
    /// The part filter's catalog category and payload (research b1035).
    pub category: Option<u32>,
    pub payload: String,
    /// The camera target the option posts (+0x20, research b1036).
    pub camera: String,
    /// The body target the option posts (+0x28: the pose and yaw row).
    pub body: String,
    /// The morph a "morph" row edits (+0x18: the `cac_morph_params` vault key, research b1041).
    pub morph: String,
}

/// One `cac_morph_params` row (class 0x7F47CA62F4100F41, research b994 / b1041): the look's `morphs.<target>`
/// raw value and its range. Both gender branches of the TU3 data are the same; the first is used [inferred].
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CasMorph {
    pub target: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// `cac_fe_morph_settings/default` (class 0xBB852B438BF578ED): the stick threshold and the normalized step per FE
/// frame of the morph rows (825FBAE8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MorphSettings {
    pub threshold: f32,
    pub step: f32,
}

/// One head material (Rostral model + material) and its head fields, from the parts library.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HeadLook {
    pub model: String,
    pub material: String,
    pub fields: std::collections::HashMap<String, String>,
}

impl HeadLook {
    fn field(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }
}

/// One catalog part as setup lists it (`cas-parts.json`, tools/asset_pipeline/cas_grid.py): a model with one of
/// its material instances and the cas fields the filters, thumbnails and info panel read.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub(crate) struct CasPart {
    pub slot: String,
    pub model_id: String,
    pub model: String,
    pub material_id: String,
    pub material: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub toptype: String,
    #[serde(default)]
    pub sockstyle: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub manufacturerid: String,
    #[serde(default)]
    pub unlock: String,
    #[serde(default)]
    pub sponsorunlock: String,
    #[serde(default)]
    pub dlcid: String,
    /// Set on the in-game only materials (the dirty "_d" boards and shoes): the hidden byte rec+0x44.
    #[serde(default)]
    pub isusedingame: String,
    /// The wear style group (rec+0xE0) and the group's shown member (rec+0x2C), 82539188.
    #[serde(default)]
    pub stylesubcategory: String,
    #[serde(default)]
    pub isdefaultwearstyle: String,
}

impl CasPart {
    /// Listed only when the hidden byte is clear (the query 825368A8 and the plain record test 825F3230).
    fn hidden(&self) -> bool {
        !self.isusedingame.is_empty()
    }

    /// The wear style group key: rec+0xE0, hats fall back to rec+0xB0 (their material) when it is unset.
    fn group(&self) -> Option<&str> {
        if !self.stylesubcategory.is_empty() {
            Some(&self.stylesubcategory)
        } else if self.slot == HAT {
            Some(&self.material)
        } else {
            None
        }
    }

    /// The DLC gate (825F4500): the DlcID list parses to non-zero numbers (82AE8EC0 skips leading non-digits).
    fn dlc(&self) -> bool {
        let digits: String = self.dlcid.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u64>().is_ok_and(|n| n != 0)
    }
}

#[derive(serde::Deserialize)]
struct CasPartsFile {
    parts: Vec<CasPart>,
}

/// The part list next to the catalog (none in a set prepared before it).
pub(crate) fn load_parts(asset_root: &std::path::Path) -> Vec<CasPart> {
    std::fs::read(crate::customiser_parts::asset_directory(asset_root).join("cas-parts.json")).ok()
        .and_then(|b| serde_json::from_slice::<CasPartsFile>(&b).ok()).map(|f| f.parts).unwrap_or_default()
}

/// One CAS camera row (Attrib class 0x0F21DD698DF85813, research b1036): eye, look-at and up in stage space, FOV in
/// degrees. 82531868 snaps the camera to it (no blend) when the screen posts the row's key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CasCamera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_degrees: f32,
}

const CAMERA_CLASS: &str = "Hash_0F21DD698DF85813";
const CAMERA_EYE: &str = "Hash_F323B5D55CA55A89";
const CAMERA_TARGET: &str = "Hash_F0FC2B540F11109E";
const CAMERA_UP: &str = "Hash_513070D9CBBE801A";
const CAMERA_FOV: &str = "Hash_EFB7F7EF35801C19";
/// The whole-body target the top and Merchandise menus post (82600AF8).
pub(crate) const WHOLE_BODY: &str = "AD454A0ECD0E2EDF";

/// The brand logo records (82604070: class 0x548B58B0C27C9564, the record whose `name` is the brand) and their image
/// path field (825FFA38 returns it) [inferred: the path field of the three; the other two are the name and a text id].
const LOGO_CLASS: &str = "Hash_548B58B0C27C9564";
const LOGO_IMAGE: &str = "Hash_FF4850A86FD9E4C9";

/// A body target row (class 0x95E142284549717F, read by 82532800 CreateAnimator when cMsgPresCASBodyTarget names it):
/// the character's clip, its yaw on the set in degrees (record +0x28 times pi/180) and its position relative to the
/// stage origin (+0x10).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CasBody {
    pub clip: String,
    pub yaw_degrees: f32,
    pub position: [f32; 3],
    pub once: bool,
    /// The host's post count when it was posted (`CasHost::pose`).
    pub post: u32,
}

/// One color of a CAS color zone (`cac_colour`, class 0x6D934D0E38C95E19; research b991 / b1038): the swatch the menu
/// shows, the two recipe vectors a pick writes (A HDR up to 2.69, B), and the match text (a part's EyeColour /
/// DeckColour / HairlineShadowColour / SkinTone value).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CasColour {
    pub key: String,
    pub swatch: [f32; 3],
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub text: String,
}

/// A color zone (`cac_colour_zone`, class 0xAE1F950CA0D1ADF2): its colors in retail order (field order, ascending,
/// 82541490) and its default color key.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CasColourZone {
    pub default: String,
    pub colours: Vec<CasColour>,
}

const ZONE_CLASS: &str = "Hash_AE1F950CA0D1ADF2";
const ZONE_SLOT: &str = "Hash_97220B633B540525";
const ZONE_GROUP: &str = "Hash_9EE25A772ECA5BD4";
const ZONE_DEFAULT: &str = "Hash_74DF44D2C52AFC68";
const COLOUR_CLASS: &str = "Hash_6D934D0E38C95E19";
const COLOUR_SWATCH: &str = "Hash_7827ED970A88B70C";
const COLOUR_A: &str = "Hash_97462E6ABC78A423";
const COLOUR_B: &str = "Hash_BC3B0D9B2DB64358";
const COLOUR_TEXT: &str = "Hash_AB0E65FA2F485ED8";
const COLOUR_ORDER: &str = "Hash_9C2DC17266699DD9";

/// The color zones by COLORIZED_SLOT (0 hat, 1 hair, 2 skin, 6 eye, 7 eyebrow, 8 facial hair, 9 top, 10 sock,
/// 11 skateboard, 12 bottom), inheritance resolved.
fn load_colour_zones(data: &skate_data::collections::Collections) -> std::collections::HashMap<u32, CasColourZone> {
    let hex = |s: &str, from: usize, len: usize| s.get(from..from + len).map(str::to_owned);
    let text = |class: &str, key: &str, field: &str| data.field(class, key, field).ok().map(|f| f.data.clone());
    let vec3 = |key: &str, field: &str| data.words::<4>(COLOUR_CLASS, key, field).ok().map(|w| [w[0], w[1], w[2]].map(f32::from_bits));
    let mut colours: Vec<(String, i32, CasColour)> = data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == COLOUR_CLASS)
        .filter_map(|c| {
            let order = text(COLOUR_CLASS, &c.key, COLOUR_ORDER).and_then(|d| u32::from_str_radix(d.get(..8)?, 16).ok())? as i32;
            Some((skate_data::attrib_hash::numeric_name(&c.parent), order, CasColour {
                key: skate_data::attrib_hash::numeric_name(&c.key).trim_start_matches("Hash_").to_owned(),
                swatch: vec3(&c.key, COLOUR_SWATCH)?,
                a: vec3(&c.key, COLOUR_A).unwrap_or([0.0; 3]),
                b: vec3(&c.key, COLOUR_B).unwrap_or([0.0; 3]),
                text: text(COLOUR_CLASS, &c.key, COLOUR_TEXT).unwrap_or_default(),
            }))
        })
        .collect();
    colours.sort_by_key(|(_, order, _)| *order);
    data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == ZONE_CLASS)
        .filter_map(|c| {
            let slot = text(ZONE_CLASS, &c.key, ZONE_SLOT).and_then(|d| u32::from_str_radix(d.get(..8)?, 16).ok())?;
            let group = text(ZONE_CLASS, &c.key, ZONE_GROUP).and_then(|d| hex(&d, 16, 16))?;
            let default = text(ZONE_CLASS, &c.key, ZONE_DEFAULT).and_then(|d| hex(&d, 0, 16)).unwrap_or_default();
            let list = colours.iter().filter(|(parent, ..)| parent.trim_start_matches("Hash_").eq_ignore_ascii_case(&group))
                .map(|(.., colour)| colour.clone()).collect::<Vec<_>>();
            (!list.is_empty()).then(|| (slot, CasColourZone { default: default.to_ascii_uppercase(), colours: list }))
        })
        .collect()
}

/// Tattoo menus (research b1042): Body Mods' Upper / Lower Body Tattoo rows push the region menus 4 / 5; a region
/// pushes the categories (6), a category the design list (11); the placement editors are 7 / 8.
const BODY_MODS: usize = 17;
const TATTOO_UPPER: usize = 4;
const TATTOO_LOWER: usize = 5;
const TATTOO_CATEGORIES: usize = 6;
const TATTOO_DESIGNS: usize = 11;
/// The category rows' labels (ID_CAC_TATTOO_CATEGORY_01..06); row i is the vault's category i.
const TATTOO_CATEGORY_ROW: i32 = 21;
/// The region rows: upper 27 front, 28 back, 29 left arm, 30 right arm; lower 31 left leg, 32 right leg.
const TATTOO_UPPER_REGIONS: std::ops::RangeInclusive<i32> = 27..=30;
const TATTOO_LOWER_REGIONS: std::ops::RangeInclusive<i32> = 31..=32;
/// Our stamp `side` per retail region (customiser_parts: 0 / 1 the left / right quadrant, 2 chest, 3 back)
/// [inferred: retail picks the constraint through its region tables 0x83044468 / 0x83044498, not read].
const TATTOO_UPPER_SIDE: [u32; 4] = [2, 3, 0, 1];
const TATTOO_LOWER_SIDE: [u32; 2] = [0, 1];
/// Per region: the camera row and the body (pose) row the region pick posts [data 0x83044548 / 0x830445E8 upper,
/// 0x83044568 / 0x83044608 lower; the front body row is not in the TU3 vault, so the pose stays the whole-body one].
const TATTOO_UPPER_TARGETS: [(&str, &str); 4] = [
    ("94ADE31AD9BCCBAB", "4A663E264EABE260"),
    ("99D608AFAD331C62", "A3D6441668F00463"),
    ("BB4783E9DB0E4647", "BED40F6AC6664346"),
    ("387F458990473B6B", "862562BC11BB028C"),
];
const TATTOO_LOWER_TARGETS: [(&str, &str); 2] = [("C8946ECFD41E5776", "DFAC4B4514810678"), ("B115E4069E2C5B1E", "8C38F0625AB9DFE7")];
/// The pants material a lower tattoo puts on (the PS3 recording's gray shorts).
const TATTOO_SHORTS: &str = "Retroshorts Grey";
const TATTOO_UPPER_ADJUST: usize = 7;
const TATTOO_LOWER_ADJUST: usize = 8;
/// Region -> placement row (= constraint index) [data 0x83044468 upper, 0x83044498 lower].
/// Custom graphics (research b1051): the menus, the rows menu 30 adds (100 the shirt's Add Graphic, 103 the board's
/// and hat's, 104 Remove), the category rows (101 brand, 102 team) and the shirt regions (front, back, left arm, right
/// arm).
const GRAPHIC_CATEGORIES: usize = 12;
const BRAND_GRAPHICS: usize = 13;
const TEAM_GRAPHICS: usize = 14;
const SHIRT_GRAPHIC_LOCATION: usize = 33;
const SHIRT_GRAPHIC_ADJUST: usize = 34;
const BOARD_GRAPHIC_ADJUST: usize = 35;
const HAT_GRAPHIC_ADJUST: usize = 36;
const ADD_SHIRT_GRAPHIC: i32 = 100;
const BRAND_GRAPHICS_ROW: i32 = 101;
const TEAM_GRAPHICS_ROW: i32 = 102;
const ADD_GRAPHIC: i32 = 103;
const REMOVE_GRAPHIC: i32 = 104;
const SHIRT_GRAPHIC_REGIONS: std::ops::RangeInclusive<i32> = 111..=114;
/// Default placement rows of 0x830442B8: the shirt's per region (table 0x83044438), the board's 2, the hat's 3; the
/// cameras (0x83044528 per shirt region, the board's and hat's rows) and the body target the board and hat share
/// (0x83044648 holds the same key) [data, b1051].
const SHIRT_GRAPHIC_ROW: [usize; 4] = [0, 4, 6, 9];
const SHIRT_GRAPHIC_CAMERAS: [&str; 4] = ["496CC99E42AD2C2F", "3F111150D2AFE8B5", "4EC0B69C787D6BDA", "A47411BBF4FCB5D5"];
const BOARD_GRAPHIC_TARGETS: (&str, &str) = ("A9A4A1C434A3E055", "4A663E264EABE260");
const HAT_GRAPHIC_CAMERA: &str = "256799DCA03F926D";

fn graphic_row(slot: &str, region: usize) -> usize {
    match slot {
        OUTER_TORSO => SHIRT_GRAPHIC_ROW[region.min(3)],
        BOARD_SLOT => 2,
        _ => 3,
    }
}

fn graphic_editor(slot: &str) -> usize {
    match slot {
        OUTER_TORSO => SHIRT_GRAPHIC_ADJUST,
        BOARD_SLOT => BOARD_GRAPHIC_ADJUST,
        _ => HAT_GRAPHIC_ADJUST,
    }
}

const TATTOO_UPPER_ROW: [usize; 4] = [1, 5, 7, 10];
const TATTOO_LOWER_ROW: [usize; 2] = [8, 11];
/// The default placement per row, (rotation, scale, u, v) for the two gender variants [data 0x830442B8; which
/// variant is which gender is [inferred]: the first is used for the male skater].
const GRAPHIC_DEFAULTS: [[[f32; 4]; 2]; 12] = [
    [[0.0, 5.0, 0.25, 0.25], [0.0, 5.0, 0.25, 0.25]],
    [[0.0, 5.0, 0.25, 0.25], [0.0, 5.0, 0.25, 0.21]],
    [[1.5712, 7.0, 0.25, 0.25], [1.5712, 7.0, 0.25, 0.25]],
    [[0.0, 5.0, 0.25, 0.32], [0.0, 5.0, 0.25, 0.32]],
    [[0.0, 5.0, -0.25, 0.25], [0.0, 5.0, -0.25, 0.25]],
    [[0.0, 5.0, -0.25, 0.30], [0.0, 5.0, -0.25, 0.10]],
    [[0.0, 7.0, 0.25, -0.15], [0.0, 7.0, 0.25, -0.15]],
    [[0.0, 7.0, 0.234, -0.15], [0.0, 7.0, 0.25, -0.15]],
    [[0.0, 10.0, 0.17, -0.35], [0.0, 10.0, 0.19, -0.35]],
    [[0.0, 7.0, -0.234, -0.15], [0.0, 7.0, -0.25, -0.15]],
    [[0.0, 7.0, -0.25, -0.15], [0.0, 7.0, -0.25, -0.15]],
    [[0.0, 10.0, -0.17, -0.35], [0.0, 10.0, -0.19, -0.35]],
];
/// The placement editor's per-frame factors and limits (825FBAE8 / 82C2E698, research b1042 "Main's dumps"):
/// dead zone, rotate, scale, u, v, the scale cap, the push-back margin and the fit loop's growth.
const GRAPHIC_DEADZONE: f32 = 0.01;
const GRAPHIC_ROTATE: f32 = 0.01;
const GRAPHIC_SCALE: f32 = -0.1;
const GRAPHIC_MOVE_U: f32 = -0.005;
const GRAPHIC_MOVE_V: f32 = 0.005;
const GRAPHIC_SCALE_MAX: f32 = 15.0;
const GRAPHIC_MARGIN: f32 = 0.001;
const GRAPHIC_GROW: f64 = 1.001;
/// Our cap on the fit loop (retail's has none).
const GRAPHIC_FIT_LIMIT: usize = 20_000;

/// A body UV point of graphic UV (gx, gy) under placement p (the inverse of 82C2E3A8's matrix, research b1043).
fn graphic_to_body(p: [f32; 4], gx: f32, gy: f32) -> [f32; 2] {
    let [rot, k, u, v] = p;
    let (s, c) = rot.sin_cos();
    let (a, b) = ((gx - 0.5) / k, (gy - 0.5) / (2.0 * k));
    [c * a + s * b + 0.5 - u, -s * a + c * b + 0.5 - v]
}

/// The graphic bounds' four corners on the body (82C2EBD8's order).
fn graphic_corners(p: [f32; 4], bounds: [f32; 4]) -> [[f32; 2]; 4] {
    [(bounds[2], bounds[1]), (bounds[2], bounds[0]), (bounds[3], bounds[1]), (bounds[3], bounds[0])]
        .map(|(gx, gy)| graphic_to_body(p, gx, gy))
}

/// 82C2EBD8: every corner inside the constraint rect `[xmin, xmax, ymin, ymax]`, inclusive.
fn graphic_fits(p: [f32; 4], rect: [f32; 4], bounds: [f32; 4]) -> bool {
    graphic_corners(p, bounds).iter().all(|q| (rect[0]..=rect[1]).contains(&q[0]) && (rect[2]..=rect[3]).contains(&q[1]))
}

/// One placement editor op (82C2E698): 0 rotate, 1 scale (capped at 15), 2 move u, 3 move v; then, when the graphic
/// leaves the constraint rect, u and then v are pushed back by the first corner's overshoot (only when it does not
/// stick out on both sides), and the scale grows by 1.001 until it fits.
fn adjust_graphic(mut p: [f32; 4], op: usize, amount: f32, rect: [f32; 4], bounds: [f32; 4]) -> [f32; 4] {
    match op {
        0 => p[0] += amount,
        1 => p[1] = (p[1] + amount).min(GRAPHIC_SCALE_MAX),
        2 => p[2] += amount,
        _ => p[3] += amount,
    }
    if graphic_fits(p, rect, bounds) {
        return p;
    }
    for axis in 0..2 {
        let corners = graphic_corners(p, bounds);
        let (lo, hi) = (rect[axis * 2], rect[axis * 2 + 1]);
        let both = corners.iter().any(|q| q[axis] < lo) && corners.iter().any(|q| q[axis] > hi);
        if both {
            continue;
        }
        if let Some(q) = corners.iter().find(|q| q[axis] < lo || q[axis] > hi) {
            let (d, margin) = if q[axis] > hi { (q[axis] - hi, GRAPHIC_MARGIN) } else { (q[axis] - lo, -GRAPHIC_MARGIN) };
            p[2 + axis] += d + margin;
        }
    }
    let mut n = 0;
    while !graphic_fits(p, rect, bounds) && n < GRAPHIC_FIT_LIMIT {
        p[1] = (f64::from(p[1]) * GRAPHIC_GROW) as f32;
        n += 1;
    }
    p
}

/// A part's stamp constraint rect for a constraint index (82536C40): flag StampUVConstraintQ1 (0..3), Q2 (4..5), Q3
/// (6..8), Q4 (9..11) as [top, bottom, left, right] -> x [left, right], y [1 - top, 1 - bottom]; missing or all
/// zero: the index's quadrant of the UV square.
pub(crate) fn constraint_rect(flag: impl Fn(&str) -> String, index: usize) -> [f32; 4] {
    let (key, quadrant) = match index {
        0..=3 => ("StampUVConstraintQ1", [0.0, 0.5, 0.0, 0.5]),
        4..=5 => ("StampUVConstraintQ2", [0.5, 1.0, 0.0, 0.5]),
        6..=8 => ("StampUVConstraintQ3", [0.0, 0.5, 0.5, 1.0]),
        _ => ("StampUVConstraintQ4", [0.5, 1.0, 0.5, 1.0]),
    };
    let r: Vec<f32> = flag(key).split(',').filter_map(|n| n.trim().parse().ok()).collect();
    match <[f32; 4]>::try_from(r) {
        Ok(r) if r.iter().any(|v| *v != 0.0) => [r[2], r[3], 1.0 - r[0], 1.0 - r[1]],
        _ => quadrant,
    }
}
const TATTOO_CATEGORIES_CLASS: &str = "Hash_D38EA152CCFDEE0D";
const TATTOO_CATEGORIES_LIST: &str = "Hash_FFDEC982A33E9230";
const TATTOO_CATEGORY_CLASS: &str = "Hash_345C8861CFA6BCA5";
const TATTOO_CATEGORY_TEXT: &str = "Hash_C283F7484D8204B6";

/// The tattoo categories in the menu's order: `cac_tattoo_categories/default`'s list (825F3050), each entry a
/// `cac_tattoo_category` record whose text field equals its designs' cas.TattooCategory [data: '1', '2', '3' in TU3].
fn load_tattoo_categories(data: &skate_data::collections::Collections) -> Vec<String> {
    let Some(list) = data.entries().iter()
        .find(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == TATTOO_CATEGORIES_CLASS)
        .and_then(|c| data.field(TATTOO_CATEGORIES_CLASS, &c.key, TATTOO_CATEGORIES_LIST).ok())
        .and_then(|f| f.array.clone()) else { return Vec::new() };
    list.items.iter().filter_map(|item| {
        let key = format!("Hash_{}", item.get(..16)?.to_ascii_uppercase());
        Some(data.field(TATTOO_CATEGORY_CLASS, &key, TATTOO_CATEGORY_TEXT).ok()?.data.clone())
    }).collect()
}

/// One face preset row (class 0x3269B4F0F5E38E09, research b1038; 825FC7E8 option 95, 82604208): its index (0 =
/// Custom), gender, text id, the colors it picks (zone slot, color key) and the morphs it sets (morph key, 0..1).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CasFacePreset {
    pub index: i32,
    pub male: bool,
    pub text: String,
    pub colours: Vec<(u32, String)>,
    pub morphs: Vec<(String, f32)>,
}

const PRESET_CLASS: &str = "Hash_3269B4F0F5E38E09";
const PRESET_TEXT: &str = "Hash_D0D7CE67D4A27FD6";
const PRESET_INDEX: &str = "Hash_0DF636FBCE4D41B0";
const PRESET_COLOURS: &str = "Hash_08C2C7A755AF4140";
const PRESET_MORPHS: &str = "Hash_9FC1E6E6BACC7ECB";

/// The face preset rows with a text (the `default` / `male` / `female` parents have none). Colour entries are 48
/// bytes (zone ref at +8, color ref at +32), morph entries 32 (morph ref at +8, value at +24).
fn load_face_presets(data: &skate_data::collections::Collections) -> Vec<CasFacePreset> {
    let zone_slots: std::collections::HashMap<String, u32> = data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == ZONE_CLASS)
        .filter_map(|c| {
            let slot = data.field(ZONE_CLASS, &c.key, ZONE_SLOT).ok().and_then(|f| u32::from_str_radix(f.data.get(..8)?, 16).ok())?;
            Some((skate_data::attrib_hash::numeric_name(&c.key).trim_start_matches("Hash_").to_owned(), slot))
        })
        .collect();
    let items = |key: &str, field: &str| data.field(PRESET_CLASS, key, field).ok().and_then(|f| f.array.clone()).map(|a| a.items).unwrap_or_default();
    let mut presets: Vec<CasFacePreset> = data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == PRESET_CLASS)
        .filter_map(|c| {
            let text = data.field(PRESET_CLASS, &c.key, PRESET_TEXT).ok()?.data.clone();
            if text.is_empty() {
                return None;
            }
            Some(CasFacePreset {
                index: data.integer(PRESET_CLASS, &c.key, PRESET_INDEX).ok()? as i32,
                male: data.field(PRESET_CLASS, &c.key, "male").is_ok_and(|f| f.data.starts_with("01")),
                text,
                colours: items(&c.key, PRESET_COLOURS).iter().filter_map(|i| {
                    let zone = zone_slots.get(&i.get(16..32)?.to_ascii_uppercase())?;
                    Some((*zone, i.get(64..80)?.to_ascii_uppercase()))
                }).collect(),
                morphs: items(&c.key, PRESET_MORPHS).iter().filter_map(|i| {
                    Some((i.get(16..32)?.to_ascii_uppercase(), f32::from_bits(u32::from_str_radix(i.get(48..56)?, 16).ok()?)))
                }).collect(),
            })
        })
        .collect();
    presets.sort_by_key(|p| (p.index, !p.male));
    presets
}

const MORPH_CLASS: &str = "Hash_7F47CA62F4100F41";
const MORPH_TARGET: &str = "Hash_AAC0F30B119BD14F";
const MORPH_ORDER: &str = "Hash_0D381DFA82C751E1";
const MORPH_MIN: &str = "Hash_0279ED3135F03661";
const MORPH_MAX: &str = "Hash_D4340AC527D228E1";
const MORPH_DEFAULT: &str = "Hash_49E3727FEB2F6475";
const MORPH_SETTINGS_CLASS: &str = "Hash_BB852B438BF578ED";
const MORPH_THRESHOLD: &str = "Hash_2A501F561E088458";
const MORPH_STEP: &str = "Hash_2D4497B70742FD97";

/// The morph rows by vault key hash (16 hex digits, the options' +0x18) and the morph row settings (8253CA60,
/// 825FBAE8; field hashes from tools/asset_pipeline/customisation_native.py and research b1041). Rows with
/// `target_order` -1 are the schema default, not a morph.
fn load_morphs(data: &skate_data::collections::Collections)
    -> (std::collections::HashMap<String, CasMorph>, Option<MorphSettings>) {
    let float = |class: &str, key: &str, field: &str| data.words::<1>(class, key, field).ok().map(|w| f32::from_bits(w[0]));
    let morphs = data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == MORPH_CLASS)
        .filter_map(|c| {
            let order = data.words::<1>(MORPH_CLASS, &c.key, MORPH_ORDER).ok()?[0] as i32;
            (order >= 0).then_some(())?;
            Some((skate_data::attrib_hash::numeric_name(&c.key).trim_start_matches("Hash_").to_owned(), CasMorph {
                target: data.field(MORPH_CLASS, &c.key, MORPH_TARGET).ok()?.data.clone(),
                min: float(MORPH_CLASS, &c.key, MORPH_MIN)?,
                max: float(MORPH_CLASS, &c.key, MORPH_MAX)?,
                default: float(MORPH_CLASS, &c.key, MORPH_DEFAULT)?,
            }))
        })
        .filter(|(_, m)| m.max > m.min)
        .collect();
    let settings = data.entries().iter()
        .find(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == MORPH_SETTINGS_CLASS
            && skate_data::attrib_hash::numeric_name(&c.key) == skate_data::attrib_hash::numeric_name("default"))
        .and_then(|c| Some(MorphSettings {
            threshold: float(MORPH_SETTINGS_CLASS, &c.key, MORPH_THRESHOLD)?,
            step: float(MORPH_SETTINGS_CLASS, &c.key, MORPH_STEP)?,
        }));
    (morphs, settings)
}

const BODY_CLASS: &str = "Hash_95E142284549717F";
const BODY_CLIP: &str = "Hash_249D0D4624998D68";
const BODY_YAW: &str = "Hash_4591287983616615";
const BODY_POSITION: &str = "Hash_6D2BEA1EFEA33425";
/// Set on the 46 gesture rows only: the row plays its own clip once; any other row plays the posture's stand
/// (82532800, record +0x2C [inferred]).
const BODY_ONCE: &str = "Hash_9C04CB22EB98D1CC";
/// `cac_posture` (class 0xDE50DFA8296749A7): the index field (look posture - 1) and the stand clip.
const POSTURE_CLASS: &str = "Hash_DE50DFA8296749A7";
const POSTURE_INDEX: &str = "Hash_C356DF187557B552";
const POSTURE_STAND: &str = "Hash_D32BD93ACFF3E542";

/// The posture stand clips by index (`CasHost::posture_stands`).
fn load_posture_stands(data: &skate_data::collections::Collections) -> std::collections::HashMap<i32, String> {
    data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == POSTURE_CLASS)
        .filter_map(|c| {
            let index = data.field(POSTURE_CLASS, &c.key, POSTURE_INDEX).ok()?.data.clone();
            let index = i32::from_str_radix(&index, 16).ok().or_else(|| u32::from_str_radix(&index, 16).ok().map(|v| v as i32))?;
            Some((index, data.field(POSTURE_CLASS, &c.key, POSTURE_STAND).ok()?.data.clone()))
        })
        .collect()
}

/// The vault data FE 15 reads: every CAS camera row by key (16 hex digits), the brand logo image by brand name and
/// every body target row by key, inheritance resolved; empty without the vault export.
pub(crate) fn load_vault(asset_root: &std::path::Path) -> (std::collections::HashMap<String, CasCamera>,
    std::collections::HashMap<String, String>, std::collections::HashMap<String, CasBody>,
    std::collections::HashMap<u32, CasColourZone>, (std::collections::HashMap<String, CasMorph>, Option<MorphSettings>),
    Vec<String>, Vec<CasFacePreset>, std::collections::HashMap<i32, String>) {
    let Ok(data) = skate_data::collections::Collections::load(asset_root) else { return Default::default() };
    let bodies = data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == BODY_CLASS)
        .filter_map(|c| {
            let position = data.words::<4>(BODY_CLASS, &c.key, BODY_POSITION).ok().map(|w| [w[0], w[1], w[2]].map(f32::from_bits))?;
            let body = CasBody {
                clip: data.field(BODY_CLASS, &c.key, BODY_CLIP).ok()?.data.clone(),
                yaw_degrees: data.float(BODY_CLASS, &c.key, BODY_YAW).ok()?,
                position,
                once: data.field(BODY_CLASS, &c.key, BODY_ONCE).ok().is_some_and(|f| f.data == "01"),
                post: 0,
            };
            Some((skate_data::attrib_hash::numeric_name(&c.key).trim_start_matches("Hash_").to_owned(), body))
        })
        .collect();
    let text = |class: &str, key: &str, field: &str| data.field(class, key, field).ok().map(|f| f.data.clone());
    let logos = data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == LOGO_CLASS)
        .filter_map(|c| Some((text(LOGO_CLASS, &c.key, "name")?, text(LOGO_CLASS, &c.key, LOGO_IMAGE)?)))
        .collect::<Vec<_>>();
    // The walk takes the first record with the name (82604070).
    let mut by_brand = std::collections::HashMap::new();
    for (name, image) in logos {
        by_brand.entry(name).or_insert(image);
    }
    (load_cameras(&data), by_brand, bodies, load_colour_zones(&data), load_morphs(&data), load_tattoo_categories(&data),
        load_face_presets(&data), load_posture_stands(&data))
}

fn load_cameras(data: &skate_data::collections::Collections) -> std::collections::HashMap<String, CasCamera> {
    // Math::Vector3 rows hold four words (x, y, z, padding).
    let vector = |key: &str, field: &str| data.words::<4>(CAMERA_CLASS, key, field).ok().map(|w| [w[0], w[1], w[2]].map(f32::from_bits));
    data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == CAMERA_CLASS)
        .filter_map(|c| {
            let camera = CasCamera {
                eye: vector(&c.key, CAMERA_EYE)?,
                target: vector(&c.key, CAMERA_TARGET)?,
                up: vector(&c.key, CAMERA_UP).unwrap_or([0.0, 1.0, 0.0]),
                fov_degrees: data.float(CAMERA_CLASS, &c.key, CAMERA_FOV).ok()?,
            };
            Some((skate_data::attrib_hash::numeric_name(&c.key).trim_start_matches("Hash_").to_owned(), camera))
        })
        .collect()
}

/// The brand logos setup exported (`private/menu-movies/assets/data/fe/cas/logos/<name>`).

/// The look between the screen and the skater: brought in from the customizer's draft when the screen opens; a pick
/// is resolved through the parts library (its dependencies: hair under hats, sleeves, socks) into the draft, which
/// the skater wears (customiser_parts); Yes on the exit confirmation saves it. A removed slot the look cannot do
/// without gets the body's default part [inferred; retail's OuterTorso fallback in 825377E0 is not ported].
pub(crate) fn sync_look(host: &mut CasHost, customiser: &mut crate::customiser::Customiser, parts: &crate::customiser_parts::Parts) {
    if host.defaults.is_empty() {
        host.defaults = parts.library.defaults.clone();
    }
    if host.heads.is_empty() {
        host.heads = head_looks(&parts.library);
    }
    if host.hair_models.is_empty() {
        host.hair_models = parts.library.models.iter().filter(|(_, p)| p.slot == HAIR_SLOT)
            .map(|(id, p)| (id.clone(), (p.flag("HairModelType").to_owned(), p.flag("HairStyle").to_owned()))).collect();
    }
    host.tattoo_fit = match (host.current(), host.tattoo_region) {
        (Some(TATTOO_UPPER_ADJUST | TATTOO_LOWER_ADJUST), Some((upper, region))) => {
            let slot = if upper { "Arm" } else { "Leg" };
            let row = if upper { TATTOO_UPPER_ROW.get(region) } else { TATTOO_LOWER_ROW.get(region) }.copied().unwrap_or(0);
            let part = host.look["selections"][slot]["asset_id"].as_str().and_then(|id| parts.library.models.get(id));
            let rect = constraint_rect(|k| part.map(|p| p.flag(k).to_owned()).unwrap_or_default(), row);
            host.look["tattoos"][slot]["id"].as_str().and_then(|id| parts.library.tattoos.get(id)).map(|t| (rect, t.bounds))
        }
        _ => None,
    };
    if host.bare_torsos.is_empty() {
        for (id, p) in parts.library.models.iter().filter(|(_, p)| p.slot == "OuterTorso") {
            let skin: Vec<(String, String)> = p.materials.iter().filter_map(|m| parts.library.materials.get(m)
                .map(|mat| (m.clone(), mat.flag("SkinTone").to_owned()))).filter(|(_, t)| !t.is_empty()).collect();
            if !skin.is_empty() {
                host.bare_torsos.insert(p.flag("Gender") != "female", (id.clone(), skin));
            }
        }
        for (id, p) in parts.library.models.iter().filter(|(_, p)| p.slot == "Pants") {
            if let Some(m) = p.materials.iter().find(|m| parts.library.materials.get(*m).is_some_and(|mat| mat.name == TATTOO_SHORTS)) {
                host.tattoo_shorts.insert(p.flag("Gender") != "female", (id.clone(), m.clone()));
            }
        }
    }
    if host.tattoos.is_empty() {
        let mut tattoos: Vec<(String, String)> = parts.library.tattoos.iter().map(|(id, t)| (id.clone(), t.category.clone())).collect();
        tattoos.sort();
        host.tattoos = tattoos;
        host.tattoo_names = parts.library.tattoos.iter().map(|(id, t)| (id.clone(), t.name.clone())).collect();
        host.tattoo_thumbnails = parts.library.tattoos.iter().filter(|(_, t)| !t.thumbnail.is_empty())
            .map(|(id, t)| (id.clone(), format!("fe\\cas\\logos\\{}", t.thumbnail))).collect();
        // The custom graphics' logos share the names / thumbnails maps (material ids never collide).
        for (id, t) in &parts.library.logos {
            host.tattoo_names.insert(id.clone(), t.name.clone());
            if !t.thumbnail.is_empty() {
                host.tattoo_thumbnails.insert(id.clone(), format!("fe\\cas\\logos\\{}", t.thumbnail));
            }
        }
        let mut logos: Vec<(u32, String, bool)> = parts.library.logos.iter().map(|(id, t)| (t.order, id.clone(), t.kind == "team")).collect();
        logos.sort();
        host.graphic_logos = logos.into_iter().map(|(_, id, team)| (id, team)).collect();
    }
    if host.logoable.is_empty() {
        host.logoable = parts.library.materials.iter()
            .filter(|(_, m)| m.flag("IsLogoable").eq_ignore_ascii_case("true")).map(|(id, _)| id.clone()).collect();
    }
    host.graphic_fit = match (host.current(), host.graphic.as_ref()) {
        (Some(SHIRT_GRAPHIC_ADJUST | BOARD_GRAPHIC_ADJUST | HAT_GRAPHIC_ADJUST), Some((slot, region))) => {
            let row = graphic_row(slot, *region);
            let part = host.look["selections"][slot.as_str()]["asset_id"].as_str().and_then(|id| parts.library.models.get(id));
            let rect = constraint_rect(|k| part.map(|p| p.flag(k).to_owned()).unwrap_or_default(), row);
            host.look["graphics"][slot.as_str()]["id"].as_str().and_then(|id| parts.library.logos.get(id)).map(|t| (rect, t.bounds))
        }
        _ => None,
    };
    if host.eye_models.is_empty() {
        host.eye_models = parts.library.models.iter().filter(|(_, p)| p.slot == EYE_SLOT)
            .map(|(id, p)| (id.clone(), EyeModel {
                male: p.flag("Gender") != "female",
                materials: p.materials.iter().filter_map(|m| parts.library.materials.get(m)
                    .map(|mat| (m.clone(), mat.flag("EyeColour").to_owned()))).filter(|(_, c)| !c.is_empty()).collect(),
            })).collect();
    }
    if host.colourizable.is_empty() {
        host.colourizable = parts.library.materials.iter()
            .filter(|(_, m)| m.flag("IsColourizable").eq_ignore_ascii_case("true")).map(|(id, _)| id.clone()).collect();
        host.decks = parts.library.models.iter().filter(|(_, p)| p.slot == BOARD_SLOT)
            .map(|(id, p)| (id.clone(), p.materials.iter().filter_map(|m| parts.library.materials.get(m)
                .map(|mat| (m.clone(), mat.flag("DeckColour").to_owned()))).filter(|(_, c)| !c.is_empty()).collect::<Vec<_>>()))
            .filter(|(_, d)| !d.is_empty()).collect();
    }
    if host.open && host.look.is_null() {
        host.look = customiser.draft.clone();
        host.look_copy = host.look.clone();
        host.face_preset = None;
        host.male = host.look["gender"].as_str() != Some("female");
        // The sliders start from the profile (customiser::apply's defaults when unset).
        host.trucks = host.look["truck"].as_f64().unwrap_or(PROFILE_TRUCK_DEFAULT) as f32;
        host.wheels = host.look["wheel"].as_f64().unwrap_or(PROFILE_TRUCK_DEFAULT) as f32;
    }
    let resolve = |look: &serde_json::Value| parts.resolve(look).or_else(|first| {
        let gender = look["gender"].as_str().unwrap_or("male");
        let mut look = look.clone();
        let empty: Vec<String> = look["selections"].as_object().into_iter().flatten()
            .filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect();
        for slot in empty {
            look["selections"][&slot] = host.defaults.get(gender).map_or(serde_json::Value::Null, |d| d["selections"][&slot].clone());
        }
        parts.resolve(&look).map_err(|_| first)
    });
    if std::mem::take(&mut host.look_changed) {
        match resolve(&host.look) {
            Ok(look) => {
                customiser.draft = look.clone();
                customiser.enabled = true;
                host.look = look;
            }
            Err(e) => {
                bevy::log::warn!("RETAIL_MENUS CreateASkater look not applied: {e}");
                host.look = customiser.draft.clone();
            }
        }
    }
    // The highlight preview is shown, never saved: the draft gets the committed look back before a save.
    if std::mem::take(&mut host.preview_changed) || host.save_requested && host.preview.take().is_some() {
        match host.preview.as_ref().map(resolve) {
            Some(Ok(look)) => {
                customiser.draft = look;
                customiser.enabled = true;
            }
            Some(Err(e)) => bevy::log::warn!("RETAIL_MENUS CreateASkater preview not shown: {e}"),
            None => customiser.draft = host.look.clone(),
        }
    }
    if std::mem::take(&mut host.save_requested) {
        match customiser.save_look() {
            Ok(()) => bevy::log::info!("RETAIL_MENUS CreateASkater look saved"),
            Err(e) => bevy::log::warn!("RETAIL_MENUS CreateASkater look not saved: {e}"),
        }
    }
}

/// Every head model's materials with their head fields (HEAD_FIELDS), models and materials in library order.
fn head_looks(library: &crate::customiser_parts::Library) -> Vec<HeadLook> {
    let mut models: Vec<(&String, &crate::customiser_parts::Part)> = library.models.iter().filter(|(_, p)| p.slot == HEAD_SLOT).collect();
    models.sort_by(|a, b| a.0.cmp(b.0));
    models.into_iter()
        .flat_map(|(id, part)| part.materials.iter().filter_map(move |m| {
            let material = library.materials.get(m)?;
            let fields = HEAD_FIELDS.iter().map(|f| (f.to_string(), material.flag(f).to_owned())).collect();
            Some(HeadLook { model: id.clone(), material: m.clone(), fields })
        }))
        .collect()
}

/// The part thumbnail grid (menu 10, PartThumbnails).
pub(crate) const PART_GRID: usize = 10;
/// Record tests of the part filters (research b1035, retail values; data a mod can change): t-shirts keep these
/// TopTypes, the other OuterTorso categories their payload; socks drop this SockStyle; piercing slots keep their
/// payload as Location.
const TSHIRT_TOPTYPES: [&str; 2] = ["tshirt", "ls_tshirt"];
const OUTER_TORSO: &str = "OuterTorso";
const SOCK: &str = "Sock";
const SOCK_EXCLUDED: &str = "tiny";
const PIERCINGS: &str = "Piercings";
/// The brands the post-pass (826035F0) sorts after the others.
const LAST_BRANDS: [&str; 2] = ["skate", "no_brand"];
/// Board slots: their thumbnails have no gender folder.
const BOARD_SLOTS: [&str; 3] = ["SkateBoard", "SkateTruck", "SkateWheel"];
const HAT: &str = "Hat";
/// The slots whose lists keep one part per wear style group (82539188: slot types 11, 15, 18, 23).
const GROUPED_SLOTS: [&str; 4] = ["SkateBoard", "Pants", HAT, OUTER_TORSO];
/// The Merchandise menu a pick returns to (82600820(S, 1)) and the wear style menu a part with variants opens
/// (82600780(S, 0x1E)).
const MERCHANDISE: usize = 1;
const WEAR_STYLES: usize = 30;
/// Option 99 of menu 30: Left / Right cycles the picked part's wear styles.
const WEAR_STYLE_OPTION: i32 = 99;
/// 825FBAE8: `cmpwi r11,500` on the milliseconds the highlight rested.
const PREVIEW_DELAY: f32 = 0.5;
/// The turntable (825FBAE8 posts cMsgPresCASCharacterRotate per frame, 8253B618 turns the character): a trigger
/// past 0.02 (821E9580) adds rate * value to the angle each frame; the rate is the body target row's float at +0x24
/// (0.1 in all 69 rows of class 0x95E142284549717F). Frames at the console's 30 Hz FE cadence.
/// The head looks the Facial Hair (67, menu 21) and Eyebrow (80, menu 24) style selectors step through (825FC7E8):
/// materials of the worn head model that keep every other head field of the worn one (the screen's Facial Hair /
/// Eyebrow Style Filters 825F3F38 / 825F3DA8 over record +0x7C, +0xD4, +0x84, +0x80, +0x78, +0x6C; research b1037).
/// The fields by name, so a mod's head materials join by their flags.
const HEAD_SLOT: &str = "Rostral";
const HEAD_FIELDS: [&str; 5] = ["SkinTone", "HairlineShadowStyle", "HairlineShadowColour", "FacialHairStyle", "EyebrowStyle"];
const FACIAL_HAIR: i32 = 67;
const EYEBROWS: i32 = 80;
const FACIAL_HAIR_MENU: usize = 21;
const EYEBROW_MENU: usize = 24;
/// The Eyebrow Style Filter drops this style for a male skater (82600AF8's menu 24 branch: name 82302284, flag set
/// when the gender check returns 0) [inferred: the flag's meaning from b1035's gender check].
const EYEBROW_EXCLUDED_MALE: &str = "style7";
/// Hair Style (option 65, menu 20; research b1039, main verified 82304518 +0x10 = 825F3C70 and 8230224C = "full"):
/// the "Hair Style Filter" keeps the hair parts whose HairModelType is "full", of the skater's gender, in database
/// order; a male skater gets the Remove entry (bald) first; a worn hat model counts as its style's full model.
const HAIR_STYLE: i32 = 65;
const HAIR_MENU: usize = 20;
const HAIR_SLOT: &str = "Hair";
const HAIR_FULL: &str = "full";
/// Color rows and their zone (82603E98, research b1038): Skin Color 62 -> skin (2), Hair Color 66 -> hair (1), Facial
/// Hair Color 68 -> facial hair (8; in no TU3 menu), Eye Color 76 -> eye (6).
const COLOUR_ROWS: [(i32, u32); 4] = [(62, 2), (66, 1), (68, 8), (76, 6)];
const EYE_ZONE: u32 = 6;
const SKIN_ZONE: u32 = 2;
/// The Facial Presets row (menu table option 95) and the Undo Changes rows: 96 on the face, hair and skin menus,
/// 97 on Body Shape.
const FACE_PRESET: i32 = 95;
/// The `fe` records core_nav_left / core_nav_right (825FC7E8's Left / Right sounds).
const SOUND_NAV_LEFT: u64 = 0xAD8B_F9A0_B499_7B49;
const SOUND_NAV_RIGHT: u64 = 0xD8EE_38A6_1C45_EA28;
/// core_a_button (A on the graphic editors, 825FC7E8).
const SOUND_A_BUTTON: u64 = 0x373D_38D7_EA16_7A71;
/// core_error (a locked part, 825FD9F0).
const SOUND_ERROR: u64 = 0x47F7_2022_2F2C_AED4;
const UNDO_LOOK: i32 = 96;
const UNDO_MORPHS: i32 = 97;
const EYE_SLOT: &str = "Organ";

#[derive(Clone, Debug, Default)]
pub(crate) struct EyeModel {
    pub male: bool,
    /// (material id, EyeColour).
    pub materials: Vec<(String, String)>,
}
const BOARD_ZONE: u32 = 11;
const HAIR_ZONE: u32 = 1;
const BOARD_SLOT: &str = "SkateBoard";
/// Option 98 of menu 30 (ID_CAC_WEARSTYLES_COLOUR): its zone is the edited part's (825416A8: hat 0, top 9, sock 10,
/// bottom 12, board 11; research b1038).
const WEAR_STYLE_COLOUR: i32 = 98;
/// Edit Style rows (menu 16; 825FC7E8 / 825FFE08): Posture and Style step 0..3 with wraparound (look +0x2018 /
/// +0x2014), Stance toggles (+0x2010: 0 = goofy, 1 = regular). They are the profile's `posture`, `style` and `stance`,
/// which `customiser::apply` already hands to the skater's animation (defaults 0, 0, 1); labels are retail's text ids.
const POSTURE: i32 = 53;
const SKATE_STYLE: i32 = 54;
const STANCE: i32 = 55;
const POSTURE_LABELS: [&str; 4] = ["ID_CAC_LABEL_POSTURE_1", "ID_CAC_LABEL_POSTURE_2", "ID_CAC_LABEL_POSTURE_3", "ID_CAC_LABEL_POSTURE_4"];
const SKATE_STYLE_LABELS: [&str; 4] = ["ID_CAC_LABEL_STYLE_NORMAL", "ID_CAC_LABEL_STYLE_LOOSE", "ID_CAC_LABEL_STYLE_GONZO", "ID_CAC_LABEL_STYLE_AGGRESSIVE"];
/// Gesture slot rows 49..52 (menu 16) and the profile `gestures` key each edits (82600AF8's menu 16 branch: S+0x180C /
/// +0x1818 / +0x1814 / +0x1810 from look +0x201C / +0x2028 / +0x2024 / +0x2020, our slots 0 / 3 / 2 / 1).
const GESTURE_ROWS: [(i32, &str); 4] = [(49, "0"), (50, "3"), (51, "2"), (52, "1")];
/// The 37 gestures in gameplay's table order (motion_character_gesture.rs NAMES): the kind 4 unlock names the list
/// keeps (82600AF8) and the labels (ctor 825F4B98 at S+0x1830 + 16 * index).
const GESTURE_UNLOCKS: [&str; 37] = ["gesture_airguitar", "gesture_airplane", "gesture_boxing", "gesture_bruce",
    "gesture_checktime", "gesture_devil", "gesture_doublegun", "gesture_dunno", "gesture_fingerwag", "gesture_fists",
    "gesture_flex", "gesture_fliptable", "gesture_fonz", "gesture_freedom", "gesture_fu", "gesture_getaway",
    "gesture_getouttahere", "gesture_handcuffs", "gesture_pumphi", "gesture_pumplo", "gesture_prewind", "gesture_peace",
    "gesture_fingerpoint", "gesture_raisetheroof", "gesture_shaka", "gesture_shrug", "gesture_sky", "gesture_snap",
    "gesture_soularch", "gesture_spock", "gesture_surfsup", "gesture_swinghigh", "gesture_swinglow",
    "gesture_throwarms", "gesture_thumbdown", "gesture_wings", "gesture_yardsale"];
/// The body target row each gesture posts when a slot steps to it (ctor 825F4B98 at S+0x1838 + 16 * index; 82604AD0
/// writes it to the look and posts it): the vault's `cac_<gender>_<gesture>` rows.
const GESTURE_BODIES: [u64; 37] = [
    0xBCACFFAA652FAC9B, 0xB4689C41B8329176, 0x55C88E3F1634398F, 0x6D66EA0B63A7440E, 0xD823630C21BBFC7D, 0xD5DEF8F55620FB08,
    0xAD17A9A2283DD9CA, 0xB0DC570164161DF6, 0xCF3D7CDC67C109EA, 0xE0A914A40BF94EF2, 0x151E783EEA2F0CBE, 0x60C67224E6FDCC28,
    0xD1396FDCA3740CF8, 0xD85E25F874803D8B, 0x9A1F454A38F70280, 0xC807B0CB9B8E44B4, 0x6C53A0716D15EFE2, 0x7CD55C9E294B931D,
    0xB0742355DC5B5C9B, 0xA93DD26DE9C84EA6, 0x62CB73DE4DD6FD86, 0x362C06C263B593BD, 0x7138813FCD93E42D, 0xE6A8E1C2514E0095,
    0x79027F8466FFA848, 0x04883DF1614F6CA0, 0x55A60F8BF2E662E8, 0xEAA19B768D9B222F, 0xF7B057149574EF35, 0x1EF1DE28802EA038,
    0xB5E67EC99EADFAE8, 0xF45B889FCD9831A7, 0x4751C21A35429D0B, 0x130FF44033181BB6, 0x81C7C64D5290E218, 0xCB97CA0E1DDBDF14,
    0xBD81B91A70FB59B1,
];
const GESTURE_LABELS: [&str; 37] = ["ID_GESTURE_AIRGUITAR", "ID_GESTURE_AIRPLANE", "ID_GESTURE_BOXING",
    "ID_GESTURE_BRUCE", "ID_GESTURE_CHECKTIME", "ID_GESTURE_DEVIL", "ID_GESTURE_DOUBLE_GUN", "ID_GESTURE_DUNNO",
    "ID_GESTURE_FINGERWAG", "ID_GESTURE_FISTS", "ID_GESTURE_BICEPFLEX", "ID_GESTURE_FLIPTABLE", "ID_GESTURE_FONZ",
    "ID_GESTURE_FREEDOM", "ID_GESTURE_FU", "ID_GESTURE_GETAWAY", "ID_GESTURE_GETOUTTAHERE", "ID_GESTURE_HANDCUFFS",
    "ID_GESTURE_PUMP_HI", "ID_GESTURE_PUMP_LO", "ID_GESTURE_PREWIND", "ID_GESTURE_PEACE", "ID_GESTURE_FINGERPOINT",
    "ID_GESTURE_RAISEROOF", "ID_GESTURE_SHAKA", "ID_GESTURE_SHRUG", "ID_GESTURE_SKY", "ID_GESTURE_SNAP",
    "ID_GESTURE_SOULARCH", "ID_GESTURE_SPOCK", "ID_GESTURE_SURFSUP", "ID_GESTURE_SWINGHIGH", "ID_GESTURE_SWINGLOW",
    "ID_GESTURE_THROWARMS", "ID_GESTURE_THMB_DWN", "ID_GESTURE_WINGS", "ID_GESTURE_YARDSALE"];
const TURNTABLE_DEADZONE: f32 = 0.02;
const TURNTABLE_RATE: f32 = 0.1;
const FE_HZ: f32 = 30.0;
/// CAC_GetIntegerValue's scale for a morph row's 0..1 value (822F95E4).
const MORPH_DISPLAY: f32 = 122.0;

#[derive(Default)]
pub(crate) struct CasHost {
    /// The catalog parts and the slot names by category number (menu-tables.json cas_slots).
    pub parts: Vec<CasPart>,
    pub slots: Vec<String>,
    /// The skater is male (the gender filter, [db+0x54]+0x2030), from the look's gender.
    pub male: bool,
    /// The look the screen edits: the skater's outfit (customiser draft: `gender`, `selections` per slot =
    /// {asset_id, material_id}). `sync_look` brings it in on open and puts picks on the skater.
    pub look: serde_json::Value,
    pub look_changed: bool,
    /// The highlight preview (825FEDB8 / 825FBAE8 / 825FEEA0): the look with the highlighted grid part on, shown on
    /// the skater only; None = the skater shows `look`.
    pub preview: Option<serde_json::Value>,
    pub preview_changed: bool,
    /// The highlighted grid row (S+0x17A8) and the time since it was highlighted while the preview waits (S+0x1C40 /
    /// +0x1C44).
    pub preview_row: usize,
    pub preview_wait: Option<f32>,
    /// The rest the highlight needs before the preview (retail 500 ms, 825FBAE8; a mod can change it).
    pub preview_delay: f32,
    /// Every head look (sync_look fills it from the parts library) and, on the Facial Hair / Eyebrow menus, the
    /// ones the selector steps through (S+0x1798) with the shown one (S+0x1808).
    pub heads: Vec<HeadLook>,
    pub style_list: Vec<usize>,
    pub style_at: usize,
    /// Hair models' HairModelType and HairStyle by model id (parts library), the Hair menu's list (None = Remove,
    /// bald; else an index into `parts`) and the shown entry.
    /// The color zones by COLORIZED_SLOT (vault); the look keeps each zone's picked color key in `colour_keys`.
    pub colour_zones: std::collections::HashMap<u32, CasColourZone>,
    pub hair_models: std::collections::HashMap<String, (String, String)>,
    /// The eye models (slot Organ) and their materials' EyeColour, which an eye color pick swaps by its match text.
    pub eye_models: std::collections::HashMap<String, EyeModel>,
    /// The materials flagged IsColourizable, and the board models' (material, DeckColour) a board color pick swaps
    /// by its match text (parts library).
    pub colourizable: std::collections::HashSet<String>,
    pub decks: std::collections::HashMap<String, Vec<(String, String)>>,
    /// The morph rows by vault key hash and the row settings (vault); without settings the morph rows do not move.
    pub morphs: std::collections::HashMap<String, CasMorph>,
    pub morph_settings: Option<MorphSettings>,
    /// The face presets (vault), the face preset row's pick (S+0x1C20; None until set) and the look copy (+0x5DF0,
    /// 82538E78: on opening, a shop mode change and every Body Mods menu entry) that Custom and Undo put back.
    pub face_presets: Vec<CasFacePreset>,
    pub face_preset: Option<usize>,
    pub look_copy: serde_json::Value,
    /// A held morph step changed the highlighted row: the stage refreshes it (SetState 50).
    pub value_refresh: bool,
    /// The current menu was entered again (825FAAF0 state 5, Undo row 97): the stage redraws all its rows.
    pub menu_refresh: bool,
    /// `fe` record keys to play (the stage sends them as `FrontendSound`).
    pub sounds: Vec<u64>,
    /// Popups to open (the stage pushes them) and the tattoo region row waiting for the replace answer.
    pub popups: Vec<skate_core::fe_popup::PopupRequest>,
    pub tattoo_replace: Option<i32>,
    /// The tattoo categories (vault order) and the designs (library id, category), id order.
    pub tattoo_categories: Vec<String>,
    pub tattoos: Vec<(String, String)>,
    pub tattoo_names: std::collections::HashMap<String, String>,
    pub tattoo_thumbnails: std::collections::HashMap<String, String>,
    /// The picked region (upper?, region index) and the design list of the picked category (None = remove, last).
    pub tattoo_region: Option<(bool, usize)>,
    pub tattoo_list: Vec<Option<String>>,
    /// Custom graphics (research b1051): the logos in database order (id, team), the materials flagged IsLogoable,
    /// the part being given a graphic (slot, shirt region), the open grid's logos and the editor's fit.
    pub graphic_logos: Vec<(String, bool)>,
    pub logoable: std::collections::HashSet<String>,
    pub graphic: Option<(String, usize)>,
    pub graphic_list: Vec<String>,
    pub graphic_fit: Option<([f32; 4], [f32; 4])>,
    /// The Base Outfit opt-in (living world opt-ins, user ask 713).
    pub base_outfit: bool,
    /// The clothing taken off for a tattoo (slot, the worn selection), put back on leaving the editor or by X.
    pub tattoo_clothes: Option<(String, serde_json::Value)>,
    /// By gender (male = true): the skin torso model and its (material, SkinTone), and the tattoo shorts (model,
    /// material) the tattoo editor puts on.
    pub bare_torsos: std::collections::HashMap<bool, (String, Vec<(String, String)>)>,
    pub tattoo_shorts: std::collections::HashMap<bool, (String, String)>,
    /// In the placement editor: the region's constraint rect and the design's bounds (sync_look, from the parts).
    pub tattoo_fit: Option<([f32; 4], [f32; 4])>,
    pub hair_list: Vec<Option<usize>>,
    pub hair_at: usize,
    /// The career's unlock store (kinds 5 / 6 decide the parts' locks), copied in while the screen is open.
    pub unlocks: skate_core::career::unlocks::Unlocks,
    /// The character's turntable angle in radians (CAS animation manager +0x64) and its rate per 30 Hz frame at a
    /// full trigger (+0x60; a mod can change it).
    pub turntable: f32,
    pub turntable_rate: f32,
    /// The slot defaults per gender (customiser_parts Library `defaults`), for a Remove the look cannot do without.
    pub defaults: std::collections::HashMap<String, serde_json::Value>,
    /// The part name ids the language table has (ID_CAC_PART_*), for the info panel's name (82605528).
    pub names: std::collections::HashSet<String>,
    /// The picked part's wear styles (menu 30; 82538D38 then 82603CB0: by model name).
    pub wear_styles: Vec<usize>,
    /// Yes on the exit confirmation: the look is saved (82604748 commits it to the profile).
    pub save_requested: bool,
    /// The brand logo images by brand name (load_vault).
    pub logos: std::collections::HashMap<String, String>,
    /// The camera rows by key and the target the screen last posted (cMsgPresCASCameraTarget).
    pub cameras: std::collections::HashMap<String, CasCamera>,
    pub camera: String,
    /// The body target rows by key and the target last posted (cMsgPresCASBodyTarget).
    pub bodies: std::collections::HashMap<String, CasBody>,
    /// The `cac_posture` stand clips by the row's index field (look posture - 1).
    pub posture_stands: std::collections::HashMap<i32, String>,
    pub body: String,
    /// Counts the posts, so posting the same gesture row again plays it again.
    pub body_posts: u32,
    /// The open grid: None = the Remove entry, else an index into `parts`; its slot.
    pub grid: Vec<Option<usize>>,
    pub grid_slot: Option<String>,
    pub grid_title: String,
    pub open: bool,
    pub menus: Vec<CasMenu>,
    pub options: Vec<CasOption>,
    /// The menu stack (S+0x16D4); the current menu is the last.
    pub stack: Vec<usize>,
    /// Per menu: the highlighted row and the window start (M+0x18 / +0x1C; the movie writes them through
    /// CAC_UpdateMenuIndices).
    pub indices: Vec<(u32, u32)>,
    /// S+0x177C, set on menu entry (82600AF8: menu 15 = 2, 17 = 1, 1 = 0): the banner and info panel kind.
    pub shop: u32,
    /// Truck tightness and wheel stiffness, 0..1 (look record +0x2004 / +0x2008; research b1033). NOT RETAIL YET:
    /// not read from or saved to the profile; retail's defaults not read (0.5 meanwhile).
    pub trucks: f32,
    pub wheels: f32,
}

/// `customiser::apply`'s default for an unset `truck` / `wheel` (retail's default not read yet).
const PROFILE_TRUCK_DEFAULT: f64 = 0.7;
/// Option ids of the top menu's sliders.
const TRUCKS: i32 = 47;
const WHEELS: i32 = 48;
/// Left / Right step and the label thresholds (825FC7E8, 825FFE08; data a mod can change).
const SLIDER_STEP: f32 = 0.1;
const SLIDER_LOW: f32 = 0.35;
const SLIDER_HIGH: f32 = 0.75;

impl CasHost {
    pub(crate) fn new(menus: Vec<CasMenu>, options: Vec<skate_data::menu_tables::CasOptionRow>) -> Self {
        let indices = menus.iter().map(|m| (m.index, m.window)).collect();
        let options = options.into_iter().map(|o| CasOption {
            text: o.text,
            description: o.description,
            target: usize::try_from(o.target).ok(),
            kind: o.kind,
            category: o.category,
            payload: o.payload,
            camera: o.camera,
            body: o.body,
            morph: o.morph,
        }).collect();
        Self { menus, options, indices, trucks: 0.5, wheels: 0.5, male: true, preview_delay: PREVIEW_DELAY,
            turntable_rate: TURNTABLE_RATE, ..Default::default() }
    }

    /// LT / RT (825FBAE8: one query turns the character one way, the other the other way [which trigger is which:
    /// inferred]); `seconds` of frames at the 30 Hz FE cadence.
    pub(crate) fn rotate(&mut self, triggers: [f32; 2], seconds: f32) {
        let frames = seconds * FE_HZ;
        for (value, sign) in [(triggers[1], 1.0), (triggers[0], -1.0)] {
            if value >= TURNTABLE_DEADZONE {
                self.turntable += sign * self.turntable_rate * value * frames;
            }
        }
    }

    /// A morph's value over its range, 0..1 (8253CD18: below min reads as min, above max as 1).
    fn morph_n(&self, morph: &CasMorph) -> f32 {
        let raw = self.look["morphs"][&morph.target].as_f64().map_or(morph.default, |v| v as f32);
        ((raw - morph.min) / (morph.max - morph.min)).clamp(0.0, 1.0)
    }

    fn set_morph_n(&mut self, morph: &CasMorph, n: f32) {
        if !self.look["morphs"].is_object() {
            self.look["morphs"] = serde_json::json!({});
        }
        self.look["morphs"][&morph.target] = serde_json::json!(morph.min + n.clamp(0.0, 1.0) * (morph.max - morph.min));
    }

    /// The body row's partner: the body shape row edits `fat` and `thin` as one slider (825FB420 / 825FB490).
    fn thin_morph(&self) -> Option<CasMorph> {
        self.morphs.values().find(|m| m.target == "thin").cloned()
    }

    /// The highlighted row's morph and its shown value 0..1: the morph's own, or for the body row
    /// (1 - thin) / 2 + fat / 2 (825FB490).
    fn morph_row(&self, row: usize) -> Option<(CasMorph, f32)> {
        let (_, option) = self.option(row)?;
        let option = option.filter(|o| o.kind == "morph")?;
        let morph = self.morphs.get(&option.morph)?.clone();
        let n = match (morph.target.as_str(), self.thin_morph()) {
            ("fat", Some(thin)) => ((1.0 - self.morph_n(&thin)) * 0.5 + self.morph_n(&morph) * 0.5).clamp(0.0, 1.0),
            _ => self.morph_n(&morph),
        };
        Some((morph, n))
    }

    /// The morph rows' Left / Right (825FBAE8, research b1041): every FE frame the stick is past the threshold the
    /// shown value moves one step, clamped 0..1, no repeat delay; written back raw over the morph's range (82536B10),
    /// the body row as fat above the middle, thin below (825FB5B8); a face row edit sets the face preset row to Custom.
    /// `seconds` of frames at the FE cadence. Retail starts / stops the looping cas_slider here; that record's level
    /// is 0 on the disc and the front-end queue drops level 0 (82495828), so it is silent and not played.
    pub(crate) fn tick_morph(&mut self, x: f32, seconds: f32) -> bool {
        let row = self.current().map(|m| self.indices[m].0 as usize);
        let (Some(settings), Some((morph, n))) = (self.morph_settings, row.and_then(|r| self.morph_row(r))) else { return false };
        let dir = if x > settings.threshold { 1.0 } else if x < -settings.threshold { -1.0 } else { return false };
        let next = (n + dir * settings.step * seconds * FE_HZ).clamp(0.0, 1.0);
        if next == n {
            return false;
        }
        match (morph.target.as_str(), self.thin_morph()) {
            ("fat", Some(thin)) => {
                let (fat, thin_n) = if next >= 0.5 { ((next - 0.5) * 2.0, 0.0) } else { (0.0, 1.0 - next * 2.0) };
                self.set_morph_n(&morph, fat);
                self.set_morph_n(&thin, thin_n);
            }
            _ => {
                self.set_morph_n(&morph, next);
                self.face_preset = self.custom_preset();
            }
        }
        self.look_changed = true;
        self.value_refresh = true;
        true
    }

    /// The placement editor's analog ops (825FBAE8 on menus 7 / 8): `input` = [rotate (right bumper - left bumper),
    /// scale (left stick y), move u (right stick x), move v (right stick y)] [which pad input drives which op is
    /// inferred from the screen's legend]; each past the dead zone adjusts the placement by its factor per FE frame
    /// (82C2E698 with the fit). Returns whether the placement moved.
    pub(crate) fn tick_tattoo(&mut self, input: [f32; 4], seconds: f32) -> bool {
        // The tattoo editors (menus 7 / 8) and the custom graphic editors (34..36, the same tick, b1051).
        let (group, slot, (rect, bounds)) = match (self.current(), self.tattoo_fit, self.graphic_fit, self.graphic.clone()) {
            (Some(menu @ (TATTOO_UPPER_ADJUST | TATTOO_LOWER_ADJUST)), Some(fit), _, _) => ("tattoos", (if menu == TATTOO_UPPER_ADJUST { "Arm" } else { "Leg" }).to_owned(), fit),
            (Some(SHIRT_GRAPHIC_ADJUST | BOARD_GRAPHIC_ADJUST | HAT_GRAPHIC_ADJUST), _, Some(fit), Some((slot, _))) => ("graphics", slot, fit),
            _ => return false,
        };
        let slot = slot.as_str();
        let Ok(mut p) = serde_json::from_value::<[f32; 4]>(self.look[group][slot]["placement"].clone()) else { return false };
        let before = p;
        let frames = seconds * FE_HZ;
        for (op, (x, factor)) in input.iter().zip([GRAPHIC_ROTATE, GRAPHIC_SCALE, GRAPHIC_MOVE_U, GRAPHIC_MOVE_V]).enumerate() {
            if x.abs() > GRAPHIC_DEADZONE {
                p = adjust_graphic(p, op, x * factor * frames, rect, bounds);
            }
        }
        if p == before {
            return false;
        }
        self.look[group][slot]["placement"] = serde_json::json!(p);
        self.look_changed = true;
        true
    }

    /// An Edit Style value from the look (0..3, the profile's default 0 when missing).
    fn style_value(&self, key: &str) -> usize {
        self.look[key].as_f64().map_or(0, |v| (v.max(0.0) as usize).min(3))
    }

    /// Stance 0 = goofy; missing = 1 (regular), the profile's default.
    fn goofy(&self) -> bool {
        self.look["stance"].as_f64().unwrap_or(1.0) < 0.5
    }

    /// Left / Right on an Edit Style row: Posture / Style wrap over 0..3, Stance toggles. Posture then re-posts the
    /// current body target (82539688 without a key), which restarts the clip.
    fn step_edit_style(&mut self, id: i32, right: bool) -> bool {
        let (key, next) = match id {
            POSTURE | SKATE_STYLE => {
                let key = if id == POSTURE { "posture" } else { "style" };
                let at = self.style_value(key);
                (key, serde_json::json!(if right { (at + 1) % 4 } else { (at + 3) % 4 }))
            }
            STANCE => ("stance", serde_json::json!(if self.goofy() { 1 } else { 0 })),
            _ => return false,
        };
        bevy::log::info!("RETAIL_MENUS CreateASkater {key} {next}");
        self.look[key] = next;
        self.look_changed = true;
        if id == POSTURE {
            self.body_posts = self.body_posts.wrapping_add(1);
        }
        true
    }

    /// The profile `gestures` key a gesture row edits.
    fn gesture_slot(id: i32) -> Option<&'static str> {
        GESTURE_ROWS.iter().find(|(row, _)| *row == id).map(|(_, slot)| *slot)
    }

    /// The gesture in a slot (the profile's, else the slot's own index: customiser::apply's default).
    fn gesture(&self, slot: &str) -> usize {
        self.look["gestures"][slot].as_u64().map_or(slot.parse().unwrap_or(0), |v| v as usize).min(GESTURE_LABELS.len() - 1)
    }

    /// Left / Right on a gesture row (825FC7E8): the next / previous unlocked gesture (kind 4, by name) in table order,
    /// wrapping; the slot's gesture plays on the skater (82604AD0, doc 43 "Gesture preview").
    fn step_gesture(&mut self, slot: &str, right: bool) -> bool {
        use skate_core::career::unlocks::kind;
        let list: Vec<usize> = (0..GESTURE_UNLOCKS.len())
            .filter(|&i| self.unlocks.is_unlocked(kind::GESTURES, GESTURE_UNLOCKS[i])).collect();
        if list.is_empty() {
            return false;
        }
        let at = list.iter().position(|&i| i == self.gesture(slot)).unwrap_or(0);
        let next = list[if right { (at + 1) % list.len() } else { (at + list.len() - 1) % list.len() }];
        if !self.look["gestures"].is_object() {
            self.look["gestures"] = serde_json::json!({});
        }
        self.look["gestures"][slot] = serde_json::json!(next);
        self.look_changed = true;
        if let Some(id) = GESTURE_ROWS.iter().find(|(_, s)| *s == slot).map(|(id, _)| *id) {
            self.post_edit_style_body(id);
        }
        bevy::log::info!("RETAIL_MENUS CreateASkater gesture slot {slot} {}", GESTURE_LABELS[next]);
        true
    }

    fn edit_style_text(&self, id: i32) -> Option<&'static str> {
        if let Some(slot) = Self::gesture_slot(id) {
            return Some(GESTURE_LABELS[self.gesture(slot)]);
        }
        match id {
            POSTURE => Some(POSTURE_LABELS[self.style_value("posture")]),
            SKATE_STYLE => Some(SKATE_STYLE_LABELS[self.style_value("style")]),
            STANCE => Some(if self.goofy() { "ID_CAC_STANCE_GOOFY" } else { "ID_CAC_STANCE_REGULAR" }),
            _ => None,
        }
    }

    fn colour_zone_of(&self, id: i32) -> Option<u32> {
        if id == WEAR_STYLE_COLOUR {
            return self.wear_style_zone();
        }
        COLOUR_ROWS.iter().find(|(row, _)| *row == id).map(|(_, zone)| *zone)
    }

    /// Row 98's zone (82603E98 via [[S+0x17A8]+0x20] and 825416A8): the zone of the slot of the part whose wear styles
    /// menu 30 shows, while its worn material is colorizable [inferred: retail reads the part type only; a printed
    /// part has no IsColourizable flag and takes no tint].
    fn wear_style_zone(&self) -> Option<u32> {
        let worn = self.wear_styles.iter().copied().find(|&v| self.wears(v)).or_else(|| self.wear_styles.first().copied())?;
        let part = self.parts.get(worn)?;
        if !self.colourizable.contains(&part.material_id) {
            return None;
        }
        if part.slot == BOARD_SLOT { Some(BOARD_ZONE) } else { crate::customiser_parts::colour_zone_for_slot(&part.slot) }
    }

    /// The zone's picked color (82603F38 via 825388B8: the look's key for the zone), else the zone's default, as an
    /// index into its list (0 when neither is in it).
    fn colour_index(&self, zone: u32) -> usize {
        let Some(z) = self.colour_zones.get(&zone) else { return 0 };
        let key = self.look["colour_keys"][zone.to_string()].as_str().unwrap_or(&z.default).to_ascii_uppercase();
        z.colours.iter().position(|c| c.key == key).unwrap_or(0)
    }

    /// Left / Right on a color row (825FC7E8): the previous / next color, clamped (no wrap); the picked color's key is
    /// kept per zone in the look; recipe A becomes the zone's tint (`colour_tints`), recipe B the hair highlight
    /// (`colour_highlights`).
    fn step_colour(&mut self, zone: u32, right: bool) -> bool {
        let Some(n) = self.colour_zones.get(&zone).map(|z| z.colours.len()) else { return false };
        let at = self.colour_index(zone);
        let next = if right { (at + 1).min(n.saturating_sub(1)) } else { at.saturating_sub(1) };
        if next == at && self.look["colour_keys"][zone.to_string()].is_string() {
            return false;
        }
        // A skin or eye pick drops the face preset row to Custom (825FC7E8: zones 0x3AA5050BEDEFDDFA / 0x1A5E3EA9A0A82591).
        if matches!(zone, SKIN_ZONE | EYE_ZONE) {
            self.face_preset = self.custom_preset();
        }
        self.pick_colour(zone, next);
        true
    }

    /// CAS_ColorPickerApply for one zone: the color's key, tint and the part or texture swaps its match text makes.
    fn pick_colour(&mut self, zone: u32, next: usize) {
        let key = self.colour_zones[&zone].colours[next].key.clone();
        if !self.look["colour_keys"].is_object() {
            self.look["colour_keys"] = serde_json::json!({});
        }
        bevy::log::info!("RETAIL_MENUS CreateASkater colour zone {zone} {} ({})", self.colour_zones[&zone].colours[next].text, key);
        self.look["colour_keys"][zone.to_string()] = serde_json::Value::String(key);
        // CAS_ColorPickerApply writes the recipe into the zone's tint channel (b991); the skater's material takes
        // recipe A as its modulate color (customiser_parts `colour_tints`). The eye (6) and board (11) zones swap a
        // part by the color's text instead (82541788: eye = the material with that EyeColour; board not ported).
        // Skin: the color's match text (light / dark) picks the SkinTone texture of the head, arm and leg materials
        // (the look's `skin`, customiser_parts resolve); recipe A tints over it. [inferred from data: the dark colors'
        // recipe A is no darker than the light ones, so the dark texture does the darkening; the retail code that
        // swaps it is not read (82538060 writes tints only, b1038)].
        if zone == SKIN_ZONE {
            let text = self.colour_zones[&zone].colours[next].text.clone();
            if !text.is_empty() {
                self.look["skin"] = serde_json::Value::String(text);
            }
        }
        // Hair: the match text (black / medium brown / blonde) picks the head's HairlineShadowColour, the hairline,
        // stubble and brow texture (the look's `hairline`, customiser_parts resolve). Retail also copies the tint to
        // the eyebrow / facial hair channels 7 / 8, which no exported material reads. [inferred from data like skin:
        // every hair color's text is one of the head materials' three HairlineShadowColour values]
        if zone == HAIR_ZONE {
            let text = self.colour_zones[&zone].colours[next].text.clone();
            if !text.is_empty() {
                self.look["hairline"] = serde_json::Value::String(text);
            }
        }
        // Board (82DDC160 / 82DDEDB0 set the deck by the text): the worn deck's material with that DeckColour, the
        // same one of its kind (clean / worn) as before.
        if zone == BOARD_ZONE {
            let text = self.colour_zones[&zone].colours[next].text.clone();
            if let Some((model, material)) = self.worn(BOARD_SLOT).map(|(a, m)| (a.to_owned(), m.to_owned())) {
                if let Some(deck) = self.decks.get(&model) {
                    let colour = deck.iter().find(|(m, _)| *m == material).map(|(_, c)| c.clone()).unwrap_or_default();
                    let nth = deck.iter().filter(|(_, c)| *c == colour).position(|(m, _)| *m == material).unwrap_or(0);
                    let same: Vec<&String> = deck.iter().filter(|(_, c)| c.eq_ignore_ascii_case(&text)).map(|(m, _)| m).collect();
                    if let Some(pick) = same.get(nth).or(same.first()).map(|m| (*m).clone()) {
                        self.set_slot(BOARD_SLOT, Some((model, pick)));
                    }
                }
            }
        }
        if zone == EYE_ZONE {
            let text = self.colour_zones[&zone].colours[next].text.clone();
            let worn = self.look["selections"][EYE_SLOT]["asset_id"].as_str().map(str::to_owned);
            let male = self.male;
            let model = worn.filter(|m| self.eye_models.contains_key(m))
                .or_else(|| self.eye_models.iter().filter(|(_, e)| e.male == male).map(|(id, _)| id.clone()).min());
            if let Some(model) = model {
                if let Some((material, _)) = self.eye_models[&model].materials.iter().find(|(_, c)| c.eq_ignore_ascii_case(&text)).cloned() {
                    self.set_slot(EYE_SLOT, Some((model, material)));
                }
            }
        }
        if !matches!(zone, EYE_ZONE | BOARD_ZONE) {
            if !self.look["colour_tints"].is_object() {
                self.look["colour_tints"] = serde_json::json!({});
            }
            self.look["colour_tints"][zone.to_string()] = serde_json::json!(self.colour_zones[&zone].colours[next].a);
            if !self.look["colour_highlights"].is_object() {
                self.look["colour_highlights"] = serde_json::json!({});
            }
            self.look["colour_highlights"][zone.to_string()] = serde_json::json!(self.colour_zones[&zone].colours[next].b);
        }
        self.look_changed = true;
    }

    fn custom_preset(&self) -> Option<usize> {
        self.face_presets.iter().position(|p| p.index == 0)
    }

    fn face_preset_index(&self) -> i32 {
        self.face_preset.and_then(|i| self.face_presets.get(i)).map_or(0, |p| p.index)
    }

    /// CAC_GetStringValue on the face preset row (825FFE08: the row's text field). Unset reads as Custom, as the
    /// retail recording shows on entering Facial Presets.
    fn face_preset_text(&self) -> String {
        self.face_preset.or_else(|| self.custom_preset()).and_then(|i| self.face_presets.get(i))
            .map_or_else(|| "ID_CAC_PRESET_CUSTOM".to_owned(), |p| p.text.clone())
    }

    /// Left / Right on the face preset row (825FC7E8 option 95): the next index among the skater's gender's presets;
    /// Left from Custom goes to the last, past the last or onto index 0 is Custom. A preset sets its morphs and colors
    /// (82604208); Custom puts back the look as it was on entering the menu (82539048, the copy `enter` keeps).
    fn step_face_preset(&mut self, right: bool) -> bool {
        let male = self.male;
        let last = self.face_presets.iter().filter(|p| p.male == male).map(|p| p.index).max().unwrap_or(0);
        let next = self.face_preset_index() + if right { 1 } else { -1 };
        let next = if next < 0 { last } else { next };
        let pick = if next != 0 && next <= last {
            self.face_presets.iter().position(|p| (p.male == male || p.index == 0) && p.index == next)
        } else {
            None
        };
        match pick {
            Some(i) => {
                self.face_preset = Some(i);
                let preset = self.face_presets[i].clone();
                for (key, n) in &preset.morphs {
                    // 82604208 skips the body rows (fat, thin).
                    if let Some(m) = self.morphs.get(key).filter(|m| m.target != "fat" && m.target != "thin").cloned() {
                        self.set_morph_n(&m, *n);
                    }
                }
                for (zone, colour) in &preset.colours {
                    if let Some(at) = self.colour_zones.get(zone).and_then(|z| z.colours.iter().position(|c| c.key.eq_ignore_ascii_case(colour))) {
                        self.pick_colour(*zone, at);
                    }
                }
            }
            None => {
                self.face_preset = self.custom_preset();
                self.restore_look_copy();
                self.reenter();
            }
        }
        bevy::log::info!("RETAIL_MENUS CreateASkater face preset {}", self.face_preset_text());
        self.look_changed = true;
        true
    }

    /// CASScreen_EnterMenu on the current menu: the stage redraws every row from the look.
    fn reenter(&mut self) {
        if let Some(m) = self.current() {
            self.enter(m);
            self.menu_refresh = true;
        }
    }

    /// 82539048: the whole look back from the copy (CharacterSlot_Deserialize), the skater reloaded.
    fn restore_look_copy(&mut self) {
        if self.look_copy.is_null() {
            return;
        }
        self.look = self.look_copy.clone();
        self.male = self.look["gender"].as_str() != Some("female");
        self.trucks = self.look["truck"].as_f64().unwrap_or(PROFILE_TRUCK_DEFAULT) as f32;
        self.wheels = self.look["wheel"].as_f64().unwrap_or(PROFILE_TRUCK_DEFAULT) as f32;
        self.look_changed = true;
    }

    /// A on Undo Changes (825FE3C0): row 96 restores the whole look from the copy (state 5; 825FAAF0 re-enters the
    /// menu when the reload is done); row 97 (Body Shape) copies the copy's 19 morph values back (82538F60 /
    /// 82542D38) and re-enters the menu at once. Entering copies the look again.
    fn undo(&mut self, id: i32) -> bool {
        match id {
            UNDO_LOOK => self.restore_look_copy(),
            UNDO_MORPHS => {
                if self.look_copy.is_null() {
                    return false;
                }
                self.look["morphs"] = self.look_copy["morphs"].clone();
                self.look_changed = true;
            }
            _ => return false,
        }
        self.reenter();
        bevy::log::info!("RETAIL_MENUS CreateASkater undo {id}");
        true
    }

    /// CAC_GetColours on a color row (825A77D8): the zone's swatches as r, g, b numbers, in list order.
    fn colour_swatches(&self, zone: u32) -> Vec<Value> {
        self.colour_zones.get(&zone).into_iter().flat_map(|z| z.colours.iter())
            .flat_map(|c| c.swatch.map(|v| Value::Number(f64::from(v)))).collect()
    }

    fn hair_style_of(&self, model_id: &str) -> Option<&str> {
        self.hair_models.get(model_id).map(|(_, style)| style.as_str())
    }

    /// Entering the Hair menu (82600AF8 menu 20): the full hair parts of the skater's gender, Remove first for a male
    /// skater, on the worn style (a hat model maps to its style's full part; no hair = Remove).
    fn build_hair_list(&mut self) {
        let male = self.male;
        let mut list: Vec<Option<usize>> = if male { vec![None] } else { Vec::new() };
        list.extend(self.parts.iter().enumerate()
            .filter(|(_, p)| p.slot == HAIR_SLOT && if male { p.gender != "female" } else { p.gender != "male" })
            .filter(|(_, p)| self.hair_models.get(&p.model_id).is_some_and(|(kind, _)| kind == HAIR_FULL))
            .map(|(i, _)| Some(i)));
        let worn_style = self.worn(HAIR_SLOT).and_then(|(model, _)| self.hair_style_of(model)).map(str::to_owned);
        self.hair_at = match worn_style {
            Some(style) => list.iter().position(|e| e.is_some_and(|i| self.hair_style_of(&self.parts[i].model_id) == Some(style.as_str()))).unwrap_or(0),
            None => 0,
        };
        self.hair_list = list;
    }

    /// Left / Right on Hair Style (825FC7E8, wraps): the next style goes on; Remove takes the hair off (retail puts
    /// the bald default part on, ThumbnailLookup(cas, 2, 1)). The hair color is its own row (66) and stays.
    fn step_hair(&mut self, right: bool) -> bool {
        let n = self.hair_list.len();
        if n < 2 {
            return false;
        }
        self.hair_at = if right { (self.hair_at + 1) % n } else { (self.hair_at + n - 1) % n };
        match self.hair_list[self.hair_at] {
            Some(i) => {
                let p = self.parts[i].clone();
                bevy::log::info!("RETAIL_MENUS CreateASkater hair {}", self.hair_style_of(&p.model_id).unwrap_or(""));
                self.set_slot(HAIR_SLOT, Some((p.model_id, p.material_id)));
            }
            None => {
                bevy::log::info!("RETAIL_MENUS CreateASkater hair bald");
                self.set_slot(HAIR_SLOT, None);
            }
        }
        true
    }

    /// CAC_GetStringValue on Hair Style (825FFE08): ID_CAC_HAIRSTYLE_BALD for Remove, else ID_CAC_HAIRSTYLE_<style>.
    fn hair_text(&self) -> String {
        match self.hair_list.get(self.hair_at) {
            Some(None) => "ID_CAC_HAIRSTYLE_BALD".into(),
            Some(Some(i)) => self.hair_style_of(&self.parts[*i].model_id).filter(|s| !s.is_empty())
                .map_or("#".into(), |s| format!("ID_CAC_HAIRSTYLE_{}", s.to_ascii_uppercase().replace(' ', "_"))),
            None => "#".into(),
        }
    }

    /// The style a selector row steps (its head field), if it is one.
    fn style_field(id: i32) -> Option<&'static str> {
        match id {
            FACIAL_HAIR => Some("FacialHairStyle"),
            EYEBROWS => Some("EyebrowStyle"),
            _ => None,
        }
    }

    /// Entering the Facial Hair / Eyebrow menu (82600AF8 menus 21 / 24): the worn head model's looks that keep every
    /// head field but `field`, in library order; the eyebrow list sorted (guess_Menu_SortPtrRange) [inferred: by the
    /// style name] and without EYEBROW_EXCLUDED_MALE for a male skater. Starts on the worn look (FindByKeyPair).
    fn build_style_list(&mut self, field: &str) {
        self.style_list.clear();
        self.style_at = 0;
        let Some((model, material)) = self.worn(HEAD_SLOT).map(|(a, m)| (a.to_owned(), m.to_owned())) else { return };
        let Some(worn) = self.heads.iter().find(|h| h.model == model && h.material == material).cloned() else { return };
        let mut list: Vec<usize> = self.heads.iter().enumerate()
            .filter(|(_, h)| h.model == model && HEAD_FIELDS.iter().all(|f| *f == field || h.field(f) == worn.field(f)))
            .filter(|(_, h)| !(field == "EyebrowStyle" && self.male && h.field(field) == EYEBROW_EXCLUDED_MALE))
            .map(|(i, _)| i)
            .collect();
        if field == "EyebrowStyle" {
            list.sort_by(|a, b| self.heads[*a].field(field).cmp(self.heads[*b].field(field)));
        }
        self.style_at = list.iter().position(|&i| self.heads[i].material == material).unwrap_or(0);
        self.style_list = list;
    }

    /// Left / Right on a style row (825FC7E8, wraps): the next look goes on. Facial hair also sets the look's
    /// `beard`, which the parts library matches the head material to.
    fn step_style(&mut self, field: &str, right: bool) -> bool {
        let n = self.style_list.len();
        if n < 2 {
            return false;
        }
        self.style_at = if right { (self.style_at + 1) % n } else { (self.style_at + n - 1) % n };
        let head = self.heads[self.style_list[self.style_at]].clone();
        if field == "FacialHairStyle" {
            self.look["beard"] = serde_json::Value::String(head.field(field).to_owned());
        }
        bevy::log::info!("RETAIL_MENUS CreateASkater {field} {}", head.field(field));
        self.set_slot(HEAD_SLOT, Some((head.model, head.material)));
        true
    }

    /// CAC_GetStringValue on a style row (825FFE08): `ID_CAC_FACIAL_HAIR_<style>` / `ID_CAC_EYEBROW_<style>` (upper
    /// case, spaces to underscores), the retail error text for an untagged look.
    fn style_text(&self, field: &str) -> String {
        let value = self.style_list.get(self.style_at).map_or("", |&i| self.heads[i].field(field));
        match (field, value.is_empty()) {
            ("FacialHairStyle", true) => "#FacialHairStyleNotTaggedError".into(),
            (_, true) => "#BrowStyleNotTaggedError".into(),
            ("FacialHairStyle", false) => format!("ID_CAC_FACIAL_HAIR_{}", value.to_ascii_uppercase().replace(' ', "_")),
            _ => format!("ID_CAC_EYEBROW_{}", value.to_ascii_uppercase().replace(' ', "_")),
        }
    }

    /// cMsgPresCASCameraTarget; retail always follows it with CharacterRotate(0, absolute): the turntable resets.
    fn post_camera(&mut self, key: String) {
        self.camera = key;
        self.turntable = 0.0;
    }

    /// The CAS OnMenuHilight (vt+0x44 = 825FEDB8): on the part grid the row is kept and the preview waits again; on
    /// Edit Style (menu 16) the row's body posts (82604AD0: a gesture slot plays its gesture).
    fn hilight(&mut self, row: usize) {
        if self.current() == Some(PART_GRID) && row < self.grid.len() {
            self.preview_row = row;
            self.preview_wait = Some(0.0);
        }
        if self.current() == Some(EDIT_STYLE) {
            let id = self.menus[EDIT_STYLE].options.get(row).copied().unwrap_or(-1);
            self.post_edit_style_body(id);
        }
    }

    /// Per frame (825FBAE8 on menu 10): once the highlight rested `preview_delay`, the skater shows the highlighted
    /// part (825FEEA0: NewAssetRequest), or the committed look for the Remove entry or a part it wears already (one of
    /// its wear styles included; 82537268).
    pub(crate) fn tick(&mut self, seconds: f32) {
        let Some(waited) = self.preview_wait.as_mut() else { return };
        *waited += seconds;
        if *waited <= self.preview_delay {
            return;
        }
        self.preview_wait = None;
        match self.grid.get(self.preview_row).copied().flatten() {
            Some(i) if self.part_type(i) != "owned" => {
                let p = self.parts[i].clone();
                let mut look = self.look.clone();
                if !look["selections"].is_object() {
                    look["selections"] = serde_json::json!({});
                }
                look["selections"][&p.slot] = serde_json::json!({ "asset_id": p.model_id, "material_id": p.material_id });
                bevy::log::info!("RETAIL_MENUS CreateASkater preview {} {} / {}", p.slot, p.model, p.material);
                self.preview = Some(look);
                self.preview_changed = true;
            }
            _ => self.end_preview(),
        }
    }

    /// The skater shows the committed look again (826008C0 leaving menu 10, 82537268).
    fn end_preview(&mut self) {
        self.preview_wait = None;
        if self.preview.take().is_some() {
            self.preview_changed = true;
        }
    }

    /// The grid for a part category option (82603480): the Remove entry, then the parts of the option's slot that
    /// are not hidden and pass the gender filter and the category's record test, one per wear style group
    /// (82539188), branded parts before "skate" / "no_brand" (826035F0). NOT RETAIL YET: the pack / DLC helper
    /// filters, the unlock-group check.
    fn build_grid(&mut self, option: usize) {
        self.grid = vec![None];
        self.grid_slot = None;
        let Some(row) = self.options.get(option) else { return };
        let Some(slot) = row.category.and_then(|c| self.slots.get(c as usize)).cloned() else { return };
        self.grid_slot = Some(slot.clone());
        let payload = row.payload.clone();
        let gender_ok = |p: &CasPart| if self.male { p.gender != "female" } else { p.gender != "male" };
        let test = |p: &CasPart| match slot.as_str() {
            OUTER_TORSO if payload.is_empty() => TSHIRT_TOPTYPES.contains(&p.toptype.as_str()),
            OUTER_TORSO => p.toptype == payload,
            SOCK => p.sockstyle != SOCK_EXCLUDED,
            PIERCINGS if !payload.is_empty() => p.location == payload,
            _ => true,
        };
        let listed: Vec<usize> = self.parts.iter().enumerate()
            .filter(|(_, p)| p.slot == slot && !p.hidden() && gender_ok(p) && test(p)).map(|(i, _)| i).collect();
        let mut listed = if GROUPED_SLOTS.contains(&slot.as_str()) { self.one_per_group(listed) } else { listed };
        listed.sort_by_key(|&i| LAST_BRANDS.contains(&self.parts[i].manufacturerid.as_str()));
        self.grid.extend(listed.into_iter().map(Some));
    }

    /// 82539188: per wear style group the first part flagged as the default wear style stays in place; a group
    /// without one keeps its first part, moved to the end; parts without a group stay.
    fn one_per_group(&self, listed: Vec<usize>) -> Vec<usize> {
        let (mut kept, mut pending, mut seen) = (Vec::new(), Vec::<usize>::new(), Vec::<&str>::new());
        for i in listed {
            let part = &self.parts[i];
            let Some(key) = part.group() else {
                kept.push(i);
                continue;
            };
            let flagged = part.isdefaultwearstyle == "yes";
            if !seen.contains(&key) {
                seen.push(key);
                if flagged { kept.push(i) } else { pending.push(i) }
            } else if flagged {
                if let Some(at) = pending.iter().position(|&j| self.parts[j].group() == Some(key)) {
                    pending.remove(at);
                    kept.push(i);
                }
            }
        }
        kept.extend(pending);
        kept
    }

    /// A part's wear styles (82538D38: same slot, gender and group, the part first; 82603CB0 sorts them by model
    /// name). Hidden parts are left out [inferred: the in-game only materials share the group of their part].
    fn variants(&self, seed: usize) -> Vec<usize> {
        let Some(s) = self.parts.get(seed) else { return Vec::new() };
        let Some(key) = s.group() else { return vec![seed] };
        let mut out: Vec<usize> = std::iter::once(seed).chain(self.parts.iter().enumerate()
            .filter(|(i, p)| *i != seed && p.slot == s.slot && p.gender == s.gender && p.group() == Some(key) && !p.hidden())
            .map(|(i, _)| i)).collect();
        out.sort_by(|a, b| self.parts[*a].model.cmp(&self.parts[*b].model).then(a.cmp(b)));
        out
    }

    /// The look's part in a slot (asset_id, material_id).
    fn worn(&self, slot: &str) -> Option<(&str, &str)> {
        let s = &self.look["selections"][slot];
        Some((s["asset_id"].as_str()?, s["material_id"].as_str()?))
    }

    fn wears(&self, i: usize) -> bool {
        self.parts.get(i).is_some_and(|p| self.worn(&p.slot) == Some((p.model_id.as_str(), p.material_id.as_str())))
    }

    /// 825F46C8 (the lock test of 825FD9F0 and 825FF5E8): a part with an Unlock (kind 5) or SponsorUnlock (kind 6)
    /// name is locked unless one of them is granted; a part with neither is open. With the career's gating off
    /// (`unlock_all`, the default) nothing is locked.
    fn locked(&self, i: usize) -> bool {
        use skate_core::career::unlocks::kind;
        let Some(p) = self.parts.get(i) else { return false };
        let names = [(kind::CLOTHING_UNLOCK, &p.unlock), (kind::CLOTHING_SPONSOR, &p.sponsorunlock)];
        let named: Vec<_> = names.iter().filter(|(_, n)| !n.is_empty()).collect();
        !named.is_empty() && !named.iter().any(|(k, n)| self.unlocks.is_unlocked(*k, n))
    }

    /// The part's info / option type (825FF5E8): "owned" when it or one of its wear styles is the slot's worn part
    /// (the check mark), "locked" (825F46C8), "dlc", else "none".
    fn part_type(&self, i: usize) -> &'static str {
        if self.wears(i) || self.variants(i).into_iter().any(|v| self.wears(v)) {
            "owned"
        } else if self.locked(i) {
            "locked"
        } else if self.parts.get(i).is_some_and(CasPart::dlc) {
            "dlc"
        } else {
            "none"
        }
    }

    /// The part's name (82605528): the first ID_CAC_PART_<model>_<material> the language table has over the part
    /// and its wear styles, else ID_CAC_COMPONENT_TYPE_<slot>. NOT RETAIL YET: the two parts 825FF160 names
    /// "#Bryant" by id.
    fn part_name(&self, i: usize) -> String {
        let Some(p) = self.parts.get(i) else { return String::new() };
        self.variants(i).into_iter().filter_map(|v| self.parts.get(v))
            .map(|v| format!("ID_CAC_PART_{}_{}", v.model, v.material).to_ascii_uppercase())
            .find(|id| self.names.contains(id))
            .unwrap_or_else(|| format!("ID_CAC_COMPONENT_TYPE_{}", p.slot).to_ascii_uppercase())
    }

    /// The unlock line (825FF3E0): ID_<Unlock, else SponsorUnlock>_UNLOCK, "#" without one.
    fn unlock_text(&self, i: usize) -> String {
        match self.parts.get(i) {
            Some(p) if !p.unlock.is_empty() => format!("ID_{}_UNLOCK", p.unlock).to_ascii_uppercase(),
            Some(p) if !p.sponsorunlock.is_empty() => format!("ID_{}_UNLOCK", p.sponsorunlock).to_ascii_uppercase(),
            _ => "#".into(),
        }
    }

    /// The brand logo (825FFA38 / 82604070): the image of the logo record named like the part's brand ("no_brand"
    /// when unset), "no_image" without one.
    fn logo(&self, i: usize) -> String {
        let brand = self.parts.get(i).map(|p| p.manufacturerid.as_str()).filter(|b| !b.is_empty()).unwrap_or("no_brand");
        self.logos.get(brand).filter(|l| !l.is_empty()).cloned().unwrap_or_else(|| "no_image".into())
    }

    fn set_slot(&mut self, slot: &str, part: Option<(String, String)>) {
        if !self.look["selections"].is_object() {
            self.look["selections"] = serde_json::json!({});
        }
        self.look["selections"][slot] = match part {
            Some((asset, material)) => serde_json::json!({ "asset_id": asset, "material_id": material }),
            None => serde_json::Value::Null,
        };
        self.look_changed = true;
        // The committed look goes on (sync_look); a pending preview is dropped with it.
        self.preview = None;
        self.preview_changed = false;
        self.preview_wait = None;
    }

    fn equip(&mut self, i: usize) {
        if let Some(p) = self.parts.get(i).cloned() {
            bevy::log::info!("RETAIL_MENUS CreateASkater equip {} {} / {}", p.slot, p.model, p.material);
            self.set_slot(&p.slot, Some((p.model_id, p.material_id)));
        }
    }

    /// 82600820: pops to `menu` and enters it; without it on the stack, one pop.
    fn pop_to(&mut self, menu: usize) {
        if self.stack.contains(&menu) {
            while self.stack.last() != Some(&menu) {
                self.stack.pop();
            }
        } else if self.stack.len() > 1 {
            self.stack.pop();
        }
        if let Some(m) = self.current() {
            self.enter(m);
        }
    }

    /// A thumbnail pick on menu 10 (825FD9F0). Remove clears the slot (825377E0 with no part: the slot key is set to
    /// none) and returns to Merchandise. A part with wear styles opens menu 30 (worn already: kept as it is); a
    /// single part is put on unless worn and returns to Merchandise. NOT RETAIL YET: the lock check, the purchase
    /// popup 0x11E, the board graphic special case and the two rec+0x24 / +0x34 flags that also open menu 30.
    fn pick(&mut self, row: usize) {
        let Some(entry) = self.grid.get(row).copied() else { return };
        // core_a_button on Remove and on an unlocked part, core_error on a locked one (825FD9F0).
        self.sounds.push(if entry.is_some_and(|i| self.locked(i)) { SOUND_ERROR } else { SOUND_A_BUTTON });
        let Some(i) = entry else {
            let slot = self.grid_slot.clone();
            self.pop_to(MERCHANDISE);
            if let Some(slot) = slot {
                // The Base Outfit opt-in (user ask 713; ours): a top or pants removed leaves the base part on.
                let base = self.base_outfit.then(|| self.base_part(&slot)).flatten();
                bevy::log::info!("RETAIL_MENUS CreateASkater remove {slot}{}", if base.is_some() { " (base outfit)" } else { "" });
                self.set_slot(&slot, base);
            }
            return;
        };
        // A locked part stays on the grid, not put on (the highlight preview still shows it: 825FEEA0 has no lock
        // test). NOT RETAIL YET: the DLC purchase popup 0x11E.
        if self.locked(i) {
            bevy::log::info!("RETAIL_MENUS CreateASkater locked {}", self.parts[i].model);
            return;
        }
        let variants = self.variants(i);
        if variants.len() > 1 {
            let worn = variants.iter().any(|&v| self.wears(v));
            self.wear_styles = variants;
            self.stack.push(WEAR_STYLES);
            if let Some(slot) = self.indices.get_mut(WEAR_STYLES) {
                *slot = (0, 0);
            }
            self.enter(WEAR_STYLES);
            if !worn {
                self.equip(i);
            }
            return;
        }
        let worn = self.wears(i);
        self.pop_to(MERCHANDISE);
        if !worn {
            self.equip(i);
        }
    }

    /// Left / Right on menu 30's style row: the next / previous wear style goes on.
    fn cycle_wear_style(&mut self, right: bool) -> bool {
        let n = self.wear_styles.len();
        if n < 2 {
            return false;
        }
        let at = self.wear_styles.iter().position(|&v| self.wears(v)).unwrap_or(0);
        let next = if right { (at + 1) % n } else { (at + n - 1) % n };
        self.equip(self.wear_styles[next]);
        true
    }

    /// A grid entry's thumbnail (825FF7D0): `fe\cas\<gender>\<slot>\<model>_<material>`, the Remove entry's icon.
    /// The gender folder is the skater's for clothing, none for board parts [inferred from the files].
    fn thumbnail(&self, entry: Option<usize>) -> String {
        let Some(p) = entry.and_then(|i| self.parts.get(i)) else { return "fe\\cas\\general\\remove".into() };
        let folder = if BOARD_SLOTS.contains(&p.slot.as_str()) { String::new() }
            else if self.male { "male\\".into() } else { "female\\".into() };
        format!("fe\\cas\\{folder}{}\\{}_{}", p.slot, p.model, p.material).to_ascii_lowercase()
    }

    /// The view the screen shows now: the posted target's row (None: no rows exported).
    pub(crate) fn view(&self) -> Option<CasCamera> {
        self.cameras.get(&self.camera).or_else(|| self.cameras.get(WHOLE_BODY)).copied()
    }

    /// The character's pose and place now: the posted body target's row (None: no rows exported).
    /// The posted body row; a row that does not play once takes the posture's stand clip when it has one (82532800).
    pub(crate) fn pose(&self) -> Option<CasBody> {
        let mut body = self.bodies.get(&self.body).or_else(|| self.bodies.get(WHOLE_BODY)).cloned()?;
        if !body.once {
            if let Some(stand) = self.posture_stands.get(&(self.style_value("posture") as i32 - 1)).filter(|s| !s.is_empty()) {
                body.clip = stand.clone();
            }
        }
        Some(CasBody { post: self.body_posts, ..body })
    }

    /// 82604AD0 on an Edit Style row (option id): a gesture slot posts its gesture's body row, any other row the stand
    /// (CAS_Stand 0xAD454A0ECD0E2EDF); then PostMsg_PresCASCharacterRotate(1, 0.0) turns the skater back.
    fn post_edit_style_body(&mut self, id: i32) {
        self.body = match Self::gesture_slot(id) {
            Some(slot) => format!("{:016X}", GESTURE_BODIES[self.gesture(slot)]),
            None => WHOLE_BODY.into(),
        };
        self.body_posts = self.body_posts.wrapping_add(1);
        self.turntable = 0.0;
        bevy::log::info!("RETAIL_MENUS CreateASkater body {} for row {id} (post {})", self.body, self.body_posts);
    }

    /// Entering a menu (82600AF8): the shop mode it sets; the top and Merchandise menus post the whole-body camera.
    /// NOT RETAIL YET: the other menus' targets (tables 83044528 / 83044548: the head, face and tattoo rows).
    fn enter(&mut self, menu: usize) {
        if menu != PART_GRID {
            self.end_preview();
        }
        match menu {
            HAIR_MENU => self.build_hair_list(),
            FACIAL_HAIR_MENU => self.build_style_list("FacialHairStyle"),
            EYEBROW_MENU => self.build_style_list("EyebrowStyle"),
            // One row per vault category, options 21.. (how retail fills menu 6's rows is open, b1042).
            TATTOO_CATEGORIES => {
                self.menus[menu].options = (0..self.tattoo_categories.len().min(6) as i32).map(|i| TATTOO_CATEGORY_ROW + i).collect();
            }
            WEAR_STYLES => self.build_wear_style_rows(),
            // The board's and hat's editors post their camera and body on entry (the hat's body 0x83044648, dumped
            // 2026-11-21, is the board's row).
            BOARD_GRAPHIC_ADJUST => {
                self.post_camera(BOARD_GRAPHIC_TARGETS.0.into());
                self.body = BOARD_GRAPHIC_TARGETS.1.into();
            }
            HAT_GRAPHIC_ADJUST => {
                self.post_camera(HAT_GRAPHIC_CAMERA.into());
                self.body = BOARD_GRAPHIC_TARGETS.1.into();
            }
            _ => {}
        }
        if matches!(menu, TOP_MENU | MERCHANDISE) {
            self.post_camera(WHOLE_BODY.into());
            self.body = WHOLE_BODY.into();
        }
        let shop = match menu {
            TOP_MENU => 2,
            17 => 1,
            1 => 0,
            _ => self.shop,
        };
        // 82538E78 copies the look when the shop mode changes and on every menu entry in Body Mods (mode 1).
        if (shop != self.shop || shop == 1) && !self.look.is_null() {
            self.look_copy = self.look.clone();
        }
        self.shop = shop;
    }

    /// The highlighted row's option id.
    fn highlighted(&self) -> Option<i32> {
        let m = self.current()?;
        self.option(self.indices[m].0 as usize).map(|(id, _)| id)
    }

    /// Left / Right (825FC7E8, C++) on the rows it takes: the direction's sound, the row's step (the top menu's
    /// sliders move by 0.1 within 0..1), then the movie refreshes it (SetScreenState state 50). True when a row took
    /// it or a value changed.
    pub(crate) fn left_right(&mut self, right: bool) -> bool {
        let handled = self.takes_left_right();
        let changed = self.step_value(right);
        // 825FC7E8 plays the direction's sound on every row it takes (a clamped color too), then state 50.
        if handled {
            self.sounds.push(if right { SOUND_NAV_RIGHT } else { SOUND_NAV_LEFT });
        }
        handled || changed
    }

    /// The rows 825FC7E8's Left / Right takes.
    fn takes_left_right(&self) -> bool {
        let Some(id) = self.highlighted() else { return false };
        (self.current() == Some(WEAR_STYLES) && id == WEAR_STYLE_OPTION)
            || Self::style_field(id).is_some()
            || self.colour_zone_of(id).is_some()
            || Self::gesture_slot(id).is_some()
            || matches!(id, HAIR_STYLE | FACE_PRESET | POSTURE | SKATE_STYLE | STANCE | TRUCKS | WHEELS)
    }

    fn step_value(&mut self, right: bool) -> bool {
        let step = if right { SLIDER_STEP } else { -SLIDER_STEP };
        if self.current() == Some(WEAR_STYLES) && self.highlighted() == Some(WEAR_STYLE_OPTION) {
            return self.cycle_wear_style(right);
        }
        if let Some(field) = self.highlighted().and_then(Self::style_field) {
            return self.step_style(field, right);
        }
        if self.highlighted() == Some(HAIR_STYLE) {
            return self.step_hair(right);
        }
        if let Some(zone) = self.highlighted().and_then(|id| self.colour_zone_of(id)) {
            return self.step_colour(zone, right);
        }
        if self.highlighted() == Some(FACE_PRESET) {
            return self.step_face_preset(right);
        }
        if let Some(id) = self.highlighted().filter(|id| matches!(*id, POSTURE | SKATE_STYLE | STANCE)) {
            return self.step_edit_style(id, right);
        }
        if let Some(slot) = self.highlighted().and_then(Self::gesture_slot) {
            return self.step_gesture(slot, right);
        }
        let value = match self.highlighted() {
            Some(TRUCKS) => &mut self.trucks,
            Some(WHEELS) => &mut self.wheels,
            _ => return false,
        };
        let next = ((*value + step) * 10.0).round() / 10.0;
        let next = next.clamp(0.0, 1.0);
        let changed = next != *value;
        *value = next;
        // The profile's `truck` / `wheel` (0..1), which `customiser::apply` hands to the board physics.
        if changed {
            let key = if self.highlighted() == Some(TRUCKS) { "truck" } else { "wheel" };
            self.look[key] = serde_json::json!(next);
            self.look_changed = true;
        }
        changed
    }

    fn slider(&self, id: i32) -> Option<f32> {
        match id {
            TRUCKS => Some(self.trucks),
            WHEELS => Some(self.wheels),
            _ => None,
        }
    }

    /// The movie's A (Game.OnMenuSelect -> 825FE3C0): a row with a target menu pushes it (82600780: its highlight and
    /// window start at 0); a leaf (target -1: sliders, selectors, colors, morphs) does nothing. Returns the pushed
    /// menu. NOT RETAIL YET: the menu / leaf branches of 825FE3C0 (menus 4, 5, 6, 31..33, options 56, 98..114) and
    /// the part list setup before menu 10.
    pub(crate) fn select(&mut self, row: usize) -> Option<usize> {
        // Menu 30's A is "Accept" (help kind 2): back to Merchandise with the worn style [inferred: the menu 30 branch
        // of 825FE3C0 is not read].
        let id = self.option(row)?.0;
        if self.current() == Some(WEAR_STYLES) && !matches!(id, ADD_SHIRT_GRAPHIC | ADD_GRAPHIC | REMOVE_GRAPHIC) {
            self.wear_styles.clear();
            self.pop_to(MERCHANDISE);
            return self.current();
        }
        if matches!(id, ADD_SHIRT_GRAPHIC | ADD_GRAPHIC | REMOVE_GRAPHIC | BRAND_GRAPHICS_ROW | TEAM_GRAPHICS_ROW) || SHIRT_GRAPHIC_REGIONS.contains(&id) {
            return self.graphic_select(id);
        }
        if let Some(menu) = self.tattoo_select(id) {
            return Some(menu);
        }
        if self.undo(id) {
            return None;
        }
        let option = self.option(row)?.1?;
        let target = option.target.filter(|t| *t < self.menus.len())?;
        let (title, camera, body) = (option.text.clone(), option.camera.clone(), option.body.clone());
        // 825FE3C0 posts the option's camera target with the push, and its body target (+0x28) when set.
        if !camera.is_empty() {
            self.post_camera(camera);
        }
        if !body.is_empty() {
            self.body = body;
        }
        if target == PART_GRID {
            // CAC_GetTitleText on menu 10 is the part list's title S+0x1788: the category's text (the PS3 recording
            // titles the hat grid "Hats").
            self.grid_title = title;
            self.build_grid(id as usize);
            let worn = self.grid.iter().position(|e| e.is_some_and(|i| self.wears(i)));
            bevy::log::info!("RETAIL_MENUS CreateASkater grid for option {id}: {} entries, worn at {worn:?}", self.grid.len());
        }
        self.stack.push(target);
        self.indices[target] = (0, 0);
        self.enter(target);
        Some(target)
    }

    /// Menu 30's rows at its entry (82600AF8, research b1051): Color (98) when the worn style's material is
    /// colorizable, Style (99) unless it is a board and when there is more than one style, Add Graphic when the material
    /// takes a logo (100 for a shirt, to the regions; 103 otherwise, to the categories), then Remove Graphic (104).
    fn build_wear_style_rows(&mut self) {
        let worn = self.wear_styles.iter().copied().find(|&v| self.wears(v)).or_else(|| self.wear_styles.first().copied());
        let Some(part) = worn.and_then(|i| self.parts.get(i)).cloned() else { return };
        let mut rows = Vec::new();
        if self.colourizable.contains(&part.material_id) {
            rows.push(WEAR_STYLE_COLOUR);
        }
        if part.slot != BOARD_SLOT && self.wear_styles.len() > 1 {
            rows.push(WEAR_STYLE_OPTION);
        }
        if self.logoable.contains(&part.material_id) {
            rows.push(if part.slot == OUTER_TORSO { ADD_SHIRT_GRAPHIC } else { ADD_GRAPHIC });
        }
        rows.push(REMOVE_GRAPHIC);
        self.menus[WEAR_STYLES].options = rows;
        self.graphic = crate::customiser_parts::GRAPHIC_SLOTS.contains(&part.slot.as_str()).then(|| {
            let region = self.look["graphics"][part.slot.as_str()]["region"].as_u64().unwrap_or(0) as usize;
            (part.slot.clone(), region)
        });
    }

    /// The custom graphic rows of OnMenuSelect (825FE3C0, research b1051): 100 pushes the shirt's regions, 103 the
    /// categories (with the option's camera and body targets); a region (111..114) keeps the region, posts its camera
    /// and body targets and pushes the categories; Brand (101) / Team (102) list their logos (database order; offline
    /// the team list is the 42 `team_logo_*`, retail's downloaded user logos are online only) and push the grid;
    /// Remove (104) clears the part's graphic.
    fn graphic_select(&mut self, id: i32) -> Option<usize> {
        match id {
            ADD_SHIRT_GRAPHIC => self.push(SHIRT_GRAPHIC_LOCATION),
            ADD_GRAPHIC => {
                if let Some(o) = usize::try_from(id).ok().and_then(|i| self.options.get(i)).cloned() {
                    if !o.camera.is_empty() {
                        self.post_camera(o.camera);
                    }
                    if !o.body.is_empty() {
                        self.body = o.body;
                    }
                }
                self.push(GRAPHIC_CATEGORIES)
            }
            BRAND_GRAPHICS_ROW | TEAM_GRAPHICS_ROW => {
                let team = id == TEAM_GRAPHICS_ROW;
                self.graphic_list = self.graphic_logos.iter().filter(|(_, t)| *t == team).map(|(i, _)| i.clone()).collect();
                bevy::log::info!("RETAIL_MENUS CreateASkater graphics {}: {} logos", if team { "team" } else { "brand" }, self.graphic_list.len());
                self.push(if team { TEAM_GRAPHICS } else { BRAND_GRAPHICS })
            }
            REMOVE_GRAPHIC => {
                if let Some((slot, _)) = self.graphic.clone() {
                    if let Some(g) = self.look["graphics"].as_object_mut() {
                        g.remove(&slot);
                    }
                    bevy::log::info!("RETAIL_MENUS CreateASkater graphic removed from {slot}");
                    self.look_changed = true;
                }
                self.reenter();
                None
            }
            r if SHIRT_GRAPHIC_REGIONS.contains(&r) => {
                let region = (r - SHIRT_GRAPHIC_REGIONS.start()) as usize;
                self.graphic = Some((OUTER_TORSO.into(), region));
                self.post_camera(SHIRT_GRAPHIC_CAMERAS[region].into());
                self.body = TATTOO_UPPER_TARGETS[region].1.into();
                self.push(GRAPHIC_CATEGORIES)
            }
            _ => None,
        }
    }

    /// The default placement of the part's graphic (0x830442B8 by the part's row, the gender's variant).
    fn graphic_default(&self, slot: &str, region: usize) -> [f32; 4] {
        GRAPHIC_DEFAULTS[graphic_row(slot, region)][usize::from(!self.male)]
    }

    /// A logo pick on menu 13 / 14 (825FD9F0 -> load state 8 / 11 / 12): the part's graphic at its default
    /// placement, then the part's editor (34 / 35 / 36).
    fn pick_graphic(&mut self, row: usize) {
        let (Some(id), Some((slot, region))) = (self.graphic_list.get(row).cloned(), self.graphic.clone()) else { return };
        self.sounds.push(SOUND_A_BUTTON);
        let placement = self.graphic_default(&slot, region);
        if !self.look["graphics"].is_object() {
            self.look["graphics"] = serde_json::json!({});
        }
        bevy::log::info!("RETAIL_MENUS CreateASkater graphic {id} on {slot} region {region} placement {placement:?}");
        self.look["graphics"][slot.as_str()] = serde_json::json!({ "id": id, "region": region, "placement": placement });
        self.look_changed = true;
        self.push(graphic_editor(&slot));
    }

    fn push(&mut self, menu: usize) -> Option<usize> {
        if menu >= self.menus.len() {
            return None;
        }
        self.stack.push(menu);
        self.indices[menu] = (0, 0);
        self.enter(menu);
        Some(menu)
    }

    /// The tattoo branches of OnMenuSelect (825FE3C0 menus 4 / 5 / 6, research b1042): a region row keeps the region
    /// (825FE0E0 / 825FE280) and pushes the categories, after a Yes / No warning when the location already holds a
    /// tattoo (ID_CAC_UPPER / LOWER_TATTOO_CHANGE_CONFIRMATION; the row kept in S+0x1C3C, Yes = 82604618 / 82604658);
    /// a category row lists its designs (8253E3D8, the enter of menu 11) and pushes the list. NOT RETAIL YET: the
    /// design list's vault order (ours: id order).
    fn tattoo_select(&mut self, id: i32) -> Option<usize> {
        match self.current()? {
            TATTOO_UPPER | TATTOO_LOWER => {
                let upper = TATTOO_UPPER_REGIONS.contains(&id);
                if !upper && !TATTOO_LOWER_REGIONS.contains(&id) {
                    return None;
                }
                if self.look["tattoos"][if upper { "Arm" } else { "Leg" }]["id"].is_string() {
                    self.tattoo_replace = Some(id);
                    self.popups.push(tattoo_replace_popup(upper));
                    bevy::log::info!("RETAIL_MENUS CreateASkater tattoo replace warning for row {id}");
                    return None;
                }
                self.pick_tattoo_region(id)
            }
            TATTOO_CATEGORIES => {
                let category = self.tattoo_categories.get(usize::try_from(id - TATTOO_CATEGORY_ROW).ok()?)?.clone();
                // The remove entry comes first, and only when the location already holds a tattoo [the PS3
                // recording 2026-11-16 11-13-28.mp4 ~4:25 / ~4:35: the first visit lists designs only, a later one
                // starts with the remove icon; b1042 read "appended last" from the code, which the recording does not
                // show].
                let slot = if self.tattoo_region.is_some_and(|(upper, _)| upper) { "Arm" } else { "Leg" };
                let has = self.look["tattoos"][slot]["id"].is_string();
                self.tattoo_list = has.then_some(None).into_iter()
                    .chain(self.tattoos.iter().filter(|(_, c)| *c == category).map(|(t, _)| Some(t.clone()))).collect();
                bevy::log::info!("RETAIL_MENUS CreateASkater tattoo category {category}: {} entries", self.tattoo_list.len());
                self.push(TATTOO_DESIGNS)
            }
            _ => None,
        }
    }

    /// CASScreen_PickUpper / LowerTattooRegion (825FE0E0 / 825FE280): the region kept, its camera and body targets,
    /// the clothing off, the categories pushed.
    fn pick_tattoo_region(&mut self, id: i32) -> Option<usize> {
        let region = if TATTOO_UPPER_REGIONS.contains(&id) { (true, (id - TATTOO_UPPER_REGIONS.start()) as usize) }
            else if TATTOO_LOWER_REGIONS.contains(&id) { (false, (id - TATTOO_LOWER_REGIONS.start()) as usize) }
            else { return None };
        bevy::log::info!("RETAIL_MENUS CreateASkater tattoo region {region:?}");
        self.tattoo_region = Some(region);
        // The region's camera and body targets (tables 0x83044548 / 0x83044568, 0x830445E8 / 0x83044608).
        let (camera, body) = if region.0 { TATTOO_UPPER_TARGETS[region.1] } else { TATTOO_LOWER_TARGETS[region.1] };
        self.post_camera(camera.into());
        self.body = body.into();
        self.take_clothing_off(region.0);
        self.push(TATTOO_CATEGORIES)
    }

    /// The replace warning's answer (82604618 / 82604658): Yes (0) picks the kept region row, No stays.
    pub(crate) fn tattoo_replace_answer(&mut self, choice: i64) {
        if let Some(id) = self.tattoo_replace.take().filter(|_| choice == 0) {
            self.pick_tattoo_region(id);
        }
    }

    /// A design pick on menu 11 (825FD9F0): the design goes on the region's location (the look's `tattoos.Arm` /
    /// `tattoos.Leg`, one each like retail's tail locations 1 / 2); "none" clears it; back to Body Mods. NOT RETAIL
    /// YET: the placement editor (menus 7 / 8: move, scale, rotate, fit; research b1042), so the stamp keeps our
    /// fixed placement for the region.
    fn pick_tattoo(&mut self, row: usize) {
        let (Some(entry), Some((upper, region))) = (self.tattoo_list.get(row).cloned(), self.tattoo_region) else { return };
        let slot = if upper { "Arm" } else { "Leg" };
        let side = if upper { TATTOO_UPPER_SIDE.get(region) } else { TATTOO_LOWER_SIDE.get(region) }.copied().unwrap_or(0);
        if !self.look["tattoos"].is_object() {
            self.look["tattoos"] = serde_json::json!({});
        }
        match entry {
            Some(id) => {
                let placement = self.default_placement(upper, region);
                bevy::log::info!("RETAIL_MENUS CreateASkater tattoo {slot} {id} side {side} placement {placement:?}");
                self.look["tattoos"][slot] = serde_json::json!({ "id": id, "side": side, "region": region, "placement": placement });
                self.look_changed = true;
                // 825FAAF0 states 9 / 10: the design loaded, its default placement posted, the editor pushed.
                self.push(if upper { TATTOO_UPPER_ADJUST } else { TATTOO_LOWER_ADJUST });
            }
            None => {
                bevy::log::info!("RETAIL_MENUS CreateASkater tattoo {slot} removed");
                if let Some(t) = self.look["tattoos"].as_object_mut() {
                    t.remove(slot);
                }
                self.look_changed = true;
                self.put_clothing_back();
                self.pop_to(BODY_MODS);
            }
        }
    }

    /// 825FB718 / 825FB8C8: the shirt (upper) or pants (lower) come off for the tattoo: the worn part is kept and
    /// the slot gets its lookup part 0 (ThumbnailLookup(look, slot, 0)). Which parts those are [data + the PS3
    /// recording 2026-11-16 11-13-28.mp4 ~4:25 / ~5:30]: upper, the gender's skin torso ("Arms Wholetorso": the
    /// female one is the sports bra) in the look's skin tone; lower, the gray Retro Shorts ("Retroshorts Grey").
    /// The base part of a slot: the bare torso in the look's skin tone (the sports bra on a female skater) for a top,
    /// the short shorts for pants; None for any other slot.
    fn base_part(&self, slot: &str) -> Option<(String, String)> {
        match slot {
            OUTER_TORSO => {
                let tone = self.look["skin"].as_str().unwrap_or("light");
                self.bare_torsos.get(&self.male).and_then(|(model, materials)| {
                    let material = materials.iter().find(|(_, t)| t == tone).or(materials.first())?;
                    Some((model.clone(), material.0.clone()))
                })
            }
            "Pants" => self.tattoo_shorts.get(&self.male).cloned(),
            _ => None,
        }
    }

    fn take_clothing_off(&mut self, upper: bool) {
        if self.tattoo_clothes.is_some() {
            return;
        }
        let slot = if upper { OUTER_TORSO } else { "Pants" };
        let part = self.base_part(slot).map(|(model, material)| (slot, model, material));
        let Some((slot, model, material)) = part else { return };
        self.tattoo_clothes = Some((slot.into(), self.look["selections"][slot].clone()));
        bevy::log::info!("RETAIL_MENUS CreateASkater tattoo clothing off: {slot} {model} / {material}");
        self.set_slot(slot, Some((model, material)));
    }

    /// The kept part back on (825FB718's second branch: find it by key and apply it).
    fn put_clothing_back(&mut self) {
        let Some((slot, worn)) = self.tattoo_clothes.take() else { return };
        bevy::log::info!("RETAIL_MENUS CreateASkater tattoo clothing back {slot}");
        if !self.look["selections"].is_object() {
            self.look["selections"] = serde_json::json!({});
        }
        self.look["selections"][slot.as_str()] = worn;
        self.look_changed = true;
    }

    /// The region's default placement (rotation, scale, u, v): table 0x830442B8 by the region's row.
    fn default_placement(&self, upper: bool, region: usize) -> [f32; 4] {
        let row = if upper { TATTOO_UPPER_ROW.get(region) } else { TATTOO_LOWER_ROW.get(region) }.copied().unwrap_or(0);
        GRAPHIC_DEFAULTS[row][usize::from(!self.male)]
    }

    /// The placement editor's buttons (825FC7E8 on menus 7 / 8, research b1042): A back to Body Mods (core_a_button,
    /// the clothing back on), key 5 (X) clothing off / on (825FB718), key 4 (Y) the region's default placement.
    pub(crate) fn tattoo_editor_key(&mut self, key: u32) -> bool {
        // The custom graphic editors 34..36 (b1051): A back to Merchandise (menu 1), key 4 the default placement, no
        // clothing key.
        if let (Some(SHIRT_GRAPHIC_ADJUST | BOARD_GRAPHIC_ADJUST | HAT_GRAPHIC_ADJUST), Some((slot, region))) = (self.current(), self.graphic.clone()) {
            return match key {
                2 => {
                    self.sounds.push(SOUND_A_BUTTON);
                    self.pop_to(MERCHANDISE);
                    true
                }
                4 if self.look["graphics"][slot.as_str()].is_object() => {
                    self.look["graphics"][slot.as_str()]["placement"] = serde_json::json!(self.graphic_default(&slot, region));
                    self.look_changed = true;
                    true
                }
                5 => true,
                _ => false,
            };
        }
        let (Some(menu @ (TATTOO_UPPER_ADJUST | TATTOO_LOWER_ADJUST)), Some((upper, region))) = (self.current(), self.tattoo_region) else { return false };
        let slot = if menu == TATTOO_UPPER_ADJUST { "Arm" } else { "Leg" };
        match key {
            2 => {
                self.sounds.push(SOUND_A_BUTTON);
                self.put_clothing_back();
                self.pop_to(BODY_MODS);
                true
            }
            5 => {
                if self.tattoo_clothes.is_some() { self.put_clothing_back() } else { self.take_clothing_off(upper) }
                true
            }
            4 if self.look["tattoos"][slot].is_object() => {
                self.look["tattoos"][slot]["placement"] = serde_json::json!(self.default_placement(upper, region));
                self.look_changed = true;
                true
            }
            _ => false,
        }
    }

    /// B (825FC768): pops one menu while the stack holds more than one; false on the top menu (retail opens the exit
    /// confirmation there).
    pub(crate) fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            if let Some(m) = self.current() {
                self.enter(m);
            }
            true
        } else {
            false
        }
    }

    /// The current menu.
    pub(crate) fn menu(&self) -> Option<usize> {
        self.current()
    }

    /// The current menu's panel kind (M+8: the movie's SetState 0 list, 1 thumbnails, 2 style, 3 morph, 4 scale,
    /// 5 tattoo, 6 custom thumbnails; research b1032).
    pub(crate) fn panel(&self) -> Option<u32> {
        self.current().map(|m| self.menus[m].panel)
    }

    /// Opens on the top menu with the table's initial highlights.
    pub(crate) fn begin(&mut self) {
        self.open = true;
        self.look = serde_json::Value::Null;
        self.look_changed = false;
        self.preview = None;
        self.preview_changed = false;
        self.preview_wait = None;
        self.wear_styles.clear();
        self.stack = vec![TOP_MENU];
        self.indices = self.menus.iter().map(|m| (m.index, m.window)).collect();
        self.enter(TOP_MENU);
    }

    fn current(&self) -> Option<usize> {
        self.stack.last().copied().filter(|m| *m < self.menus.len())
    }

    fn option(&self, row: usize) -> Option<(i32, Option<&CasOption>)> {
        let menu = &self.menus[self.current()?];
        let id = *menu.options.get(row)?;
        Some((id, usize::try_from(id).ok().and_then(|i| self.options.get(i))))
    }

    pub(crate) fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        let _ = vm;
        if !self.open || object != "Game"
            || !(method.starts_with("CAC_") || matches!(method, "OnMenuSelect" | "OnMenuHilight" | "OnMenuGetItemEnabled")) {
            return Ok(None);
        }
        // The CAS vt+0x48 is 8281DD70 (returns 1): every row of the list pickers is enabled. Unanswered, the
        // Merchandise picker had no highlight and ignored Up / Down.
        if method == "OnMenuGetItemEnabled" {
            return Ok(Some(Value::Bool(true)));
        }
        let Some(m) = self.current() else { return Ok(None) };
        if method == "OnMenuHilight" {
            // (menu id, row, text), as on the other screens (825FEDB8 reads the row).
            let row = args.get(1).map_or(-1, |v| crate::apt_vm::to_integer(v));
            if let Ok(row) = usize::try_from(row) {
                self.hilight(row);
            }
            return Ok(Some(Value::Undefined));
        }
        if method == "OnMenuSelect" {
            // (screen id, row).
            let row = args.get(1).map_or(-1, |v| crate::apt_vm::to_integer(v));
            if let Ok(row) = usize::try_from(row) {
                self.select(row);
            }
            return Ok(Some(Value::Undefined));
        }
        let row = args.first().map(|v| v.number().max(0.0) as usize).unwrap_or(0);
        let text = |s: &str| Value::Text(if s.is_empty() { "#".into() } else { s.into() });
        // The tattoo designs (menu 11, panel 6): each design's thumbnail `fe\cas\logos\<model>_<variant>` (the disc's
        // 190 tattoo images live there; the PS3 recording shows them in this grid), the remove icon for "none".
        if m == TATTOO_DESIGNS {
            let entry = self.tattoo_list.get(row).cloned();
            match method {
                "CAC_GetNumItems" => return Ok(Some(Value::Number(self.tattoo_list.len() as f64))),
                "CAC_GetThumbnailFileName" => {
                    let thumb = match &entry {
                        Some(Some(id)) => self.tattoo_thumbnails.get(id).cloned().unwrap_or_else(|| "no_image".into()),
                        Some(None) => "fe\\cas\\general\\remove".into(),
                        None => "no_image".into(),
                    };
                    return Ok(Some(Value::Text(thumb)));
                }
                // "owned" (the check mark) only on the location's current design, as the part grid does; the
                // recording shows no marks on a first visit.
                "CAC_GetOptionType" => {
                    let slot = if self.tattoo_region.is_some_and(|(upper, _)| upper) { "Arm" } else { "Leg" };
                    let worn = matches!(&entry, Some(Some(id)) if self.look["tattoos"][slot]["id"].as_str() == Some(id.as_str()));
                    return Ok(Some(Value::Text(if worn { "owned" } else { "none" }.into())));
                }
                "CAC_GetDescriptionText" | "CAC_GetItemText" => {
                    let name = entry.flatten().and_then(|id| self.tattoo_names.get(&id).cloned());
                    return Ok(Some(Value::Text(name.unwrap_or_else(|| "ID_CAC_REMOVE_PART".into()))));
                }
                "CAC_OnThumbnailSelect" => {
                    self.pick_tattoo(row);
                    return Ok(Some(Value::Undefined));
                }
                _ => {}
            }
        }
        // The custom graphics' logo grids (menus 13 / 14, panel 6; b1051): the logos' thumbnails, no "none" entry, the
        // check mark on the part's current logo.
        if matches!(m, BRAND_GRAPHICS | TEAM_GRAPHICS) {
            let entry = self.graphic_list.get(row).cloned();
            match method {
                "CAC_GetNumItems" => return Ok(Some(Value::Number(self.graphic_list.len() as f64))),
                "CAC_GetThumbnailFileName" => {
                    let thumb = entry.and_then(|id| self.tattoo_thumbnails.get(&id).cloned()).unwrap_or_else(|| "no_image".into());
                    return Ok(Some(Value::Text(thumb)));
                }
                "CAC_GetOptionType" => {
                    let slot = self.graphic.as_ref().map_or("", |(s, _)| s.as_str());
                    let worn = entry.is_some_and(|id| self.look["graphics"][slot]["id"].as_str() == Some(id.as_str()));
                    return Ok(Some(Value::Text(if worn { "owned" } else { "none" }.into())));
                }
                "CAC_GetDescriptionText" | "CAC_GetItemText" => {
                    return Ok(Some(text(&entry.and_then(|id| self.tattoo_names.get(&id).cloned()).unwrap_or_default())));
                }
                "CAC_OnThumbnailSelect" => {
                    self.pick_graphic(row);
                    return Ok(Some(Value::Undefined));
                }
                _ => {}
            }
        }
        // The part grid (menu 10) answers from its entries.
        if m == PART_GRID {
            match method {
                "CAC_GetNumItems" => return Ok(Some(Value::Number(self.grid.len() as f64))),
                "CAC_GetTitleText" if !self.grid_title.is_empty() => return Ok(Some(Value::Text(self.grid_title.clone()))),
                "CAC_GetThumbnailFileName" => {
                    return Ok(Some(Value::Text(self.grid.get(row).map_or("no_image".into(), |e| self.thumbnail(*e)))));
                }
                // 825FF4C8 / 825A74F0: the Remove entry "none" / "blurb", a part its own type (825FF5E8).
                "CAC_GetOptionType" | "CAC_GetInfoPanelType" if self.shop == 0 || method == "CAC_GetOptionType" => {
                    let kind = match self.grid.get(row) {
                        Some(Some(i)) => self.part_type(*i),
                        _ if method == "CAC_GetOptionType" => "none",
                        _ => "blurb",
                    };
                    return Ok(Some(Value::Text(kind.into())));
                }
                // 825FF160: the Remove entry's text, else the part's name.
                "CAC_GetDescriptionText" => {
                    return Ok(Some(Value::Text(match self.grid.get(row) {
                        Some(Some(i)) => self.part_name(*i),
                        _ => "ID_CAC_REMOVE_PART".into(),
                    })));
                }
                "CAC_GetItemUnlockHALID" => {
                    return Ok(Some(Value::Text(self.grid.get(row).copied().flatten().map_or("#".into(), |i| self.unlock_text(i)))));
                }
                "CAC_GetSponsorLogoFileName" => {
                    let logo = self.grid.get(row).copied().flatten().map(|i| self.logo(i));
                    return Ok(Some(Value::Text(logo.unwrap_or_else(|| "no_image".into()))));
                }
                "CAC_OnThumbnailSelect" => {
                    let entry = self.grid.get(row).copied().flatten().and_then(|i| self.parts.get(i));
                    bevy::log::info!("RETAIL_MENUS CreateASkater pick {row}: {:?}", entry.map(|p| (&p.slot, &p.model, &p.material)));
                    self.pick(row);
                    return Ok(Some(Value::Undefined));
                }
                _ => {}
            }
        }
        Ok(Some(match method {
            "CAC_GetTitleText" => text(&self.menus[m].title),
            "CAC_GetNumItems" => Value::Number(self.menus[m].options.len() as f64),
            // A table without the option records (extracted before them) shows the option id.
            "CAC_GetItemText" => match self.option(row) {
                Some((_, Some(o))) => text(&o.text),
                Some((id, None)) => Value::Text(format!("#option {id}")),
                None => text(""),
            },
            "CAC_GetDescriptionText" => self.option(row).and_then(|(_, o)| o).map_or(text(""), |o| text(&o.description)),
            "CAC_GetOptionType" => self.option(row).and_then(|(_, o)| o).map_or(text(""), |o| text(&o.kind)),
            "CAC_GetCurrentMenuIndex" => Value::Number(self.indices[m].0 as f64),
            "CAC_GetCurrentMenuMinWindowIndex" => Value::Number(self.indices[m].1 as f64),
            "CAC_UpdateMenuIndices" => {
                let index = args.first().map_or(0.0, |v| v.number()).max(0.0) as u32;
                let window = args.get(1).map_or(0.0, |v| v.number()).max(0.0) as u32;
                self.indices[m] = (index, window);
                Value::Undefined
            }
            // 825A7A08: help A by kind (1 SELECT, except the top menu's two sliders; 2 ACCEPT).
            "CAC_GetButtonHelpTextA" => {
                let slider = m == TOP_MENU && matches!(self.option(self.indices[m].0 as usize), Some((47 | 48, _)));
                text(match self.menus[m].help_a {
                    1 if !slider => "ID_COMMON_SELECT",
                    2 => "ID_COMMON_ACCEPT",
                    _ => "",
                })
            }
            // 825A7B28: help B by kind.
            "CAC_GetButtonHelpTextB" => text(match self.menus[m].help_b {
                1 => "ID_CAC_EXIT",
                2 => "ID_COMMON_BACK",
                3 => "ID_CAC_CHANGE_GRAPHIC",
                _ => "",
            }),
            "CAC_GetFilterText" => text(""),
            "CAC_GetReceiptBalance" => Value::Text(String::new()),
            // 825A7988 / 825A74F0 by the shop mode (S+0x177C). NOT RETAIL YET: the part thumbnails' own panel types.
            "CAC_GetBannerType" => Value::Text(match self.shop { 0 => "slappys", 1 => "petes", _ => "none" }.into()),
            "CAC_GetInfoPanelType" => Value::Text(match self.shop { 1 => "petes", 2 => "blurb", _ => "noblurb" }.into()),
            // 825FFB18 / 825FFE08 for the sliders: round(value * 10); LOOSE / MEDIUM / TIGHT, SOFT / MEDIUM / HARD.
            "CAC_GetIntegerValue" if self.option(row).and_then(|(id, _)| self.colour_zone_of(id)).is_some() => {
                let zone = self.option(row).and_then(|(id, _)| self.colour_zone_of(id)).unwrap_or_default();
                Value::Number(self.colour_index(zone) as f64)
            }
            // A morph row: the shown value x 122, rounded (b1041: 822F95E4 = 122.0; the movie places its knob by it).
            "CAC_GetIntegerValue" if self.morph_row(row).is_some() => {
                Value::Number((self.morph_row(row).map_or(0.0, |(_, n)| n) * MORPH_DISPLAY + 0.5).floor() as f64)
            }
            "CAC_GetIntegerValue" => match self.option(row).and_then(|(id, _)| self.slider(id)) {
                Some(v) => Value::Number((v * 10.0).round() as f64),
                None => Value::Number(0.0),
            },
            "CAC_GetColours" => {
                let zone = self.option(row).and_then(|(id, _)| self.colour_zone_of(id));
                let swatches = zone.map(|z| self.colour_swatches(z)).unwrap_or_default();
                Value::Object(vm.array(swatches)?)
            }
            "CAC_GetStringValue" if self.option(row).is_some_and(|(id, _)| id == HAIR_STYLE) => Value::Text(self.hair_text()),
            "CAC_GetStringValue" if self.option(row).is_some_and(|(id, _)| id == FACE_PRESET) => Value::Text(self.face_preset_text()),
            "CAC_GetStringValue" if self.option(row).and_then(|(id, _)| self.edit_style_text(id)).is_some() => {
                Value::Text(self.option(row).and_then(|(id, _)| self.edit_style_text(id)).unwrap_or("#").into())
            }
            "CAC_GetStringValue" if self.option(row).and_then(|(id, _)| Self::style_field(id)).is_some() => {
                let field = self.option(row).and_then(|(id, _)| Self::style_field(id)).unwrap_or_default();
                Value::Text(self.style_text(field))
            }
            "CAC_GetStringValue" => match self.option(row).and_then(|(id, _)| self.slider(id).map(|v| (id, v))) {
                Some((id, v)) => {
                    let level = if v < SLIDER_LOW { 0 } else if v > SLIDER_HIGH { 2 } else { 1 };
                    let labels = if id == TRUCKS {
                        ["ID_CAC_EDIT_TRUCKS_LOOSE", "ID_CAC_EDIT_TRUCKS_MEDIUM", "ID_CAC_EDIT_TRUCKS_TIGHT"]
                    } else {
                        ["ID_CAC_EDIT_WHEELS_SOFT", "ID_CAC_EDIT_WHEELS_MEDIUM", "ID_CAC_EDIT_WHEELS_HARD"]
                    };
                    Value::Text(labels[level].into())
                }
                None => text(""),
            },
            "CAC_IsPartNew" => Value::Bool(false),
            "CAC_GetThumbnailFileName" => Value::Text("no_image".into()),
            _ => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(name: &str, title: &str, help: (u32, u32), options: &[i32]) -> CasMenu {
        CasMenu { name: name.into(), title: title.into(), panel: 0, mode: 0, help_a: help.0, help_b: help.1, index: 0,
            window: 0, options: options.to_vec() }
    }

    fn host() -> CasHost {
        let mut menus: Vec<CasMenu> = (0..16).map(|i| menu(&format!("m{i}"), "", (0, 0), &[])).collect();
        menus[TOP_MENU] = menu("EditSkaterMain", "ID_CAC_EDITSKATER_TITLE", (1, 1), &[0, 1, 46, 47, 48]);
        let mut h = CasHost::new(menus, vec![skate_data::menu_tables::CasOptionRow { text: "ID_BODY".into(),
            description: String::new(), target: 17, kind: "option".into(), ..Default::default() }]);
        h.begin();
        h
    }

    #[test]
    fn the_top_menu_answers_from_the_table() {
        let mut h = host();
        let mut vm = Vm::new();
        let mut call = |h: &mut CasHost, m: &str, args: &[Value]| h.call(&mut vm, "Game", m, args).unwrap().unwrap();
        assert_eq!(call(&mut h, "CAC_GetTitleText", &[]), Value::Text("ID_CAC_EDITSKATER_TITLE".into()));
        assert_eq!(call(&mut h, "CAC_GetNumItems", &[]), Value::Number(5.0));
        assert_eq!(call(&mut h, "CAC_GetItemText", &[Value::Number(0.0)]), Value::Text("ID_BODY".into()));
        assert_eq!(call(&mut h, "CAC_GetButtonHelpTextA", &[]), Value::Text("ID_COMMON_SELECT".into()));
        assert_eq!(call(&mut h, "CAC_GetButtonHelpTextB", &[]), Value::Text("ID_CAC_EXIT".into()));
        // The sliders (options 47 / 48) show no A help.
        call(&mut h, "CAC_UpdateMenuIndices", &[Value::Number(3.0), Value::Number(0.0)]);
        assert_eq!(call(&mut h, "CAC_GetCurrentMenuIndex", &[]), Value::Number(3.0));
        assert_eq!(call(&mut h, "CAC_GetButtonHelpTextA", &[]), Value::Text("#".into()));
    }

    #[test]
    fn a_pushes_the_rows_target_and_b_pops_it() {
        let mut menus: Vec<CasMenu> = (0..18).map(|i| menu(&format!("m{i}"), "", (0, 0), &[])).collect();
        menus[TOP_MENU] = menu("EditSkaterMain", "ID_CAC_EDITSKATER_TITLE", (1, 1), &[0]);
        menus[17] = menu("BackAlleyPete", "ID_CAC_BODY_MODIFICATION_TITLE", (1, 2), &[]);
        let mut h = CasHost::new(menus, vec![skate_data::menu_tables::CasOptionRow { text: "ID_BODY".into(),
            description: String::new(), target: 17, kind: "option".into(), ..Default::default() }]);
        h.begin();
        assert_eq!(h.shop, 2);
        let mut vm = Vm::new();
        h.call(&mut vm, "Game", "OnMenuSelect", &[Value::Number(688.0), Value::Number(0.0)]).unwrap();
        assert_eq!((h.menu(), h.shop), (Some(17), 1));
        assert!(h.back());
        assert_eq!((h.menu(), h.shop), (Some(TOP_MENU), 2));
        assert!(!h.back());
    }

    #[test]
    fn the_sliders_step_by_a_tenth_and_name_their_level() {
        let mut h = host();
        let mut vm = Vm::new();
        h.indices[TOP_MENU] = (3, 0);
        assert_eq!(h.call(&mut vm, "Game", "CAC_GetIntegerValue", &[Value::Number(3.0)]).unwrap(), Some(Value::Number(5.0)));
        assert!(h.left_right(true));
        assert!(h.left_right(true));
        assert!(h.left_right(true));
        assert_eq!(h.call(&mut vm, "Game", "CAC_GetStringValue", &[Value::Number(3.0)]).unwrap(),
            Some(Value::Text("ID_CAC_EDIT_TRUCKS_TIGHT".into())));
        for _ in 0..12 {
            h.left_right(false);
        }
        assert_eq!(h.trucks, 0.0);
        h.left_right(false);
        assert_eq!(h.trucks, 0.0);
        assert_eq!(h.call(&mut vm, "Game", "CAC_GetStringValue", &[Value::Number(4.0)]).unwrap(),
            Some(Value::Text("ID_CAC_EDIT_WHEELS_MEDIUM".into())));
    }

    #[test]
    fn a_category_lists_its_slots_parts_for_the_skater() {
        let part = |slot: &str, model: &str, gender: &str, toptype: &str, brand: &str| CasPart { slot: slot.into(),
            model: model.into(), material: format!("{model}_mat"), gender: gender.into(), toptype: toptype.into(),
            manufacturerid: brand.into(), ..Default::default() };
        let mut h = host();
        h.slots = ["Arm", "Leg", "Hair", "Rostral", "Torso", "Feet", "Accessory", "Equipment", "Skeleton", "Organ", "Misc",
            "SkateBoard", "SkateTruck", "SkateWheel", "Beard", "Pants", "Underwear", "Sock", "Hat", "Belt", "Glasses",
            "WristItem", "InnerTorso", "OuterTorso", "Piercings", "Jewellery"].map(String::from).to_vec();
        h.parts = vec![part("Hat", "cap_a", "male", "", "skate"), part("Hat", "cap_b", "female", "", "dc"),
            part("Hat", "cap_c", "unisex", "", "dc"), part("OuterTorso", "tee", "male", "tshirt", "dc"),
            part("OuterTorso", "hood", "male", "hoody", "dc"), part("SkateBoard", "skateboard", "unisex", "", "dc")];
        let row = |category: u32, payload: &str| CasOption { category: Some(category), payload: payload.into(), ..Default::default() };
        h.options = vec![row(18, ""), row(23, ""), row(23, "hoody"), row(11, "")];
        h.build_grid(0);
        // Remove first, then the male and unisex hats, branded before "skate".
        assert_eq!(h.grid, vec![None, Some(2), Some(0)]);
        assert_eq!(h.thumbnail(None), "fe\\cas\\general\\remove");
        assert_eq!(h.thumbnail(Some(0)), "fe\\cas\\male\\hat\\cap_a_cap_a_mat");
        h.build_grid(1);
        assert_eq!(h.grid, vec![None, Some(3)]);
        h.build_grid(2);
        assert_eq!(h.grid, vec![None, Some(4)]);
        h.build_grid(3);
        assert_eq!(h.thumbnail(h.grid[1]), "fe\\cas\\skateboard\\skateboard_skateboard_mat");
    }

    fn grid_host() -> CasHost {
        let mut menus: Vec<CasMenu> = (0..31).map(|i| menu(&format!("m{i}"), "", (0, 0), &[])).collect();
        menus[TOP_MENU] = menu("EditSkaterMain", "", (1, 1), &[0]);
        menus[MERCHANDISE] = menu("Slappys", "", (1, 2), &[1, 2]);
        menus[WEAR_STYLES] = menu("WearStyles", "", (2, 0), &[98, 99]);
        let row = |target: i32, category: Option<u32>| skate_data::menu_tables::CasOptionRow { target, category, ..Default::default() };
        let mut options = vec![row(MERCHANDISE as i32, None), row(PART_GRID as i32, Some(18)), row(PART_GRID as i32, Some(5))];
        options.resize(100, row(-1, None));
        let mut h = CasHost::new(menus, options);
        h.slots = (0..26).map(|i| match i { 5 => "Feet", 18 => "Hat", _ => "Other" }.to_string()).collect();
        let part = |slot: &str, model: &str, material: &str| CasPart { slot: slot.into(), model_id: format!("{model}#"),
            model: model.into(), material_id: format!("{material}#"), material: material.into(), gender: "male".into(),
            ..Default::default() };
        h.parts = vec![
            CasPart { isdefaultwearstyle: "yes".into(), ..part("Hat", "cap", "red") },
            part("Hat", "cap_2_45left", "red"),
            part("Hat", "cap_4_backward", "blue"),
            CasPart { isdefaultwearstyle: "yes".into(), ..part("Hat", "cap", "blue") },
            CasPart { manufacturerid: "dc".into(), unlock: "cac_unlock_bs_group2".into(), ..part("Feet", "shoe", "white") },
            CasPart { isusedingame: "1".into(), ..part("Feet", "shoe", "white_d") },
        ];
        h.names = ["ID_CAC_PART_CAP_BLUE".to_string()].into_iter().collect();
        h.logos = [("no_brand".to_string(), "fe\\source\\images\\sponsor_logos\\skate".to_string())].into_iter().collect();
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": { "Hat": { "asset_id": "cap#", "material_id": "red#" } } });
        h
    }

    fn open_grid(h: &mut CasHost, option_row: usize) {
        h.select(0);
        assert_eq!(h.menu(), Some(MERCHANDISE));
        h.select(option_row);
        assert_eq!(h.menu(), Some(PART_GRID));
    }

    #[test]
    fn a_grid_keeps_one_part_per_wear_style_group_and_hides_in_game_materials() {
        let mut h = grid_host();
        open_grid(&mut h, 0);
        // The hats group by material (no StyleSubcategory): the default wear style of each.
        assert_eq!(h.grid, vec![None, Some(0), Some(3)]);
        h.back();
        h.select(1);
        // The dirty "_d" shoe is hidden.
        assert_eq!(h.grid, vec![None, Some(4)]);
    }

    #[test]
    fn the_info_panel_names_the_part_its_brand_and_unlock_and_marks_the_worn_one() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        open_grid(&mut h, 0);
        let mut call = |h: &mut CasHost, m: &str, row: f64| h.call(&mut vm, "Game", m, &[Value::Number(row)]).unwrap().unwrap();
        assert_eq!(call(&mut h, "CAC_GetOptionType", 0.0), Value::Text("none".into()));
        assert_eq!(call(&mut h, "CAC_GetInfoPanelType", 0.0), Value::Text("blurb".into()));
        assert_eq!(call(&mut h, "CAC_GetDescriptionText", 0.0), Value::Text("ID_CAC_REMOVE_PART".into()));
        // The red cap is worn: "owned" (the check mark); the blue one is not.
        assert_eq!(call(&mut h, "CAC_GetOptionType", 1.0), Value::Text("owned".into()));
        assert_eq!(call(&mut h, "CAC_GetOptionType", 2.0), Value::Text("none".into()));
        // Names: the language table's part id, else the slot's.
        assert_eq!(call(&mut h, "CAC_GetDescriptionText", 1.0), Value::Text("ID_CAC_COMPONENT_TYPE_HAT".into()));
        assert_eq!(call(&mut h, "CAC_GetDescriptionText", 2.0), Value::Text("ID_CAC_PART_CAP_BLUE".into()));
        assert_eq!(call(&mut h, "CAC_GetItemUnlockHALID", 1.0), Value::Text("#".into()));
        assert_eq!(call(&mut h, "CAC_GetSponsorLogoFileName", 0.0), Value::Text("no_image".into()));
        // No brand: the "no_brand" record's image; a brand without a record: no_image.
        assert_eq!(call(&mut h, "CAC_GetSponsorLogoFileName", 1.0), Value::Text("fe\\source\\images\\sponsor_logos\\skate".into()));
        h.back();
        h.select(1);
        assert_eq!(call(&mut h, "CAC_GetItemUnlockHALID", 1.0), Value::Text("ID_CAC_UNLOCK_BS_GROUP2_UNLOCK".into()));
        assert_eq!(call(&mut h, "CAC_GetSponsorLogoFileName", 1.0), Value::Text("no_image".into()));
    }

    #[test]
    fn a_pick_wears_the_part_and_remove_clears_the_slot() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        open_grid(&mut h, 1);
        h.call(&mut vm, "Game", "CAC_OnThumbnailSelect", &[Value::Number(1.0)]).unwrap();
        assert_eq!(h.look["selections"]["Feet"], serde_json::json!({ "asset_id": "shoe#", "material_id": "white#" }));
        assert!(h.look_changed);
        assert_eq!(h.menu(), Some(MERCHANDISE));
        h.select(0);
        h.call(&mut vm, "Game", "CAC_OnThumbnailSelect", &[Value::Number(0.0)]).unwrap();
        assert!(h.look["selections"]["Hat"].is_null());
        assert_eq!(h.menu(), Some(MERCHANDISE));
    }

    #[test]
    fn a_part_with_an_ungranted_unlock_is_locked_and_cannot_be_picked() {
        use skate_core::career::unlocks::kind;
        let mut h = grid_host();
        let mut vm = Vm::new();
        open_grid(&mut h, 1);
        let mut call = |h: &mut CasHost, m: &str, row: f64| h.call(&mut vm, "Game", m, &[Value::Number(row)]).unwrap().unwrap();
        // Gating off (the default): open.
        assert_eq!(call(&mut h, "CAC_GetOptionType", 1.0), Value::Text("none".into()));
        h.unlocks.unlock_all = false;
        assert_eq!(call(&mut h, "CAC_GetOptionType", 1.0), Value::Text("locked".into()));
        call(&mut h, "CAC_OnThumbnailSelect", 1.0);
        assert!(h.look["selections"]["Feet"].is_null(), "not put on");
        assert_eq!(h.menu(), Some(PART_GRID), "stays on the grid");
        h.unlocks.grant(kind::CLOTHING_UNLOCK, "cac_unlock_bs_group2");
        assert_eq!(call(&mut h, "CAC_GetOptionType", 1.0), Value::Text("none".into()));
        call(&mut h, "CAC_OnThumbnailSelect", 1.0);
        assert_eq!(h.look["selections"]["Feet"], serde_json::json!({ "asset_id": "shoe#", "material_id": "white#" }));
    }

    #[test]
    fn the_facial_hair_and_eyebrow_selectors_step_head_looks_that_keep_the_other_fields() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.menus[FACIAL_HAIR_MENU] = menu("FacialHair", "", (0, 0), &[FACIAL_HAIR]);
        h.menus[EYEBROW_MENU] = menu("Brow", "", (0, 0), &[EYEBROWS]);
        h.options[0].target = Some(FACIAL_HAIR_MENU);
        h.options[1].target = Some(EYEBROW_MENU);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0, 1]);
        let look = |m: &str, tone: &str, beard: &str, brow: &str| HeadLook { model: "head#".into(), material: m.into(),
            fields: [("SkinTone", tone), ("HairlineShadowStyle", ""), ("HairlineShadowColour", "black"),
                ("FacialHairStyle", beard), ("EyebrowStyle", brow)].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() };
        h.heads = vec![look("a", "light", "stubble", "style2"), look("b", "light", "full beard", "style2"),
            look("c", "dark", "goatee", "style2"), look("d", "light", "stubble", "style7"), look("e", "light", "stubble", "style1")];
        h.look["selections"]["Rostral"] = serde_json::json!({ "asset_id": "head#", "material_id": "a" });
        h.begin();
        h.look["selections"]["Rostral"] = serde_json::json!({ "asset_id": "head#", "material_id": "a" });
        // Facial hair: the light-skinned style2 looks only (not the dark one, not other brows), on the worn one.
        h.select(0);
        assert_eq!(h.style_list.iter().map(|&i| h.heads[i].material.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        let text = |h: &mut CasHost, vm: &mut Vm| h.call(vm, "Game", "CAC_GetStringValue", &[Value::Number(0.0)]).unwrap().unwrap();
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_FACIAL_HAIR_STUBBLE".into()));
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"]["Rostral"]["material_id"], "b");
        assert_eq!(h.look["beard"], "full beard");
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_FACIAL_HAIR_FULL_BEARD".into()));
        assert!(h.left_right(true), "wraps");
        assert_eq!(h.look["selections"]["Rostral"]["material_id"], "a");
        // Eyebrows: sorted by style, style7 left out for the male skater.
        h.back();
        h.select(1);
        assert_eq!(h.style_list.iter().map(|&i| h.heads[i].material.as_str()).collect::<Vec<_>>(), ["e", "a"]);
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_EYEBROW_STYLE2".into()));
        h.male = false;
        h.back();
        h.select(1);
        assert_eq!(h.style_list.len(), 3, "a female skater keeps style7");
    }

    #[test]
    fn edit_style_rows_write_the_profiles_posture_style_stance_and_the_sliders_its_truck_and_wheel() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.menus[16] = menu("EditStyle", "", (0, 0), &[POSTURE, SKATE_STYLE, STANCE]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0, TRUCKS]);
        h.options[0].target = Some(16);
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {} });
        h.select(0);
        let text = |h: &mut CasHost, vm: &mut Vm, row: f64| h.call(vm, "Game", "CAC_GetStringValue", &[Value::Number(row)]).unwrap().unwrap();
        // Unset: posture 1, style normal, stance regular (the profile's defaults 0, 0, 1).
        assert_eq!(text(&mut h, &mut vm, 0.0), Value::Text("ID_CAC_LABEL_POSTURE_1".into()));
        assert_eq!(text(&mut h, &mut vm, 2.0), Value::Text("ID_CAC_STANCE_REGULAR".into()));
        let posts = h.body_posts;
        assert!(h.left_right(false), "posture wraps back from 0");
        assert_eq!(h.look["posture"], 3);
        assert_eq!(h.body_posts, posts + 1, "posture re-posts the body target, restarting the clip");
        h.call(&mut vm, "Game", "CAC_UpdateMenuIndices", &[Value::Number(1.0), Value::Number(0.0)]).unwrap();
        let posts = h.body_posts;
        assert!(h.left_right(true));
        assert_eq!(h.look["style"], 1);
        assert_eq!(h.body_posts, posts, "style does not re-post");
        assert_eq!(text(&mut h, &mut vm, 1.0), Value::Text("ID_CAC_LABEL_STYLE_LOOSE".into()));
        h.call(&mut vm, "Game", "CAC_UpdateMenuIndices", &[Value::Number(2.0), Value::Number(0.0)]).unwrap();
        assert!(h.left_right(true));
        assert_eq!(h.look["stance"], 0, "goofy");
        assert_eq!(text(&mut h, &mut vm, 2.0), Value::Text("ID_CAC_STANCE_GOOFY".into()));
        assert!(h.look_changed);
        // The Trucks slider writes the profile's truck.
        h.back();
        h.call(&mut vm, "Game", "CAC_UpdateMenuIndices", &[Value::Number(1.0), Value::Number(0.0)]).unwrap();
        assert!(h.left_right(true));
        assert_eq!(h.look["truck"].as_f64().map(|v| (v * 10.0).round()), Some(6.0));
    }

    #[test]
    fn gesture_rows_step_the_unlocked_gestures_into_the_profiles_slots() {
        use skate_core::career::unlocks::kind;
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.menus[16] = menu("EditStyle", "", (0, 0), &[49, 50, 51, 52]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(16);
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {} });
        h.select(0);
        let text = |h: &mut CasHost, vm: &mut Vm, row: f64| h.call(vm, "Game", "CAC_GetStringValue", &[Value::Number(row)]).unwrap().unwrap();
        // Unset: each slot shows its own index (row 1 = option 50 = slot "3" = Bruce).
        assert_eq!(text(&mut h, &mut vm, 0.0), Value::Text("ID_GESTURE_AIRGUITAR".into()));
        assert_eq!(text(&mut h, &mut vm, 1.0), Value::Text("ID_GESTURE_BRUCE".into()));
        assert!(h.left_right(false), "wraps to the last gesture");
        assert_eq!(h.look["gestures"]["0"], 36);
        assert_eq!(text(&mut h, &mut vm, 0.0), Value::Text("ID_GESTURE_YARDSALE".into()));
        assert_eq!(h.body, "BD81B91A70FB59B1", "the yardsale body target row plays it on the skater");
        // Gating on: only granted gestures are offered.
        h.unlocks.unlock_all = false;
        h.unlocks.grant(kind::GESTURES, "gesture_fingerpoint");
        h.unlocks.grant(kind::GESTURES, "gesture_wings");
        // The current gesture is not in the list: retail counts from index 0 (825FC7E8), so Right gives list[1].
        assert!(h.left_right(true));
        assert_eq!(h.look["gestures"]["0"], 35);
        assert_eq!(text(&mut h, &mut vm, 0.0), Value::Text("ID_GESTURE_WINGS".into()));
        assert!(h.left_right(true), "wraps");
        assert_eq!(h.look["gestures"]["0"], 22);
    }

    #[test]
    fn highlighting_an_edit_style_row_posts_its_gesture_or_the_stand() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.menus[16] = menu("EditStyle", "", (0, 0), &[49, 50, 55]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(16);
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {} });
        h.select(0);
        let hilight = |h: &mut CasHost, vm: &mut Vm, row: f64| {
            h.call(vm, "Game", "OnMenuHilight", &[Value::Number(16.0), Value::Number(row)]).unwrap();
        };
        h.turntable = 1.0;
        hilight(&mut h, &mut vm, 1.0);
        assert_eq!(h.body, format!("{:016X}", GESTURE_BODIES[3]), "row 50 = slot 3 = Bruce (825FEDB8 -> 82604AD0)");
        assert_eq!(h.turntable, 0.0);
        let post = h.body_posts;
        hilight(&mut h, &mut vm, 1.0);
        assert_eq!(h.body_posts, post + 1, "the same gesture posts again, so it plays again");
        hilight(&mut h, &mut vm, 2.0);
        assert_eq!(h.body, WHOLE_BODY, "Stance posts the stand");
    }

    #[test]
    fn the_hair_style_selector_steps_full_hair_of_the_gender_with_bald_first_for_a_male_skater() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.menus[HAIR_MENU] = menu("Hair", "", (0, 0), &[HAIR_STYLE]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(HAIR_MENU);
        let hair = |model: &str, gender: &str| CasPart { slot: "Hair".into(), model_id: model.into(), model: model.into(),
            material_id: format!("{model}-m"), material: "m".into(), gender: gender.into(), ..Default::default() };
        h.parts = vec![hair("afro", "male"), hair("afro_hat", "male"), hair("mohawk", "male"), hair("bob", "female")];
        h.hair_models = [("afro", "full", "afro"), ("afro_hat", "hat", "afro"), ("mohawk", "full", "moosed up"), ("bob", "full", "bob")]
            .into_iter().map(|(m, k, s)| (m.to_string(), (k.to_string(), s.to_string()))).collect();
        h.begin();
        // Wearing the afro's hat model: the list starts on the afro.
        h.look = serde_json::json!({ "gender": "male", "selections": { "Hair": { "asset_id": "afro_hat", "material_id": "afro_hat-m" } } });
        h.select(0);
        assert_eq!(h.hair_list, vec![None, Some(0), Some(2)], "bald, then the male full models in database order");
        let text = |h: &mut CasHost, vm: &mut Vm| h.call(vm, "Game", "CAC_GetStringValue", &[Value::Number(0.0)]).unwrap().unwrap();
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_HAIRSTYLE_AFRO".into()));
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"]["Hair"]["asset_id"], "mohawk");
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_HAIRSTYLE_MOOSED_UP".into()));
        assert!(h.left_right(true), "wraps to bald");
        assert!(h.look["selections"]["Hair"].is_null());
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_HAIRSTYLE_BALD".into()));
        // A female skater: her full models, no Remove.
        h.male = false;
        h.back();
        h.select(0);
        assert_eq!(h.hair_list, vec![Some(3)]);
    }

    #[test]
    fn a_color_row_starts_on_the_zones_default_steps_clamped_and_keeps_the_key() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.menus[HAIR_MENU] = menu("Hair", "", (0, 0), &[66]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(HAIR_MENU);
        let colour = |key: &str, r: f32| CasColour { key: key.into(), swatch: [r, 0.0, 0.0], a: [r; 3], b: [0.0; 3], text: key.into() };
        h.colour_zones = [(1u32, CasColourZone { default: "B".into(), colours: vec![colour("A", 0.1), colour("B", 0.2), colour("C", 0.3)] })].into();
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {} });
        h.select(0);
        let call = |h: &mut CasHost, vm: &mut Vm, m: &str| h.call(vm, "Game", m, &[Value::Number(0.0)]).unwrap().unwrap();
        assert_eq!(call(&mut h, &mut vm, "CAC_GetIntegerValue"), Value::Number(1.0), "the default B");
        assert!(matches!(call(&mut h, &mut vm, "CAC_GetColours"), Value::Object(_)));
        assert!(h.left_right(true));
        assert_eq!(h.look["colour_keys"]["1"], "C");
        assert_eq!(h.look["colour_tints"]["1"], serde_json::json!([0.3f32, 0.3f32, 0.3f32]), "recipe A goes on the zone's tint");
        assert_eq!(h.look["colour_highlights"]["1"], serde_json::json!([0.0f32, 0.0f32, 0.0f32]), "recipe B on the highlight");
        h.sounds.clear();
        assert!(h.left_right(true), "a clamped row still refreshes (825FC7E8)");
        assert_eq!(h.look["colour_keys"]["1"], "C", "clamped at the last color, no wrap");
        assert_eq!(h.sounds, [SOUND_NAV_RIGHT], "and plays the direction's sound");
        assert!(h.left_right(false) && h.left_right(false));
        assert_eq!(h.look["colour_keys"]["1"], "A");
        assert_eq!(call(&mut h, &mut vm, "CAC_GetIntegerValue"), Value::Number(0.0));
        h.left_right(false);
        assert_eq!(h.look["colour_keys"]["1"], "A", "clamped at the first");
        assert_eq!(h.sounds.last(), Some(&SOUND_NAV_LEFT));
    }

    #[test]
    fn an_eye_color_pick_wears_the_eye_material_with_that_color_and_no_tint() {
        let mut h = grid_host();
        h.menus[HAIR_MENU] = menu("Eyes", "", (0, 0), &[76]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(HAIR_MENU);
        let colour = |key: &str, text: &str| CasColour { key: key.into(), swatch: [0.0; 3], a: [1.0; 3], b: [0.0; 3], text: text.into() };
        h.colour_zones = [(EYE_ZONE, CasColourZone { default: "A".into(), colours: vec![colour("A", "blue"), colour("B", "green")] })].into();
        let eyes = |male: bool, p: &str| EyeModel { male, materials: vec![(format!("{p}blue"), "blue".into()), (format!("{p}green"), "green".into())] };
        h.eye_models = [("eyes_m".to_string(), eyes(true, "m_")), ("eyes_f".to_string(), eyes(false, "f_"))].into();
        h.begin();
        h.look = serde_json::json!({ "gender": "female", "selections": {} });
        h.male = false;
        h.select(0);
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"][EYE_SLOT], serde_json::json!({ "asset_id": "eyes_f", "material_id": "f_green" }));
        assert!(h.look["colour_tints"][EYE_ZONE.to_string()].is_null(), "the eye zone swaps a part, no tint");
    }

    #[test]
    fn morph_rows_step_per_frame_clamped_and_the_body_row_splits_fat_and_thin() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.menus[HAIR_MENU] = menu("Eyes", "", (0, 0), &[78, 64]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(HAIR_MENU);
        for (id, key) in [(78usize, "EYE"), (64, "FAT")] {
            if h.options.len() <= id {
                h.options.resize(id + 1, CasOption::default());
            }
            h.options[id] = CasOption { kind: "morph".into(), morph: key.into(), ..Default::default() };
        }
        let m = |t: &str, d: f32| CasMorph { target: t.into(), min: 0.0, max: 0.5, default: d };
        h.morphs = [("EYE".to_string(), m("local_eye_width", 0.25)), ("FAT".to_string(), m("fat", 0.0)), ("THIN".to_string(), m("thin", 0.0))].into();
        h.morph_settings = Some(MorphSettings { threshold: 0.1, step: 0.01 });
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {} });
        h.select(0);
        let value = |h: &mut CasHost, vm: &mut Vm, row: f64| h.call(vm, "Game", "CAC_GetIntegerValue", &[Value::Number(row)]).unwrap().unwrap();
        assert_eq!(value(&mut h, &mut vm, 0.0), Value::Number(61.0), "the default 0.25 of 0..0.5 = 0.5 x 122");
        let frame = 1.0 / FE_HZ;
        assert!(!h.tick_morph(0.05, frame), "inside the threshold");
        assert!(h.tick_morph(1.0, frame) && h.value_refresh);
        assert!((h.look["morphs"]["local_eye_width"].as_f64().unwrap() - 0.255).abs() < 1e-6, "one step = 0.01 of the range");
        for _ in 0..200 {
            h.tick_morph(1.0, frame);
        }
        assert_eq!(h.look["morphs"]["local_eye_width"].as_f64(), Some(0.5));
        assert!(!h.tick_morph(1.0, frame), "clamped at the end");
        assert_eq!(value(&mut h, &mut vm, 0.0), Value::Number(122.0));
        // The body row: shown 0.5 with fat = thin = 0; moving down makes it thin, up fat.
        assert_eq!(value(&mut h, &mut vm, 1.0), Value::Number(61.0));
        let m = h.current().unwrap();
        h.indices[m].0 = 1;
        for _ in 0..10 {
            h.tick_morph(-1.0, frame);
        }
        assert!((h.look["morphs"]["thin"].as_f64().unwrap() - 0.1).abs() < 1e-5, "0.4 shown = thin 0.2 of the range");
        assert_eq!(h.look["morphs"]["fat"].as_f64(), Some(0.0));
    }

    #[test]
    fn a_hair_color_pick_sets_the_hairline_by_its_text() {
        let mut h = grid_host();
        h.menus[HAIR_MENU] = menu("Hair", "", (0, 0), &[66]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(HAIR_MENU);
        let colour = |key: &str, text: &str| CasColour { key: key.into(), swatch: [0.0; 3], a: [0.5; 3], b: [0.0; 3], text: text.into() };
        h.colour_zones = [(HAIR_ZONE, CasColourZone { default: "A".into(), colours: vec![colour("A", "black"), colour("B", "blonde")] })].into();
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {} });
        h.select(0);
        assert!(h.left_right(true));
        assert_eq!(h.look["hairline"], "blonde");
        assert_eq!(h.look["colour_tints"]["1"], serde_json::json!([0.5f32, 0.5f32, 0.5f32]));
    }

    #[test]
    fn the_face_preset_row_steps_the_gender_s_presets_through_custom() {
        let mut h = grid_host();
        h.menus[HAIR_MENU] = menu("Facial Presets", "", (0, 0), &[95, 62]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(HAIR_MENU);
        let colour = |key: &str, text: &str| CasColour { key: key.into(), swatch: [0.0; 3], a: [0.5; 3], b: [0.0; 3], text: text.into() };
        h.colour_zones = [(SKIN_ZONE, CasColourZone { default: "A".into(), colours: vec![colour("A", "light"), colour("B", "dark")] })].into();
        h.morphs = [("E".to_owned(), CasMorph { target: "local_eye_width".into(), min: 0.0, max: 0.5, default: 0.25 }),
            ("F".to_owned(), CasMorph { target: "fat".into(), min: 0.0, max: 0.5, default: 0.0 })].into();
        let preset = |index: i32, male: bool, text: &str, skin: &str| CasFacePreset { index, male, text: text.into(),
            colours: vec![(SKIN_ZONE, skin.into())], morphs: vec![("E".into(), 0.8), ("F".into(), 1.0)] };
        h.face_presets = vec![preset(0, false, "ID_CAC_PRESET_CUSTOM", ""), preset(1, true, "ID_CAC_PRESET_01", "B"),
            preset(1, false, "ID_CAC_PRESET_01F", "B"), preset(2, true, "ID_CAC_PRESET_02", "A"), preset(3, false, "ID_CAC_PRESET_03F", "A")];
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {}, "morphs": { "local_eye_width": 0.1 } });
        h.look_copy = h.look.clone();
        h.select(0);
        let mut vm = Vm::new();
        let text = |h: &mut CasHost, vm: &mut Vm| h.call(vm, "Game", "CAC_GetStringValue", &[Value::Number(0.0)]).unwrap().unwrap();
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_PRESET_CUSTOM".into()), "unset reads as Custom");
        assert!(h.left_right(true));
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_PRESET_01".into()));
        assert!((h.look["morphs"]["local_eye_width"].as_f64().unwrap() - 0.4).abs() < 1e-6, "0.8 of the range");
        assert!(h.look["morphs"]["fat"].is_null(), "body rows are skipped");
        assert_eq!(h.look["colour_keys"]["2"], "B");
        assert_eq!(h.look["skin"], "dark");
        assert!(h.left_right(true));
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_PRESET_02".into()), "the female preset 3 is not counted");
        assert!(h.left_right(true));
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_PRESET_CUSTOM".into()), "past the last");
        assert_eq!(h.look, h.look_copy, "Custom puts back the entry look");
        assert!(h.left_right(false));
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_PRESET_02".into()), "Left from Custom: the last");
        h.indices[HAIR_MENU].0 = 1;
        assert!(h.left_right(true));
        h.indices[HAIR_MENU].0 = 0;
        assert_eq!(text(&mut h, &mut vm), Value::Text("ID_CAC_PRESET_CUSTOM".into()), "a skin pick drops to Custom");
    }

    #[test]
    fn undo_rows_put_back_the_look_from_entering_the_menu() {
        let mut h = grid_host();
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.menus[BODY_MODS] = menu("BodyMods", "", (0, 0), &[3, 4]);
        h.menus[HAIR_MENU] = menu("Eyes", "", (0, 0), &[76, 96]);
        h.menus[19] = menu("BodyShape", "", (0, 0), &[97]);
        h.options[0].target = Some(BODY_MODS);
        h.options[3].target = Some(HAIR_MENU);
        h.options[4].target = Some(19);
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {}, "morphs": { "a": 0.1 } });
        h.select(0);
        h.look["morphs"]["a"] = serde_json::json!(0.2);
        h.select(0);
        assert_eq!(h.look_copy["morphs"]["a"], 0.2, "every Body Mods menu entry copies the look");
        h.look["skin"] = serde_json::json!("dark");
        h.look["morphs"]["a"] = serde_json::json!(0.4);
        h.indices[HAIR_MENU].0 = 1;
        h.menu_refresh = false;
        assert_eq!(h.select(1), None);
        assert_eq!(h.look, h.look_copy, "96 puts back the whole look");
        assert!(h.look["skin"].is_null());
        assert!(h.menu_refresh, "the reload re-enters the menu, so every row redraws (825FAAF0 state 5)");
        assert!(h.back());
        h.look["skin"] = serde_json::json!("dark");
        h.look["morphs"]["a"] = serde_json::json!(0.3);
        h.indices[BODY_MODS].0 = 1;
        h.select(1);
        h.look["morphs"]["a"] = serde_json::json!(0.5);
        h.select(0);
        assert_eq!(h.look["morphs"]["a"], 0.3, "97 puts back the morphs");
        assert_eq!(h.look["skin"], "dark", "97 leaves the rest");
    }

    #[test]
    fn the_wear_style_color_row_follows_the_part_and_a_board_pick_swaps_the_deck() {
        let mut h = grid_host();
        let colour = |key: &str, text: &str| CasColour { key: key.into(), swatch: [0.0; 3], a: [1.0; 3], b: [0.0; 3], text: text.into() };
        let zone = |c: Vec<CasColour>| CasColourZone { default: c[0].key.clone(), colours: c };
        h.colour_zones = [
            (BOARD_ZONE, zone(vec![colour("R", "red"), colour("B", "blue")])),
            (0, zone(vec![colour("RC", ""), colour("BC", "")])),
        ].into();
        let deck = |m: &str| CasPart { slot: BOARD_SLOT.into(), model_id: "deck#".into(), model: "deck".into(), material_id: m.into(), material: m.into(), ..Default::default() };
        let base = h.parts.len();
        h.parts.extend([deck("red_clean"), deck("red_worn"), deck("blue_clean"), deck("blue_worn")]);
        h.decks = [("deck#".to_string(), vec![("red_clean".into(), "red".into()), ("red_worn".into(), "red".into()),
            ("blue_clean".into(), "blue".into()), ("blue_worn".into(), "blue".into())])].into();
        h.colourizable = ["red_clean", "red_worn", "blue_clean", "blue_worn"].map(String::from).into();
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": { "SkateBoard": { "asset_id": "deck#", "material_id": "red_worn" } } });
        h.wear_styles = (base..base + 4).collect();
        h.stack.push(WEAR_STYLES);
        h.enter(WEAR_STYLES);
        assert_eq!(h.colour_zone_of(WEAR_STYLE_COLOUR), Some(BOARD_ZONE));
        h.indices[WEAR_STYLES] = (0, 0);
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"][BOARD_SLOT]["material_id"], "blue_worn", "same kind (worn), new color");
        assert!(h.look["colour_tints"][BOARD_ZONE.to_string()].is_null(), "the deck swaps, no tint");
        // A hat that is not colorizable has no color list; a colorizable one has the hat zone.
        let hat = h.parts.iter().position(|p| p.slot == "Hat").unwrap();
        h.wear_styles = vec![hat];
        assert_eq!(h.colour_zone_of(WEAR_STYLE_COLOUR), None);
        h.colourizable.insert(h.parts[hat].material_id.clone());
        assert_eq!(h.colour_zone_of(WEAR_STYLE_COLOUR), Some(0));
    }

    #[test]
    fn with_the_base_outfit_opt_in_remove_leaves_the_sports_bra_and_shorts() {
        let mut h = grid_host();
        h.male = false;
        h.bare_torsos = [(false, ("torso_f".to_string(), vec![("bra_light".to_string(), "light".to_string()), ("bra_dark".to_string(), "dark".to_string())]))].into();
        h.tattoo_shorts = [(false, ("shorts_f".to_string(), "shorts_mat".to_string()))].into();
        h.look = serde_json::json!({ "gender": "female", "skin": "dark", "selections": {
            "OuterTorso": { "asset_id": "tee", "material_id": "white" }, "Pants": { "asset_id": "jeans", "material_id": "blue" } } });
        let remove = |h: &mut CasHost, slot: &str| {
            h.stack = vec![TOP_MENU, MERCHANDISE, PART_GRID];
            h.grid = vec![None];
            h.grid_slot = Some(slot.to_string());
            h.pick(0);
        };
        h.base_outfit = true;
        remove(&mut h, "OuterTorso");
        assert_eq!(h.look["selections"]["OuterTorso"], serde_json::json!({ "asset_id": "torso_f", "material_id": "bra_dark" }), "the bra in the skin tone");
        remove(&mut h, "Pants");
        assert_eq!(h.look["selections"]["Pants"], serde_json::json!({ "asset_id": "shorts_f", "material_id": "shorts_mat" }));
        h.base_outfit = false;
        remove(&mut h, "Pants");
        assert!(h.look["selections"]["Pants"].is_null(), "off: Remove clears the slot as before");
    }

    #[test]
    fn custom_graphics_go_rows_category_logo_editor_and_remove() {
        let mut h = grid_host();
        while h.menus.len() <= HAT_GRAPHIC_ADJUST {
            h.menus.push(menu("", "", (0, 0), &[]));
        }
        if h.options.len() <= 114 {
            h.options.resize(115, CasOption::default());
        }
        let deck = |m: &str| CasPart { slot: BOARD_SLOT.into(), model_id: "deck#".into(), model: "deck".into(), material_id: m.into(), material: m.into(), ..Default::default() };
        let base = h.parts.len();
        h.parts.extend([deck("plain_clean"), deck("plain_worn")]);
        h.colourizable = ["plain_clean", "plain_worn"].map(String::from).into();
        h.logoable = ["plain_clean", "plain_worn"].map(String::from).into();
        h.graphic_logos = vec![("brand_a".into(), false), ("team_a".into(), true), ("brand_b".into(), false)];
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": { "SkateBoard": { "asset_id": "deck#", "material_id": "plain_clean" } } });
        h.wear_styles = vec![base, base + 1];
        h.stack.push(MERCHANDISE);
        h.stack.push(WEAR_STYLES);
        h.enter(WEAR_STYLES);
        assert_eq!(h.menus[WEAR_STYLES].options, [WEAR_STYLE_COLOUR, ADD_GRAPHIC, REMOVE_GRAPHIC], "a board: no style row, Add Graphic 103");
        h.indices[WEAR_STYLES] = (1, 0);
        assert_eq!(h.select(1), Some(GRAPHIC_CATEGORIES));
        h.menus[GRAPHIC_CATEGORIES] = menu("GraphicCategories", "", (0, 0), &[BRAND_GRAPHICS_ROW, TEAM_GRAPHICS_ROW]);
        assert_eq!(h.select(0), Some(BRAND_GRAPHICS));
        assert_eq!(h.graphic_list, ["brand_a", "brand_b"], "brand logos in database order");
        let mut vm = Vm::new();
        h.call(&mut vm, "Game", "CAC_OnThumbnailSelect", &[Value::Number(1.0)]).unwrap();
        assert_eq!(h.menu(), Some(BOARD_GRAPHIC_ADJUST));
        assert_eq!(h.look["graphics"][BOARD_SLOT], serde_json::json!({ "id": "brand_b", "region": 0, "placement": [1.5712f32, 7.0f32, 0.25f32, 0.25f32] }), "row 2's default");
        h.look["graphics"][BOARD_SLOT]["placement"] = serde_json::json!([0.0, 9.0, 0.0, 0.0]);
        assert!(h.tattoo_editor_key(4), "reset");
        assert_eq!(h.look["graphics"][BOARD_SLOT]["placement"], serde_json::json!([1.5712f32, 7.0f32, 0.25f32, 0.25f32]));
        assert!(h.tattoo_editor_key(5), "no clothing key, taken");
        assert!(h.tattoo_editor_key(2));
        assert_eq!(h.menu(), Some(MERCHANDISE), "A pops to menu 1");
        // Remove (104) on menu 30 clears only the board's graphic.
        h.look["graphics"]["Hat"] = serde_json::json!({ "id": "brand_a", "region": 0 });
        h.stack.push(WEAR_STYLES);
        h.enter(WEAR_STYLES);
        h.select(2);
        assert!(h.look["graphics"][BOARD_SLOT].is_null() && h.look["graphics"]["Hat"].is_object());
        // A shirt: Add Graphic 100 goes to the regions; the right arm keeps region 3 and its default row 9.
        let tee = CasPart { slot: OUTER_TORSO.into(), model_id: "tee#".into(), model: "tee".into(), material_id: "tee_white".into(), material: "tee_white".into(), ..Default::default() };
        h.parts.push(tee);
        h.logoable.insert("tee_white".into());
        h.look["selections"][OUTER_TORSO] = serde_json::json!({ "asset_id": "tee#", "material_id": "tee_white" });
        h.wear_styles = vec![h.parts.len() - 1];
        h.enter(WEAR_STYLES);
        assert_eq!(h.menus[WEAR_STYLES].options, [ADD_SHIRT_GRAPHIC, REMOVE_GRAPHIC]);
        assert_eq!(h.select(0), Some(SHIRT_GRAPHIC_LOCATION));
        h.menus[SHIRT_GRAPHIC_LOCATION] = menu("ShirtGraphicLocation", "", (0, 0), &[111, 112, 113, 114]);
        assert_eq!(h.select(3), Some(GRAPHIC_CATEGORIES));
        assert_eq!(h.graphic, Some((OUTER_TORSO.into(), 3)));
        assert_eq!(h.camera, SHIRT_GRAPHIC_CAMERAS[3]);
        assert_eq!(h.select(1), Some(TEAM_GRAPHICS));
        h.call(&mut vm, "Game", "CAC_OnThumbnailSelect", &[Value::Number(0.0)]).unwrap();
        assert_eq!(h.menu(), Some(SHIRT_GRAPHIC_ADJUST));
        assert_eq!(h.look["graphics"][OUTER_TORSO]["placement"], serde_json::json!(GRAPHIC_DEFAULTS[9][0]));
    }

    #[test]
    fn a_skin_color_pick_sets_the_skin_tone_by_its_text_and_tints() {
        let mut h = grid_host();
        h.menus[HAIR_MENU] = menu("Skin", "", (0, 0), &[62]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        h.options[0].target = Some(HAIR_MENU);
        let colour = |key: &str, text: &str, a: f32| CasColour { key: key.into(), swatch: [0.0; 3], a: [a; 3], b: [0.0; 3], text: text.into() };
        h.colour_zones = [(SKIN_ZONE, CasColourZone { default: "A".into(), colours: vec![colour("A", "light", 0.8), colour("B", "dark", 0.56)] })].into();
        h.begin();
        h.look = serde_json::json!({ "gender": "male", "selections": {} });
        h.select(0);
        assert!(h.left_right(true));
        assert_eq!(h.look["skin"], "dark");
        assert_eq!(h.look["colour_tints"]["2"], serde_json::json!([0.56f32, 0.56f32, 0.56f32]));
        assert!(h.left_right(false));
        assert_eq!(h.look["skin"], "light");
    }

    #[test]
    fn a_tattoo_goes_region_category_design_and_none_removes_it() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        while h.menus.len() <= BODY_MODS {
            h.menus.push(menu("", "", (0, 0), &[]));
        }
        h.menus[BODY_MODS] = menu("BackAlleyPete", "", (0, 0), &[19]);
        h.menus[TATTOO_UPPER] = menu("TattooUpperRegions", "", (0, 0), &[27, 28, 29, 30]);
        h.menus[TATTOO_CATEGORIES] = menu("TattooCategories", "", (0, 0), &[]);
        h.menus[TATTOO_DESIGNS] = menu("TattooThumbnails", "", (0, 0), &[]);
        h.menus[TOP_MENU] = menu("EditSkaterMain", "", (0, 0), &[0]);
        if h.options.len() <= 32 {
            h.options.resize(33, CasOption::default());
        }
        h.options[0].target = Some(BODY_MODS);
        h.options[19].target = Some(TATTOO_UPPER);
        h.options[21].text = "ID_CAC_TATTOO_CATEGORY_01".into();
        h.tattoo_categories = vec!["1".into(), "2".into()];
        h.tattoo_names = [("c".to_string(), "Viper".to_string())].into();
        h.tattoos = vec![("a".into(), "2".into()), ("b".into(), "1".into()), ("c".into(), "2".into())];
        h.begin();
        let shirt = serde_json::json!({ "asset_id": "tee", "material_id": "white" });
        h.look = serde_json::json!({ "gender": "male", "skin": "dark", "selections": { "OuterTorso": shirt } });
        h.bare_torsos = [(true, ("torso".to_string(), vec![("skin_light".to_string(), "light".to_string()), ("skin_dark".to_string(), "dark".to_string())]))].into();
        let call = |h: &mut CasHost, vm: &mut Vm, m: &str, row: f64| h.call(vm, "Game", m, &[Value::Number(row)]).unwrap().unwrap();
        h.select(0);
        h.select(0);
        assert_eq!(h.select(3), Some(TATTOO_CATEGORIES), "right arm");
        let torso = serde_json::json!({ "asset_id": "torso", "material_id": "skin_dark" });
        assert_eq!(h.look["selections"]["OuterTorso"], torso, "the shirt comes off: the skin torso in the look's tone");
        assert_eq!(h.tattoo_region, Some((true, 3)));
        assert_eq!((h.camera.as_str(), h.body.as_str()), ("387F458990473B6B", "862562BC11BB028C"), "the right arm's targets");
        assert_eq!(call(&mut h, &mut vm, "CAC_GetNumItems", 0.0), Value::Number(2.0));
        assert_eq!(call(&mut h, &mut vm, "CAC_GetItemText", 0.0), Value::Text("ID_CAC_TATTOO_CATEGORY_01".into()));
        assert_eq!(h.menus[TATTOO_CATEGORIES].options, vec![21, 22], "rows filled on enter");
        assert_eq!(h.select(1), Some(TATTOO_DESIGNS), "category 2");
        assert_eq!(h.tattoo_list, vec![Some("a".into()), Some("c".into())], "the category's designs; no remove entry without a tattoo");
        assert_eq!(call(&mut h, &mut vm, "CAC_GetDescriptionText", 1.0), Value::Text("Viper".into()));
        assert_eq!(call(&mut h, &mut vm, "CAC_GetOptionType", 1.0), Value::Text("none".into()), "no check mark");
        h.tattoo_thumbnails = [("c".to_string(), "fe\\cas\\logos\\logo_object_tattoo_viper_mat".to_string())].into();
        assert_eq!(call(&mut h, &mut vm, "CAC_GetThumbnailFileName", 1.0), Value::Text("fe\\cas\\logos\\logo_object_tattoo_viper_mat".into()));
        call(&mut h, &mut vm, "CAC_OnThumbnailSelect", 1.0);
        // The right arm's default placement (row 10 of 0x830442B8), then the placement editor.
        assert_eq!(h.look["tattoos"]["Arm"], serde_json::json!({ "id": "c", "side": 1, "region": 3, "placement": [0.0f32, 7.0f32, -0.25f32, -0.15f32] }));
        assert_eq!(h.menu(), Some(TATTOO_UPPER_ADJUST));
        h.look["tattoos"]["Arm"]["placement"] = serde_json::json!([0.5, 9.0, 0.0, 0.0]);
        assert!(h.tattoo_editor_key(4), "reset");
        assert_eq!(h.look["tattoos"]["Arm"]["placement"], serde_json::json!([0.0f32, 7.0f32, -0.25f32, -0.15f32]));
        assert!(h.tattoo_editor_key(5), "X: shirt back on");
        assert_eq!(h.look["selections"]["OuterTorso"], shirt);
        assert!(h.tattoo_editor_key(5), "X: off again");
        assert_eq!(h.look["selections"]["OuterTorso"], torso);
        assert!(h.tattoo_editor_key(2), "A");
        assert_eq!(h.menu(), Some(BODY_MODS));
        assert_eq!(h.look["selections"]["OuterTorso"], shirt, "A puts it back");
        h.select(0);
        assert_eq!(h.select(3), None, "the location holds a tattoo: the replace warning first");
        assert_eq!(h.popups.pop().map(|p| p.description), Some("ID_CAC_UPPER_TATTOO_CHANGE_CONFIRMATION".into()));
        h.tattoo_replace_answer(1);
        assert_eq!(h.menu(), Some(TATTOO_UPPER), "No stays on the regions");
        h.select(3);
        h.tattoo_replace_answer(0);
        assert_eq!(h.menu(), Some(TATTOO_CATEGORIES), "Yes picks the region");
        h.select(1);
        assert_eq!(h.tattoo_list, vec![None, Some("a".into()), Some("c".into())], "with a tattoo the remove entry comes first");
        assert_eq!(call(&mut h, &mut vm, "CAC_GetThumbnailFileName", 0.0), Value::Text("fe\\cas\\general\\remove".into()));
        assert_eq!(call(&mut h, &mut vm, "CAC_GetOptionType", 2.0), Value::Text("owned".into()), "the worn design's check mark");
        call(&mut h, &mut vm, "CAC_OnThumbnailSelect", 0.0);
        assert!(h.look["tattoos"]["Arm"].is_null(), "none removes it");
    }

    #[test]
    fn the_placement_matrix_and_the_fit_follow_retail() {
        // rot 0, k 5, (0.25, 0.25): the graphic covers body x 0.15..0.35, y 0.20..0.30 (b1043's example).
        let (rx, ry) = crate::customiser_material::placement_rows([0.0, 5.0, 0.25, 0.25]);
        let g = |x: f32, y: f32| (rx.x * x + rx.y * y + rx.z, ry.x * x + ry.y * y + ry.z);
        let close = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5;
        assert!(close(g(0.15, 0.20), (0.0, 0.0)) && close(g(0.35, 0.30), (1.0, 1.0)));
        assert!(close((graphic_to_body([0.3, 7.0, 0.1, -0.2], 0.5, 0.5)[0], graphic_to_body([0.3, 7.0, 0.1, -0.2], 0.5, 0.5)[1]), (0.4, 0.7)),
            "the graphic center sits at (0.5 - u, 0.5 - v)");
        let rect = [0.0, 0.5, 0.0, 0.5];
        let bounds = [1.0, 0.0, 0.0, 1.0];
        let start = [0.0, 5.0, 0.25, 0.25];
        assert!(graphic_fits(start, rect, bounds));
        // Moving u far left (the graphic right) pushes it back inside, just past the edge.
        let moved = adjust_graphic(start, 2, -0.3, rect, bounds);
        assert!(graphic_fits(moved, rect, bounds) && moved[2] > -0.05 && moved[2] < 0.25, "{moved:?}");
        // Growing the graphic past the rect: the scale is raised again until it fits; the cap holds at 15.
        let grown = adjust_graphic(start, 1, -4.5, rect, bounds);
        assert!(graphic_fits(grown, rect, bounds) && grown[1] >= 1.0 && grown[1] < 5.0, "{grown:?}");
        assert_eq!(adjust_graphic(start, 1, 20.0, rect, bounds)[1], 15.0);
        // The constraint flag flips v; missing gives the quadrant.
        assert_eq!(constraint_rect(|_| "0.9,0.1,0.2,0.4".into(), 7), [0.2, 0.4, 0.100000024, 0.9]);
        assert_eq!(constraint_rect(|_| String::new(), 10), [0.5, 1.0, 0.5, 1.0]);
    }

    #[test]
    fn every_picker_row_is_enabled() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.select(0);
        assert_eq!(h.call(&mut vm, "Game", "OnMenuGetItemEnabled", &[Value::Number(0.0), Value::Number(9.0)]).unwrap(), Some(Value::Bool(true)));
    }

    #[test]
    fn an_options_body_target_poses_the_skater_and_merchandise_puts_the_whole_body_back() {
        let mut h = grid_host();
        let body = |yaw: f32| CasBody { clip: "CAS_Stand".into(), yaw_degrees: yaw, position: [-1.0, 0.0, -7.05], once: false, post: 0 };
        h.bodies = [(WHOLE_BODY.to_string(), body(260.0)), ("91098B2B2461D2DC".to_string(), body(149.25))].into_iter().collect();
        h.options[2].body = "91098B2B2461D2DC".into();
        h.select(0);
        assert_eq!(h.pose().unwrap().yaw_degrees, 260.0);
        h.select(1);
        assert_eq!(h.pose().unwrap().yaw_degrees, 149.25);
        h.back();
        assert_eq!(h.pose().unwrap().yaw_degrees, 260.0);
        // Without the row the whole-body one stands in.
        h.body = "0000000000000000".into();
        assert_eq!(h.pose().unwrap().yaw_degrees, 260.0);
    }

    #[test]
    fn a_stand_row_plays_the_postures_stand_and_a_gesture_row_its_own_clip() {
        let mut h = grid_host();
        let body = |clip: &str, once: bool| CasBody { clip: clip.into(), yaw_degrees: 260.0, position: [0.0; 3], once, post: 0 };
        h.bodies = [(WHOLE_BODY.to_string(), body("CAS_Stand", false)), ("A".to_string(), body("cac_male_spock", true))].into_iter().collect();
        h.posture_stands = [(-1, "CAS_Stand".to_string()), (0, "CAS_Stand_Stiff".to_string()), (2, "CAS_Stand_Buff".to_string())].into_iter().collect();
        h.body = WHOLE_BODY.into();
        assert_eq!(h.pose().unwrap().clip, "CAS_Stand", "posture 0 (Default) = index -1");
        h.look["posture"] = 3.into();
        assert_eq!(h.pose().unwrap().clip, "CAS_Stand_Buff");
        h.look["posture"] = 2.into();
        assert_eq!(h.pose().unwrap().clip, "CAS_Stand", "no row for the index: the body row's own clip");
        h.body = "A".into();
        assert_eq!(h.pose().unwrap().clip, "cac_male_spock");
    }

    #[test]
    fn the_triggers_turn_the_skater_frame_rate_independent_and_a_new_camera_resets_it() {
        let mut h = grid_host();
        // A full trigger for one second: 0.1 rad per 30 Hz frame, the same at 30, 60 and 240 fps.
        for fps in [30.0f32, 60.0, 240.0] {
            h.turntable = 0.0;
            for _ in 0..fps as usize {
                h.rotate([0.0, 1.0], 1.0 / fps);
            }
            assert!((h.turntable - 3.0).abs() < 1e-3, "{fps} fps: {}", h.turntable);
        }
        // The other trigger turns back; under the dead zone nothing moves.
        h.rotate([0.5, 0.0], 1.0 / 30.0);
        assert!((h.turntable - 2.95).abs() < 1e-4);
        h.rotate([0.01, 0.019], 1.0);
        assert!((h.turntable - 2.95).abs() < 1e-4);
        // Entering a category posts its camera: back to 0.
        open_grid(&mut h, 0);
        assert_eq!(h.turntable, 0.0);
    }

    #[test]
    fn a_rested_highlight_previews_its_part_and_leaving_the_grid_takes_it_off() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        open_grid(&mut h, 0);
        let mut hilight = |h: &mut CasHost, row: f64| h.call(&mut vm, "Game", "OnMenuHilight", &[Value::Number(0.0), Value::Number(row), Value::Text(String::new())]).unwrap();
        let red = h.look.clone();
        // The blue cap: nothing before the highlight rested 500 ms, then the skater wears it; the look stays.
        hilight(&mut h, 2.0);
        h.tick(0.3);
        assert!(h.preview.is_none());
        h.tick(0.3);
        assert_eq!(h.preview.as_ref().unwrap()["selections"]["Hat"], serde_json::json!({ "asset_id": "cap#", "material_id": "blue#" }));
        assert!(h.preview_changed);
        assert_eq!(h.look, red);
        // Moving on restarts the wait; the worn red cap shows the look again.
        h.preview_changed = false;
        hilight(&mut h, 1.0);
        h.tick(0.4);
        assert!(h.preview.is_some());
        h.tick(0.2);
        assert!(h.preview.is_none() && h.preview_changed);
        // B off the grid takes a preview off.
        hilight(&mut h, 2.0);
        h.tick(0.6);
        assert!(h.preview.is_some());
        h.back();
        assert!(h.preview.is_none() && h.preview_wait.is_none());
        // A pick commits it.
        h.select(0);
        hilight(&mut h, 2.0);
        h.tick(0.6);
        h.call(&mut vm, "Game", "CAC_OnThumbnailSelect", &[Value::Number(2.0)]).unwrap();
        assert!(h.preview.is_none());
        assert_eq!(h.look["selections"]["Hat"], serde_json::json!({ "asset_id": "cap#", "material_id": "blue#" }));
    }

    #[test]
    fn a_part_with_wear_styles_opens_menu_30_and_left_right_cycles_them() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        open_grid(&mut h, 0);
        // The blue cap and its backward style.
        h.call(&mut vm, "Game", "CAC_OnThumbnailSelect", &[Value::Number(2.0)]).unwrap();
        assert_eq!(h.menu(), Some(WEAR_STYLES));
        assert_eq!(h.wear_styles, vec![3, 2]);
        assert_eq!(h.look["selections"]["Hat"]["asset_id"], "cap#");
        assert_eq!(h.look["selections"]["Hat"]["material_id"], "blue#");
        assert_eq!(h.menus[WEAR_STYLES].options, [WEAR_STYLE_OPTION, REMOVE_GRAPHIC], "a hat that is not colorizable: Style, then Remove Graphic (82600AF8)");
        h.indices[WEAR_STYLES] = (0, 0);
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"]["Hat"]["asset_id"], "cap_4_backward#");
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"]["Hat"]["asset_id"], "cap#");
        // A accepts: back to Merchandise.
        h.select(0);
        assert_eq!(h.menu(), Some(MERCHANDISE));
    }

    #[test]
    fn a_category_posts_its_camera_and_merchandise_the_whole_body_one() {
        let mut h = grid_host();
        let cam = |fov: f32| CasCamera { eye: [0.0; 3], target: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], fov_degrees: fov };
        h.cameras = [(WHOLE_BODY.to_string(), cam(35.0)), ("256799DCA03F926D".to_string(), cam(45.0))].into_iter().collect();
        h.options[1].camera = "256799DCA03F926D".into();
        h.options[1].text = "ID_CAC_HATS_TITLE".into();
        h.begin();
        assert_eq!(h.view().map(|c| c.fov_degrees), Some(35.0));
        open_grid(&mut h, 0);
        assert_eq!(h.view().map(|c| c.fov_degrees), Some(45.0));
        // The grid is titled by its category.
        let mut vm = Vm::new();
        assert_eq!(h.call(&mut vm, "Game", "CAC_GetTitleText", &[]).unwrap(), Some(Value::Text("ID_CAC_HATS_TITLE".into())));
        h.back();
        assert_eq!(h.view().map(|c| c.fov_degrees), Some(35.0));
        // The vault names the hat row "hat": its key is the hats option's +0x20 hash.
        assert_eq!(skate_data::attrib_hash::numeric_name("hat"), "Hash_256799DCA03F926D");
    }

    /// The stock export's rows (skips without SKATE3_ASSET_ROOT): the whole-body and hat cameras, the dc logo.
    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_vault_gives_the_camera_rows_and_logos() {
        let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT") else { return };
        let (cameras, logos, bodies, zones, (morphs, morph_settings), tattoo_categories, presets, stands) = load_vault(std::path::Path::new(&root));
        assert_eq!(tattoo_categories, ["1", "2", "3"], "the vault's category list, its records' text fields");
        // Face presets: Custom (0) and ten per gender, each with an eye and a skin color and the 19 morphs.
        assert_eq!(presets.len(), 21);
        assert_eq!((presets[0].index, presets[0].text.as_str()), (0, "ID_CAC_PRESET_CUSTOM"));
        assert_eq!(presets.iter().filter(|p| p.male).map(|p| p.index).max(), Some(10));
        let first = presets.iter().find(|p| p.male && p.index == 1).unwrap();
        assert_eq!(first.colours.iter().map(|c| c.0).collect::<Vec<_>>(), [6, 2]);
        assert!(first.colours.iter().all(|(zone, key)| zones[zone].colours.iter().any(|c| &c.key == key)));
        assert_eq!(first.morphs.len(), 19);
        assert!(first.morphs.iter().all(|(key, n)| morphs.contains_key(key) && (0.0..=1.0).contains(n)));
        // The 19 morphs by key hash (b994 / b1041): eye width 0..0.5, default 0.25; the row settings 0.1 / 0.01.
        assert_eq!(morphs.len(), 19);
        assert_eq!(morphs.get("2219FCFA4A03AEE0"), Some(&CasMorph { target: "local_eye_width".into(), min: 0.0, max: 0.5, default: 0.25 }));
        assert_eq!(morph_settings, Some(MorphSettings { threshold: 0.1, step: 0.01 }));
        // Color zones by slot: hair 10, skin 7, eye 5, hat 10 (b1038), the hair default key from the zone row.
        assert_eq!([1u32, 2, 6, 0].map(|s| zones.get(&s).map_or(0, |z| z.colours.len())), [10, 7, 5, 10]);
        assert_eq!(zones[&1].default, "0D86C9E2EBF8EB59");
        assert_eq!(zones[&6].colours.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["blue", "dark blue", "green", "light brown", "dark brown"]);
        // The body rows: 69, the whole body at 260 degrees, the wrist row (keyed "wrist") at 149.25.
        assert_eq!(bodies.len(), 69);
        assert_eq!(bodies[WHOLE_BODY], CasBody { clip: "CAS_Stand".into(), yaw_degrees: 260.0, position: [-1.0, 0.0, -7.05], once: false, post: 0 });
        assert!((bodies["91098B2B2461D2DC"].yaw_degrees - 149.25).abs() < 0.01);
        assert_eq!(stands.get(&-1).map(String::as_str), Some("CAS_Stand"));
        assert_eq!(stands.get(&2).map(String::as_str), Some("CAS_Stand_Buff"));
        assert_eq!(cameras.len(), 47);
        let body = cameras[WHOLE_BODY];
        assert_eq!((body.fov_degrees, body.eye, body.target), (35.0, [-3.8, 0.88, -7.6], [49.0, 0.7, -9.0]));
        assert_eq!(cameras["256799DCA03F926D"].fov_degrees, 45.0);
        assert_eq!(logos["dc"], "fe\\source\\images\\sponsor_logos\\dc");
    }

    #[test]
    fn closed_or_other_objects_are_not_answered() {
        let mut h = host();
        let mut vm = Vm::new();
        assert!(h.call(&mut vm, "TeamManagement", "CAC_GetNumItems", &[]).unwrap().is_none());
        h.open = false;
        assert!(h.call(&mut vm, "Game", "CAC_GetNumItems", &[]).unwrap().is_none());
    }
}
