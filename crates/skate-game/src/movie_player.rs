//! The movie player's picture (step 2, docs/hails-additions/47-movie-player.md): a worker thread decodes the disc's VP6
//! movie ahead (`skate-movie`), the frame clock picks the newest due frame (retail 82A92DC8: target frame = elapsed ms
//! x fps x 0.001, late frames dropped, no slow-down), and the three YUV planes are written straight into persistent
//! single-channel textures in the render world (no asset re-upload per frame); a UI material converts to RGB.
//! Audio, the FE 24 screen with the retail skip rules, subtitles and the boot / career hooks are the next steps.
//! Dev entry: `SKATE_MOVIE=<file.vp6>` plays one movie fullscreen at startup; Escape ends it.
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::time::Instant;

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    image::ImageSampler,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{AsBindGroup, Extent3d, TexelCopyBufferLayout, TextureDimension, TextureFormat, TextureUsages},
        renderer::RenderQueue,
        texture::GpuImage,
    },
    shader::ShaderRef,
    ui_render::UiMaterialPlugin,
};
use bevy::audio::{AddAudioSource, AudioPlayer, PlaybackSettings};
use skate_movie::{FrameDecoder, Vp6Decoder, YuvFrame, container};

/// The audio language tag of the movies' SD blocks (EN, GE, FR on the disc); `SKATE_MOVIE_LANG` for tests.
/// NOT RETAIL YET: retail picks it from the game language word (b955 5b).
fn movie_language() -> [u8; 2] {
    let l = std::env::var("SKATE_MOVIE_LANG").unwrap_or_default().into_bytes();
    if l.len() == 2 { [l[0], l[1]] } else { *b"EN" }
}

/// How many decoded frames the worker keeps ready.
const AHEAD: usize = 6;
/// The movies' audio rate (b924: EA-XA v2, 6 channels, 48 kHz).
const AUDIO_RATE: u32 = 48_000;

/// MixMap Master out4 (audio manager +0x468, default 32767 = 1.0), written by the native audio frame. The movie's
/// gain is the row's volume x out4 / 32767 (b955 2a / 2b: 822F8898 = 1/32767).
pub(crate) static MASTER_OUT4: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(32767);

/// A movie plays: the game's sound and music are paused meanwhile (game_audio `silenced`).
pub(crate) static PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn publish_playing(player: Res<MoviePlayer>) {
    PLAYING.store(player.playing(), std::sync::atomic::Ordering::Relaxed);
}

/// `--mute`: the movie's sound is silent like the game's (it plays through its own Bevy audio player, not the native mix).
pub(crate) static MUTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn movie_gain(row_volume: f32, out4: i32) -> f32 {
    if MUTED.load(std::sync::atomic::Ordering::Relaxed) {
        return 0.0;
    }
    (row_volume * out4.clamp(0, 32767) as f32 / 32767.0).max(0.0)
}

/// Movie audio between the decode worker and the audio device: stereo f32, the frames the device took (the movie
/// clock while there is audio; retail 82A92DC8 uses its audio clock when it has one), and whether the movie has sound.
#[derive(Default)]
pub(crate) struct AudioShared {
    queue: std::sync::Mutex<std::collections::VecDeque<f32>>,
    played_frames: std::sync::atomic::AtomicU64,
    has_audio: std::sync::atomic::AtomicBool,
    ended: std::sync::atomic::AtomicBool,
}

#[derive(Asset, TypePath, Clone)]
pub(crate) struct MovieAudio {
    shared: Arc<AudioShared>,
}

pub(crate) struct MovieAudioDecoder {
    shared: Arc<AudioShared>,
    odd: bool,
}

impl Iterator for MovieAudioDecoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        use std::sync::atomic::Ordering::Relaxed;
        let sample = self.shared.queue.lock().ok().and_then(|mut q| q.pop_front());
        match sample {
            Some(s) => {
                if self.odd {
                    self.shared.played_frames.fetch_add(1, Relaxed);
                }
                self.odd = !self.odd;
                Some(s)
            }
            None if self.shared.ended.load(Relaxed) => None,
            None => Some(0.0),
        }
    }
}

