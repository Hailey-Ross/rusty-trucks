//! Dev view of the retail pause menu on the front-end stage (doc 31, Milestone 6).
//!
//! On unless `SKATE3_RETAIL_MENUS=0` (`SKATE3_RETAIL_MENUS_OPEN=1` also opens it after 4 s). Start and F8 open / close the retail pause menu (core_menu through the retail
//! screen manager, retail CrossBar arguments); while it is open the arrow keys, Enter and Backspace drive the
//! ported crossbar handler (skate_core::crossbar), whose sounds play as fe records and whose SetState goes to the
//! movie through `_global.ScreenManager.SetScreenState` like retail's C++. The PC overlay (`graphics_menu`) stays on
//! Guide / F1 / Escape, and takes Start too while the retail pause menu cannot open (`RetailPause`).
//!
//! Input: the pad (all four controllers) and the keyboard (arrows = d-pad, Enter = A, Backspace = B, F8 = Start)
//! feed retail's Apt actions (`skate_core::fe_input`); `SKATE3_RETAIL_MENUS_KEYS=13,13,2` injects key indices
//! once the menu is ready (screenshot checks). Repeat: retail's tick rule (fe_input; tick rate assumed).
use crate::{
    apt_stage::Stage,
    apt_vm::Value,
    crossbar_host::FrontEnd,
    retail_menu_movies::{MenuImage, RetailMenuMovies},
    retail_menus::RetailMenus,
    scoring_hud::{render_draws, HudMaterial, Slot},
    ui_audio::FrontendSound,
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use skate_core::crossbar::{CrossbarAction, CrossbarRules, CrossbarState, NavEvent};
use skate_core::fe_input::{fired, parse_apt_keys, AptKey, FeInput, RepeatRule};
use skate_core::fe_stack::{Effect, FeStack, Layer, Request};
use skate_core::menu_values::Direction;
use skate_core::menus::{EntryRef, MenuAction, Outcome, Owner};
use std::collections::BTreeMap;

const CROSSBAR: &str = "CrossBar";
/// Key index of AptStart (Apt key table record 0).
const APT_START: u32 = 0;
/// AptPrevious (B), AptLeft, AptRight key indices.
const APT_PREVIOUS: u32 = 3;
const APT_LEFT: u32 = 14;
const APT_RIGHT: u32 = 15;
/// The pause menu template's path under `source/screens/` (template table 0x8302F1A8, record 69).
const CROSSBAR_PATH: &str = "main/core_menu.swf";
/// The Game Settings template (record 90) and its path; FE screen 45's init opens it (sub_8260BF58).
const OPTIONS: &str = "Options";
const OPTIONS_PATH: &str = "options/options.swf";
const CROSSBAR_ENUMS: [&str; 6] =
    ["RefreshMenu", "RefreshOption", "InitialiseMenu", "InitialiseSubOption", "InitSubOptionOnlineTeam", "RefreshDLCBanner"];
/// Above the scoring HUD's draws on the shared layer.
const MENU_Z: f32 = 50.0;

#[derive(Resource)]
struct MenuStage {
    stage: Stage,
    crossbar: CrossbarState,
    rules: CrossbarRules,
    language: BTreeMap<String, String>,
    /// The FE screen stack (skate_core::fe_stack); the pause crossbar is the only screen with a movie so far.
    stack: FeStack,
    textures: BTreeMap<String, Handle<Image>>,
    white: Handle<Image>,
    slots: Vec<Slot>,
    failed: bool,
    /// `SKATE3_RETAIL_MENUS_OPEN=1`: open the pause menu once the world has run 4 s (screenshot checks).
    auto_open: bool,
    /// The pause menu's AS screen id (600 + template index; retail 669).
    crossbar_id: i32,
    /// The Game Settings screen's AS id (retail 690) and state; its titles from the menu tables.
    options_id: i32,
    /// The chyron's visibility as last set (vt+48 acts on changes only).
    chyron: bool,
    settings: crate::settings_host::SettingsState,
    settings_titles: Vec<String>,
    /// Retail Apt keys in key-table order (input.cfg expressions).
    apt_keys: Vec<AptKey>,
    pads: [FeInput; 4],
    repeat: RepeatRule,
    /// `SKATE3_RETAIL_MENUS_KEYS`: key indices to inject, one per frame, while the pause menu takes input.
    injected: std::collections::VecDeque<u32>,
}

/// The retail menus are wanted: on unless `SKATE3_RETAIL_MENUS=0`.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE3_RETAIL_MENUS").as_deref() != Ok("0"))
}

