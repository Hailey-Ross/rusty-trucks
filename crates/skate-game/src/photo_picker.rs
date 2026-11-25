//! The photo challenges' shot picker in play (research b1060, b1061): FE screen 2 PhotoSelect's logic
//! (`skate_core::challenges::photo_select`) over the photo NIS the replay holds (`Replay::reel_still`). The pad goes
//! through the retail FE keys (input.cfg Apt*, the menu stage's repeat rule): LT / RT step the shots, A takes.
//! NOT RETAIL YET (logged): the `challenge/photoselect` movie, the edits on the picture, the sticks (rotate / zoom),
//! MagazinePromo and the career photo capture.

use bevy::prelude::*;
use skate_core::challenges::photo_select::{Action, EditTables, FinishMode, Out};
use skate_core::fe_input::{fired, AptKey, FeInput, RepeatRule};

#[derive(serde::Deserialize, Default)]
struct File {
    #[serde(default)]
    photo_edits: Option<EditsJson>,
}

#[derive(serde::Deserialize)]
struct EditsJson {
    brightness: Vec<f32>,
    contrast: Vec<f32>,
    saturation: Vec<f32>,
    depth_of_field: Vec<f32>,
}

/// The edit tables from the export's `photo_edits` (skatercollections 5CFA2765DFA23B43); one neutral entry each when
/// the export predates them (a Refresh Game Files adds them).
pub(crate) fn edit_tables(asset_root: &std::path::Path) -> EditTables {
    let file: File = std::fs::read(asset_root.join("private/challenges.json")).ok()
        .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    match file.photo_edits {
        Some(e) => EditTables { brightness: e.brightness, contrast: e.contrast, saturation: e.saturation, depth_of_field: e.depth_of_field },
        None => {
            warn!("SKATE_PHOTO the export has no photo_edits (Refresh Game Files): the edits stay neutral");
            EditTables { brightness: vec![0.0], contrast: vec![0.0], saturation: vec![0.0], depth_of_field: vec![0.0] }
        }
    }
}

/// The picker's movie (`challenge/photoselect`, template PhotoSelect) between the picker and the menu stage.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct PhotoScreen {
    pub open: bool,
    pub shots: u32,
    /// Mission.GetChallengePhotoLayout's answer.
    pub layout: i32,
    /// Movie states to send in order (the screen's vt+0x14: -1 / -2 arrows, -3 / -10 the take, else the shot index).
    pub states: Vec<i32>,
}

/// MagazinePromo (FE 69, template MagazinePromo, research b1067) between the picker and the menu stage: the taken photo
/// on its magazine page.
#[derive(Resource, Default, Clone, Debug)]
pub(crate) struct MagazineScreen {
    pub open: bool,
    /// The movie state: the layout kind (Mission_GetPhotoLayoutKind, + 1 for 3 / 5 on our widescreen display).
    pub state: i32,
    /// The photo (the held picture captured when the photo was taken); the page shows once it is here (retail waits
    /// for its photo render, 8264E778 states 0..2).
    pub image: Option<Handle<Image>>,
    /// The stage's answer (A Continue, Y Edit, B then Yes Retry).
    pub choice: Option<skate_core::challenges::photo_select::MagazineChoice>,
}

/// The asset name the movie loads the photo by (Game.GetPhotoMemcardAsset; ours: one name, the stage supplies the
/// texture under it).
pub(crate) const MAGAZINE_PHOTO_ASSET: &str = "magazinepromo_photo";

/// Callback name of MagazinePromo's retry confirmation.
pub(crate) const MAGAZINE_RETRY_POPUP: &str = "photo_magazine_retry";

/// MagazinePromo's B (8264EA88): retail popup 0xD9 (table 83028050: ID_COMMON_WARNING_TITLE /
/// ID_REMATCH_CONFIRM_MESSAGE, No / Yes); Yes (1) retries (8264F1B0).
pub(crate) fn magazine_retry_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_COMMON_WARNING_TITLE".into(),
        description: "ID_REMATCH_CONFIRM_MESSAGE".into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: MAGAZINE_RETRY_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

/// The magazine state for a layout kind (8264ECE8 adds 1 to 3 / 5 when the display flag is set: widescreen, inferred).
pub(crate) fn magazine_state(kind: Option<u8>) -> i32 {
    match kind {
        Some(k @ (3 | 5)) => i32::from(k) + 1,
        Some(k) => i32::from(k),
        None => 0,
    }
}

