//! The loading screen: the skating crab, a skateboard rolling under it and what is loading. It is
//! up while a map loads (the start map on the Switch, `world::spawn`, and every map change) and
//! then until that world is on the GPU, so nothing pops in once it goes: every mesh, image and
//! storage buffer the scene published is ready in the render world (meshes bevy_horizon holds
//! back for later frames aren't), and no shader pipeline is still compiling, for a few frames in
//! a row (findings 59).
//!
//! Once a world is committed it runs behind the screen, so its first frames render and their
//! pipelines compile, but the player's input doesn't reach it (`input::publish_actions`). The
//! render-side check runs only while the screen waits for a world.
//!
//! The board is Hails's side-view skateboard from the Windows start-up splash on her
//! `world/living-world` branch (splash.rs `draw_board`): the same shapes and colours, drawn here
//! without GDI so it works on the Switch.
use bevy::{
    asset::{AssetId, RenderAssetUsages},
    image::ImageSampler,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        mesh::RenderMesh,
        render_asset::RenderAssets,
        render_resource::{Extent3d, PipelineCache, TextureDimension, TextureFormat},
        storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
        texture::GpuImage,
    },
    window::PrimaryWindow,
};
use std::sync::{Arc, Mutex, PoisonError, atomic::{AtomicU64, Ordering}};
use std::time::{Duration, Instant};

/// Frames in a row the world must be all on the GPU before the screen goes: a mesh that has just
/// arrived asks for its pipelines a frame or two later.
const READY_FRAMES: u8 = 5;
/// The longest the screen waits for a published world before it gives way.
const MAX_WAIT: Duration = Duration::from_secs(90);

const BACKGROUND: Color = Color::srgb_u8(14, 14, 17);
const TEXT: Color = Color::srgb_u8(217, 217, 217);
/// The crab's height, the board's, and the time the board takes to cross the screen.
const CRAB_VH: f32 = 40.;
const BOARD_VH: f32 = 9.;
const ROLL_SECONDS: f32 = 3.;

pub(crate) struct LoadingScreenPlugin;

impl Plugin for LoadingScreenPlugin {
    fn build(&self, app: &mut App) {
        // A `--verify` capture has to show the world, not this.
        let enabled = app.world().resource::<crate::config::Config>().verification_capture.is_none();
        let readiness = Readiness::default();
        app.insert_resource(LoadingScreen::new(enabled, readiness.clone()))
            .add_systems(Startup, spawn_screen)
            .add_systems(PreUpdate, update_screen.after(crate::map_transition::MapTransitionSet))
            .add_systems(Update, roll_board);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            // After `RenderSystems::Render`, where the pipeline cache works through its queue.
            render.insert_resource(readiness).add_systems(
                Render,
                check_world.after(RenderSystems::Render).before(RenderSystems::Cleanup),
            );
        }
    }
}

/// The world the screen waits for, and what the render world last found of it.
#[derive(Resource, Clone, Default)]
struct Readiness(Arc<Shared>);

#[derive(Default)]
struct Shared {
    wanted: Mutex<Option<Wanted>>,
    /// The token of the wanted world once the render world found all of it on the GPU, else 0.
    ready: AtomicU64,
    /// What it was still waiting for, for the log.
    missing: Mutex<Missing>,
}

struct Wanted {
    token: u64,
    meshes: Vec<AssetId<Mesh>>,
    images: Vec<AssetId<Image>>,
    buffers: Vec<AssetId<ShaderStorageBuffer>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Missing {
    meshes: usize,
    images: usize,
    buffers: usize,
    pipelines: usize,
}

#[derive(Resource)]
pub(crate) struct LoadingScreen {
    enabled: bool,
    shown: bool,
    shown_at: Option<Instant>,
    /// The published world the screen waits for: its token and when it was published.
    waiting: Option<(u64, Instant)>,
    ready_frames: u8,
    last_token: u64,
    readiness: Readiness,
}

/// What a frame changed, for the log.
#[derive(Debug, PartialEq)]
enum Change {
    None,
    Up,
    /// The world was ready this long after its publish.
    Ready(Duration),
    /// It still wasn't, this long after.
    GaveUp(Duration),
    /// The load ended with no world to wait for: it failed.
    Down,
}

impl LoadingScreen {
    fn new(enabled: bool, readiness: Readiness) -> Self {
        Self { enabled, shown: false, shown_at: None, waiting: None, ready_frames: 0, last_token: 0, readiness }
    }