/// The retail pause menu can open this frame: the stage loaded and the menu tables carry the FE screen
/// records. False on installs set up before those existed (or with the menus off): Start then opens the PC
/// overlay, the fallback, until setup runs again.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RetailPause(pub bool);

pub(crate) struct RetailMenuStagePlugin;
impl Plugin for RetailMenuStagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RetailPause>();
        if !enabled() {
            return;
        }
        app.add_systems(PostStartup, setup).add_systems(Update, (update, render).chain());
    }
}

/// Retail ScreenManagerReady handler (825D9450), in its order, with values as "%d" strings like retail:
/// templates (only CrossBar so far: NOT RETAIL YET, retail registers all 133 in table order), the 56 Apt keys
/// (value = key index, the same index UpdateInput passes), FE_SOUND_<NAME> = "<name>" per fe record, then
/// platform "0", screen size 1280 x 720, edge offset "136", PAL mode "false". The 191 challenge enums
/// (eChallengeTypes ..) and RefreshSubtitle are not registered yet (no menu reads them so far).
fn boot_registration(stage: &mut Stage, fe: &mut FrontEnd, assets: &std::path::Path) -> Result<Vec<String>, String> {
    let text = |s: &str| Value::Text(s.into());
    let tables = std::fs::read_to_string(assets.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok());
    // All templates in table order (the AS hands out ids from 600); tables extracted before templates were
    // added register CrossBar alone.
    let names: Vec<String> = match tables.as_ref().map(|t| &t.templates).filter(|t| !t.is_empty()) {
        Some(templates) => templates.iter().map(|t| t.name.clone()).collect(),
        None => vec![CROSSBAR.into()],
    };
    for name in &names {
        stage.call_path("_global.ScreenManager.RegisterTemplate", vec![text(name)], fe)?;
    }
    for (i, key) in tables.iter().flat_map(|t| &t.keys).enumerate() {
        stage.call_path("_level0._root.RegisterAptInputEnum", vec![text(&key.name), text(&i.to_string())], fe)?;
    }
    let manifest: serde_json::Value = std::fs::read_to_string(assets.join("private/audio/audio_manifest.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut sounds: Vec<&str> = manifest["frontend"]["sounds"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(|s| s["name"].as_str())
        .collect();
    sounds.sort_unstable();
    for name in sounds {
        stage.call_path(
            "_level0._root.RegisterAudioEnum",
            vec![text(&format!("FE_SOUND_{}", name.to_ascii_uppercase())), text(name)],
            fe,
        )?;
    }
    stage.call_path("_level0._root.SetPlatform", vec![text("0")], fe)?;
    stage.call_path("_level0._root.SetScreenSize", vec![text("1280"), text("720")], fe)?;
    stage.call_path("_level0._root.SetScreenEdgeOffset", vec![text("136")], fe)?;
    stage.call_path("_level0._root.UpdatePALMode", vec![text("false")], fe)?;
    Ok(names)
}

/// The Apt keys: names from the menu tables' key table, expressions from the install's stock input.cfg.
fn apt_keys(assets: &std::path::Path) -> Vec<AptKey> {
    let names: Vec<String> = std::fs::read_to_string(assets.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
        .map(|t| t.keys.into_iter().map(|k| k.name).collect())
        .unwrap_or_default();
    let cfg = std::fs::read_to_string(assets.join(skate_data::input_config::STOCK_INPUT_PATH)).unwrap_or_default();
    parse_apt_keys(&cfg, &names).unwrap_or_else(|e| {
        warn!("RETAIL_MENUS input.cfg: {e}");
        Vec::new()
    })
}

/// The keys fired this frame per controller (FE poll sub_825DFDC8: every connected controller, keys in table
/// order). The keyboard drives controller 0's slots.
fn poll_keys(ms: &mut MenuStage, keys: &ButtonInput<KeyCode>, seconds: f32) -> Vec<(u32, usize)> {
    let mut out = Vec::new();
    for controller in 0..4 {
        let mut values = crate::input::platform::poll(controller)
            .map(|p| skate_core::input::xbox::convert(&p.state, u8::from(p.subtype == 7)))
            .unwrap_or([0.0; 24]);
        if controller == 0 {
            for (key, slot) in [
                (KeyCode::ArrowUp, 0),
                (KeyCode::ArrowDown, 1),
                (KeyCode::ArrowLeft, 2),
                (KeyCode::ArrowRight, 3),
                (KeyCode::F8, 4),
                (KeyCode::Enter, 12),
                (KeyCode::Backspace, 13),
            ] {
                if keys.pressed(key) {
                    values[slot] = 1.0;
                }
            }
        }
        let edges = ms.pads[controller].update(&values, seconds, &ms.repeat);
        out.extend(fired(&ms.apt_keys, &edges).into_iter().map(|k| (k, controller)));
    }
    out
}

fn front_end<'a>(
    menus: &'a RetailMenus,
    language: &'a BTreeMap<String, String>,
    crossbar: CrossbarState,
) -> FrontEnd<'a> {
    let mut fe = FrontEnd::new(&menus.registry, &menus.context, language);
    fe.state = crossbar;
    fe.sub_options = SUB_OPTIONS.get().cloned().unwrap_or_default();
    let counts = crate::crossbar_host::sub_option_counts(&fe.sub_options, &menus.context);
    fe.view = skate_core::crossbar::RegistryView::new(&menus.registry, &menus.context, counts);
    fe
}

/// The crossbar sub-option tables from the menu tables (read once at setup).
static SUB_OPTIONS: std::sync::OnceLock<std::sync::Arc<Vec<crate::crossbar_host::SubOptionRow>>> = std::sync::OnceLock::new();

fn setup(
    mut commands: Commands,
    config: Res<crate::config::Config>,
    movies: Option<Res<RetailMenuMovies>>,
    menus: Option<Res<RetailMenus>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(movies), Some(menus)) = (movies, menus) else {
        warn!("RETAIL_MENUS stage: menu movies or tables missing");
        return;
    };
    let language = movies
        .movies
        .get("source/screens/main/core_menu")
        .map(|m| m.text_assets.language.clone())
        .unwrap_or_default();
    let _ = SUB_OPTIONS.set(std::sync::Arc::new(
        std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
            .ok()
            .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
            .map(|t| t.sub_options)
            .unwrap_or_default(),
    ));
    let result = (|| -> Result<(Stage, Vec<String>), String> {
        let mut fe = front_end(&menus, &language, CrossbarState::default());
        let mut stage = Stage::new(&fe)?;
        // Retail boot (sub_825D7E10): screens/main into _level0; its root loads the screen manager into
        // gScreenManager and the HUD / autosave / fmv levels, which land on the next updates.
        stage.load_root("source/screens/main", &movies.movies, &mut fe)?;
        for _ in 0..4 {
            stage.update(16.0, &movies.movies, &mut fe)?;
        }
        // ScreenManagerReady (825D9450; `.local/research/boot-enums.md`): the C++ answers the screen manager's
        // ready call with the boot registration.
        let mut templates = vec![CROSSBAR.to_owned()];
        if fe.calls.iter().any(|c| c == "Game.ScreenManagerReady()") {
            templates = boot_registration(&mut stage, &mut fe, &config.asset_root)?;
        }
        // Crossbar enums (sub_8261B9C8): RegisterChallengeEnum(name, "%d"), value = index in table 0x830274F0.
        for (i, name) in CROSSBAR_ENUMS.iter().enumerate() {
            stage.call_path(
                "_level0._root.RegisterChallengeEnum",
                vec![Value::Text(format!("FECrossBarScreenMode_{name}")), Value::Text(i.to_string())],
                &mut fe,
            )?;
        }
        Ok((stage, templates))
    })();
    match result {
        Ok((stage, templates)) => {
            // AS screen ids: 600 + the template's registration index.
            let as_id = |name: &str| 600 + templates.iter().position(|n| n == name).unwrap_or(0) as i32;
            let tables = std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
                .ok()
                .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok());
            let white = images.add(Image::new(
                Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                TextureDimension::D2,
                vec![255; 4],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            ));
            info!("RETAIL_MENUS stage ready (F8 opens the retail pause menu)");
            commands.insert_resource(MenuStage {
                stage,
                crossbar: CrossbarState::default(),
                rules: CrossbarRules::default(),
                language,
                stack: FeStack::default(),
                textures: BTreeMap::new(),
                white,
                slots: Vec::new(),
                failed: false,
                auto_open: std::env::var("SKATE3_RETAIL_MENUS_OPEN").as_deref() == Ok("1"),
                crossbar_id: as_id(CROSSBAR),
                options_id: as_id(OPTIONS),
                chyron: false,
                settings: Default::default(),
                settings_titles: tables.map(|t| t.settings_titles).unwrap_or_default(),
                apt_keys: apt_keys(&config.asset_root),
                pads: Default::default(),
                repeat: RepeatRule::RETAIL,
                injected: std::env::var("SKATE3_RETAIL_MENUS_KEYS")
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|k| k.trim().parse().ok())
                    .collect(),
            });
        }
        Err(e) => error!("RETAIL_MENUS stage failed: {e}"),
    }
}