impl bevy::audio::Source for MovieAudioDecoder {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        AUDIO_RATE
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

impl bevy::audio::Decodable for MovieAudio {
    type DecoderItem = f32;
    type Decoder = MovieAudioDecoder;
    fn decoder(&self) -> MovieAudioDecoder {
        MovieAudioDecoder { shared: self.shared.clone(), odd: false }
    }
}

/// Retail's stereo output table (skate-audio dsp/routes.rs `output_stereo`): Lo = 0.707 L + 0.5 C + 0.5 Ls, Ro
/// mirrored, LFE dropped; samples scaled by 1/32767 (b955 2b).
fn fold_stereo(interleaved6: &[i16], out: &mut std::collections::VecDeque<f32>) {
    const G707: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let k = 1.0 / 32767.0;
    for f in interleaved6.chunks_exact(6) {
        let s = |i: usize| f32::from(f[i]) * k;
        out.push_back((G707 * s(0) + 0.5 * s(1) + 0.5 * s(3)).clamp(-1.0, 1.0));
        out.push_back((G707 * s(2) + 0.5 * s(1) + 0.5 * s(4)).clamp(-1.0, 1.0));
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct MovieMaterial {
    #[texture(0)]
    #[sampler(1)]
    luma: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    u: Handle<Image>,
    #[texture(4)]
    #[sampler(5)]
    v: Handle<Image>,
}

impl UiMaterial for MovieMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/movie.wgsl".into()
    }
}

enum Decoded {
    Frame(u32, Arc<YuvFrame>),
    Done,
}

struct Playback {
    rx: std::sync::Mutex<Receiver<Decoded>>,
    fps: f64,
    started: Option<Instant>,
    pending: Option<(u32, Arc<YuvFrame>)>,
    done: bool,
    root: Entity,
    planes: [Handle<Image>; 3],
    shown: u32,
    dropped: u32,
    audio: Arc<AudioShared>,
    audio_entity: Entity,
    rules: SkipRules,
    /// The row's `volume`.
    volume: f32,
    elapsed_ms: f32,
    prompt: bool,
    prev_buttons: u16,
    /// The row's cue file base and its `A3A0` / `2914` flags (none for a file played without a row).
    subtitles: Option<(String, bool, bool)>,
    /// Decided on the first update (retail 825E0790 at the movie's start): forced, and the cue list when shown.
    subtitles_decided: bool,
    forced_subtitles: bool,
    subtitler: Option<skate_data::subtitles::Subtitler>,
}

/// The disc's subtitle cues (`skate_data::subtitles`), loaded once.
#[derive(Resource, Default)]
pub(crate) struct MovieSubtitles(pub skate_data::subtitles::Subtitles);

/// The subtitle line for the menu stage (retail HUD block M: the line, the show gate `option || forced`): the stage
/// answers `Game.GetMovieSubtitleString` / `Audio.HasSubtitles` from it and refreshes the subtitle movie when
/// `generation` changes.
#[derive(Resource, Default)]
pub(crate) struct MovieSubtitle {
    /// The current cue's string label (localized by the stage).
    pub label: Option<String>,
    pub show: bool,
    pub generation: u32,
}

impl MovieSubtitle {
    fn set(&mut self, label: Option<String>) {
        if self.label != label {
            self.label = label;
            self.generation = self.generation.wrapping_add(1);
        }
    }
}

/// The subtitle folder of the movie language (`SubTitles_<name>`); forced subtitles are for French and German (b955 5b).
fn subtitle_language(tag: [u8; 2]) -> (&'static str, bool) {
    match &tag {
        b"FR" => ("French", true),
        b"GE" => ("German", true),
        _ => ("English", false),
    }
}

/// The movie rows from the disc (`skate_data::movie_rows`), loaded once.
#[derive(Resource, Default)]
pub(crate) struct MovieRows {
    pub rows: Vec<skate_data::movie_rows::MovieRow>,
    asset_root: std::path::PathBuf,
}

impl MovieRows {
    /// The row's file in the install (setup copies the disc's `data/movies` into `private/stock/data/movies`).
    pub(crate) fn path(&self, row: &skate_data::movie_rows::MovieRow) -> std::path::PathBuf {
        self.asset_root.join("private/stock/data").join(&row.folder).join(format!("{}_english_ntsc.vp6", row.file))
    }
}

/// Plays the movie row `key` (how every caller starts a movie: retail writes the row key and pushes FE 24).
pub(crate) fn play_row(key: &str, rows: &MovieRows, player: &mut MoviePlayer, commands: &mut Commands, images: &mut Assets<Image>,
                       materials: &mut Assets<MovieMaterial>, sounds: &mut Assets<MovieAudio>) -> Result<(), String> {
    let row = rows.rows.iter().find(|r| r.key == key).ok_or_else(|| format!("no movie row {key}"))?;
    let rules = SkipRules { skippable: row.skippable, skip_after_ms: row.skip_after_ms, back_exits: row.back_exits };
    play(&rows.path(row), rules, row.volume, player, commands, images, materials, sounds)?;
    if let Some(p) = player.playback.as_mut() {
        p.subtitles = Some((row.file.clone(), row.subtitles, row.forced_subtitles));
    }
    Ok(())
}

fn load_rows(mut rows: ResMut<MovieRows>, mut subtitles: ResMut<MovieSubtitles>, config: Res<crate::config::Config>) {
    rows.asset_root = config.asset_root.clone();
    MUTED.store(config.mute, std::sync::atomic::Ordering::Relaxed);
    match skate_data::subtitles::Subtitles::load(&config.asset_root) {
        Ok(s) => subtitles.0 = s,
        Err(e) => warn!("MOVIE subtitles not loaded (Refresh Game Files adds them): {e}"),
    }
    match skate_data::collections::Collections::load(&config.asset_root) {
        Ok(c) => rows.rows = skate_data::movie_rows::movie_rows(&c),
        Err(e) => warn!("MOVIE rows not loaded: {e}"),
    }
    info!("MOVIE rows {}", rows.rows.len());
}

#[derive(Resource, Default)]
pub(crate) struct MoviePlayer {
    playback: Option<Playback>,
}

/// The frame the render world writes into the planes (new `seq` = new frame).
#[derive(Resource, Clone, Default, ExtractResource)]
struct MovieUpload {
    seq: u64,
    frame: Option<Arc<YuvFrame>>,
    planes: Option<[AssetId<Image>; 3]>,
}

impl MoviePlayer {
    pub(crate) fn playing(&self) -> bool {
        self.playback.is_some()
    }
}

/// Starts `path`; the old movie, if any, ends.
#[allow(clippy::too_many_arguments)]
pub(crate) fn play(path: &std::path::Path, rules: SkipRules, volume: f32, player: &mut MoviePlayer, commands: &mut Commands,
                   images: &mut Assets<Image>, materials: &mut Assets<MovieMaterial>, sounds: &mut Assets<MovieAudio>) -> Result<(), String> {
    stop(player, commands);
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let reader = container::Reader::new(std::io::BufReader::new(file)).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let header = reader.header;
    let (w, h) = (u32::from(header.width), u32::from(header.height));
    if w == 0 || h == 0 {
        return Err(format!("{}: no picture size", path.display()));
    }
    let mut plane = |w: u32, h: u32| {
        let mut img = Image::new_fill(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, &[16],
            TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
        img.texture_descriptor.usage |= TextureUsages::COPY_DST;
        img.sampler = ImageSampler::linear();
        images.add(img)
    };
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let planes = [plane(w, h), plane(cw, ch), plane(cw, ch)];
    let material = materials.add(MovieMaterial { luma: planes[0].clone(), u: planes[1].clone(), v: planes[2].clone() });
    let root = commands.spawn((Name::new("movie"), GlobalZIndex(1000), BackgroundColor(Color::BLACK),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center,
            align_items: AlignItems::Center, ..default() }))
        .with_child((MaterialNode(material), Node { width: percent(100), max_height: percent(100), aspect_ratio: Some(w as f32 / h as f32), ..default() }))
        // Fallback when no menu stage runs; otherwise the fmv_popup chyron shows (MovieChyron).
        .with_child((SkipPrompt, Visibility::Hidden, Text::new("Press A to skip"), TextFont { font_size: 22.0, ..default() },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
            Node { position_type: PositionType::Absolute, right: percent(4), bottom: percent(5), ..default() }))
        .id();
    let (tx, rx) = sync_channel(AHEAD);
    let audio = Arc::new(AudioShared::default());
    // The row's volume x MixMap Master out4, followed every frame (`advance`).
    let gain = movie_gain(volume, MASTER_OUT4.load(std::sync::atomic::Ordering::Relaxed));
    let audio_entity = commands.spawn((Name::new("movie audio"), AudioPlayer(sounds.add(MovieAudio { shared: audio.clone() })),
        PlaybackSettings::ONCE.with_volume(bevy::audio::Volume::Linear(gain)))).id();
    let (shared, language) = (audio.clone(), movie_language());
    std::thread::Builder::new().name("movie-decode".into()).spawn(move || decode(reader, tx, shared, language)).map_err(|e| e.to_string())?;
    info!("MOVIE_START {} {}x{} {:.3} fps {} frames gain {gain:.3}", path.display(), w, h, header.fps(), header.frames);
    player.playback = Some(Playback { rx: std::sync::Mutex::new(rx), fps: header.fps(), started: None, pending: None, done: false, root, planes, shown: 0, dropped: 0, audio, audio_entity,
        rules, volume, elapsed_ms: 0.0, prompt: false, prev_buttons: 0, subtitles: None, subtitles_decided: false, forced_subtitles: false,
        subtitler: None });
    Ok(())
}

pub(crate) fn stop(player: &mut MoviePlayer, commands: &mut Commands) {
    if let Some(p) = player.playback.take() {
        commands.entity(p.root).despawn();
        p.audio.ended.store(true, std::sync::atomic::Ordering::Relaxed);
        commands.entity(p.audio_entity).despawn();
        let played = p.audio.played_frames.load(std::sync::atomic::Ordering::Relaxed);
        info!("MOVIE_END shown {} dropped {} audio {} frames ({:.2} s)", p.shown, p.dropped, played, played as f64 / f64::from(AUDIO_RATE));
    }
}

fn decode<R: std::io::Read + std::io::Seek>(mut reader: container::Reader<R>, tx: SyncSender<Decoded>, audio: Arc<AudioShared>, language: [u8; 2]) {
    use std::sync::atomic::Ordering::Relaxed;
    let mut dec = Vp6Decoder::new();
    let mut xa = [skate_movie::audio::XaChannel::default(); 6];
    let mut index = 0u32;
    while let Ok(Some(chunk)) = reader.next_chunk() {
        match chunk {
            container::Chunk::Video { data, .. } => {
                if let Ok(frame) = dec.decode(&data) {
                    if tx.send(Decoded::Frame(index, Arc::new(frame))).is_err() {
                        break;
                    }
                }
                index += 1;
            }
            container::Chunk::Audio { language: l, data } if l == language => {
                if let Some(samples) = skate_movie::audio::decode_block(&data, &mut xa) {
                    audio.has_audio.store(true, Relaxed);
                    if let Ok(mut q) = audio.queue.lock() {
                        fold_stereo(&samples, &mut q);
                    }
                }
            }
            _ => {}
        }
    }
    let _ = tx.send(Decoded::Done);
}

/// The frame due at `elapsed_s` (0-based): retail's frame pick, floor(elapsed ms x fps x 0.001).
pub(crate) fn due_frame(elapsed_s: f64, fps: f64) -> u32 {
    (elapsed_s * 1000.0 * fps * 0.001).floor().max(0.0) as u32
}

/// A row's skip rules (FE 24 input 8260B240 + can_skip 825E0680, b898 §2 / b955 §1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SkipRules {
    pub skippable: bool,
    pub skip_after_ms: f32,
    pub back_exits: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkipStep {
    None,
    ShowPrompt,
    HidePrompt,
    Leave,
}

/// One input step: A or Start first shows the prompt, A while it shows skips; B hides it, and leaves only on rows
/// with back_exits (credits). Nothing happens before the row's skip delay or on a row that is not skippable, except
/// B on a back_exits row.
pub(crate) fn skip_step(rules: SkipRules, elapsed_ms: f32, prompt: bool, a: bool, start: bool, b: bool) -> SkipStep {
    if b {
        return if rules.back_exits { SkipStep::Leave } else if prompt { SkipStep::HidePrompt } else { SkipStep::None };
    }
    if !(rules.skippable && elapsed_ms > rules.skip_after_ms) {
        return SkipStep::None;
    }
    match (a, start, prompt) {
        (true, _, true) => SkipStep::Leave,
        (true, _, false) | (_, true, false) => SkipStep::ShowPrompt,
        _ => SkipStep::None,
    }
}

/// The skip chyron (fmv_popup on `_level3`: `ShowMovieSkip` / `HideMovieSkip`, text ID_MOVIESKIP). The menu stage
/// applies `want` and sets `live` on every frame it runs with the functions defined; otherwise the movie shows its own
/// label.
#[derive(Resource, Default)]
pub(crate) struct MovieChyron {
    pub want: bool,
    pub applied: bool,
    pub live: bool,
}

/// The stage marks itself live each frame it runs.
fn reset_chyron_live(mut c: ResMut<MovieChyron>) {
    c.live = false;
}

/// While a movie plays, the APT composite (HUD, menus, fmv_popup) draws above the video so the chyron shows; the
/// gameplay HUD hides meanwhile (scoring_hud `render`).
fn raise_apt(player: Res<MoviePlayer>, mut nodes: Query<&mut GlobalZIndex, With<crate::scoring_hud::AptComposite>>) {
    let z = if player.playing() || crate::career::boot_flow_covers() { 1001 } else { 1 };
    for mut node in &mut nodes {
        if node.0 != z {
            node.0 = z;
        }
    }
}

/// The black layer behind a new career's boot flow screens (retail runs them before the world loads): between the
/// world and the raised APT composite, while the flow covers the screen and no movie (which has its own black) plays.
#[derive(Component)]
struct BootBackdrop;

/// Retail runs the boot flow before the world loads, so nothing simulates under its screens: ours holds the world
/// step (a named hold on the pause manager, beside the mods' holds) while the flow covers the screen.
const BOOT_FLOW_HOLD: &str = "engine:boot_flow";

fn boot_backdrop(mut commands: Commands, player: Res<MoviePlayer>, backdrops: Query<Entity, With<BootBackdrop>>,
    pause: Option<ResMut<crate::pause_man::ThePauseMan>>) {
    let covers = crate::career::boot_flow_covers();
    if let Some(mut p) = pause {
        if p.mods.contains(BOOT_FLOW_HOLD) != covers {
            p.hold_for_mod(BOOT_FLOW_HOLD, covers);
            info!("BOOT_FLOW world step {}", if covers { "held" } else { "released" });
        }
    }
    let want = covers && !player.playing();
    match (want, backdrops.iter().next()) {
        (true, None) => {
            commands.spawn((BootBackdrop, Name::new("boot flow backdrop"), GlobalZIndex(999), BackgroundColor(Color::BLACK),
                Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }));
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
}

const PAD_A: u16 = 0x1000;
const PAD_B: u16 = 0x2000;
const PAD_START: u16 = 0x0010;

#[derive(Component)]
struct SkipPrompt;

fn advance(mut player: ResMut<MoviePlayer>, mut upload: ResMut<MovieUpload>, mut commands: Commands, keys: Res<ButtonInput<KeyCode>>,
           pads: Option<Res<crate::input::ControllerInput>>, time: Res<Time>, mut prompts: Query<&mut Visibility, With<SkipPrompt>>,
           mut sinks: Query<&mut bevy::audio::AudioSink>, mut chyron: ResMut<MovieChyron>,
           (menus, cue_files, mut line): (Option<Res<crate::retail_menus::RetailMenus>>, Res<MovieSubtitles>, ResMut<MovieSubtitle>)) {
    // Game Settings > Subtitles (option byte +157, retail default off).
    let option = menus.as_deref().and_then(|m| crate::retail_menus::setting(m, "ID_GAMESETTINGS_SUBTITLE_TOGGLE"))
        .is_some_and(|v| skate_core::menu_values::number(&v) != 0.0);
    let Some(p) = player.playback.as_mut() else {
        chyron.want = false;
        line.show = option;
        line.set(None);
        return;
    };
    if let Ok(mut sink) = sinks.get_mut(p.audio_entity) {
        use bevy::audio::AudioSinkPlayback;
        sink.set_volume(bevy::audio::Volume::Linear(movie_gain(p.volume, MASTER_OUT4.load(std::sync::atomic::Ordering::Relaxed))));
    }
    // Retail counts FE updates (dt / 100 ms ticks), not movie time (b931 §2).
    p.elapsed_ms += time.delta_secs() * 1000.0;
    // Subtitles (825E0790 at the start): forced on `2914` rows in French / German; a Subtitler only on `A3A0` rows
    // while shown (option or forced), its cues on FE time.
    // A movie started at boot waits for the settings (the menu tables load a few frames later).
    if !p.subtitles_decided && menus.as_deref().is_none_or(|m| m.loaded) {
        p.subtitles_decided = true;
        let (folder, forcing_language) = subtitle_language(movie_language());
        if let Some((file, allowed, forced)) = &p.subtitles {
            p.forced_subtitles = *forced && forcing_language;
            let cues = cue_files.0.cues(folder, file);
            if *allowed && (option || p.forced_subtitles) {
                p.subtitler = cues.map(skate_data::subtitles::Subtitler::new);
            }
            info!("MOVIE_SUBTITLES {file}: row {allowed} option {option} forced {} cues {} running {}", p.forced_subtitles,
                cues.map_or(0, <[_]>::len), p.subtitler.is_some());
        }
    }
    line.show = option || p.forced_subtitles;
    if let Some(cue) = p.subtitler.as_mut().and_then(|s| s.due(p.elapsed_ms)) {
        info!("MOVIE_SUBTITLE {:.1} s {}", p.elapsed_ms / 1000.0, cue.as_deref().unwrap_or("(clear)"));
        line.set(cue);
    }
    let buttons = pads.map_or(0, |c| c.raw_input().buttons);
    let pressed = |bit: u16| buttons & bit != 0 && p.prev_buttons & bit == 0;
    let (a, start, b) = (pressed(PAD_A) || keys.just_pressed(KeyCode::Enter), pressed(PAD_START), pressed(PAD_B) || keys.just_pressed(KeyCode::Escape));
    p.prev_buttons = buttons;
    let step = skip_step(p.rules, p.elapsed_ms, p.prompt, a, start, b);
    if matches!(step, SkipStep::ShowPrompt | SkipStep::HidePrompt) {
        p.prompt = step == SkipStep::ShowPrompt;
    }
    // Test aid (no simulated input): SKATE_MOVIE_PROMPT=1 shows the prompt from the start; =<seconds> (above 1) shows
    // it until then and hides it (as B does).
    match std::env::var("SKATE_MOVIE_PROMPT").ok().and_then(|v| v.parse::<f32>().ok()) {
        Some(s) if s == 1.0 => p.prompt = true,
        Some(s) if s > 1.0 => p.prompt = p.elapsed_ms < s * 1000.0,
        _ => {}
    }
    chyron.want = p.prompt;
    // Retail's chyron through the menu stage; our label only when no stage runs.
    let label = p.prompt && !chyron.live;
    for mut v in &mut prompts {
        let want = if label { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if step == SkipStep::Leave || next_frame(&mut *player, &mut *upload) {
        stop(&mut *player, &mut commands);
        upload.frame = None;
    }
}

/// Moves to the newest due frame; true when the movie has ended.
fn next_frame(player: &mut MoviePlayer, upload: &mut MovieUpload) -> bool {
    let Some(p) = player.playback.as_mut() else { return false };
    // The audio clock while the movie has sound (the frames the device played), else the wall clock.
    let played = p.audio.played_frames.load(std::sync::atomic::Ordering::Relaxed);
    let due = if p.audio.has_audio.load(std::sync::atomic::Ordering::Relaxed) && played > 0 {
        due_frame(played as f64 / f64::from(AUDIO_RATE), p.fps)
    } else {
        p.started.map_or(0, |t| due_frame(t.elapsed().as_secs_f64(), p.fps))
    };
    let mut newest: Option<Arc<YuvFrame>> = None;
    loop {
        if p.pending.is_none() && !p.done {
            let rx = p.rx.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
            match rx.try_recv() {
                Ok(Decoded::Frame(i, f)) => p.pending = Some((i, f)),
                Ok(Decoded::Done) | Err(TryRecvError::Disconnected) => p.done = true,
                Err(TryRecvError::Empty) => {}
            }
        }
        match &p.pending {
            Some((i, _)) if *i <= due => {
                if newest.is_some() {
                    p.dropped += 1;
                }
                newest = p.pending.take().map(|(_, f)| f);
            }
            _ => break,
        }
    }
    if let Some(f) = newest {
        p.started.get_or_insert_with(Instant::now);
        p.shown += 1;
        upload.seq += 1;
        upload.frame = Some(f);
        upload.planes = Some([p.planes[0].id(), p.planes[1].id(), p.planes[2].id()]);
        return false;
    }
    p.done && p.pending.is_none()
}

fn write_planes(upload: Res<MovieUpload>, images: Res<RenderAssets<GpuImage>>, queue: Res<RenderQueue>, mut last: Local<u64>) {
    if upload.seq == *last {
        return;
    }
    let (Some(frame), Some(planes)) = (&upload.frame, upload.planes) else { return };
    for (p, id) in planes.iter().enumerate() {
        let Some(gpu) = images.get(*id) else { return };
        let (w, h) = frame.plane_size(p);
        let size = gpu.texture.size();
        if size.width as usize != w || size.height as usize != h {
            return;
        }
        queue.write_texture(gpu.texture.as_image_copy(), &frame.planes[p],
            TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w as u32), rows_per_image: Some(h as u32) }, size);
    }
    *last = upload.seq;
}

/// Boot (retail 826FDEF0): the `ea_intro` row plays while the loader runs, over the loading screen; part of the retail
/// startup, so behind the Career and Challenges opt-in for now (PR 68). Not in automated test runs
/// (`SKATE_BACKGROUND_WINDOW=1`) or with `SKATE_BOOT_MOVIE=0`; `SKATE_MOVIE` replaces it.
fn boot_movie(living: Option<Res<crate::living_world::LivingWorldSettings>>, mut player: ResMut<MoviePlayer>, rows: Res<MovieRows>,
              mut commands: Commands, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<MovieMaterial>>,
              mut sounds: ResMut<Assets<MovieAudio>>) {
    let off = |k: &str, v: &str| std::env::var(k).is_ok_and(|x| x == v);
    if player.playing() || std::env::var("SKATE_MOVIE").is_ok() || off("SKATE_BACKGROUND_WINDOW", "1") || off("SKATE_BOOT_MOVIE", "0") {
        return;
    }
    if !living.is_some_and(|l| l.user_opt_ins.challenges) {
        return;
    }
    if let Err(e) = play_row("ea_intro", &rows, &mut player, &mut commands, &mut images, &mut materials, &mut sounds) {
        warn!("MOVIE boot: {e}");
    }
}

/// `SKATE_MOVIE=<row key>` (e.g. intro_movie) or a `.vp6` path (played as a skippable row with the default delay).
fn dev_start(mut player: ResMut<MoviePlayer>, rows: Res<MovieRows>, mut commands: Commands, mut images: ResMut<Assets<Image>>,
             mut materials: ResMut<Assets<MovieMaterial>>, mut sounds: ResMut<Assets<MovieAudio>>) {
    let Ok(what) = std::env::var("SKATE_MOVIE") else { return };
    let result = if what.to_ascii_lowercase().ends_with(".vp6") {
        let rules = SkipRules { skippable: true, skip_after_ms: 5000.0, back_exits: false };
        play(std::path::Path::new(&what), rules, 1.0, &mut player, &mut commands, &mut images, &mut materials, &mut sounds)
    } else {
        play_row(&what, &rows, &mut player, &mut commands, &mut images, &mut materials, &mut sounds)
    };
    if let Err(e) = result {
        warn!("SKATE_MOVIE {e}");
    }
}

pub(crate) struct MoviePlayerPlugin;

impl Plugin for MoviePlayerPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "movie.wgsl");
        app.add_plugins(UiMaterialPlugin::<MovieMaterial>::default())
            .add_plugins(ExtractResourcePlugin::<MovieUpload>::default())
            .add_audio_source::<MovieAudio>()
            .init_resource::<MoviePlayer>()
            .init_resource::<MovieUpload>()
            .init_resource::<MovieRows>()
            .init_resource::<MovieChyron>()
            .init_resource::<MovieSubtitles>()
            .init_resource::<MovieSubtitle>()
            .add_systems(PostUpdate, (raise_apt, boot_backdrop))
            .add_systems(First, (reset_chyron_live, publish_playing))
            .add_systems(Startup, (load_rows, dev_start).chain())
            .add_systems(PostStartup, boot_movie)
            // After the menu stage: it marks the chyron live in the same frame.
            .add_systems(Update, advance.after(crate::retail_menu_stage::MenuStageSet).after(crate::retail_menus::sync_stored_values));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            // As video_screens' copy: after the frame's render, so the new frame shows on the next one.
            render_app.add_systems(Render, write_planes.after(RenderSystems::Render).before(RenderSystems::Cleanup));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_movie_gain_is_the_row_volume_times_master_out4() {
        assert!((movie_gain(0.8, 32767) - 0.8).abs() < 1e-6, "default out4 = 1.0");
        assert!((movie_gain(0.6, 16384) - 0.3).abs() < 1e-4);
        assert_eq!(movie_gain(0.8, 0), 0.0);
        assert!((movie_gain(1.0, 40000) - 1.0).abs() < 1e-6, "out4 clamps at 32767");
    }