    pub(crate) fn shown(&self) -> bool {
        self.shown
    }

    /// A world was published; returns the token it waits on, 0 when the screen is off.
    fn wait_for(&mut self, now: Instant) -> u64 {
        if !self.enabled {
            return 0;
        }
        self.last_token += 1;
        self.waiting = Some((self.last_token, now));
        self.ready_frames = 0;
        self.last_token
    }

    /// One frame: `busy` while a map loads, `ready` the token the render world last found all on
    /// the GPU.
    fn advance(&mut self, busy: bool, ready: u64, now: Instant) -> Change {
        let mut change = Change::None;
        if let Some((token, published)) = self.waiting {
            self.ready_frames = if ready == token { self.ready_frames.saturating_add(1) } else { 0 };
            let waited = now.saturating_duration_since(published);
            if self.ready_frames >= READY_FRAMES {
                self.waiting = None;
                change = Change::Ready(waited);
            } else if waited > MAX_WAIT {
                self.waiting = None;
                change = Change::GaveUp(waited);
            }
        }
        let shown = self.enabled && (busy || self.waiting.is_some());
        if shown && !self.shown {
            self.shown_at = Some(now);
            change = Change::Up;
        } else if !shown && self.shown && change == Change::None {
            change = Change::Down;
        }
        self.shown = shown;
        change
    }
}

/// The scene just published `meshes`, `images` and `buffers` (`PreparedScene::publish`): the
/// screen stays up until they are on the GPU. Does nothing without the screen (bare-world tests).
pub(crate) fn await_world(
    world: &mut World,
    meshes: Vec<AssetId<Mesh>>,
    images: Vec<AssetId<Image>>,
    buffers: Vec<AssetId<ShaderStorageBuffer>>,
) {
    let Some(mut screen) = world.get_resource_mut::<LoadingScreen>() else { return };
    let token = screen.wait_for(Instant::now());
    if token == 0 {
        return;
    }
    let shared = &screen.readiness.0;
    shared.ready.store(0, Ordering::Relaxed);
    *shared.wanted.lock().unwrap_or_else(PoisonError::into_inner) = Some(Wanted { token, meshes, images, buffers });
}

/// Render world: whether the wanted world is all there, every frame while the screen waits.
fn check_world(
    readiness: Res<Readiness>,
    meshes: Option<Res<RenderAssets<RenderMesh>>>,
    images: Option<Res<RenderAssets<GpuImage>>>,
    buffers: Option<Res<RenderAssets<GpuShaderStorageBuffer>>>,
    pipelines: Res<PipelineCache>,
) {
    let wanted = readiness.0.wanted.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(wanted) = wanted.as_ref() else { return };
    let missing = Missing {
        meshes: meshes.map_or(wanted.meshes.len(), |ready| wanted.meshes.iter().filter(|&&id| ready.get(id).is_none()).count()),
        images: images.map_or(wanted.images.len(), |ready| wanted.images.iter().filter(|&&id| ready.get(id).is_none()).count()),
        buffers: buffers.map_or(wanted.buffers.len(), |ready| wanted.buffers.iter().filter(|&&id| ready.get(id).is_none()).count()),
        pipelines: pipelines.waiting_pipelines().count(),
    };
    let ready = if missing == Missing::default() { wanted.token } else { 0 };
    readiness.0.ready.store(ready, Ordering::Relaxed);
    *readiness.0.missing.lock().unwrap_or_else(PoisonError::into_inner) = missing;
}

#[derive(Component)]
struct ScreenRoot;
#[derive(Component)]
struct Board;
#[derive(Component)]
struct Status;

fn spawn_screen(mut commands: Commands, mut images: ResMut<Assets<Image>>, screen: Res<LoadingScreen>) {
    if !screen.enabled {
        return;
    }
    let crab = crab_image()
        .inspect_err(|error| warn!("Loading screen: the crab didn't decode: {error}"))
        .ok()
        .map(|image| {
            let aspect = image.width() as f32 / image.height() as f32;
            (images.add(image), aspect)
        });
    let board = images.add(board_image());
    let board_aspect = (BOARD_LENGTH + 2. * BOARD_PAD) / BOARD_HEIGHT;
    commands
        .spawn((
            ScreenRoot,
            // Over the menu, the HUD and the frame-time counter.
            GlobalZIndex(1000),
            Visibility::Hidden,
            bevy::ui::FocusPolicy::Block,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.),
                height: Val::Percent(100.),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Vh(3.),
                ..default()
            },
            BackgroundColor(BACKGROUND),
        ))
        .with_children(|root| {
            if let Some((crab, aspect)) = crab {
                root.spawn((
                    ImageNode::new(crab),
                    Node { height: Val::Vh(CRAB_VH), width: Val::Vh(CRAB_VH * aspect), ..default() },
                ));
            }
            root.spawn(Node {
                width: Val::Percent(100.),
                height: Val::Vh(BOARD_VH),
                overflow: Overflow::clip_x(),
                ..default()
            })
            .with_children(|track| {
                track.spawn((
                    Board,
                    ImageNode::new(board),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(-100.),
                        height: Val::Vh(BOARD_VH),
                        width: Val::Vh(BOARD_VH * board_aspect),
                        ..default()
                    },
                ));
            });
            root.spawn((Status, Text::new(""), TextFont { font_size: 18., ..default() }, TextColor(TEXT)));
        });
}

