//! Create-a-Skater (FE 15, template CreateASkater, movie cas/createskater): the `Game.CAC_*` natives over the menu
//! table the setup extracts (`menu-tables.json` cas_menus; research b1030). Details: docs/hails-additions/43-career.md#create-a-skater
use crate::apt_vm::{Value, Vm};
use skate_data::menu_tables::CasMenu;

/// The top menu (EditSkaterMain).
pub(crate) const TOP_MENU: usize = 15;

/// Who wants FE 15 open (the boot flow's Customize row); `done` once it closed.
#[derive(bevy::prelude::Resource, Default, Debug)]
pub(crate) struct CasScreen {
    pub open: bool,
    pub done: bool,
}

/// The exit confirmation's callback (82603290 -> 82604748).
pub(crate) const EXIT_POPUP: &str = "engine:cas.exit";

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

/// NOT RETAIL YET: the retail screen opens only with `SKATE_RETAIL_CAS=1` until its navigation (menu push / pop, B,
/// the exit popup; research b1031) is ported; otherwise the boot flow keeps our customizer.
pub(crate) fn retail_enabled() -> bool {
    std::env::var("SKATE_RETAIL_CAS").is_ok_and(|v| v == "1")
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
}

const BODY_CLASS: &str = "Hash_95E142284549717F";
const BODY_CLIP: &str = "Hash_249D0D4624998D68";
const BODY_YAW: &str = "Hash_4591287983616615";
const BODY_POSITION: &str = "Hash_6D2BEA1EFEA33425";

