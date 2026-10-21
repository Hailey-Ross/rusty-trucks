//! Opt-in startup verification, with no in-game tool UI.
use crate::{
    animation::AnimationStatus,
    app::FrameSet,
    config::Config,
    input::ControllerInput,
    physics::{GamePhysics, PlayerControls, SkaterRuntime},
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

#[derive(Resource, Default)]
struct Verification {
    elapsed: f32,
    requested: bool,
    captured: bool,
    photo: bool,
    export: bool,
}

pub(crate) struct VerificationPlugin;
impl Plugin for VerificationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Verification>()
            .add_systems(Update, verify.in_set(FrameSet::Verification));
    }
}
fn verify(
    mut commands: Commands,
    time: Res<Time<Real>>,
    config: Res<Config>,
    animation: Res<AnimationStatus>,
    input: Res<ControllerInput>,
    physics: Res<GamePhysics>,
    controls: Res<PlayerControls>,
    skater: Res<SkaterRuntime>,
    camera: Res<crate::camera::CameraRuntime>,
    mut replay: ResMut<crate::replay::Replay>,
    mut export: Option<ResMut<crate::replay::export_run::ReplayExport>>,
    mut state: ResMut<Verification>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &config.verification_capture else {
        return;
    };
    state.elapsed += time.delta_secs();
    // Opt-in visual smoke check of the replay HUD and presentation endpoints.
    let replay_at = std::env::var("SKATE_VERIFY_REPLAY_AT").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(2.0);
    if animation.ready && state.elapsed > replay_at && !replay.active
        && std::env::var("SKATE_VERIFY_REPLAY").as_deref() == Ok("1") {
        replay.enter();
    }
    // SKATE_VERIFY_REPLAY_EDITOR=timeline / effects: the editor (A, then A again for the effects menu), a second after
    // the replay opened (needs SKATE_VERIFY_REPLAY_AT past 5 s of recording).
    if let Ok(mode) = std::env::var("SKATE_VERIFY_REPLAY_EDITOR") {
        if replay.active && replay.editor.is_none() && state.elapsed > replay_at + 1.0 {
            let (start, end) = replay.bounds();
            replay.editor = crate::replay::editor::Editor::open(start, end, Default::default());
        }
        // The effects menu half a second later, from the timeline (as A does).
        if let Some(e) = replay.editor.as_mut().filter(|e| mode == "effects" && e.mode() == 6 && state.elapsed > replay_at + 1.5) {
            e.session.mode = skate_core::replay_edit::session::EditorMode::Effects;
            e.row = crate::replay::editor::GAME_CAMERA_ROW;
        }
    }
    // SKATE_VERIFY_REPLAY_EXPORT=<file.mp4>: export the open replay (as Save to PC) a second after it opened; the
    // capture waits until the export is done.
    if let (Ok(path), Some(export)) = (std::env::var("SKATE_VERIFY_REPLAY_EXPORT"), export.as_mut()) {
        if replay.active && !state.export && state.elapsed > replay_at + 1.0 {
            state.export = true;
            export.export_to(std::path::PathBuf::from(path));
        }
        if state.export && (replay.exporting || export.busy()) {
            state.elapsed = state.elapsed.min(replay_at + 1.5);
        }
    }
    // SKATE_VERIFY_REPLAY_PHOTO=1: take a replay photo (as Yes on Take Photo) two seconds in.
    if replay.active && state.elapsed > 4.0 && !state.photo && std::env::var("SKATE_VERIFY_REPLAY_PHOTO").as_deref() == Ok("1") {
        state.photo = true;
        replay.photo = crate::replay::Photo::Capture;
    }
    // SKATE_VERIFY_REPLAY_HELP=1: the replay's help screen too (Start), a second after the replay opened.
    if replay.active && state.elapsed > 3.0 && !replay.help_open && std::env::var("SKATE_VERIFY_REPLAY_HELP").as_deref() == Ok("1") {
        replay.help_open = true;
    }
    // SKATE_VERIFY_AT: capture time in seconds (default 4), e.g. to catch an effect.
    let capture_at = std::env::var("SKATE_VERIFY_AT").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(4.);
    if animation.ready && state.elapsed > capture_at && !state.requested {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()))
            .observe(
                |_: On<ScreenshotCaptured>, mut state: ResMut<Verification>| {
                    state.captured = true;
                },
            );
        state.requested = true;
    }
    if state.captured && state.elapsed > capture_at + 2. {
        let input_report = format!(
            "Game integration capture (not a Skate 3 parity verdict)\nExecutable: {}\nPolls: {}\nConsumed batches: {}\nStatus: {:?}\nPacket numbers: {:?}\nActions 64..81 by device: {:?}\nDerived ticks: {}\nIntents: {:?}\nDifficulty index: {}\nPhysics ticks: {}\nContacts: {}\nBody positions: {:?}\nPhysical pose publications: {}\nGraph animation ticks: {}\nTruck targets: {:?}\nGround speed: {}\nGameplay camera shot: {}\nCamera frame: {:?}\n",
            std::env::current_exe()
                .map_or_else(|_| "unavailable".into(), |p| p.display().to_string()),
            input.publications,
            input.consumed_batches,
            input.status,
            input.packet_numbers,
            input.mapped_actions,
            controls.ticks,
            controls.intents,
            physics.difficulty_index(),
            physics.ticks,
            physics.contact_count,
            physics.board.bodies().map(|body| body.rates.position),
            skater.pose_generation,
            skater.animation.ticks,
            skater.ground.steering.targets,
            physics.riding.motion.ground_speed,
            camera.selected_shot(),
            camera.frame,
        );
        let input_report = format!(
            "{input_report}Native OnBoard/OffBoard clips available: {}\n",
            skater.animation.evaluator.frames.clip_count()
        );
        if let Err(error) = std::fs::write(path.with_extension("input.txt"), input_report) {
            eprintln!("Cannot save controller verification: {error}");
            exit.write(AppExit::error());
            return;
        }
        info!("GAME_VERIFY_OK");
        exit.write(AppExit::Success);
    } else if state.elapsed > 45. {
        error!("Game startup/capture timed out");
        exit.write(AppExit::error());
    }
}