fn update_screen(
    mut screen: ResMut<LoadingScreen>,
    transition: Res<crate::map_transition::MapTransition>,
    current: Res<crate::map_transition::CurrentMap>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut roots: Query<&mut Visibility, With<ScreenRoot>>,
    mut status: Query<(&mut Text, &mut TextFont), With<Status>>,
) {
    let now = Instant::now();
    let ready = screen.readiness.0.ready.load(Ordering::Relaxed);
    let waiting = screen.waiting;
    let shown_for = screen.shown_at.map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
    match screen.advance(transition.busy(), ready, now) {
        Change::None => {}
        Change::Up => info!("LOADING_SCREEN up: {}", if transition.busy() { transition.label() } else { format!("{} published", current.name) }),
        Change::Ready(waited) => info!(
            "LOADING_SCREEN {} on the GPU {:.1} s after its publish; the screen was up {:.1} s",
            current.name, waited.as_secs_f32(), shown_for.as_secs_f32(),
        ),
        Change::GaveUp(waited) => {
            let missing = *screen.readiness.0.missing.lock().unwrap_or_else(PoisonError::into_inner);
            warn!("LOADING_SCREEN gave up waiting for {} after {} s, still not on the GPU: {missing:?}", current.name, waited.as_secs());
        }
        Change::Down => info!("LOADING_SCREEN down: the load ended without a world to wait for"),
    }
    // Done waiting: the render world stops checking.
    if let Some((token, _)) = waiting.filter(|_| screen.waiting.is_none()) {
        let mut wanted = screen.readiness.0.wanted.lock().unwrap_or_else(PoisonError::into_inner);
        if wanted.as_ref().is_some_and(|w| w.token == token) {
            *wanted = None;
        }
    }
    let visibility = if screen.shown { Visibility::Inherited } else { Visibility::Hidden };
    for mut root in &mut roots {
        root.set_if_neq(visibility);
    }
    if !screen.shown {
        return;
    }
    let text = if transition.busy() {
        transition.label()
    } else {
        format!("Loading {} — uploading textures and shaders…", current.name)
    };
    let font_size = windows.single().map_or(18., |w| (w.height() * 0.024).max(12.));
    for (mut status, mut font) in &mut status {
        if status.0 != text {
            status.0.clone_from(&text);
        }
        if font.font_size != font_size {
            font.font_size = font_size;
        }
    }
}