fn update(
    stage: Option<ResMut<MenuStage>>,
    movies: Option<Res<RetailMenuMovies>>,
    menus: Option<ResMut<RetailMenus>>,
    keys: Res<ButtonInput<KeyCode>>,
    // Real time: the menus keep running while they suspend gameplay (virtual time paused).
    time: Res<Time<Real>>,
    mut sounds: MessageWriter<FrontendSound>,
    mut backdrop: ResMut<crate::fe_backdrop::FeBackdrop>,
    mut overlay: Option<ResMut<crate::graphics_menu::Menu>>,
    mut fe_hud: ResMut<crate::fe_backdrop::FeHud>,
    mut available: ResMut<RetailPause>,
    mut warned: Local<bool>,
) {
    let ready = stage.as_ref().is_some_and(|s| !s.failed) && movies.is_some() && menus.is_some();
    // The ported screens in this build's screen table (TU3 FE ids 56 and 45); none in tables extracted before
    // screens.
    let pause = backdrop.index(crate::fe_backdrop::PAUSE_SCREEN);
    let options = backdrop.index(crate::fe_backdrop::SETTINGS_SCREEN);
    available.set_if_neq(RetailPause(ready && pause.is_some()));
    if ready && pause.is_none() && !std::mem::replace(&mut *warned, true) {
        warn!("RETAIL_MENUS: the menu tables have no FE screens yet (run setup); Start opens the PC overlay until then");
    }
    let (Some(mut ms), Some(movies), Some(mut menus)) = (stage, movies, menus) else { return };
    if ms.failed {
        // A stopped stage gives the game back: no suspended gameplay, no pause backdrop (Start then opens the PC
        // overlay). Without this a stage error with the pause menu open froze the game (session 2026-10-12).
        if let Some(overlay) = overlay.as_mut().filter(|o| o.fe_suspended) {
            warn!("RETAIL_MENUS stage stopped with the pause menu open: gameplay resumed");
            overlay.fe_suspended = false;
            backdrop.set_stack(Vec::new());
            *fe_hud = crate::fe_backdrop::FeHud::default();
        }
        return;
    }
    let ms = &mut *ms;
    // Retail Apt keys fired this frame (pad and keyboard); none while the PC overlay is open on top.
    let mut fired = poll_keys(ms, &keys, time.delta_secs());
    if overlay.as_ref().is_some_and(|o| o.open) {
        fired.clear();
    }
    let mut fe = front_end(&menus, &ms.language, ms.crossbar.clone());
    fe.settings = std::mem::take(&mut ms.settings);
    fe.settings_titles = ms.settings_titles.clone();
    fe.settings.open = options.is_some() && ms.stack.top(Layer::Main) == options;
    // Work for the registry once the movie work is done: the crossbar item, settings steps.
    let mut selected: Option<String> = None;
    let mut steps: Vec<(String, Direction)> = Vec::new();
    let result = (|| -> Result<(), String> {
        let (crossbar_id, options_id) = (ms.crossbar_id, ms.options_id);
        let auto = ms.auto_open && time.elapsed_secs() > 4.0;
        ms.auto_open &= !auto;
        if auto {
            fired.insert(0, (APT_START, 0));
        }
        if ms.stack.input_target().is_some() {
            fired.extend(ms.injected.pop_front().map(|k| (k, 0)));
        }
        for (key, controller) in fired {
            // Session log: every key the menus took, by retail's Apt key name (the user's play tests).
            info!("RETAIL_MENUS key {} pad {controller} screen {:?}", ms.apt_keys.get(key as usize).map_or("?", |k| k.name.as_str()), ms.stack.input_target().and_then(|t| backdrop.id(t)));
            if key == APT_START {
                // Retail Start (sub_825EB998, mode 0) pushes FE 56 with the plain push. NOT RETAIL YET (unverified):
                // Start while it is open closes it.
                match pause {
                    Some(p) if ms.stack.active(p) => ms.stack.request(Layer::Main, Request::Close { screen: p }),
                    Some(p) => ms.stack.request(Layer::Main, Request::push(p)),
                    // Tables from before the FE screen records: the PC overlay takes Start (`RetailPause`).
                    None => {}
                }
                continue;
            }
            let target = ms.stack.input_target();
            let Some(screen) = target.filter(|t| Some(*t) == pause || Some(*t) == options) else { continue };
            // sub_825EC0E8: the screen's C++ handler first, then the movie's UpdateInput when the handler did not
            // consume the key and the screen's record says so (+12).
            let mut consumed = false;
            let movie_id = if Some(screen) == pause { crossbar_id } else { options_id };
            if Some(screen) == pause {
                // Crossbar sub_8261B560: consumes only its close branch.
                if let Some(event) = NavEvent::from_key_index(key) {
                    let out = fe.state.handle(event, &fe.view, &ms.rules, false);
                    if let Some(sound) = &out.sound {
                        sounds.write(FrontendSound::named(sound));
                    }
                    if let Some(state) = out.set_state {
                        // Retail passes both as "%d" strings (sub_8263F320 index 10).
                        set_state(&mut ms.stage, crossbar_id, state, &mut fe)?;
                    }
                    match out.action {
                        Some(CrossbarAction::Close) => {
                            consumed = true;
                            ms.stack.request(Layer::Main, Request::Close { screen });
                        }
                        Some(CrossbarAction::SelectItem { tab, row }) => {
                            selected = fe.view.item(tab, row).map(str::to_owned);
                        }
                        _ => {}
                    }
                }
            } else {
                // Settings sub_8260C258: Back (3) leaves a sub-screen for the main list (sub_8260D078) or closes the
                // screen; Left / Right (14 / 15) step the highlighted row's value, then SetState(50) refreshes.
                // NOT RETAIL YET: the per-row UI sounds are fe records not named yet; nav left / right stand in.
                match key {
                    APT_PREVIOUS => {
                        consumed = true;
                        sounds.write(FrontendSound::named("core_b_button"));
                        if fe.settings.screen > 1 {
                            info!("RETAIL_MENUS settings back to the main list from screen {}", fe.settings.screen);
                            fe.settings.screen = 1;
                            fe.settings.row = 0;
                            set_state(&mut ms.stage, options_id, 1, &mut fe)?;
                        } else {
                            ms.stack.request(Layer::Main, Request::Close { screen });
                        }
                    }
                    APT_LEFT | APT_RIGHT => {
                        consumed = true;
                        if let Some(id) = fe.settings.row_id(fe.settings.row) {
                            let left = key == APT_LEFT;
                            steps.push((id.to_owned(), if left { Direction::Left } else { Direction::Right }));
                            sounds.write(FrontendSound::named(if left { "core_nav_left" } else { "core_nav_right" }));
                        }
                    }
                    _ => {}
                }
            }
            if !consumed && backdrop.info(screen).input_to_movie {
                ms.stage.call_path(
                    "_global.ScreenManager.UpdateInput",
                    vec![Value::Text(movie_id.to_string()), Value::Text(key.to_string()), Value::Text(controller.to_string())],
                    &mut fe,
                )?;
            }
        }
        // The stack's update (sub_82D0A560) and what it asks of the movies and the game.
        for effect in ms.stack.update(&|i| backdrop.info(i)) {
            match effect {
                Effect::Open(i) if Some(i) == pause => {
                    info!("RETAIL_MENUS open pause menu (FE {i})");
                    fe.state = CrossbarState::open(&fe.view);
                    open_screen(&mut ms.stage, CROSSBAR_PATH, CROSSBAR, &mut fe)?;
                }
                Effect::Open(i) if Some(i) == options => {
                    // Settings init sub_8260BF58: screen index -1 -> 0 (the pause path opens on the main list).
                    info!("RETAIL_MENUS open Game Settings (FE {i})");
                    fe.settings.screen = 0;
                    fe.settings.row = 0;
                    open_screen(&mut ms.stage, OPTIONS_PATH, OPTIONS, &mut fe)?;
                }
                Effect::Close(i) if Some(i) == pause => {
                    info!("RETAIL_MENUS close pause menu (FE {i})");
                    close(&mut ms.stage, crossbar_id, &mut fe)?
                }
                Effect::Close(i) if Some(i) == options => {
                    info!("RETAIL_MENUS close Game Settings (FE {i})");
                    // Begin close sub_8260C048: SetScreenState(2) outside the front end (FE state 2), then the outro.
                    set_state(&mut ms.stage, options_id, 2, &mut fe)?;
                    close(&mut ms.stage, options_id, &mut fe)?;
                    fe.settings.open = false;
                }
                Effect::Open(i) | Effect::Close(i) => warn!("RETAIL_MENUS: FE screen {i} has no template yet"),
                Effect::SuspendGameplay(on) => {
                    info!("RETAIL_MENUS gameplay {}", if on { "suspended" } else { "resumed" });
                    if let Some(overlay) = overlay.as_mut() {
                        overlay.fe_suspended = on;
                    }
                }
                // vt+44(hud, online): 0 both on, 1 both off, 2 online only (3 = keep is never emitted).
                Effect::Hud(mode) => {
                    use skate_core::fe_stack::HudMode;
                    *fe_hud = match mode {
                        HudMode::On | HudMode::Keep => crate::fe_backdrop::FeHud { hud: true, online: true },
                        HudMode::Off => crate::fe_backdrop::FeHud { hud: false, online: false },
                        HudMode::OnlineOnly => crate::fe_backdrop::FeHud { hud: false, online: true },
                    };
                }
                // vt+48 -> sub_825E4408: on a change, the chyron movie's _global.Chyron_SetVisible("true" / "false").
                // NOT RETAIL YET: retail plays Chyron_PlayOutro instead of hiding when the chyron is mid-show.
                Effect::Chyron(on) if on != ms.chyron => {
                    ms.chyron = on;
                    let defined = matches!(
                        ms.stage.vm.get(ms.stage.vm.global, "Chyron_SetVisible"),
                        crate::apt_vm::Value::Object(_)
                    );
                    if defined {
                        ms.stage.call_path(
                            "_global.Chyron_SetVisible",
                            vec![Value::Text(if on { "true" } else { "false" }.into())],
                            &mut fe,
                        )?;
                    }
                }
                Effect::Chyron(_) => {}
                // The blur follows the stack's top through FeBackdrop (mod overrides); show / param / focus have
                // no screen that uses them yet.
                Effect::Blur(_) | Effect::Show(..) | Effect::Param(..) | Effect::Focus(_) => {}
            }
        }
        ms.stage.update(time.delta_secs() * 1000.0, &movies.movies, &mut fe)?;
        // Settings loaded (vt+108 sub_8260C010): SetScreenState(1) builds the rows; intro complete = ready.
        if let Some(o) = options.filter(|o| ms.stack.top(Layer::Main) == Some(*o)) {
            let loaded = format!("Game.OnScreenLoaded({options_id})");
            if take_call(&mut fe, &loaded) {
                fe.settings.open = true;
                set_state(&mut ms.stage, options_id, 1, &mut fe)?;
            }
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({options_id})")) {
                ms.stack.ready(o);
            }
        }
        // Crossbar IntroComplete (vtable 0x82303F90 +104 = 8261B1F0) selects the mode menu (8261B9E8): when the
        // mode changed it resets level 1, row 0, tab 0 and walks to the first enabled row, then it always posts
        // SetState(InitialiseMenu) through the facade (+56 vt+20). The screen is ready (state 3) from here.
        if let Some(p) = pause.filter(|p| ms.stack.top(Layer::Main) == Some(*p)) {
            if take_call(&mut fe, &format!("Game.OnScreenIntroComplete({crossbar_id})")) {
                ms.stack.ready(p);
                fe.state = CrossbarState::open(&fe.view);
                set_state(&mut ms.stage, crossbar_id, ms.rules.state_initialise_menu, &mut fe)?;
            }
        }
        Ok(())
    })();
    // `SKATE3_RETAIL_MENUS_TRACE=1`: log the host calls the movies made that nothing answered (diagnostics).
    if std::env::var_os("SKATE3_RETAIL_MENUS_TRACE").is_some() && !fe.calls.is_empty() {
        info!("RETAIL_MENUS calls: {}", fe.calls.join(" | "));
    }
    // Audio.PlaySound(FE_SOUND_X) from the movies: the value is the fe record name (RegisterAudioEnum).
    for call in fe.calls.iter().filter_map(|c| c.strip_prefix("Audio.PlaySound(")) {
        let name = call.trim_end_matches(')').split(',').next().unwrap_or("");
        if !name.is_empty() && name != "undefined" {
            sounds.write(FrontendSound::named(name));
        }
    }
    ms.crossbar = fe.state;
    ms.settings = fe.settings;
    let settings_selected = std::mem::take(&mut ms.settings.selected);
    if result.is_ok() {
        let mut refresh: Option<i32> = None;
        if let Some(item) = selected {
            // Retail runs the item (its FE screen); ours goes through the registry: an engine handler pushes a
            // ported FE screen or opens an overlay section, a mod's gets its menu_select event.
            let outcome = menus.act(MenuAction::Select { entry: EntryRef::item(&item) });
            info!("RETAIL_MENUS select {item}: {outcome:?}");
            if let Outcome::Forward(Owner::Engine(name), _) = &outcome {
                if let Some(id) = name.strip_prefix(crate::retail_menus::SCREEN) {
                    match backdrop.index(id) {
                        Some(screen) => ms.stack.request(Layer::Main, Request::push(screen)),
                        None => warn!("RETAIL_MENUS: no FE screen {id}"),
                    }
                } else if let (Some(section), Some(overlay)) =
                    (name.strip_prefix(crate::retail_menus::OVERLAY), overlay.as_mut())
                {
                    if let Some(p) = pause {
                        ms.stack.request(Layer::Main, Request::Close { screen: p });
                    }
                    overlay.open_section(section);
                }
            }
        }
        // Settings OnMenuSelect (sub_8260D290): a link row opens its screen (+68) and rebuilds (SetState(1)); other
        // rows go to their handler (engine or mod).
        for row in settings_selected {
            let Some(id) = ms.settings.row_id(row).map(str::to_owned) else { continue };
            sounds.write(FrontendSound::named("core_a_button"));
            match menus.act(MenuAction::Select { entry: EntryRef::setting(&id) }) {
                Outcome::OpenScreen(screen) => {
                    ms.settings.screen = screen;
                    ms.settings.row = 0;
                    refresh = Some(1);
                }
                outcome => info!("RETAIL_MENUS settings select {id}: {outcome:?}"),
            }
        }
        for (id, direction) in steps {
            let outcome = menus.act(MenuAction::Step { entry: EntryRef::setting(&id), direction });
            info!("RETAIL_MENUS settings step {id} {direction:?}: {outcome:?}");
            refresh = refresh.or(Some(50));
        }
        if let Some(state) = refresh {
            let mut fe = front_end(&menus, &ms.language, ms.crossbar.clone());
            fe.settings = std::mem::take(&mut ms.settings);
            fe.settings_titles = ms.settings_titles.clone();
            if let Err(e) = set_state(&mut ms.stage, ms.options_id, state, &mut fe) {
                error!("RETAIL_MENUS settings refresh: {e}");
            }
            ms.settings = fe.settings;
        }
    }
    if ms.stack.top(Layer::Main).is_none() {
        *fe_hud = crate::fe_backdrop::FeHud::default();
    }
    let stack: Vec<String> = ms.stack.screens(Layer::Main).filter_map(|i| backdrop.id(i).map(str::to_owned)).collect();
    backdrop.set_stack(stack);
    if let Err(e) = result {
        error!("RETAIL_MENUS stage stopped: {e}");
        ms.failed = true;
    }
}

