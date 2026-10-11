//! Dev view of the retail pause menu on the front-end stage (doc 31, Milestone 6).
//!
//! Off unless `SKATE3_RETAIL_MENUS=1` (`SKATE3_RETAIL_MENUS_OPEN=1` also opens it after 4 s). F8 opens / closes the retail pause menu (core_menu through the retail
//! screen manager, retail CrossBar arguments); while it is open the arrow keys, Enter and Backspace drive the
//! ported crossbar handler (skate_core::crossbar), whose sounds play as fe records and whose SetState goes to the
//! movie through `_global.ScreenManager.SetScreenState` like retail's C++. The current overlay menu is untouched;
//! swapping it out is a later milestone.
//!
//! Input: the pad (all four controllers) and the keyboard (arrows = d-pad, Enter = A, Backspace = B, F8 = Start)
//! feed retail's Apt actions (`skate_core::fe_input`); `SKATE3_RETAIL_MENUS_KEYS=13,13,2` injects key indices
//! once the menu is ready (screenshot checks). NOT RETAIL YET: the repeat timing (see fe_input).
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
use skate_core::menus::{EntryRef, MenuAction, Outcome, Owner};
use std::collections::BTreeMap;

const CROSSBAR: &str = "CrossBar";
/// Key index of AptStart (Apt key table record 0).
const APT_START: u32 = 0;
/// The pause menu template's path under `source/screens/` (template table 0x8302F1A8, record 69).
const CROSSBAR_PATH: &str = "main/core_menu.swf";
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
    /// Retail Apt keys in key-table order (input.cfg expressions).
    apt_keys: Vec<AptKey>,
    pads: [FeInput; 4],
    repeat: RepeatRule,
    /// `SKATE3_RETAIL_MENUS_KEYS`: key indices to inject, one per frame, while the pause menu takes input.
    injected: std::collections::VecDeque<u32>,
}

/// The dev view is on (`SKATE3_RETAIL_MENUS=1`).
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE3_RETAIL_MENUS").as_deref() == Ok("1"))
}