/// The board rolls left to right across the screen and comes round again.
fn roll_board(
    screen: Res<LoadingScreen>,
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut boards: Query<&mut Node, With<Board>>,
) {
    if !screen.shown() {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let board = BOARD_VH / 100. * window.height() * (BOARD_LENGTH + 2. * BOARD_PAD) / BOARD_HEIGHT;
    let track = window.width() + board;
    let x = (time.elapsed_secs() * track / ROLL_SECONDS) % track - board;
    for mut node in &mut boards {
        node.left = Val::Px(x);
    }
}

/// The skating crab (docs/images), decoded and scaled down to the rows it is drawn at: the Switch
/// build's Bevy has no PNG decoder, and the 920-row original drawn at 40% of a 720-row screen
/// would alias without mips.
const CRAB_PNG: &[u8] = include_bytes!("../../../docs/images/skating-crab.png");
const CRAB_HEIGHT: u32 = 480;

fn crab_image() -> Result<Image, String> {
    let mut crab = ::image::load_from_memory_with_format(CRAB_PNG, ::image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let width = (u64::from(crab.width()) * u64::from(CRAB_HEIGHT) / u64::from(crab.height())) as u32;
    // Scaled with premultiplied alpha, so the transparent surround doesn't fringe the edge dark.
    for pixel in crab.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let mut scaled = ::image::imageops::resize(&crab, width, CRAB_HEIGHT, ::image::imageops::FilterType::CatmullRom);
    for pixel in scaled.pixels_mut() {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 {
            for channel in &mut pixel.0[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Ok(rgba_image(width, CRAB_HEIGHT, scaled.into_raw()))
}

fn rgba_image(width: u32, height: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        // The UI lays the node out from the main world's copy.
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

/// The board's length and the height of its box, in the splash's pixels, and the margin either
/// side of it.
const BOARD_LENGTH: f32 = 180.;
const BOARD_HEIGHT: f32 = 40.;
const BOARD_PAD: f32 = 6.;
/// Image pixels per splash pixel, and samples per image pixel along each axis.
const BOARD_SCALE: usize = 2;
const SUPERSAMPLE: usize = 4;

/// The board's colours (Hails's splash.rs), as palette indices; 0 is clear.
const SHADOW: u8 = 1;
const PLY_EDGE: u8 = 2;
const PLY: u8 = 3;
const PLY_DARK: u8 = 4;
const GRIP: u8 = 5;
const METAL: u8 = 6;
const METAL_DARK: u8 = 7;
const WHEEL: u8 = 8;
const WHEEL_EDGE: u8 = 9;
const WHEEL_CORE: u8 = 10;
const BEARING: u8 = 11;
const PALETTE: [[u8; 4]; 12] = [
    [0, 0, 0, 0],
    // The splash's (7, 7, 9) on its background, which is this screen's.
    [0, 0, 0, 128],
    [92, 62, 36, 255],
    [214, 176, 124, 255],
    [150, 104, 62, 255],
    [34, 34, 38, 255],
    [176, 181, 191, 255],
    [96, 100, 110, 255],
    [240, 235, 220, 255],
    [168, 162, 148, 255],
    [86, 146, 214, 255],
    [52, 54, 60, 255],
];

/// Palette indices at `SUPERSAMPLE` samples per image pixel, painted shape over shape.
struct Canvas {
    width: usize,
    height: usize,
    /// Samples per splash pixel.
    scale: f32,
    samples: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        let per = BOARD_SCALE * SUPERSAMPLE;
        let width = (BOARD_LENGTH + 2. * BOARD_PAD) as usize * per;
        let height = BOARD_HEIGHT as usize * per;
        Self { width, height, scale: per as f32, samples: vec![0; width * height] }
    }

    /// A sample column's centre, in splash pixels.
    fn x_of(&self, column: usize) -> f32 {
        (column as f32 + 0.5) / self.scale - BOARD_PAD
    }
    /// The first column whose centre is at `x` or right of it.
    fn column_from(&self, x: f32) -> usize {
        ((x + BOARD_PAD) * self.scale - 0.5).ceil().clamp(0., self.width as f32) as usize
    }
    fn y_of(&self, row: usize) -> f32 {
        (row as f32 + 0.5) / self.scale
    }
    fn rows(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let first = (top * self.scale - 0.5).ceil().clamp(0., self.height as f32) as usize;
        let last = (bottom * self.scale - 0.5).ceil().clamp(0., self.height as f32) as usize;
        first..last
    }

    fn span(&mut self, row: usize, from: f32, to: f32, colour: u8) {
        let (start, end) = (self.column_from(from), self.column_from(to));
        if start < end {
            self.samples[row * self.width + start..row * self.width + end].fill(colour);
        }
    }

    /// A closed polygon, even-odd.
    fn polygon(&mut self, points: &[(f32, f32)], colour: u8) {
        let top = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let bottom = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        let mut crossings = Vec::new();
        for row in self.rows(top, bottom) {
            let y = self.y_of(row);
            crossings.clear();
            for (i, &(x0, y0)) in points.iter().enumerate() {
                let (x1, y1) = points[(i + 1) % points.len()];
                if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
                    crossings.push(x0 + (y - y0) * (x1 - x0) / (y1 - y0));
                }
            }
            crossings.sort_by(f32::total_cmp);
            for pair in crossings.chunks_exact(2) {
                self.span(row, pair[0], pair[1], colour);
            }
        }
    }

    fn ellipse(&mut self, (cx, cy): (f32, f32), (rx, ry): (f32, f32), colour: u8) {
        for row in self.rows(cy - ry, cy + ry) {
            let dy = (self.y_of(row) - cy) / ry;
            if dy.abs() <= 1. {
                let half = rx * (1. - dy * dy).sqrt();
                self.span(row, cx - half, cx + half, colour);
            }
        }
    }

    /// Paints the samples within `reach` of a boundary inside `bounds`, `distance` giving a
    /// sample's signed distance to it.
    fn band(&mut self, (left, top, right, bottom): (f32, f32, f32, f32), reach: f32, distance: impl Fn(f32, f32) -> f32, colour: u8) {
        let columns = self.column_from(left - reach)..self.column_from(right + reach);
        for row in self.rows(top - reach, bottom + reach) {
            let y = self.y_of(row);
            for column in columns.clone() {
                if distance(self.x_of(column), y).abs() <= reach {
                    self.samples[row * self.width + column] = colour;
                }
            }
        }
    }

    /// The outline of a closed polygon, `width` wide and centred on its edges, as GDI's pen draws it.
    fn stroke(&mut self, points: &[(f32, f32)], width: f32, colour: u8) {
        for (i, &a) in points.iter().enumerate() {
            let b = points[(i + 1) % points.len()];
            let bounds = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
            self.band(bounds, width / 2., |x, y| segment_distance((x, y), a, b), colour);
        }
    }

    fn ring(&mut self, (cx, cy): (f32, f32), r: f32, width: f32, colour: u8) {
        let bounds = (cx - r, cy - r, cx + r, cy + r);
        self.band(bounds, width / 2., |x, y| (x - cx).hypot(y - cy) - r, colour);
    }

    /// Averages each pixel's samples in linear light.
    fn into_image(self) -> Image {
        let per = SUPERSAMPLE;
        let (width, height) = (self.width / per, self.height / per);
        let linear = PALETTE.map(|[r, g, b, a]| [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), f32::from(a) / 255.]);
        let mut data = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0f32; 4];
                for sy in 0..per {
                    for sx in 0..per {
                        let [r, g, b, a] = linear[usize::from(self.samples[(y * per + sy) * self.width + x * per + sx])];
                        sum = [sum[0] + r * a, sum[1] + g * a, sum[2] + b * a, sum[3] + a];
                    }
                }
                let alpha = sum[3] / (per * per) as f32;
                if sum[3] > 0. {
                    data.extend([linear_to_srgb(sum[0] / sum[3]), linear_to_srgb(sum[1] / sum[3]), linear_to_srgb(sum[2] / sum[3])]);
                } else {
                    data.extend([0, 0, 0]);
                }
                data.push((alpha * 255.).round() as u8);
            }
        }
        rgba_image(width as u32, height as u32, data)
    }
}