/// Retail ScreenManagerBase::OpenScreen (sub_82D09140) arguments.
fn open_screen(stage: &mut Stage, path: &str, template: &str, fe: &mut FrontEnd) -> Result<(), String> {
    stage.call_path(
        "_global.ScreenManager.OpenScreen",
        vec![
            Value::Text(format!("source/screens/{path}")),
            Value::Text("screen".into()),
            Value::Text("0".into()),
            Value::Text(template.into()),
        ],
        fe,
    )?;
    Ok(())
}

/// The C++ SetScreenState through the facade: both as "%d" strings (sub_8263F320 index 10).
fn set_state(stage: &mut Stage, id: i32, state: i32, fe: &mut FrontEnd) -> Result<(), String> {
    stage.call_path("_global.ScreenManager.SetScreenState", vec![Value::Text(id.to_string()), Value::Text(state.to_string())], fe)?;
    Ok(())
}

/// Whether the movie made this call this frame (removes it).
fn take_call(fe: &mut FrontEnd, call: &str) -> bool {
    let before = fe.calls.len();
    fe.calls.retain(|c| c != call);
    fe.calls.len() != before
}

fn close(stage: &mut Stage, id: i32, fe: &mut FrontEnd) -> Result<(), String> {
    // Retail CloseScreen (sub_82D09298): the AS id as "%d"; the screen plays its outro.
    stage.call_path("_global.ScreenManager.CloseScreen", vec![Value::Text(id.to_string())], fe)?;
    Ok(())
}