/// Mission.GetChallengePhotoLayout (825C0338 -> 826186D8): base 0 when the display flag `[83083C60]+0x12040` is set
/// (taken as widescreen: ours always draws wide [inferred]), else 3; plus 1 for a single-page magazine layout, 2 for a
/// two-page one; no layout: the base (the movie's kPhotoLayout_Widescreen .. StandardTwoPage).
pub(crate) fn layout_label(layout: Option<&str>, two_page: bool) -> i32 {
    match layout {
        None => 0,
        Some(_) if two_page => 2,
        Some(_) => 1,
    }
}

/// The picker movie's natives (the HUDComponents table 8265B870 and Mission.GetChallengePhotoLayout); None for
/// anything else.
pub(crate) fn natives(screen: &PhotoScreen, object: &str, method: &str) -> Option<crate::apt_vm::Value> {
    use crate::apt_vm::Value;
    if !screen.open {
        return None;
    }
    Some(match (object, method) {
        ("Game", "GetPhotoMemcardAsset") => Value::Text(MAGAZINE_PHOTO_ASSET.into()),
        ("Game", "GetOnlineTimeRemainingString") => Value::Text(String::new()),
        (_, "PhotoSelect_GetNumPhotos") => Value::Number(f64::from(screen.shots)),
        (_, "PhotoSelect_IsSelectionAvail") => Value::Bool(screen.shots > 1),
        ("Mission", "GetChallengePhotoLayout") => Value::Number(f64::from(screen.layout)),
        ("Game", "IsOnlineMode") => Value::Bool(false),
        _ => return None,
    })
}

/// What the host has to do after a batch of picker outputs.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Effects {
    pub finished: Option<FinishMode>,
    pub magazine: bool,
    pub store: bool,
}

/// The career photo store (research b952 port spec): `settings/profiles/local/career-photos/<slot>.png` beside the
/// install's assets, and `index.json` (slot -> file, challenge, time) updated in place. Slot 0 never stores (retail
/// 82618290). Returns the image path to capture into.
pub(crate) fn career_photo_path(asset_root: &std::path::Path, slot: u32, challenge: &str) -> Option<std::path::PathBuf> {
    if slot == 0 {
        return None;
    }
    let dir = asset_root.parent().unwrap_or(asset_root).join("settings/profiles/local/career-photos");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("SKATE_PHOTO career photos folder {}: {e}", dir.display());
        return None;
    }
    let file = format!("{slot}.png");
    let index_path = dir.join("index.json");
    let mut index: serde_json::Map<String, serde_json::Value> = std::fs::read(&index_path).ok()
        .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    index.insert(slot.to_string(), serde_json::json!({"file": file, "challenge": challenge, "time": time}));
    let part = dir.join("index.json.part");
    let written = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())
        .and_then(|b| std::fs::write(&part, b).map_err(|e| e.to_string()))
        .and_then(|_| std::fs::rename(&part, &index_path).map_err(|e| e.to_string()));
    if let Err(e) = written {
        warn!("SKATE_PHOTO career photo index {}: {e}", index_path.display());
    }
    Some(dir.join(file))
}

/// Carries out the picker's outputs on the held NIS (shots), the FE sounds and the logs.
pub(crate) fn carry_out(id: &str, outs: Vec<Out>, mut replay: Option<&mut crate::replay::Replay>,
    mut sounds: Option<&mut MessageWriter<crate::ui_audio::FrontendSound>>, mut screen: Option<&mut PhotoScreen>) -> Effects {
    let mut fx = Effects::default();
    for out in outs {
        match out {
            Out::Select(n) => {
                if !replay.as_deref_mut().is_some_and(|r| r.show_photo_shot(n)) {
                    warn!("SKATE_PHOTO {id} shot {n}: no held photo NIS to show it on");
                }
            }
            Out::Sound(key) => {
                if let Some(s) = sounds.as_deref_mut() {
                    s.write(crate::ui_audio::FrontendSound { key });
                }
            }
            Out::MovieState(s) => {
                debug!("SKATE_PHOTO {id} movie state {s}");
                if let Some(sc) = screen.as_deref_mut() {
                    sc.states.push(s);
                }
            }
            Out::Brightness(_) | Out::Contrast(_) | Out::Saturation(_) => info!("SKATE_PHOTO {id} edit {out:?}"),
            Out::DepthOfField(v) => info!("SKATE_PHOTO {id} depth of field {v} (NOT RETAIL YET: no NIS blur)"),
            Out::Reset => info!("SKATE_PHOTO {id} edits reset"),
            Out::OpenMagazinePromo => {
                info!("SKATE_PHOTO {id} MagazinePromo opens");
                fx.magazine = true;
            }
            Out::Store => fx.store = true,
            Out::Finish(mode) => fx.finished = Some(mode),
        }
    }
    fx
}