fn segment_distance((px, py): (f32, f32), (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let length = dx * dx + dy * dy;
    let t = if length > 0. { (((px - ax) * dx + (py - ay) * dy) / length).clamp(0., 1.) } else { 0. };
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

fn srgb_to_linear(value: u8) -> f32 {
    let v = f32::from(value) / 255.;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(value: f32) -> u8 {
    let v = value.clamp(0., 1.);
    let encoded = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
    (encoded * 255.).round() as u8
}

/// The board, side view: kicked nose and tail, griptape over a maple edge, two trucks, solid
/// urethane wheels and a shadow on the ground (splash.rs `draw_board`, the splash's coordinates).
fn board_image() -> Image {
    let mut canvas = Canvas::new();
    let l = BOARD_LENGTH;
    canvas.ellipse((l / 2., 35.), ((l - 20.) / 2., 2.5), SHADOW);
    // The deck's top line: tail kick, flat middle, nose kick (the nose a little longer and higher).
    let top = [
        (1.0, 3.0), (5.0, 3.6), (11.0, 5.6), (18.0, 8.6), (25.0, 11.0), (31.0, 12.0),
        (l - 33.0, 12.0), (l - 26.0, 10.8), (l - 18.0, 7.8), (l - 10.0, 4.4), (l - 4.0, 2.4), (l - 1.0, 2.0),
    ];
    let outline = |upper: f32, lower: f32| -> Vec<(f32, f32)> {
        top.iter().map(|&(x, y)| (x, y + upper)).chain(top.iter().rev().map(|&(x, y)| (x, y + lower))).collect()
    };
    // Maple edge (5 px thick) with a darker centre ply, then the griptape on top.
    let wood = outline(0., 5.);
    canvas.polygon(&wood, PLY);
    canvas.stroke(&wood, 0.5, PLY_EDGE);
    canvas.polygon(&outline(2.6, 3.4), PLY_DARK);
    canvas.polygon(&outline(0., 1.4), GRIP);
    // Trucks: baseplate under the flat of the deck, the hanger down to the axle, then the wheel
    // with a coloured core and the bearing.
    for ax in [40., l - 42.] {
        let plate = [(ax - 10., 17.), (ax + 10., 17.), (ax + 9., 19.5), (ax - 9., 19.5)];
        canvas.polygon(&plate, METAL);
        canvas.stroke(&plate, 0.5, METAL_DARK);
        let hanger = [(ax - 6., 19.5), (ax + 6., 19.5), (ax + 3.5, 26.), (ax - 3.5, 26.)];
        canvas.polygon(&hanger, METAL);
        canvas.stroke(&hanger, 0.5, METAL_DARK);
        canvas.ellipse((ax, 26.5), (7.5, 7.5), WHEEL);
        canvas.ring((ax, 26.5), 7.5, 0.6, WHEEL_EDGE);
        canvas.ellipse((ax, 26.5), (3.9, 3.9), WHEEL_CORE);
        canvas.ellipse((ax, 26.5), (1.7, 1.7), BEARING);
    }
    canvas.into_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> LoadingScreen {
        LoadingScreen::new(true, Readiness::default())
    }

    #[test]
    fn the_screen_stays_up_until_the_published_world_is_ready_for_some_frames() {
        let start = Instant::now();
        let mut s = screen();
        assert_eq!(s.advance(true, 0, start), Change::Up);
        // The commit publishes while the load is still busy, then the load ends.
        let token = s.wait_for(start);
        assert_eq!(s.advance(false, 0, start), Change::None);
        assert!(s.shown());
        for _ in 1..READY_FRAMES {
            assert_eq!(s.advance(false, token, start), Change::None);
        }
        // A frame something was missing again starts the count over.
        assert_eq!(s.advance(false, 0, start), Change::None);
        for _ in 1..READY_FRAMES {
            assert_eq!(s.advance(false, token, start), Change::None);
        }
        assert!(s.shown());
        let later = start + Duration::from_secs(3);
        assert_eq!(s.advance(false, token, later), Change::Ready(Duration::from_secs(3)));
        assert!(!s.shown());
        // The token of an older world doesn't count for a newer one.
        let newer = s.wait_for(later);
        assert_ne!(newer, token);
        assert_eq!(s.advance(false, token, later), Change::Up);
    }

    #[test]
    fn a_world_that_never_gets_ready_gives_way() {
        let start = Instant::now();
        let mut s = screen();
        s.wait_for(start);
        assert_eq!(s.advance(false, 0, start), Change::Up);
        let waited = MAX_WAIT + Duration::from_secs(1);
        assert_eq!(s.advance(false, 0, start + waited), Change::GaveUp(waited));
        assert!(!s.shown());
    }

    #[test]
    fn a_load_that_ends_without_a_world_takes_the_screen_down() {
        let start = Instant::now();
        let mut s = screen();
        assert_eq!(s.advance(true, 0, start), Change::Up);
        assert_eq!(s.advance(false, 0, start), Change::Down);
        assert!(!s.shown());
    }

    #[test]
    fn a_verify_capture_never_shows_it() {
        let mut s = LoadingScreen::new(false, Readiness::default());
        assert_eq!(s.wait_for(Instant::now()), 0);
        assert_eq!(s.advance(true, 0, Instant::now()), Change::None);
        assert!(!s.shown());
    }

    fn pixel(image: &Image, x: f32, y: f32) -> [u8; 4] {
        // Splash coordinates to the image pixel they fall in.
        let (px, py) = (((x + BOARD_PAD) * BOARD_SCALE as f32) as usize, (y * BOARD_SCALE as f32) as usize);
        let data = image.data.as_ref().unwrap();
        let at = (py * image.width() as usize + px) * 4;
        data[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn the_board_has_its_deck_wheels_and_shadow_where_the_splash_draws_them() {
        let board = board_image();
        assert_eq!((board.width(), board.height()), (384, 80));
        assert_eq!(pixel(&board, -5., 1.), [0, 0, 0, 0], "clear around it");
        // The maple between the griptape (to 13.4) and the centre ply (from 14.6), mid-deck.
        assert_eq!(pixel(&board, 90., 14.), [214, 176, 124, 255]);
        assert_eq!(pixel(&board, 90., 12.5), [34, 34, 38, 255], "griptape");
        // The tail wheel's coloured core, between the bearing and the urethane.
        assert_eq!(pixel(&board, 42.8, 26.5), [86, 146, 214, 255]);
        assert_eq!(pixel(&board, BOARD_LENGTH - 42. + 5.5, 26.5), [240, 235, 220, 255], "nose wheel");
        assert_eq!(pixel(&board, 90., 35.), [0, 0, 0, 128], "shadow");
    }

    #[test]
    fn the_crab_decodes_at_its_drawing_size_with_its_transparency() {
        let crab = crab_image().unwrap();
        assert_eq!(crab.height(), CRAB_HEIGHT);
        assert_eq!(crab.width(), 1208 * CRAB_HEIGHT / 920);
        let alphas: Vec<u8> = crab.data.as_ref().unwrap().chunks_exact(4).map(|p| p[3]).collect();
        assert!(alphas.contains(&0) && alphas.contains(&255));
    }
}
