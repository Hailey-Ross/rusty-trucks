//! The grab visualizer (opt-in "Visualize Grabs", `SKATE_VISUALIZE_GRABS=1`): what the player can grab, drawn in
//! the world from the same records the game decides with, and logged.
//! - Every record in grab range (the grab owner's validated set: the props near you on foot, the car spots near you
//!   on board) is an amber line along its spline chord, with a dot at each end.
//! - The one a grab would take now is green: a thick line, a ring pulsing around its middle and an arrow pointing
//!   down at it. On foot that is the record the Ground grab decision qualified (82D324B0); on board the car spot
//!   the skitch latch holds (82D39D98).
//! - `GRAB_VIEW` (when the state, the ready record or the records in range change): each record with its owner
//!   (prop id and template, or car spot), distance and height, and the nearest prop with grab splines when nothing
//!   is in range.
//!
//! A viewer aid only: it reads the decision, never feeds it, so play is the same with it on or off.
use bevy::prelude::*;
use skate_core::player::offboard::grab_scene::{Descriptor, Record};
use skate_core::player::state::PhysicalStateId;

static LOGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The visualizer is on: the grab decisions add their per-candidate lines (`SKITCH_CANDIDATE`).
pub(crate) fn logging() -> bool {
    LOGGING.load(std::sync::atomic::Ordering::Relaxed)
}

const IN_RANGE: Color = Color::srgb(1.0, 0.7, 0.1);
const READY: Color = Color::srgb(0.2, 1.0, 0.35);

/// What a grab could take right now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct View {
    mode: &'static str,
    /// The grab input is held: the game's own decision is shown (else the preview).
    held: bool,
    ready: Option<(u32, u32)>,
    in_range: Vec<(u32, u32)>,
}

fn mode(state: PhysicalStateId) -> &'static str {
    if state == PhysicalStateId::BipedGround { "on_foot" } else { "riding" }
}