pub(crate) struct RetailMenuStagePlugin;
impl Plugin for RetailMenuStagePlugin {
    fn build(&self, app: &mut App) {
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
fn boot_registration(stage: &mut Stage, fe: &mut FrontEnd, assets: &std::path::Path) -> Result<i32, String> {
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
    let crossbar_id = 600 + names.iter().position(|n| n == CROSSBAR).unwrap_or(0) as i32;
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
    Ok(crossbar_id)
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
    fe
}

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
    let result = (|| -> Result<(Stage, i32), String> {
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
        let mut crossbar_id = 600;
        if fe.calls.iter().any(|c| c == "Game.ScreenManagerReady()") {
            crossbar_id = boot_registration(&mut stage, &mut fe, &config.asset_root)?;
        }
        // Crossbar enums (sub_8261B9C8): RegisterChallengeEnum(name, "%d"), value = index in table 0x830274F0.
        for (i, name) in CROSSBAR_ENUMS.iter().enumerate() {
            stage.call_path(
                "_level0._root.RegisterChallengeEnum",
                vec![Value::Text(format!("FECrossBarScreenMode_{name}")), Value::Text(i.to_string())],
                &mut fe,
            )?;
        }
        Ok((stage, crossbar_id))
    })();
    match result {
        Ok((stage, crossbar_id)) => {
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
                crossbar_id,
                apt_keys: apt_keys(&config.asset_root),
                pads: Default::default(),
                repeat: RepeatRule::DEFAULT,
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
) {
    let (Some(mut ms), Some(movies), Some(mut menus)) = (stage, movies, menus) else { return };
    if ms.failed {
        return;
    }
    let ms = &mut *ms;
    // The pause crossbar in this build's screen table (TU3 FE id 56); none in tables extracted before screens.
    let pause = backdrop.index(crate::fe_backdrop::PAUSE_SCREEN);
    // Retail Apt keys fired this frame (pad and keyboard).
    let mut fired = poll_keys(ms, &keys, time.delta_secs());
    let mut fe = front_end(&menus, &ms.language, ms.crossbar.clone());
    // The item selected this frame: dispatched through the registry once the movie work is done.
    let mut selected: Option<String> = None;
    let result = (|| -> Result<(), String> {
        let id = ms.crossbar_id;
        let auto = ms.auto_open && time.elapsed_secs() > 4.0;
        ms.auto_open &= !auto;
        if auto {
            fired.insert(0, (APT_START, 0));
        }
        if ms.stack.input_target().is_some() && pause.is_some() && ms.stack.input_target() == pause {
            fired.extend(ms.injected.pop_front().map(|k| (k, 0)));
        }
        for (key, controller) in fired {
            if key == APT_START {
                // Retail Start (sub_825EB998, mode 0) pushes FE 56 with the plain push. NOT RETAIL YET (unverified):
                // Start while it is open closes it.
                match pause {
                    Some(p) if ms.stack.active(p) => ms.stack.request(Layer::Main, Request::Close { screen: p }),
                    Some(p) => ms.stack.request(Layer::Main, Request::push(p)),
                    None => warn!("RETAIL_MENUS: the menu tables have no FE screens yet (run setup)"),
                }
                continue;
            }
            let Some(p) = pause.filter(|p| ms.stack.input_target() == Some(*p)) else { continue };
            // sub_825EC0E8: the screen's C++ handler first (crossbar sub_8261B560), then the movie's UpdateInput when
            // the handler did not consume the key and the screen's record says so (+12). The crossbar consumes
            // only its close branch.
            let mut consumed = false;
            if let Some(event) = NavEvent::from_key_index(key) {
                let out = fe.state.handle(event, &fe.view, &ms.rules, false);
                if let Some(sound) = &out.sound {
                    sounds.write(FrontendSound::named(sound));
                }
                if let Some(state) = out.set_state {
                    // Retail passes both as "%d" strings (sub_8263F320 index 10).
                    ms.stage.call_path(
                        "_global.ScreenManager.SetScreenState",
                        vec![Value::Text(id.to_string()), Value::Text(state.to_string())],
                        &mut fe,
                    )?;
                }
                match out.action {
                    Some(CrossbarAction::Close) => {
                        consumed = true;
                        ms.stack.request(Layer::Main, Request::Close { screen: p });
                    }
                    Some(CrossbarAction::SelectItem { tab, row }) => {
                        selected = fe.view.item(tab, row).map(str::to_owned);
                    }
                    _ => {}
                }
            }
            if !consumed && backdrop.info(p).input_to_movie {
                ms.stage.call_path(
                    "_global.ScreenManager.UpdateInput",
                    vec![Value::Text(id.to_string()), Value::Text(key.to_string()), Value::Text(controller.to_string())],
                    &mut fe,
                )?;
            }
        }
        // The stack's update (sub_82D0A560) and what it asks of the movies and the game.
        for effect in ms.stack.update(&|i| backdrop.info(i)) {
            match effect {
                Effect::Open(i) if Some(i) == pause => {
                    fe.state = CrossbarState::open(&fe.view);
                    // Retail ScreenManagerBase::OpenScreen (sub_82D09140) arguments.
                    ms.stage.call_path(
                        "_global.ScreenManager.OpenScreen",
                        vec![
                            Value::Text(format!("source/screens/{CROSSBAR_PATH}")),
                            Value::Text("screen".into()),
                            Value::Text("0".into()),
                            Value::Text(CROSSBAR.into()),
                        ],
                        &mut fe,
                    )?;
                }
                Effect::Close(i) if Some(i) == pause => close(&mut ms.stage, id, &mut fe)?,
                Effect::Open(i) | Effect::Close(i) => warn!("RETAIL_MENUS: FE screen {i} has no template yet"),
                Effect::SuspendGameplay(on) => {
                    if let Some(overlay) = overlay.as_mut() {
                        overlay.fe_suspended = on;
                    }
                }
                // NOT RETAIL YET: the HUD modes and the chyron are not hooked up.
                Effect::Hud(_) | Effect::Chyron(_) => {}
                // The blur follows the stack's top through FeBackdrop (mod overrides); show / param / focus have
                // no screen that uses them yet.
                Effect::Blur(_) | Effect::Show(..) | Effect::Param(..) | Effect::Focus(_) => {}
            }
        }
        ms.stage.update(time.delta_secs() * 1000.0, &movies.movies, &mut fe)?;
        // Crossbar IntroComplete (vtable 0x82303F90 +104 = 8261B1F0) selects the mode menu (8261B9E8): when the
        // mode changed it resets level 1, row 0, tab 0 and walks to the first enabled row, then it always posts
        // SetState(InitialiseMenu) through the facade (+56 vt+20). The screen is ready (state 3) from here.
        if let Some(p) = pause.filter(|p| ms.stack.top(Layer::Main) == Some(*p)) {
            let intro = format!("Game.OnScreenIntroComplete({id})");
            if fe.calls.iter().any(|c| *c == intro) {
                fe.calls.clear();
                ms.stack.ready(p);
                fe.state = CrossbarState::open(&fe.view);
                ms.stage.call_path(
                    "_global.ScreenManager.SetScreenState",
                    vec![Value::Text(id.to_string()), Value::Text(ms.rules.state_initialise_menu.to_string())],
                    &mut fe,
                )?;
            }
        }
        Ok(())
    })();
    // Audio.PlaySound(FE_SOUND_X) from the movies: the value is the fe record name (RegisterAudioEnum).
    for call in fe.calls.iter().filter_map(|c| c.strip_prefix("Audio.PlaySound(")) {
        let name = call.trim_end_matches(')').split(',').next().unwrap_or("");
        if !name.is_empty() && name != "undefined" {
            sounds.write(FrontendSound::named(name));
        }
    }
    ms.crossbar = fe.state;
    if let (Some(item), Ok(())) = (selected, &result) {
        // Retail runs the item (its FE screen); ours goes through the registry: an engine handler closes the
        // pause screen and opens the overlay section, a mod's gets its menu_select event.
        let outcome = menus.act(MenuAction::Select { entry: EntryRef::item(&item) });
        info!("RETAIL_MENUS select {item}: {outcome:?}");
        if let Outcome::Forward(Owner::Engine(name), _) = &outcome {
            if let (Some(section), Some(overlay)) = (name.strip_prefix(crate::retail_menus::OVERLAY), overlay.as_mut()) {
                if let Some(p) = pause {
                    ms.stack.request(Layer::Main, Request::Close { screen: p });
                }
                overlay.open_section(section);
            }
        }
    }
    let stack: Vec<String> = ms.stack.screens(Layer::Main).filter_map(|i| backdrop.id(i).map(str::to_owned)).collect();
    backdrop.set_stack(stack);
    if let Err(e) = result {
        error!("RETAIL_MENUS stage stopped: {e}");
        ms.failed = true;
    }
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
) {
    let (Some(mut ms), Some(movies)) = (stage, movies) else { return };
    let ms = &mut *ms;
    let draws = if ms.failed {
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
    );
}