/// The vault data FE 15 reads: every CAS camera row by key (16 hex digits), the brand logo image by brand name and
/// every body target row by key, inheritance resolved; empty without the vault export.
pub(crate) fn load_vault(asset_root: &std::path::Path) -> (std::collections::HashMap<String, CasCamera>,
    std::collections::HashMap<String, String>, std::collections::HashMap<String, CasBody>) {
    let Ok(data) = skate_data::collections::Collections::load(asset_root) else { return Default::default() };
    let bodies = data.entries().iter()
        .filter(|c| skate_data::attrib_hash::numeric_name(&c.class_name) == BODY_CLASS)
        .filter_map(|c| {
            let position = data.words::<4>(BODY_CLASS, &c.key, BODY_POSITION).ok().map(|w| [w[0], w[1], w[2]].map(f32::from_bits))?;
            let body = CasBody {
                clip: data.field(BODY_CLASS, &c.key, BODY_CLIP).ok()?.data.clone(),
                yaw_degrees: data.float(BODY_CLASS, &c.key, BODY_YAW).ok()?,
                position,
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
    (load_cameras(&data), by_brand, bodies)
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
    if host.open && host.look.is_null() {
        host.look = customiser.draft.clone();
        host.male = host.look["gender"].as_str() != Some("female");
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
const TURNTABLE_DEADZONE: f32 = 0.02;
const TURNTABLE_RATE: f32 = 0.1;
const FE_HZ: f32 = 30.0;

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
    pub body: String,
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

    /// cMsgPresCASCameraTarget; retail always follows it with CharacterRotate(0, absolute): the turntable resets.
    fn post_camera(&mut self, key: String) {
        self.camera = key;
        self.turntable = 0.0;
    }

    /// The CAS OnMenuHilight (vt+0x44 = 825FEDB8) on the part grid: the row is kept and the preview waits again.
    fn hilight(&mut self, row: usize) {
        if self.current() == Some(PART_GRID) && row < self.grid.len() {
            self.preview_row = row;
            self.preview_wait = Some(0.0);
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

    /// The part's info / option type (825FF5E8): "owned" when it or one of its wear styles is the slot's worn part
    /// (the check mark), "locked", "dlc", else "none". NOT RETAIL YET: no part is locked (the unlock manager's
    /// kind 5 / 6 names are not tracked).
    fn part_type(&self, i: usize) -> &'static str {
        if self.wears(i) || self.variants(i).into_iter().any(|v| self.wears(v)) {
            "owned"
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
        let Some(i) = entry else {
            let slot = self.grid_slot.clone();
            self.pop_to(MERCHANDISE);
            if let Some(slot) = slot {
                bevy::log::info!("RETAIL_MENUS CreateASkater remove {slot}");
                self.set_slot(&slot, None);
            }
            return;
        };
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
    pub(crate) fn pose(&self) -> Option<CasBody> {
        self.bodies.get(&self.body).or_else(|| self.bodies.get(WHOLE_BODY)).cloned()
    }

    /// Entering a menu (82600AF8): the shop mode it sets; the top and Merchandise menus post the whole-body camera.
    /// NOT RETAIL YET: the other menus' targets (tables 83044528 / 83044548: the head, face and tattoo rows).
    fn enter(&mut self, menu: usize) {
        if menu != PART_GRID {
            self.end_preview();
        }
        if matches!(menu, TOP_MENU | MERCHANDISE) {
            self.post_camera(WHOLE_BODY.into());
            self.body = WHOLE_BODY.into();
        }
        self.shop = match menu {
            TOP_MENU => 2,
            17 => 1,
            1 => 0,
            _ => self.shop,
        };
    }

    /// The highlighted row's option id.
    fn highlighted(&self) -> Option<i32> {
        let m = self.current()?;
        self.option(self.indices[m].0 as usize).map(|(id, _)| id)
    }

    /// Left / Right (825FC7E8, C++): the top menu's sliders move by 0.1 within 0..1. True when a value changed
    /// (the movie then refreshes it, SetScreenState state 50). NOT RETAIL YET: selectors, stance, part cycles,
    /// colors and morphs.
    pub(crate) fn left_right(&mut self, right: bool) -> bool {
        let step = if right { SLIDER_STEP } else { -SLIDER_STEP };
        if self.current() == Some(WEAR_STYLES) && self.highlighted() == Some(WEAR_STYLE_OPTION) {
            return self.cycle_wear_style(right);
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
    /// menu. NOT RETAIL YET: the menu / leaf branches of 825FE3C0 (menus 4, 5, 6, 31..33, options 56, 96..114) and
    /// the part list setup before menu 10.
    pub(crate) fn select(&mut self, row: usize) -> Option<usize> {
        // Menu 30's A is "Accept" (help kind 2): back to Merchandise with the worn style [inferred: the menu 30 branch
        // of 825FE3C0 is not read].
        if self.current() == Some(WEAR_STYLES) {
            self.wear_styles.clear();
            self.pop_to(MERCHANDISE);
            return self.current();
        }
        let (id, option) = self.option(row)?;
        let option = option?;
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
            bevy::log::info!("RETAIL_MENUS CreateASkater grid for option {id}: {} entries", self.grid.len());
        }
        self.stack.push(target);
        self.indices[target] = (0, 0);
        self.enter(target);
        Some(target)
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
            "CAC_GetIntegerValue" => match self.option(row).and_then(|(id, _)| self.slider(id)) {
                Some(v) => Value::Number((v * 10.0).round() as f64),
                None => Value::Number(0.0),
            },
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
        assert!(!h.left_right(false));
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
    fn every_picker_row_is_enabled() {
        let mut h = grid_host();
        let mut vm = Vm::new();
        h.select(0);
        assert_eq!(h.call(&mut vm, "Game", "OnMenuGetItemEnabled", &[Value::Number(0.0), Value::Number(9.0)]).unwrap(), Some(Value::Bool(true)));
    }

    #[test]
    fn an_options_body_target_poses_the_skater_and_merchandise_puts_the_whole_body_back() {
        let mut h = grid_host();
        let body = |yaw: f32| CasBody { clip: "CAS_Stand".into(), yaw_degrees: yaw, position: [-1.0, 0.0, -7.05] };
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
        h.indices[WEAR_STYLES] = (1, 0);
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"]["Hat"]["asset_id"], "cap_4_backward#");
        assert!(h.left_right(true));
        assert_eq!(h.look["selections"]["Hat"]["asset_id"], "cap#");
        // A accepts: back to Merchandise.
        h.select(1);
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
        let (cameras, logos, bodies) = load_vault(std::path::Path::new(&root));
        // The body rows: 69, the whole body at 260 degrees, the wrist row (keyed "wrist") at 149.25.
        assert_eq!(bodies.len(), 69);
        assert_eq!(bodies[WHOLE_BODY], CasBody { clip: "CAS_Stand".into(), yaw_degrees: 260.0, position: [-1.0, 0.0, -7.05] });
        assert!((bodies["91098B2B2461D2DC"].yaw_degrees - 149.25).abs() < 0.01);
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