fn render(
    mut commands: Commands,
    stage: Option<ResMut<MenuStage>>,
    movies: Option<Res<RetailMenuMovies>>,
    config: Res<crate::config::Config>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HudMaterial>>,
    controllers: Option<Res<crate::input::ControllerInput>>,
) {
    let (Some(mut ms), Some(movies)) = (stage, movies) else { return };
    let ms = &mut *ms;
    let mut draws = if ms.failed {
        Vec::new()
    } else {
        match ms.stage.draw(|key, movie| movies.scene(key, movie)) {
            Ok(draws) => draws,
            Err(e) => {
                error!("RETAIL_MENUS draw: {e}");
                ms.failed = true;
                Vec::new()
            }
        }
    };
    // Button prompts (CustomButton components, `prompt/<retail button>`): the first connected pad's printed labels
    // (SDL), Xbox naming without a pad; each label's glyph is generated once (prompt_glyphs).
    let pad = controllers.as_ref().and_then(|c| c.kinds.iter().flatten().next().cloned());
    for draw in &mut draws {
        let Some(button) = draw.texture.strip_prefix("prompt/") else { continue };
        let Some(prompt) = crate::prompt_glyphs::resolve_for(button, pad.as_deref()) else { continue };
        let key = prompt.key();
        let pixels = move || crate::prompt_glyphs::render(&prompt);
        if !ms.textures.contains_key(&key) {
            let handle = images.add(Image::new(
                Extent3d { width: crate::prompt_glyphs::SIZE, height: crate::prompt_glyphs::SIZE, depth_or_array_layers: 1 },
                TextureDimension::D2,
                pixels(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            ));
            ms.textures.insert(key.clone(), handle);
        }
        draw.texture = key;
    }
    // An unresolved prompt draws nothing (an empty key would draw solid white).
    draws.retain(|d| !d.texture.starts_with("prompt/"));
    let root = crate::retail_menu_movies::set_root(&config);
    for draw in &draws {
        if draw.texture.is_empty() || ms.textures.contains_key(&draw.texture) {
            continue;
        }
        let image = match movies.image(&draw.texture) {
            Some(MenuImage::Retail { path, width, height }) => std::fs::read(root.join(path))
                .ok()
                .filter(|b| b.len() == width as usize * height as usize * 4)
                .map(|b| (width, height, b)),
            Some(MenuImage::Mod(m)) => Some((m.width, m.height, m.rgba.clone())),
            None => None,
        };
        let Some((width, height, bytes)) = image else {
            warn!("RETAIL_MENUS texture missing: {}", draw.texture);
            ms.textures.insert(draw.texture.clone(), ms.white.clone());
            continue;
        };
        let handle = images.add(Image::new(
            Extent3d { width, height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            bytes,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ));
        ms.textures.insert(draw.texture.clone(), handle);
    }
    let (textures, white) = (&ms.textures, ms.white.clone());
    render_draws(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut ms.slots,
        &draws,
        |t| if t.is_empty() { Some(white.clone()) } else { textures.get(t).cloned() },
        MENU_Z,
        // Colour transforms in gamma space like the console front end (retail shot: near-black panel body).
        true,
    );
}