fn v3(v: [f32; 4]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

/// The record a grab takes now: the Ground decision's on foot, the skitch latch's on board.
fn ready(skater: &super::SkaterRuntime) -> Option<(u32, u32)> {
    if skater.player_state.current() == PhysicalStateId::BipedGround {
        let g = &skater.biped_ground.ground;
        g.flags_144_to_150[0].then_some((g.counter_152, g.counter_156))
    } else {
        let s = &skater.ground.state;
        s.flag_2729.then_some((s.word_2560, s.word_2564))
    }
}

fn describe(physics: &super::GamePhysics, Descriptor { kind, id }: Descriptor) -> String {
    let tag = super::prop_dynamics::PROP_GRAB_TAG;
    match kind {
        2 if id & tag != 0 => {
            let prop = physics.prop_dynamics().and_then(|d| d.prop_of_grab_key(id, true));
            let template = physics.prop_dynamics().zip(prop).map_or_else(String::new, |(d, p)| d.describe_template(p));
            format!("prop {} ({template}) record {}", prop.map_or_else(|| "?".to_owned(), |p| p.to_string()), id & 63)
        }
        1 => format!("car spot {id:#x}"),
        _ => format!("kind {kind} id {id:#x}"),
    }
}

fn draw(gizmos: &mut Gizmos, record: &Record, ready: bool, time: f32) {
    let [a, b] = record.endpoints().map(v3);
    if !ready {
        gizmos.line(a, b, IN_RANGE);
        gizmos.sphere(Isometry3d::from_translation(a), 0.03, IN_RANGE);
        gizmos.sphere(Isometry3d::from_translation(b), 0.03, IN_RANGE);
        return;
    }
    for dy in [-0.015, 0.0, 0.015] {
        let up = Vec3::Y * dy;
        gizmos.line(a + up, b + up, READY);
    }
    let middle = (a + b) * 0.5;
    let pulse = 0.14 + 0.04 * (time * 6.0).sin();
    let flat = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    gizmos.circle(Isometry3d::new(middle, flat), pulse, READY);
    gizmos.circle(Isometry3d::new(middle, flat), pulse * 0.6, READY);
    gizmos.arrow(middle + Vec3::Y * 0.7, middle + Vec3::Y * 0.2, READY);
}

/// The same query and pick as the game's decision, without the grab input (on foot 82D324B0, on board 82D39BB8 /
/// 82D39D98); the three validation lines are not run, so a record behind a wall can show until grab is held.
fn preview_of(skater: &super::SkaterRuntime, physics: &super::GamePhysics) -> Result<(Vec<Record>, Option<(u32, u32)>), String> {
    let scene = super::offboard::grab_scene::Scene::new(&physics.world, &physics.offboard_grab_scene);
    if skater.player_state.current() == PhysicalStateId::BipedGround {
        let input = super::biped_ground::services::grab_input(skater);
        let (records, ready) = super::biped_ground::grab::preview(&scene, skater.biped_ground.grab_settings, &input)?;
        return Ok((records, ready.map(|d| (d.kind, d.id))));
    }
    let Some(toolkit) = skater.player_input.toolkit.as_ref() else { return Ok((Vec::new(), None)) };
    super::ground_runtime::skitch::preview(&scene, &skater.player_input.processed, toolkit.effective[3], &skater.ground_settings.skitch)
}

/// Draws the cues and writes `GRAB_VIEW` while the opt-in is on.
pub(crate) fn visualize_grabs(
    living: Option<Res<crate::living_world::LivingWorldSettings>>,
    skater: Option<Res<super::SkaterRuntime>>,
    physics: Option<Res<super::GamePhysics>>,
    time: Res<Time>,
    mut gizmos: Gizmos,
    mut last: Local<Option<View>>,
) {
    let on = living.as_deref().is_some_and(|l| crate::living_world::opt_ins::in_effect(l).visualize_grabs);
    LOGGING.store(on, std::sync::atomic::Ordering::Relaxed);
    let (Some(skater), Some(physics), true) = (skater, physics, on) else {
        *last = None;
        return;
    };
    let held = skater.player_input.processed.flags_2476 & 0x0040_0000 != 0;
    let preview;
    let (records, ready) = if held {
        (skater.offboard_grab.validated(), ready(&skater))
    } else {
        preview = match preview_of(&skater, &physics) {
            Ok(p) => p,
            Err(e) => {
                warn!("GRAB_VIEW preview failed: {e}");
                return;
            }
        };
        (preview.0.as_slice(), preview.1)
    };
    let t = time.elapsed_secs();
    for record in records {
        let d = record.descriptor();
        draw(&mut gizmos, record, ready == Some((d.kind, d.id)), t);
    }
    let view = View {
        mode: mode(skater.player_state.current()),
        held,
        ready,
        in_range: records.iter().map(|r| (r.descriptor().kind, r.descriptor().id)).collect(),
    };
    if last.as_ref() == Some(&view) {
        return;
    }
    let position = v3(skater.player_input.processed.vectors_544_560_592_608[2].map(f32::from_bits));
    let mut parts: Vec<String> = records
        .iter()
        .map(|r| {
            let [a, b] = r.endpoints().map(v3);
            let middle = (a + b) * 0.5;
            format!("{} at {:.2} m, height {:+.2} m", describe(&physics, r.descriptor()), middle.distance(position), middle.y - position.y)
        })
        .collect();
    if parts.is_empty() {
        let nearest = physics.prop_dynamics().and_then(|d| {
            let p = skate_core::math::Vector3::new(position.x, position.y, position.z);
            let (id, centre) = d.nearest_body(p, 6.0)?;
            d.grab_object(id)?;
            let c = Vec3::new(centre.x, centre.y, centre.z);
            Some(format!("nearest prop with grab splines {id} ({}) {:.2} m to its centre", d.describe_template(id), c.distance(position)))
        });
        parts.push(nearest.unwrap_or_else(|| "no prop with grab splines within 6 m".to_owned()));
    }
    let ready_text = ready.map_or_else(|| "none".to_owned(), |(kind, id)| describe(&physics, Descriptor { kind, id }));
    info!("GRAB_VIEW {} {} ready={} in_range={} [{}]", view.mode, if held { "grab held" } else { "preview" }, ready_text, records.len(), parts.join("; "));
    *last = Some(view);
}