    #[test]
    fn retail_skip_rules() {
        let skippable = SkipRules { skippable: true, skip_after_ms: 5000.0, back_exits: false };
        // Before the delay nothing happens; after it A shows the prompt, A again leaves.
        assert_eq!(skip_step(skippable, 4000.0, false, true, false, false), SkipStep::None);
        assert_eq!(skip_step(skippable, 6000.0, false, true, false, false), SkipStep::ShowPrompt);
        assert_eq!(skip_step(skippable, 6000.0, true, true, false, false), SkipStep::Leave);
        // Start only shows it; B hides it without leaving.
        assert_eq!(skip_step(skippable, 6000.0, false, false, true, false), SkipStep::ShowPrompt);
        assert_eq!(skip_step(skippable, 6000.0, true, false, true, false), SkipStep::None);
        assert_eq!(skip_step(skippable, 6000.0, true, false, false, true), SkipStep::HidePrompt);
        // ea_intro (82D9 = 0): never skippable.
        let locked = SkipRules { skippable: false, ..skippable };
        assert_eq!(skip_step(locked, 60_000.0, false, true, false, false), SkipStep::None);
        assert_eq!(skip_step(locked, 60_000.0, true, true, false, false), SkipStep::None);
        // credits (9A07 = 1): B leaves at once.
        let credits = SkipRules { back_exits: true, ..skippable };
        assert_eq!(skip_step(credits, 0.0, false, false, false, true), SkipStep::Leave);
    }

    #[test]
    fn six_channels_fold_to_stereo_with_the_retail_table() {
        // L, C, R, Ls, Rs, LFE: left takes 0.707 L + 0.5 C + 0.5 Ls, right mirrors, LFE never heard.
        let mut out = std::collections::VecDeque::new();
        fold_stereo(&[32767, 0, 0, 0, 0, 32767, 0, 32767, 0, 0, 0, 0, 0, 0, 0, 32767, 32767, 0], &mut out);
        let v: Vec<f32> = out.into_iter().collect();
        assert!((v[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6 && v[1] == 0.0, "left only: {v:?}");
        assert!((v[2] - 0.5).abs() < 1e-6 && (v[3] - 0.5).abs() < 1e-6, "center to both");
        assert!((v[4] - 0.5).abs() < 1e-6 && (v[5] - 0.5).abs() < 1e-6, "Ls left, Rs right");
    }

    #[test]
    fn the_frame_clock_picks_floor_of_elapsed_times_fps() {
        assert_eq!(due_frame(0.0, 29.9699), 0);
        assert_eq!(due_frame(1.0, 29.9699), 29);
        assert_eq!(due_frame(1.0, 60.0), 60);
        assert_eq!(due_frame(10.0, 29.9699), 299);
    }
}