/// FE pad state for the picker (controller 0, the retail Apt keys).
#[derive(Default)]
pub(crate) struct PickerPad {
    keys: Option<Vec<AptKey>>,
    pad: FeInput,
    /// The ScenePhotoCustomizer's lens (degrees added to the take's field of view, research b1063) and 60 Hz updates.
    lens: f32,
    /// The customizer's avatar move (yaw, pitch degrees).
    avatar: [f32; 2],
    carry: f32,
    /// Seconds the picker has been open (the scripted pick's delay).
    open_for: f32,
    /// The picker finished; frames left before the held picture closes (a career photo capture in flight).
    finished: Option<FinishMode>,
    /// Seconds the magazine page has shown its photo (the scripted answer's delay).
    magazine_for: f32,
    finish_in: Option<u8>,
    values: [f32; 24],
}

/// The picker update's sticks (82617100): past the 0.2 dead zone (82099280 / 8208EA7C) the zoom axis posts
/// AdjustLens(axis x -0.2) every update, which the customizer adds up within +-10 degrees (research b1063). The zoom
/// axis is the right stick's up / down [inferred from the retail prompt "R Zoom camera"].
pub(crate) fn lens_step(lens: f32, axis: f32) -> f32 {
    if (-0.2..=0.2).contains(&axis) { lens } else { (lens + axis * -0.2).clamp(-10.0, 10.0) }
}

/// The avatar move per update (82617100): when either left stick axis is past the 0.2 dead zone, AvatarMove(x x -0.4,
/// y x -0.4) (82098E30), which the customizer adds up within +-30 degrees each (8282A250) [the left stick from the
/// retail prompt "L Rotate camera", inferred].
pub(crate) fn avatar_step(avatar: [f32; 2], [x, y]: [f32; 2]) -> [f32; 2] {
    if x.abs() <= 0.2 && y.abs() <= 0.2 {
        return avatar;
    }
    [(avatar[0] + x * -0.4).clamp(-30.0, 30.0), (avatar[1] + y * -0.4).clamp(-30.0, 30.0)]
}

/// The FE actions fired this frame on controller 0 (the keyboard's Enter takes, Left / Right step).
fn actions(pad: &mut PickerPad, asset_root: &std::path::Path, keys: &ButtonInput<KeyCode>, seconds: f32) -> Vec<Action> {
    let apt = pad.keys.get_or_insert_with(|| crate::retail_menu_stage::apt_keys(asset_root));
    let mut values = crate::input::platform::poll(0)
        .map(|p| skate_core::input::xbox::convert(&p.state, u8::from(p.subtype == 7)))
        .unwrap_or([0.0; 24]);
    for (key, slot) in [(KeyCode::Enter, 12)] {
        if keys.pressed(key) {
            values[slot] = 1.0;
        }
    }
    pad.values = values;
    let edges = pad.pad.update(&values, seconds, &RepeatRule::RETAIL);
    let mut out: Vec<Action> = fired(apt, &edges).into_iter().filter_map(Action::from_fe).collect();
    if keys.just_pressed(KeyCode::ArrowRight) {
        out.push(Action::Next);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        out.push(Action::Previous);
    }
    out
}

/// Test aid (no simulated input): SKATE_PHOTO_PICK=<shot> steps to that shot and takes it once the picker has been
/// open SKATE_PHOTO_PICK_DELAY seconds (default 0; a screenshot of the picker needs a few).
fn scripted(picker: &skate_core::challenges::photo_select::PhotoSelect, open_for: f32) -> Vec<Action> {
    let Some(target) = std::env::var("SKATE_PHOTO_PICK").ok().and_then(|v| v.parse::<u32>().ok()) else { return Vec::new() };
    let delay = std::env::var("SKATE_PHOTO_PICK_DELAY").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    if picker.busy() || open_for < delay {
        Vec::new()
    } else if picker.index() != target.min(picker.shots().saturating_sub(1)) {
        vec![Action::Next]
    } else {
        vec![Action::Take]
    }
}

