//! Dev view of the retail pause menu on the front-end stage (doc 31, Milestone 6).
//!
//! Off unless `SKATE3_RETAIL_MENUS=1` (`SKATE3_RETAIL_MENUS_OPEN=1` also opens it at once). F8 opens / closes the retail pause menu (core_menu through the retail
//! screen manager, retail CrossBar arguments); while it is open the arrow keys, Enter and Backspace drive the
//! ported crossbar handler (skate_core::crossbar), whose sounds play as fe records and whose SetState goes to the
//! movie through `_global.ScreenManager.SetScreenState` like retail's C++. The current overlay menu is untouched;
//! swapping it out is a later milestone.
//!
//! NOT RETAIL YET: the keys are a dev mapping, not retail's pad path (sub_825EC0E8) and its repeat.
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
use skate_core::crossbar::{CrossbarAction, CrossbarRules, CrossbarState, NavEvent, RegistryView};
use std::collections::BTreeMap;

const CROSSBAR: &str = "CrossBar";
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
    screen: Option<i32>,
    textures: BTreeMap<String, Handle<Image>>,
    white: Handle<Image>,
    slots: Vec<Slot>,
    failed: bool,
    /// `SKATE3_RETAIL_MENUS_OPEN=1`: open the pause menu on the first update (screenshot checks).
    auto_open: bool,
    /// The pause menu's AS screen id (600 + template index; retail 669).
    crossbar_id: i32,
}

pub(crate) struct RetailMenuStagePlugin;
impl Plugin for RetailMenuStagePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("SKATE3_RETAIL_MENUS").as_deref() != Ok("1") {
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
                screen: None,
                textures: BTreeMap::new(),
                white,
                slots: Vec::new(),
                failed: false,
                auto_open: std::env::var("SKATE3_RETAIL_MENUS_OPEN").as_deref() == Ok("1"),
                crossbar_id,
            });
        }
        Err(e) => error!("RETAIL_MENUS stage failed: {e}"),
    }
}

fn update(
    stage: Option<ResMut<MenuStage>>,
    movies: Option<Res<RetailMenuMovies>>,
    menus: Option<Res<RetailMenus>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut sounds: MessageWriter<FrontendSound>,
    mut backdrop: ResMut<crate::fe_backdrop::FeBackdrop>,
) {
    let (Some(mut ms), Some(movies), Some(menus)) = (stage, movies, menus) else { return };
    if ms.failed {
        return;
    }
    let ms = &mut *ms;
    let view = RegistryView::new(&menus.registry, &menus.context, Default::default());
    let mut fe = front_end(&menus, &ms.language, ms.crossbar.clone());
    let result = (|| -> Result<(), String> {
        let toggle = keys.just_pressed(KeyCode::F8) || std::mem::take(&mut ms.auto_open);
        if toggle {
            match ms.screen {
                None => {
                    fe.state = CrossbarState::open(&view);
                    fe.view = view;
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
                    ms.screen = Some(ms.crossbar_id);
                }
                Some(id) => {
                    close(&mut ms.stage, id, &mut fe)?;
                    ms.screen = None;
                }
            }
        } else if let Some(id) = ms.screen {
            let event = [
                (KeyCode::ArrowUp, NavEvent::Up),
                (KeyCode::ArrowDown, NavEvent::Down),
                (KeyCode::ArrowLeft, NavEvent::TabLeft),
                (KeyCode::ArrowRight, NavEvent::TabRight),
                (KeyCode::Enter, NavEvent::Select),
                (KeyCode::Backspace, NavEvent::Back),
            ]
            .into_iter()
            .find(|(k, _)| keys.just_pressed(*k))
            .map(|(_, e)| e);
            if let Some(event) = event {
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
                        close(&mut ms.stage, id, &mut fe)?;
                        ms.screen = None;
                    }
                    Some(CrossbarAction::SelectItem { tab, row }) => {
                        info!("RETAIL_MENUS select {:?}", fe.view.item(tab, row));
                    }
                    _ => {}
                }
            }
        }
        ms.stage.update(time.delta_secs() * 1000.0, &movies.movies, &mut fe)?;
        // Crossbar IntroComplete (vtable 0x82303F90 +104 = 8261B1F0) selects the mode menu (8261B9E8): when the
        // mode changed it resets level 1, row 0, tab 0 and walks to the first enabled row, then it always posts
        // SetState(InitialiseMenu) through the facade (+56 vt+20).
        if let Some(id) = ms.screen {
            let intro = format!("Game.OnScreenIntroComplete({id})");
            if fe.calls.iter().any(|c| *c == intro) {
                fe.calls.clear();
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
    // FE screen stack: the pause crossbar alone (FE id 56) while it is open; retail applies its blur flag on push.
    backdrop.set_stack(ms.screen.map(|_| crate::fe_backdrop::PAUSE_SCREEN.to_owned()).into_iter().collect());
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