/// The picker's frame (82617458 for the actions, 82617100 for the update, in milliseconds [inferred]).
pub(crate) fn photo_picker(
    mut ftl: ResMut<crate::ftl_host::FollowTheLeader>,
    mut replay: Option<ResMut<crate::replay::Replay>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    config: Option<Res<crate::config::Config>>,
    mut sounds: MessageWriter<crate::ui_audio::FrontendSound>,
    mut pad: Local<PickerPad>,
    mut screen: ResMut<PhotoScreen>,
    (mut magazine, mut commands, target): (ResMut<MagazineScreen>, Commands, Option<Res<crate::graphics_menu::SceneTarget>>),
) {
    let picker_open = ftl.run.as_ref().is_some_and(|s| s.photo.is_some());
    if !picker_open && magazine.open {
        *magazine = MagazineScreen::default();
    }
    if !picker_open && screen.open {
        *screen = PhotoScreen::default();
    }
    let Some(state) = ftl.run.as_mut() else { return };
    let Some(picker) = state.photo.as_mut() else {
        pad.lens = 0.0;
        pad.avatar = [0.0; 2];
        pad.carry = 0.0;
        pad.open_for = 0.0;
        pad.finished = None;
        pad.magazine_for = 0.0;
        pad.finish_in = None;
        return;
    };
    let id = state.data.id.clone();
    if !screen.open {
        *screen = PhotoScreen { open: true, shots: picker.shots(), layout: layout_label(state.data.photo_layout.as_deref(), state.data.photo_layout_two_page),
            states: Vec::new() };
    }
    let asset_root = config.as_ref().map(|c| c.asset_root.clone()).unwrap_or_default();
    let dt = time.delta_secs();
    let mut todo = actions(&mut pad, &asset_root, &keys, dt);
    pad.open_for += dt;
    todo.extend(scripted(picker, pad.open_for));
    let mut fx = Effects::default();
    if !picker.busy() {
        if todo.contains(&Action::Reset) {
            pad.lens = 0.0;
        }
        if todo.contains(&Action::Reset) {
            pad.avatar = [0.0; 2];
        }
        pad.carry += dt * 60.0;
        let zoom = pad.values[22] - pad.values[23];
        let stick = [pad.values[16] - pad.values[17], pad.values[18] - pad.values[19]];
        while pad.carry >= 1.0 {
            pad.carry -= 1.0;
            pad.lens = lens_step(pad.lens, zoom);
            pad.avatar = avatar_step(pad.avatar, stick);
        }
    }
    // MagazinePromo open: the page owns the pad; its answer comes back from the stage.
    if magazine.open {
        todo.clear();
        // Test aid: with SKATE_PHOTO_PICK the page answers SKATE_PHOTO_MAGAZINE (continue / edit / retry, default
        // continue) once its photo has shown for SKATE_PHOTO_PICK_DELAY + 2 s.
        if magazine.image.is_some() {
            pad.magazine_for += dt;
        }
        let delay = std::env::var("SKATE_PHOTO_PICK_DELAY").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) + 2.0;
        if std::env::var_os("SKATE_PHOTO_PICK").is_some() && magazine.choice.is_none() && pad.magazine_for > delay {
            use skate_core::challenges::photo_select::MagazineChoice;
            magazine.choice = Some(match std::env::var("SKATE_PHOTO_MAGAZINE").as_deref() {
                Ok("edit") => MagazineChoice::Edit,
                Ok("retry") => MagazineChoice::Retry,
                _ => MagazineChoice::Continue,
            });
            pad.magazine_for = 0.0;
        }
        if let Some(choice) = magazine.choice.take() {
            info!("SKATE_PHOTO {id} magazine page: {choice:?}");
            *magazine = MagazineScreen::default();
            let outs = picker.magazine_closed(choice);
            carry_out(&id, outs, replay.as_deref_mut(), Some(&mut sounds), Some(&mut screen));
        }
    }
    for a in todo {
        let outs = picker.action(a);
        let f = carry_out(&id, outs, replay.as_deref_mut(), Some(&mut sounds), Some(&mut screen));
        fx.magazine |= f.magazine;
    }
    if fx.magazine {
        // 82617FE8 with a magazine layout: FE 69 over the picture; the page shows the held picture, captured now.
        *magazine = MagazineScreen { open: true, state: magazine_state(state.data.photo_layout_kind), image: None, choice: None };
        match target.as_ref() {
            Some(target) => {
                commands.spawn(bevy::render::view::screenshot::Screenshot::image(target.0.clone())).observe(
                    |ev: On<bevy::render::view::screenshot::ScreenshotCaptured>, mut images: ResMut<Assets<Image>>, mut magazine: ResMut<MagazineScreen>| {
                        if magazine.open {
                            magazine.image = Some(images.add(ev.image.clone()));
                        }
                    },
                );
            }
            None => warn!("SKATE_PHOTO {id} magazine page: no scene target to capture"),
        }
    }
    // The edits on the picture (ScenePhotoCustomizer -> the ICE env color block, research b1063).
    if let Some(r) = replay.as_deref_mut() {
        let [b, c, s, _] = picker.edits();
        let look = Some([b, c, s]);
        if r.photo_look != look {
            r.photo_look = look;
        }
        r.photo_lens_degrees = pad.lens;
        r.photo_avatar_degrees = pad.avatar;
    }
    let outs = picker.update(dt * 1000.0);
    let done = carry_out(&id, outs, replay.as_deref_mut(), Some(&mut sounds), Some(&mut screen));
    if done.store {
        // 82618290: a row with a career slot stores the picked shot (b952); the finish follows once it is written.
        match (career_photo_path(&asset_root, state.data.photo_slot, &id), replay.as_deref_mut()) {
            (Some(path), Some(r)) => {
                r.career_capture = Some(path);
                pad.finish_in = Some(CAPTURE_FRAMES);
            }
            _ => info!("SKATE_PHOTO {id} no career photo (slot {})", state.data.photo_slot),
        }
    }
    if let Some(mode) = done.finished {
        pad.finished = Some(mode);
    }
    // The finish waits for the capture of the held picture (retail: the memcard request's callback).
    if let Some(n) = pad.finish_in.as_mut() {
        *n = n.saturating_sub(1);
    }
    if let Some(mode) = pad.finished.filter(|_| pad.finish_in.is_none_or(|n| n == 0)) {
        info!("SKATE_PHOTO {id} photo taken at shot {} (career slot {}), finish {mode:?}", picker.index(), state.data.photo_slot);
        pad.finished = None;
        pad.finish_in = None;
        if let Some(r) = replay.as_deref_mut() {
            r.end_still();
        }
        state.photo_finished(mode);
    }
}

/// Frames the held picture stays up after the career photo capture was requested (the screenshot is taken from a
/// rendered frame).
const CAPTURE_FRAMES: u8 = 3;


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_axis_moves_the_lens_past_the_dead_zone_within_ten_degrees() {
        assert_eq!(lens_step(0.0, 0.2), 0.0, "inside the dead zone");
        assert!((lens_step(0.0, 1.0) + 0.2).abs() < 1e-6, "full up narrows by 0.2 degrees an update");
        let mut lens = 0.0;
        for _ in 0..100 {
            lens = lens_step(lens, -1.0);
        }
        assert_eq!(lens, 10.0, "held: clamped at 10 degrees");
    }

    #[test]
    fn a_career_photo_goes_under_its_slot_and_slot_zero_stores_nothing() {
        let root = std::env::temp_dir().join(format!("skate-career-photo-{}", std::process::id()));
        let assets = root.join("assets");
        assert!(career_photo_path(&assets, 0, "photo_univ_07").is_none(), "82618290: slot 0 saves nothing");
        let path = career_photo_path(&assets, 30, "photo_anyw_01").unwrap();
        assert_eq!(path, root.join("settings/profiles/local/career-photos/30.png"));
        career_photo_path(&assets, 1, "photo_dwtn_01").unwrap();
        let index: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("settings/profiles/local/career-photos/index.json")).unwrap()).unwrap();
        assert_eq!(index["30"]["challenge"], "photo_anyw_01");
        assert_eq!(index["1"]["file"], "1.png", "a second slot keeps the first");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_movie_natives_answer_the_shots_and_the_layout_label() {
        use crate::apt_vm::Value;
        let screen = PhotoScreen { open: true, shots: 8, layout: layout_label(Some("photo_dwtn_01"), true), states: Vec::new() };
        assert!(matches!(natives(&screen, "HUDComponents", "PhotoSelect_GetNumPhotos"), Some(Value::Number(n)) if n == 8.0));
        assert!(matches!(natives(&screen, "HUDComponents", "PhotoSelect_IsSelectionAvail"), Some(Value::Bool(true))));
        assert!(matches!(natives(&screen, "Mission", "GetChallengePhotoLayout"), Some(Value::Number(n)) if n == 2.0), "a two-page spread");
        assert_eq!((layout_label(None, false), layout_label(Some("x"), false)), (0, 1));
        assert!(natives(&PhotoScreen::default(), "HUDComponents", "PhotoSelect_GetNumPhotos").is_none(), "closed: not ours");
    }

    #[test]
    fn the_left_stick_turns_the_avatar_both_axes_once_either_is_past_the_dead_zone() {
        assert_eq!(avatar_step([0.0; 2], [0.1, 0.15]), [0.0; 2]);
        let a = avatar_step([0.0; 2], [1.0, 0.1]);
        assert!((a[0] + 0.4).abs() < 1e-6 && (a[1] + 0.04).abs() < 1e-6, "{a:?}");
        let mut a = [0.0; 2];
        for _ in 0..200 {
            a = avatar_step(a, [-1.0, 1.0]);
        }
        assert_eq!(a, [30.0, -30.0]);
    }
}