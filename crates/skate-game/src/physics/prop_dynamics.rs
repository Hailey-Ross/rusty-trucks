//! Dynamic rigid bodies for DMO prop instances (Phase 2a).
//!
//! Every spawned prop gets a box body built on the TU3 rigid-body integrator
//! (`integrate_body_rates`): gravity and cool-down/sleep come from the retail
//! simulation step; the mass properties are the retail rounded-box finalize
//! path (`primitive_mass_properties`) with the instance's scaled template AABB.
//! Density, friction, restitution and damping are the authored MOBJ per-object
//! values (`ObjectPhysics`, schema 3+); the project defaults are density
//! 100 kg/m³, friction 0.55, restitution 0.05 and damping 0.05/0.15.
//!
//! Narrowphase uses the recovered GP pair query (`primitive_pair_contacts`):
//! box vs static-world triangles, box vs box for other props, and box vs the
//! skater's board/skeleton volumes for pushes. Contact response is the retail
//! row solver the board uses (ContactBatchBuild 82AE10C8 rows, 25 iterations
//! of 82AE27D0, `PropSolverSettings`); the engine's older impulse pass stays
//! selectable as a mod option. Sleep is the retail counter (integrator
//! 82AE6590, sleep pass 82DC3130) with the DMO island values.
//!
//! Props start asleep and cost one AABB test per skater volume per tick. A
//! skater contact or a moving prop wakes them. Moved instances re-bake their
//! triangle range in the prop collision layer so skater queries stay exact.
//! The held (carried) prop is exempt from both skater pushes and the rebake:
//! while carried it is velocity-driven and its layer triangles are parked far
//! below the world so they cannot push the carrier.
use bevy::prelude::*;
use skate_core::{
    math::{Basis3, Vector3},
    physics::{
        board_world::{BoardWorld, BoardWorldVolume},
        contact::{
            RetailContactBodyState, RetailContactInput, RetailContactMaterial,
            combine_contact_materials, generate_contact,
        },
        contact_solver::{ACTIVE_BODY, RetailContactJacobian, build_contact_jacobian},
        mass::{RETAIL_UNBOUNDED_VELOCITY, primitive_mass_properties},
        rigid_body::{
            RetailInertiaDynamics, RetailQuaternion, RetailReactionCorrections, RetailBodyRates,
            RetailSimulationStep, integrate_body_rates, pack_world_inverse_inertia,
            world_inverse_inertia,
        },
        solver::solve_constraints,
        collision::WorldContactSettings,
        world_contact::{
            ContactPrimitive, PrimitiveContactManifold, PrimitivePairSettings,
            primitive_pair_contacts, primitive_triangle_world_contacts,
        },
    },
};

/// Template-space contact box that replaces the render-AABB box of one prop
/// type (centre and half extents, per-axis scale applied on top).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropBox {
    pub center: Vector3,
    pub half_extents: Vector3,
}

/// Every tunable of the prop impulse pass and the skater push. One value set
/// is the default for all props; `PropTuningTable` can replace it per prop
/// type (the MOBJ template name, the stable id a mod uses). The defaults are
/// #15's constants plus the 2026-10-05 caps (doc 26, "Board stuck inside a
/// prop"); no retail values for DMO contact response are recovered yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropTuning {
    /// Contact band of the prop pair queries (m).
    pub contact_padding: f32,
    /// Penetration ignored by the positional correction (m; older impulse
    /// pass only, `PropSolverSettings::row_solver` off).
    pub penetration_slop: f32,
    /// Fraction of the penetration removed per tick (Baumgarte; older
    /// impulse pass only).
    pub penetration_correction: f32,
    /// Upper bound on the positional correction of one body in one tick (m;
    /// older impulse pass only, retail rows have no cap).
    /// A body deep inside geometry comes out over several ticks instead of
    /// being thrown out in one.
    pub max_depenetration_per_tick: f32,
    /// Older impulse pass only: restitution applies only above this closing
    /// speed (m/s); below it
    /// contacts are inelastic so resting stacks settle.
    pub restitution_threshold: f32,
    /// Effective skater mass (kg) for prop pushes.
    pub skater_push_mass: f32,
    /// Fraction of the closing speed transferred to a prop by a skater hit.
    pub push_transfer: f32,
    /// Top speed a body bump can impart along the push direction (m/s).
    pub body_push_speed: f32,
    /// Top speed a board hit can impart along the push direction (m/s).
    pub board_push_speed: f32,
    /// Push speed used when a skater volume is inside the prop but not
    /// closing (m/s): nudges an overlapping prop apart.
    pub penetration_push_speed: f32,
    /// After this many consecutive ticks in which a skater volume sits inside
    /// a prop without closing on it, the overlap nudge stops so the prop can
    /// cool down and sleep (the skater's own contacts still separate it).
    /// 0 disables the nudge entirely.
    pub stuck_release_ticks: u32,
    /// Replaces the render-AABB contact box of this prop type.
    pub collision_box: Option<PropBox>,
}

impl Default for PropTuning {
    fn default() -> Self {
        Self {
            contact_padding: 0.02,
            penetration_slop: 0.005,
            penetration_correction: 0.4,
            max_depenetration_per_tick: 0.05,
            restitution_threshold: 1.0,
            skater_push_mass: 75.0,
            push_transfer: 0.5,
            body_push_speed: 1.2,
            board_push_speed: 6.0,
            penetration_push_speed: 0.5,
            stuck_release_ticks: 30,
            collision_box: None,
        }
    }
}

/// Prop tuning for the whole map: a default plus per prop type overrides,
/// keyed by MOBJ template name. `reset` restores the shipped defaults (mod
/// disable).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PropTuningTable {
    pub default: PropTuning,
    pub by_template: std::collections::BTreeMap<String, PropTuning>,
    /// Island settings shared by every prop (one DMO simulation in retail).
    pub solver: PropSolverSettings,
    /// Self-righting window of the phone's per-object Upright (cMsgUprightDMO).
    pub upright: PropUprightSettings,
}

/// Retail DMO self-righting ("Upright", doc 27 "Upright"). The phone's
/// per-object Upright posts cMsgUprightDMO; the DMO manager slot +40 82C4B8C0
/// sets DMO+4464 bit 0x40 and zeroes the timer DMO+4376. While the bit is set
/// the DMO update 82C56780 adds `tick_seconds` to the timer and clears the bit
/// once it exceeds `window_seconds`, then (same update) 82C573D0 measures the
/// angle between the body's up row and world up: below `stop_angle_deg` the
/// bit and timer are cleared; otherwise it builds an angular command that the
/// DMO's own angular slot 37 (82C52EE0 -> 82D9CCF0) writes into the body's
/// angular accumulator (+160), waking the body and marking it commanded.
/// External yaw commands (82C52E68) are refused while the bit is set; the
/// linear Move Object command (82C52DC0) is not gated. The defaults are the
/// retail constants; every field is a mod knob
/// (`sdk.world.set_tuning('props', {upright = {...}})`), reset on mod disable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropUprightSettings {
    /// Window length (retail 2.0 s, 0x82060C50).
    pub window_seconds: f32,
    /// Timer increment per update (retail 1/60, 0x820849C8: a fixed per-frame
    /// step, matching our 60 Hz prop step).
    pub tick_seconds: f32,
    /// Tilt below which the window ends (retail 10 deg, 0x821963E4).
    pub stop_angle_deg: f32,
    /// Tilt cap of the righting speed (retail 70 deg: 70 (0x820BB1E0) x
    /// 0.0174533 (0x8206D110)).
    pub max_angle_deg: f32,
    /// Dead band subtracted from the capped tilt (retail 5 deg, 0x820BB1D8).
    pub dead_band_deg: f32,
    /// Righting gain (1/s) at the low end of the blend (retail 3, 0x82063B08).
    pub gain_min: f32,
    /// Righting gain at the high end of the blend (retail 5, 0x821F1790).
    pub gain_max: f32,
    /// Blend weight t = clamp(|A| - gain_blend_start, 0, 1) on the body
    /// vector A (state block +72); retail 0.1 (0x820641A8) + 1.0 (0x8231A844).
    pub gain_blend_start: f32,
    /// Fraction of the off-axis spin the command removes per update (retail
    /// 0.1, 0x820641A8: the command targets w_axis + 0.1 w_perp).
    pub off_axis_spin: f32,
    /// Command scale (retail 60, 0x821FF080: one 60 Hz step to the target).
    pub command_rate: f32,
    /// Above this tilt (or with a degenerate axis) the body's own X or Z axis
    /// is used instead of up x world-up (retail 120 deg, 0x82256FE0).
    pub fallback_angle_deg: f32,
    /// External yaw commands refused while righting (retail 82C52E68).
    pub block_yaw: bool,
}

impl Default for PropUprightSettings {
    fn default() -> Self {
        Self {
            window_seconds: 2.0,
            tick_seconds: 1.0 / 60.0,
            stop_angle_deg: 10.0,
            max_angle_deg: 70.0,
            dead_band_deg: 5.0,
            gain_min: 3.0,
            gain_max: 5.0,
            gain_blend_start: 1.1,
            off_axis_spin: 0.1,
            command_rate: 60.0,
            fallback_angle_deg: 120.0,
            block_yaw: true,
        }
    }
}

/// The righting command of 82C573D0 for a body with orientation `basis`,
/// angular velocity `w` and body vector `a` (retail: the vector at the
/// physics state block +72; which body quantity that is has not been
/// identified, see [`PropDynamics::upright_vector`]). `None` = tilt below
/// `stop_angle_deg` (window ends). Pure and deterministic.
pub(crate) fn upright_command(basis: Basis3, w: Vector3, a: Vector3, s: &PropUprightSettings) -> Option<Vector3> {
    let up = mul_basis(basis, Vector3::new(0.0, 1.0, 0.0));
    let up_len = length(up);
    let cos = if up_len > 0.0 { (up.y / up_len).clamp(-1.0, 1.0) } else { 1.0 };
    let angle = cos.acos();
    let degrees = angle.to_degrees();
    if degrees < s.stop_angle_deg {
        return None;
    }
    let normalize = |v: Vector3| {
        let l = length(v);
        if l > 0.0 { scale(v, 1.0 / l) } else { Vector3::ZERO }
    };
    let mut axis = normalize(cross(up, Vector3::new(0.0, 1.0, 0.0)));
    let degenerate = axis.x.abs() <= f32::EPSILON && axis.y.abs() <= f32::EPSILON && axis.z.abs() <= f32::EPSILON;
    if degenerate || degrees > s.fallback_angle_deg {
        let local = if a.x > a.z { Vector3::new(1.0, 0.0, 0.0) } else { Vector3::new(0.0, 0.0, 1.0) };
        axis = normalize(mul_basis(basis, local));
    }
    let capped = angle.min(s.max_angle_deg.to_radians());
    let t = (length(a) - s.gain_blend_start).clamp(0.0, 1.0);
    let gain = (1.0 - t) * s.gain_min + t * s.gain_max;
    let speed = gain * (capped - s.dead_band_deg.to_radians()).max(0.0);
    let along = scale(axis, dot(axis, w));
    let across = sub(w, along);
    let keep = add(along, scale(across, s.off_axis_spin));
    Some(scale(sub(scale(axis, speed), keep), s.command_rate))
}

/// Island settings of the DMO simulation. Retail 8275DCC8 passes a 52-byte
/// block to the simulation ctor 82DC2840, which copies +16 -> island +176
/// (solver iterations), +32 -> island +172 (sleep energy) and +36 -> island
/// +168 (sleep counter cap; also the sleep pass threshold sim +204). The
/// defaults are those retail values; every field is a mod knob
/// (`sdk.world.set_tuning('props', {solver = {...}})`), reset on mod disable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropSolverSettings {
    /// Contact solve: true = the retail row solver (rows built by
    /// ContactBatchBuild 82AE10C8, iterated by 82AE27D0, the same skate-core
    /// path the board uses); false = the engine's older impulse pass with
    /// penetration slop / fraction / cap (NOT RETAIL, kept as a mod option).
    pub row_solver: bool,
    /// Row solver iterations per step (retail 25).
    pub iterations: u32,
    /// Sleep energy threshold on E = |v|^2 + s * m^-1 * |w|^2 after damping
    /// and the speed caps (integrator 82AE6590; retail 1e-5, 0x8219B100).
    pub sleep_energy: f32,
    /// Steps with E below `sleep_energy` and not rising before the sleep
    /// pass (82DC3130) puts a body to sleep; also the counter cap (retail 2).
    pub sleep_frames: u32,
    /// Bodies the sleep pass puts to sleep per step at most (retail 100).
    pub max_sleeps_per_step: u32,
    /// The engine's rest snap (NOT RETAIL): zero the velocities of a touching
    /// body whose energy is below `sleep_energy`. Retail has no snap; off by
    /// default, kept as a mod option (with the old 0.5 / 30 sleep values it
    /// reproduces the pre-2026-10-08 settling).
    pub rest_snap: bool,
}

impl Default for PropSolverSettings {
    fn default() -> Self {
        Self {
            row_solver: true,
            iterations: 25,
            sleep_energy: 1e-5,
            sleep_frames: 2,
            max_sleeps_per_step: 100,
            rest_snap: false,
        }
    }
}

impl PropTuningTable {
    pub(crate) fn for_template(&self, template: &str) -> &PropTuning {
        self.by_template.get(template).unwrap_or(&self.default)
    }
}

/// Per-step work counters, for tests and the frame-cost check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PropStepStats {
    pub awake: u32,
    pub rebakes: u32,
    pub skater_pushes: u32,
}
/// Actor id of the local player's skater volumes in the prop step (NPC skaters
/// use their proxy solid id, `living_world::npc_skaters::PROXY_ID_TAG | id`).
pub(crate) const LOCAL_PUSHER: u64 = 0;

/// Where the held prop's collision triangles are parked so skater queries
/// cannot see them while it is carried.
pub(crate) const HELD_PARK: Vector3 = Vector3::new(0.0, -10000.0, 0.0);

/// Props get their own simulation step (retail: the DMO simulation, its own
/// island settings, 8275DCC8 -> 82DC2840): sleep counter cap 2 and sleep
/// energy 1e-5 ([`PropSolverSettings`] defaults; the step applies the live
/// settings). The board's simulation carries cool_down = 0 (the host never
/// sleeps it).
pub(crate) fn prop_simulation(
    base: skate_core::physics::rigid_body::RetailSimulationStep,
) -> skate_core::physics::rigid_body::RetailSimulationStep {
    let solver = PropSolverSettings::default();
    skate_core::physics::rigid_body::RetailSimulationStep {
        cool_down: solver.sleep_frames,
        minimum_energy: solver.sleep_energy,
        ..base
    }
}

pub(crate) struct PropBody {
    /// Index into the collision layer's instance list (and its rebake target).
    instance: usize,
    id: u32,
    /// Box centre and half extents in template space, scale folded in.
    local_center: Vector3,
    half_extents: Vector3,
    rates: RetailBodyRates,
    inertia: RetailInertiaDynamics,
    /// Authored MOBJ contact material (friction/restitution).
    material: RetailContactMaterial,
    enable_sleep: bool,
    asleep: bool,
    /// MOBJ template name: the prop type key of `PropTuningTable`.
    template: String,
    /// Resolved tuning of this prop type.
    tuning: PropTuning,
    /// Box derived from the render AABB (scale folded in), kept so a
    /// `collision_box` override can be removed again.
    authored_center: Vector3,
    authored_half_extents: Vector3,
    axis_scale: Vector3,
    /// Consecutive ticks a skater volume sat inside this prop without
    /// closing on it.
    stuck_ticks: u32,
    /// Stable actor id of the skater whose contact last pushed this prop
    /// ([`LOCAL_PUSHER`] or an NPC skater proxy id): the authority owner a
    /// future host uses for the moved prop. `None` until something pushes it.
    pushed_by: Option<u64>,
    /// Pose last written to the collision layer; an unchanged pose skips the
    /// (identical) rebake.
    baked: Option<(Vector3, Basis3)>,
    /// Contact manifolds (static triangles and other props) that touched this
    /// body in its last step, for the HELD_PROP diagnostics.
    contacts: u32,
    /// Box centre height where this body last rested (spawn, sleep or grab):
    /// the ground probe for HELD_PROP / PROP_BELOW_GROUND starts above it, so
    /// a body that sank under the floor still finds the floor it left.
    rest_y: f32,
    /// Authored (spawn) template-origin pose: what a DMO reset returns the body to
    /// (retail cMsgResetDMO, doc 27 "Object Dropper and reset").
    spawn_origin: Vector3,
    spawn_basis: Basis3,
    /// A Move Object command arrived for this body since its last step
    /// (retail DMO+4465 bit 0x02, set by the slot 9 sinks 82C52DC0 /
    /// 82C52E68).
    commanded: bool,
    /// The body runs on the commanded parameter block (retail DMO+4465 bit
    /// 0x01: the previous tick's commanded bit; 82C53EF8 swaps the block when
    /// the two differ).
    commanded_block: bool,
    /// Self-righting window timer (retail DMO+4376; `Some` = DMO+4464 bit
    /// 0x40 set, [`PropUprightSettings`]).
    upright_timer: Option<f32>,
}

/// Friction pair `[static, dynamic]` of a retail body contact material block.
/// The block is three floats at physics component +48 / +52 / +56 = {static
/// friction, dynamic friction, restitution (DMO data +272)}, written by
/// 82C550A8 and pointed at by every body's +80; the collision-object builders
/// 82DC3A68 / 82DC4158 / 82DC4588 copy it to CO +116..+124 and aaCollision
/// 8277A508 combines two objects' blocks with 82763078 (static max, dynamic
/// max, restitution min = [`combine_contact_materials`]).
pub(crate) type MaterialBlock = [f32; 2];

/// Retail commanded friction pair {0.03 (0x8208EA80), 0.02 (0x821E9580)}
/// (82C53EF8 while DMO+4465 bit 0x02 is set).
pub(crate) const RETAIL_COMMANDED_MATERIAL: MaterialBlock = [0.03, 0.02];

/// Retail upright test of 82C54B00: the body's up axis (transform row 1) has
/// y > 0.65 (0x820BB0EC); sets DMO+4465 bit 0x08.
pub(crate) const RETAIL_UPRIGHT_COS: f32 = 0.65;

/// Per prop type material data (MOBJ template name). Retail reads these from
/// the DMO type data (DMO+4380 -> +4); the values per type are not extracted
/// yet, so every `None` default reproduces the authored MOBJ material
/// (NOT RETAIL YET: interim defaults, see doc 27).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PropMaterialBlocks {
    /// Friction pair while commanded; `None` = `MoveCommandRules::commanded_material`.
    pub held: Option<MaterialBlock>,
    /// Free friction pair (DMO data +320 / +328; also the only free pair when
    /// `upright_pair` is off); `None` = authored MOBJ friction for both.
    pub free: Option<MaterialBlock>,
    /// Free friction pair while upright (DMO data +316 / +324), used only when
    /// `upright_pair` is set; `None` = `free`.
    pub free_upright: Option<MaterialBlock>,
    /// Type flag DMO data +312 bit 0 (-> DMO+4465 bit 0x10, ctor 82C51E28):
    /// the free pair depends on the upright test. `None` = false.
    pub upright_pair: Option<bool>,
    /// Restitution of every block of this type (DMO data +272); `None` = the
    /// authored MOBJ restitution.
    pub restitution: Option<f32>,
    /// Record+272 of this prop type (Move Object speeds x
    /// `record_272_speed_scale`); `None` = false (retail per DMO type data
    /// +312, 82C4B960, not extracted yet).
    pub record_272: Option<bool>,
}

/// How a Move Object command reaches the held body (retail interface slot 9
/// -> 82C4C370 / 82C4C3E0 -> 82D9CC78 / 82D9CCF0, spec section 7). The
/// defaults are the retail behaviour; every field is a mod knob
/// (`sdk.world.set_tuning('carry', ...)`), cleared on mod disable.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MoveCommandRules {
    /// Friction pair every commanded body switches to (retail {0.03, 0.02});
    /// the block's restitution stays the type's (DMO data +272).
    pub commanded_material: MaterialBlock,
    /// Upright test threshold on the body's up axis y (retail 0.65).
    pub upright_cos: f32,
    /// Per prop type overrides of the held / free blocks.
    pub by_template: std::collections::BTreeMap<String, PropMaterialBlocks>,
    /// Linear command at the centre of mass (retail: accumulator +144, no
    /// torque). `false` = applied at the grip point (lever torque; mod only).
    pub apply_at_com: bool,
    /// Yaw command replaces the angular accumulator (retail 82D9CCF0
    /// overwrites +160). `false` = added to the body's torque accumulator.
    pub yaw_replaces_torque: bool,
    /// Vertical command dropped (retail 82C4C370 passes only &linear).
    pub ignore_vertical: bool,
    /// Every command wakes the body and clears its sleep counter (retail
    /// 82ADF7B8 in both sinks). `false` = only a non-zero command wakes it.
    pub wake_on_command: bool,
}

impl Default for MoveCommandRules {
    fn default() -> Self {
        Self {
            commanded_material: RETAIL_COMMANDED_MATERIAL,
            upright_cos: RETAIL_UPRIGHT_COS,
            by_template: Default::default(),
            apply_at_com: true,
            yaw_replaces_torque: true,
            ignore_vertical: true,
            wake_on_command: true,
        }
    }
}

impl MoveCommandRules {
    /// The body's own contact material block (82C53EF8 / 82C54BF0 ->
    /// 82C550A8): while commanded {held pair, restitution}; otherwise the free
    /// pair, which for a type with the upright flag (data +312 bit 0) is the
    /// upright pair while `up_y > upright_cos` (82C54B00) and the default pair
    /// when tipped. Pure function of the commanded bit and the up axis.
    pub(crate) fn body_material(&self, template: &str, authored: RetailContactMaterial, commanded: bool, up_y: f32) -> RetailContactMaterial {
        let t = self.by_template.get(template);
        let restitution = t.and_then(|b| b.restitution).unwrap_or(authored.restitution);
        let [static_friction, dynamic_friction] = if commanded {
            t.and_then(|b| b.held).unwrap_or(self.commanded_material)
        } else {
            let free = t.and_then(|b| b.free).unwrap_or([authored.static_friction, authored.dynamic_friction]);
            let upright = t.and_then(|b| b.upright_pair).unwrap_or(false) && up_y > self.upright_cos;
            if upright { t.and_then(|b| b.free_upright).unwrap_or(free) } else { free }
        };
        RetailContactMaterial { static_friction, dynamic_friction, restitution }
    }
}

/// The Move Object part of a HELD_PROP line (`stick=[x, z, rot]`: left stick
/// X / Z and right stick X as OB_ObjectMv intents).
pub(crate) fn move_fields(mv: Option<MoveDiagnostics>) -> String {
    match mv {
        Some(m) => format!(
            "stick=[{:.2}, {:.2}, {:.2}] command=[{:.2}, {:.2}] yaw_cmd={:.2} lever={:.2} rot={:.2} blocked={} drift={:.3} yaw_rate={:.3}",
            m.stick[0], m.stick[1], m.stick[2], m.linear[0], m.linear[2], m.yaw, m.lever, m.rotation, m.blocked, m.drift, m.yaw_rate
        ),
        None => "stick=none".into(),
    }
}

/// Seconds a released prop keeps logging HELD_PROP lines.
const RELEASE_LOG_SECONDS: f32 = 3.0;
/// HELD_PROP lines per second with trace-all (1 otherwise): a short push, pull, side step or
/// turn of a held prop lasts well under a second.
const HELD_PROP_HZ_TRACE_ALL: u64 = 5;
/// Minimum seconds between two PROP_BELOW_GROUND lines for one prop.
const BELOW_GROUND_LOG_SECONDS: f32 = 10.0;

/// One body's ground relation, for the diagnostics and tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropGroundProbe {
    /// Box centre.
    pub center: Vector3,
    /// World-space half height of the (possibly tilted) box.
    pub half_height: f32,
    /// Height of the first static surface under the body, probed from above
    /// its last rest height; `None` if nothing is under it.
    pub ground: Option<f32>,
}

impl PropGroundProbe {
    /// Box bottom minus the ground under it (negative = inside or below the floor).
    pub(crate) fn gap(&self) -> Option<f32> {
        self.ground.map(|g| self.center.y - self.half_height - g)
    }

    /// The centre is more than its half height below the floor: the box has
    /// passed the floor's face and the one-sided triangle fixup can no longer
    /// push it back up (see `step_with_actors`).
    pub(crate) fn below_ground(&self) -> bool {
        self.ground.is_some_and(|g| g - self.center.y > self.half_height)
    }
}

pub(crate) struct PropDynamics {
    bodies: Vec<PropBody>,
    by_id: std::collections::HashMap<u32, usize>,
    simulation: RetailSimulationStep,
    pair: PrimitivePairSettings,
    /// Prop currently carried: exempt from skater pushes and rebake.
    held: Option<u32>,
    tuning: PropTuningTable,
    stats: PropStepStats,
    /// Steps taken (diagnostics clock; deterministic, plain data).
    tick: u64,
    /// Prop released from a carry and the tick until which it keeps logging.
    released: Option<(u32, u64)>,
    /// Last tick a PROP_BELOW_GROUND line was written per prop id.
    below_logged: std::collections::BTreeMap<u32, u64>,
    /// Move Object values of the held prop for HELD_PROP.
    move_diagnostics: Option<MoveDiagnostics>,
    /// How Move Object commands reach a body (retail defaults, mod knobs).
    move_rules: MoveCommandRules,
}

/// Move Object values logged on HELD_PROP lines.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct MoveDiagnostics {
    /// OB_ObjectMvX / Z / Rot.
    pub stick: [f32; 3],
    pub linear: [f32; 3],
    pub yaw: f32,
    pub lever: f32,
    pub rotation: f32,
    pub blocked: bool,
    pub drift: f32,
    /// Measured yaw rate the yaw controller tracks (82D45318 +1192 sign kept).
    pub yaw_rate: f32,
}

fn mul_basis(basis: Basis3, v: Vector3) -> Vector3 {
    Vector3::new(
        basis.columns[0][0] * v.x + basis.columns[1][0] * v.y + basis.columns[2][0] * v.z,
        basis.columns[0][1] * v.x + basis.columns[1][1] * v.y + basis.columns[2][1] * v.z,
        basis.columns[0][2] * v.x + basis.columns[1][2] * v.y + basis.columns[2][2] * v.z,
    )
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn dot(a: Vector3, b: Vector3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn add(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn sub(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn scale(v: Vector3, s: f32) -> Vector3 {
    Vector3::new(v.x * s, v.y * s, v.z * s)
}

fn length(v: Vector3) -> f32 {
    dot(v, v).sqrt()
}

fn mul_components(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}

/// Shepperd's method; the basis is a pure rotation by construction.
fn quaternion_from_basis(basis: Basis3) -> RetailQuaternion {
    let m = |c: usize, r: usize| basis.columns[c][r];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let (x, y, z, w) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        (
            (m(1, 2) - m(2, 1)) / s,
            (m(2, 0) - m(0, 2)) / s,
            (m(0, 1) - m(1, 0)) / s,
            0.25 * s,
        )
    } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
        let s = (1.0 + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.0;
        (
            0.25 * s,
            (m(1, 0) + m(0, 1)) / s,
            (m(2, 0) + m(0, 2)) / s,
            (m(1, 2) - m(2, 1)) / s,
        )
    } else if m(1, 1) > m(2, 2) {
        let s = (1.0 + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.0;
        (
            (m(1, 0) + m(0, 1)) / s,
            0.25 * s,
            (m(2, 1) + m(1, 2)) / s,
            (m(2, 0) - m(0, 2)) / s,
        )
    } else {
        let s = (1.0 + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.0;
        (
            (m(2, 0) + m(0, 2)) / s,
            (m(2, 1) + m(1, 2)) / s,
            0.25 * s,
            (m(0, 1) - m(1, 0)) / s,
        )
    };
    let inverse = 1.0 / (x * x + y * y + z * z + w * w).sqrt().max(1e-20);
    RetailQuaternion {
        x: x * inverse,
        y: y * inverse,
        z: z * inverse,
        w: w * inverse,
    }
}

impl PropBody {
    fn box_primitive(&self) -> ContactPrimitive {
        ContactPrimitive::RoundedBox {
            center: self.rates.position,
            basis: self.rates.basis,
            half_extents: self.half_extents,
            radius: 0.0,
        }
    }

    fn bounds(&self) -> skate_core::physics::board_world::query_metadata::Bounds {
        let half = [self.half_extents.x, self.half_extents.y, self.half_extents.z];
        let extent = |axis: usize| {
            self.rates
                .basis
                .columns
                .iter()
                .zip(half)
                .map(|(column, h)| h * column[axis].abs())
                .sum::<f32>()
        };
        let (ex, ey, ez) = (extent(0), extent(1), extent(2));
        let c = self.rates.position;
        skate_core::physics::board_world::query_metadata::Bounds {
            min: Vector3::new(c.x - ex, c.y - ey, c.z - ez),
            max: Vector3::new(c.x + ex, c.y + ey, c.z + ez),
        }
    }

    fn wake(&mut self) {
        self.asleep = false;
        self.rates.cool_down = 0;
    }

    /// Impulse applied at a world point, directly to velocities. Returns the
    /// impulse vector for the other body / wake accounting.
    fn apply_impulse(&mut self, impulse: Vector3, point: Vector3) -> Vector3 {
        self.wake();
        self.rates.linear_velocity = add(
            self.rates.linear_velocity,
            scale(impulse, self.inertia.inverse_mass),
        );
        let r = sub(point, self.rates.position);
        let angular = cross(r, impulse);
        let delta = mul_basis(self.rates.world_inverse_inertia, angular);
        self.rates.angular_velocity = add(self.rates.angular_velocity, delta);
        impulse
    }

    fn velocity_at(&self, point: Vector3) -> Vector3 {
        add(
            self.rates.linear_velocity,
            cross(self.rates.angular_velocity, sub(point, self.rates.position)),
        )
    }

    /// Template-origin pose for rendering and collision rebake.
    fn origin(&self) -> Vector3 {
        sub(
            self.rates.position,
            mul_basis(self.rates.basis, self.local_center),
        )
    }
}

/// Pose tolerance for "moved" (metres / basis component). Ours: float noise
/// of a body that slept at its authored pose; retail's per-DMO moved flag is a
/// record field (sub_826666A8 reads it), not a distance. NOT RETAIL YET.
const SPAWN_POSE_EPSILON: f32 = 1e-4;

impl PropDynamics {
    /// One box body per collision-layer instance, asleep at its authored pose.
    /// Placement rows are the world images of the local axes; their lengths
    /// are the constant per-axis scale folded into the box extents. Density,
    /// damping, friction, restitution and sleep flags come from the authored
    /// MOBJ physics block (`ObjectPhysics`).
    pub(crate) fn new(
        objects: &[skate_data::skate_map::StaticObject],
        instances: &[crate::skate_world::PropCollisionInstance],
        simulation: RetailSimulationStep,
    ) -> Self {
        let mut bodies = Vec::new();
        let mut by_id = std::collections::HashMap::new();
        for (index, entry) in instances.iter().enumerate() {
            let object = &objects[entry.object];
            let authored = object.physics;
            let t = &object.transform;
            let axis_scale = [
                (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt(),
                (t[3] * t[3] + t[4] * t[4] + t[5] * t[5]).sqrt(),
                (t[6] * t[6] + t[7] * t[7] + t[8] * t[8]).sqrt(),
            ];
            let basis = Basis3 {
                columns: [
                    [t[0] / axis_scale[0], t[1] / axis_scale[0], t[2] / axis_scale[0]],
                    [t[3] / axis_scale[1], t[4] / axis_scale[1], t[5] / axis_scale[1]],
                    [t[6] / axis_scale[2], t[7] / axis_scale[2], t[8] / axis_scale[2]],
                ],
            };
            let first = entry.local_points()[0][0];
            let mut min = first;
            let mut max = first;
            for point in entry.local_points().iter().flatten() {
                min = Vector3::new(min.x.min(point.x), min.y.min(point.y), min.z.min(point.z));
                max = Vector3::new(max.x.max(point.x), max.y.max(point.y), max.z.max(point.z));
            }
            let local_center = scale(add(min, max), 0.5);
            let half_extents = scale(sub(max, min), 0.5);
            let volume = 8.0 * half_extents.x * half_extents.y * half_extents.z;
            let Some(properties) = primitive_mass_properties(
                skate_core::physics::mass::PartMassInput {
                    shape: skate_core::physics::mass::MassShape::RoundedBox {
                        half_extents,
                        radius: 0.0,
                    },
                    requested_mass: volume * authored.density.max(0.001),
                },
                RETAIL_UNBOUNDED_VELOCITY,
                authored.angular_damping,
            ) else {
                continue;
            };
            let mut inertia = properties.dynamics;
            inertia.linear_drag = authored.linear_damping;
            inertia.maximum_linear_velocity = RETAIL_UNBOUNDED_VELOCITY;
            let origin = Vector3::new(t[9], t[10], t[11]);
            let center = add(origin, mul_basis(basis, local_center));
            bodies.push(PropBody {
                instance: index,
                id: entry.id,
                local_center,
                half_extents,
                template: object.name.clone(),
                tuning: PropTuning::default(),
                authored_center: local_center,
                authored_half_extents: half_extents,
                axis_scale: Vector3::new(axis_scale[0], axis_scale[1], axis_scale[2]),
                stuck_ticks: 0,
                pushed_by: None,
                baked: None,
                contacts: 0,
                rest_y: center.y,
                spawn_origin: origin,
                spawn_basis: basis,
                commanded: false,
                commanded_block: false,
                upright_timer: None,
                rates: RetailBodyRates {
                    orientation: quaternion_from_basis(basis),
                    basis,
                    world_inverse_inertia: world_inverse_inertia(basis, inertia.inverse_tensor),
                    position: center,
                    linear_velocity: Vector3::ZERO,
                    angular_velocity: Vector3::ZERO,
                    force_acceleration: scale(
                        simulation.gravity_acceleration,
                        authored.gravity_scale,
                    ),
                    torque_acceleration: Vector3::ZERO,
                    kinetic_energy: 0.0,
                    cool_down: simulation.cool_down,
                },
                inertia,
                material: RetailContactMaterial {
                    static_friction: authored.friction,
                    dynamic_friction: authored.friction,
                    restitution: authored.restitution,
                },
                enable_sleep: authored.enable_sleep,
                asleep: !authored.initially_awake,
            });
            by_id.insert(entry.id, bodies.len() - 1);
        }
        Self {
            bodies,
            by_id,
            simulation,
            pair: PrimitivePairSettings {
                padding_a: PropTuning::default().contact_padding,
                padding_b: PropTuning::default().contact_padding,
                additional_padding: 0.0,
                edge_cos_bend_normal_threshold: 0.999,
                convexity_epsilon: 0.01,
            },
            held: None,
            tuning: PropTuningTable::default(),
            stats: PropStepStats::default(),
            tick: 0,
            released: None,
            below_logged: std::collections::BTreeMap::new(),
            move_diagnostics: None,
            move_rules: MoveCommandRules::default(),
        }
    }

    /// Move Object command rules in effect.
    pub(crate) fn move_rules(&self) -> &MoveCommandRules {
        &self.move_rules
    }

    /// Replace the Move Object command rules (mod tuning; `Default` restores
    /// retail on mod disable).
    pub(crate) fn set_move_rules(&mut self, rules: MoveCommandRules) {
        self.move_rules = rules;
    }

    /// Body `index`'s own material block: what retail copies into its
    /// collision objects (CO +116..+124) before the pair combine.
    fn body_material(&self, index: usize) -> RetailContactMaterial {
        let body = &self.bodies[index];
        self.move_rules.body_material(&body.template, body.material, body.commanded_block, body.rates.basis.columns[1][1])
    }

    /// Contact material of body `index` against a surface material: the
    /// retail pair combine 82763078 of the body's block and the other side.
    fn contact_material(&self, index: usize, other: RetailContactMaterial) -> RetailContactMaterial {
        combine_contact_materials(self.body_material(index), other)
    }

    /// Current tuning table (defaults plus per prop type overrides).
    pub(crate) fn tuning(&self) -> &PropTuningTable {
        &self.tuning
    }

    /// Replace the tuning table and re-resolve every body. A changed
    /// `collision_box` moves the box centre so the template origin (the
    /// rendered pose) stays where it is; mass properties are unchanged.
    pub(crate) fn set_tuning(&mut self, table: PropTuningTable) {
        for body in &mut self.bodies {
            let tuning = *table.for_template(&body.template);
            // Untouched prop types stay asleep: a mod changing one type must
            // not wake (and rebake) every prop on the map.
            if tuning == body.tuning {
                continue;
            }
            let origin = body.origin();
            let (center, half) = match tuning.collision_box {
                Some(override_box) => (
                    mul_components(override_box.center, body.axis_scale),
                    mul_components(override_box.half_extents, body.axis_scale),
                ),
                None => (body.authored_center, body.authored_half_extents),
            };
            body.local_center = center;
            body.half_extents = Vector3::new(half.x.abs(), half.y.abs(), half.z.abs());
            body.rates.position = add(origin, mul_basis(body.rates.basis, center));
            body.tuning = tuning;
            body.stuck_ticks = 0;
            body.wake();
        }
        self.tuning = table;
    }

    /// Restore the shipped defaults (mod disable).
    /// (Mod API entry point; the Lua binding is a follow-up.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reset_tuning(&mut self) {
        self.set_tuning(PropTuningTable::default());
    }

    /// Stable actor id of the skater that last pushed prop `id` ([`LOCAL_PUSHER`] or an NPC
    /// skater proxy id); `None` if nothing has pushed it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pushed_by(&self, id: u32) -> Option<u64> {
        self.bodies.get(*self.by_id.get(&id)?)?.pushed_by
    }

    /// Work done by the last `step`.
    /// (Mod API entry point; the Lua binding is a follow-up.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn last_step_stats(&self) -> PropStepStats {
        self.stats
    }

    /// Prop volume against static world triangles: the physics/world query
    /// (TU3 8277B720 via triangle dispatch 8277BC58), NOT the GP volume-pair
    /// query 82AD43A8. The world query forwards the query context's object
    /// byte (+61) into triangle fixup (82AD3130), which for objects accepts a
    /// contact across a welded flat edge (cosine 1, no convex bit) while the
    /// normal stays within convexity_epsilon of the face (acos(1 - 0.01) =
    /// 8.1 deg) and does not reject disabled vertices. The pair query passes
    /// false and drops every such contact: a box lying a few degrees tilted
    /// on a tiled floor lost all floor manifolds and fell through (docs 26,
    /// "contact gap"). Limit = the body's own padding, the same gap the
    /// static resolver accepts (`gap > contact_padding` is skipped there);
    /// no velocity prediction, as the prop solver has no speculative rows.
    fn world_query_for(&self, index: usize) -> WorldContactSettings {
        WorldContactSettings {
            volume_padding: self.bodies[index].tuning.contact_padding,
            maximum_separating_distance: 0.0,
            edge_cos_bend_normal_threshold: self.pair.edge_cos_bend_normal_threshold,
            convexity_epsilon: self.pair.convexity_epsilon,
            is_object: true,
        }
    }

    /// Pair query settings with this body's contact band.
    fn pair_for(&self, index: usize) -> PrimitivePairSettings {
        let padding = self.bodies[index].tuning.contact_padding;
        PrimitivePairSettings {
            padding_a: padding,
            padding_b: padding,
            ..self.pair
        }
    }

    /// Template-origin pose of one prop for render sync.
    pub(crate) fn pose(&self, id: u32) -> Option<(Vector3, Basis3)> {
        let body = self.bodies.get(*self.by_id.get(&id)?)?;
        Some((body.origin(), body.rates.basis))
    }

    /// Every body as a navigation obstacle for peds (doc 26, fix 11): id, box centre, basis,
    /// half extents (the contact box, so a `collision_box` override counts), linear velocity and
    /// whether it is carried (retail switches a carried object's obstacle off). Id order.
    pub(crate) fn obstacle_boxes(&self) -> Vec<(u32, Vector3, Basis3, Vector3, Vector3, bool)> {
        let mut out: Vec<_> = self
            .bodies
            .iter()
            .map(|b| (b.id, b.rates.position, b.rates.basis, b.half_extents, b.rates.linear_velocity, self.held == Some(b.id)))
            .collect();
        out.sort_by_key(|b| b.0);
        out
    }

    /// World position of one body's box centre.
    pub(crate) fn position_of(&self, id: u32) -> Option<Vector3> {
        Some(self.bodies.get(*self.by_id.get(&id)?)?.rates.position)
    }

    /// Nearest body within `radius` of `point`, measured to the box SURFACE
    /// (not the centre, so big ramps are grabbable by their edge), as
    /// `(id, centre)`.
    pub(crate) fn nearest_body(&self, point: Vector3, radius: f32) -> Option<(u32, Vector3)> {
        let mut best: Option<(u32, Vector3, f32)> = None;
        for body in &self.bodies {
            // Local-space point clamped into the box: the gap vector to it is
            // the surface distance (zero when the point is inside).
            let d = sub(point, body.rates.position);
            let b = body.rates.basis.columns;
            let local = [
                d.x * b[0][0] + d.y * b[0][1] + d.z * b[0][2],
                d.x * b[1][0] + d.y * b[1][1] + d.z * b[1][2],
                d.x * b[2][0] + d.y * b[2][1] + d.z * b[2][2],
            ];
            let he = body.half_extents;
            let gap = Vector3::new(
                (local[0].abs() - he.x).max(0.0),
                (local[1].abs() - he.y).max(0.0),
                (local[2].abs() - he.z).max(0.0),
            );
            let distance_squared = dot(gap, gap);
            if distance_squared > radius * radius {
                continue;
            }
            if best.map_or(true, |(_, _, b)| distance_squared < b) {
                best = Some((body.id, body.rates.position, distance_squared));
            }
        }
        best.map(|(id, position, _)| (id, position))
    }

    /// Collision-layer instance index for one body (rebake target).
    pub(crate) fn instance_of(&self, id: u32) -> Option<usize> {
        Some(self.bodies.get(*self.by_id.get(&id)?)?.instance)
    }

    /// Mark the carried prop: it stops receiving skater pushes and its layer
    /// triangles stay parked until the drop rebakes them.
    pub(crate) fn set_held(&mut self, held: Option<u32>) {
        if self.held == held {
            return;
        }
        self.move_diagnostics = None;
        // PROP_HELD: the grab / release edge itself (the HELD_PROP lines are periodic).
        let describe = |id: Option<u32>| {
            id.and_then(|id| self.by_id.get(&id).map(|&i| (id, &self.bodies[i]))).map_or("none".to_string(), |(id, b)| {
                let (p, v) = (b.rates.position, b.rates.linear_velocity);
                format!("#{id} {} at=[{:.2}, {:.2}, {:.2}] velocity=[{:.2}, {:.2}, {:.2}]", b.template, p.x, p.y, p.z, v.x, v.y, v.z)
            })
        };
        info!("PROP_HELD from={} to={} tick={}", describe(self.held), describe(held), self.tick);
        if let Some(previous) = self.held {
            let ticks = (RELEASE_LOG_SECONDS / self.simulation.time_step.max(1e-4)).ceil() as u64;
            self.released = Some((previous, self.tick + ticks));
        }
        if let Some(index) = held.and_then(|id| self.by_id.get(&id).copied()) {
            let body = &mut self.bodies[index];
            body.rest_y = body.rest_y.max(body.rates.position.y);
        }
        self.held = held;
    }

    /// Ground relation of one prop (centre, world half height, floor under it).
    /// (Diagnostics and tests; a mod-facing readout is a follow-up.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn ground_probe(&self, id: u32, world: &BoardWorld) -> Option<PropGroundProbe> {
        Some(self.probe(*self.by_id.get(&id)?, world))
    }

    fn probe(&self, index: usize, world: &BoardWorld) -> PropGroundProbe {
        let body = &self.bodies[index];
        let bounds = body.bounds();
        let center = body.rates.position;
        let half_height = 0.5 * (bounds.max.y - bounds.min.y);
        // From above the last rest height (a sunk body still finds the floor
        // it fell through), down well past the body.
        let top = center.y.max(body.rest_y) + half_height + 0.5;
        let ground = world
            .query_thin_line(
                Vector3::new(center.x, top, center.z),
                Vector3::new(center.x, center.y - half_height - 30.0, center.z),
            )
            .ok()
            .flatten()
            .map(|hit| hit.geometry.position.y);
        PropGroundProbe { center, half_height, ground }
    }

    /// HELD_PROP (held prop ~1/s, released prop ~1/s for `RELEASE_LOG_SECONDS`)
    /// and PROP_BELOW_GROUND (an awake prop centre more than its half height
    /// under the floor, once per prop per `BELOW_GROUND_LOG_SECONDS`). Probes
    /// run only for the held/released prop and, twice a second, awake bodies.
    fn log_diagnostics(&mut self, world: &BoardWorld) {
        let per_second = (1.0 / self.simulation.time_step.max(1e-4)).round().max(1.0) as u64;
        if self.released.is_some_and(|(_, until)| self.tick > until) {
            self.released = None;
        }
        let held_every = if crate::trace_all::on() { (per_second / HELD_PROP_HZ_TRACE_ALL).max(1) } else { per_second };
        if self.tick % held_every == 0 {
            let watched = [self.held.map(|id| (id, "held")), self.released.map(|(id, _)| (id, "released"))];
            for (id, phase) in watched.into_iter().flatten() {
                let Some(&index) = self.by_id.get(&id) else { continue };
                let probe = self.probe(index, world);
                let body = &self.bodies[index];
                let v = body.rates.linear_velocity;
                let up = body.rates.basis.columns[1];
                let mv = if phase == "held" { self.move_diagnostics } else { None };
                info!(
                    "HELD_PROP id={id} phase={phase} template={} center=[{:.2}, {:.2}, {:.2}] up_y={:.3} velocity=[{:.2}, {:.2}, {:.2}] ground={} gap={} contacts={} asleep={} {} tick={}",
                    body.template, probe.center.x, probe.center.y, probe.center.z, up[1], v.x, v.y, v.z,
                    probe.ground.map_or("none".into(), |g| format!("{g:.2}")),
                    probe.gap().map_or("none".into(), |g| format!("{g:.2}")),
                    body.contacts, body.asleep, move_fields(mv), self.tick
                );
            }
        }
        if self.tick % (per_second / 2).max(1) != 0 {
            return;
        }
        let quiet = (BELOW_GROUND_LOG_SECONDS * per_second as f32) as u64;
        for index in 0..self.bodies.len() {
            if self.bodies[index].asleep {
                continue;
            }
            let id = self.bodies[index].id;
            if self.below_logged.get(&id).is_some_and(|&at| self.tick < at + quiet) {
                continue;
            }
            let probe = self.probe(index, world);
            if !probe.below_ground() {
                continue;
            }
            self.below_logged.insert(id, self.tick);
            let body = &self.bodies[index];
            let v = body.rates.linear_velocity;
            info!(
                "PROP_BELOW_GROUND id={id} template={} center=[{:.2}, {:.2}, {:.2}] half_height={:.2} ground={:.2} rest_y={:.2} velocity=[{:.2}, {:.2}, {:.2}] up_y={:.3} held={} contacts={} tick={}",
                body.template, probe.center.x, probe.center.y, probe.center.z, probe.half_height,
                probe.ground.unwrap_or(f32::NAN), body.rest_y, v.x, v.y, v.z, body.rates.basis.columns[1][1],
                self.held == Some(id), body.contacts, self.tick
            );
        }
    }

    fn is_held(&self, index: usize) -> bool {
        self.held == Some(self.bodies[index].id)
    }

    /// The held prop as Move Object reads it (centre, box axes, half
    /// extents, velocity, mass, yaw inertia): the interim grab record's source.
    pub(crate) fn held_body(&self, id: u32) -> Option<crate::physics::prop_carry::HeldBody> {
        let body = self.bodies.get(*self.by_id.get(&id)?)?;
        let inverse_yaw = body.inertia.inverse_tensor.y;
        Some(crate::physics::prop_carry::HeldBody {
            center: body.rates.position,
            basis: body.rates.basis,
            half_extents: body.half_extents,
            velocity: body.rates.linear_velocity,
            mass: if body.inertia.inverse_mass > 0.0 { 1.0 / body.inertia.inverse_mass } else { f32::INFINITY },
            yaw_inertia: if inverse_yaw > 0.0 { 1.0 / inverse_yaw } else { f32::INFINITY },
            record_272: self.move_rules.by_template.get(&body.template).and_then(|b| b.record_272).unwrap_or(false),
        })
    }

    /// Move Object command for the held prop, as retail interface slot 9
    /// applies it (spec section 7: 8275FF00 queue -> flush 827601F0 -> DMO
    /// handler 82C4C370 / 82C4C3E0 -> sinks 82D9CC78 / 82D9CCF0), once per
    /// 60 Hz physics step, before this step's contact solve:
    /// - skipped for an unknown, locked or non-dynamic body (retail gates
    ///   DMO+4464 bit 0x08 and component+36; props have no lock state yet);
    /// - wakes the body and clears its sleep counter on EVERY command
    ///   (82ADF7B8), zero or not;
    /// - linear: an acceleration with no mass factor at the centre of mass
    ///   (`v += L dt`, no torque; the accumulator +144 already holds gravity,
    ///   which our integrator adds for every prop); the vertical argument is
    ///   dropped (82C4C370 forwards only &linear);
    /// - angular: the yaw command replaces the angular accumulator (+160 =
    ///   (0, Y, 0), no inertia factor): any torque queued on the body is
    ///   discarded and `w += (0, Y, 0) dt`; contacts still change pitch and
    ///   roll in the solve;
    /// - marks the body commanded, so its parameter block switches to the
    ///   commanded block on its next step (82C53EF8).
    ///
    /// The command lasts one step: the carry re-sends it every tick and our
    /// prop step runs once per tick (no substeps). `grip` is used only when a
    /// mod turns `apply_at_com` off. Returns false if the id is unknown.
    pub(crate) fn apply_move_command(&mut self, id: u32, linear: Vector3, yaw: f32, grip: Vector3, time_step: f32) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let rules = &self.move_rules;
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        let body = &mut self.bodies[index];
        if body.inertia.inverse_mass <= 0.0 {
            return true;
        }
        let vertical = if rules.ignore_vertical { 0.0 } else { finite(linear.y) };
        let l = Vector3::new(finite(linear.x), vertical, finite(linear.z));
        // 82C52E68 refuses the angular command while the righting window
        // (DMO+4464 0x40) is open; the linear sink 82C52DC0 is not gated.
        let yaw_blocked = body.upright_timer.is_some() && self.tuning.upright.block_yaw;
        let yaw = if yaw_blocked { 0.0 } else { finite(yaw) };
        if rules.wake_on_command || l != Vector3::ZERO || yaw != 0.0 {
            body.wake();
        }
        body.commanded = true;
        body.rates.linear_velocity = add(body.rates.linear_velocity, scale(l, time_step));
        if !rules.apply_at_com {
            // Mod option: the same force (m L) at the grip point adds the
            // lever torque I^-1 (r x m L).
            let mass = 1.0 / body.inertia.inverse_mass;
            let r = sub(grip, body.rates.position);
            let spin = mul_basis(body.rates.world_inverse_inertia, cross(r, scale(l, mass)));
            body.rates.angular_velocity = add(body.rates.angular_velocity, scale(spin, time_step));
        }
        if yaw_blocked {
            return true;
        }
        if rules.yaw_replaces_torque {
            body.rates.torque_acceleration = Vector3::ZERO;
        }
        let w = body.rates.angular_velocity;
        body.rates.angular_velocity = Vector3::new(w.x, w.y + yaw * time_step, w.z);
        true
    }

    /// Start the self-righting window of one body (retail cMsgUprightDMO ->
    /// DMO manager slot +40 82C4B8C0: DMO+4464 |= 0x40, timer DMO+4376 = 0).
    /// Retail sets it only when the DMO's slot 28 test (82C564D8, physics body
    /// field +28) returns 0, the same test that offers Upright on the phone
    /// (82666748 -> 82C4B578); that field is not decoded, so ours refuses only
    /// an unknown id or a body without dynamics (NOT RETAIL YET). Restarting an
    /// open window zeroes the timer, as retail does. Returns false if refused.
    pub(crate) fn upright(&mut self, id: u32) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let body = &mut self.bodies[index];
        if body.inertia.inverse_mass <= 0.0 {
            return false;
        }
        body.upright_timer = Some(0.0);
        true
    }

    /// True while the body's righting window is open (DMO+4464 bit 0x40).
    pub(crate) fn is_uprighting(&self, id: u32) -> bool {
        self.by_id.get(&id).is_some_and(|&i| self.bodies[i].upright_timer.is_some())
    }

    /// Body vector A of 82C573D0 (retail: the vector at the physics state
    /// block +72; it scales the gain blend and picks the fallback axis by
    /// A.x > A.z). Which body quantity that block holds is not identified;
    /// ours uses the body-space inverse inertia diagonal (NOT RETAIL YET).
    fn upright_vector(body: &PropBody) -> Vector3 {
        body.inertia.inverse_tensor
    }

    /// The DMO update's righting pass (82C56780), once per step before the
    /// contact solve, for every body with an open window (asleep or not): the
    /// timer advances and closes the window past `window_seconds` (that
    /// update still sends its command); a tilt under `stop_angle_deg` closes
    /// it and zeroes the timer; otherwise the command goes through the DMO's
    /// angular slot 37 (82C52EE0): skipped for a body without dynamics, else
    /// wake, mark commanded (DMO+4465 0x02) and replace the angular
    /// accumulator (82D9CCF0 writes +160), integrated like the Move Object
    /// yaw command (`w += C dt`).
    fn apply_upright(&mut self, time_step: f32) {
        let settings = self.tuning.upright;
        let replaces = self.move_rules.yaw_replaces_torque;
        for body in &mut self.bodies {
            let Some(timer) = body.upright_timer else { continue };
            let timer = timer + settings.tick_seconds;
            body.upright_timer = (timer <= settings.window_seconds).then_some(timer);
            let a = Self::upright_vector(body);
            let Some(command) = upright_command(body.rates.basis, body.rates.angular_velocity, a, &settings) else {
                body.upright_timer = None;
                continue;
            };
            if body.inertia.inverse_mass <= 0.0 {
                continue;
            }
            body.wake();
            body.commanded = true;
            if replaces {
                body.rates.torque_acceleration = Vector3::ZERO;
            }
            body.rates.angular_velocity = add(body.rates.angular_velocity, scale(command, time_step));
        }
    }

    /// Move Object values for the HELD_PROP line (stick, command, lever,
    /// rotation demand, blocked flag, latch drift).
    pub(crate) fn set_move_diagnostics(&mut self, diagnostics: Option<MoveDiagnostics>) {
        self.move_diagnostics = diagnostics;
    }

    /// Kinematic follow while carried: wake and steer the body toward
    /// `target` by velocity (never teleport), capped at `max_speed`, with
    /// rotation frozen. Returns false if the id is unknown.
    pub(crate) fn carry_to(
        &mut self,
        id: u32,
        target: Vector3,
        max_speed: f32,
        time_step: f32,
    ) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let body = &mut self.bodies[index];
        body.wake();
        let delta = sub(target, body.rates.position);
        let distance = dot(delta, delta).sqrt();
        let speed = (distance / time_step).min(max_speed);
        body.rates.linear_velocity = if distance > 1e-6 {
            scale(delta, speed / distance)
        } else {
            Vector3::ZERO
        };
        body.rates.angular_velocity = Vector3::ZERO;
        true
    }

    /// Placement follow: like `carry_to`, but also snaps the orientation to
    /// `basis` (ghost yaw edit). Position still moves by velocity only.
    pub(crate) fn carry_to_pose(
        &mut self,
        id: u32,
        target: Vector3,
        basis: Basis3,
        max_speed: f32,
        time_step: f32,
    ) -> bool {
        if !self.carry_to(id, target, max_speed, time_step) {
            return false;
        }
        let body = &mut self.bodies[self.by_id[&id]];
        body.rates.basis = basis;
        body.rates.orientation = quaternion_from_basis(basis);
        body.rates.world_inverse_inertia =
            world_inverse_inertia(basis, body.inertia.inverse_tensor);
        true
    }

    /// Confirming a placement sets the prop down gently: velocity zeroed so
    /// the leftover follow velocity does not throw it.
    pub(crate) fn release_still(&mut self, id: u32) {
        if let Some(&index) = self.by_id.get(&id) {
            self.bodies[index].rates.linear_velocity = Vector3::ZERO;
            self.bodies[index].rates.angular_velocity = Vector3::ZERO;
        }
    }

    /// Teleport a body to a saved layout pose, asleep. Returns the collision
    /// instance index so the caller can rebake its triangles.
    pub(crate) fn teleport(&mut self, id: u32, origin: Vector3, basis: Basis3) -> Option<usize> {
        let cool_down = self.step_simulation().cool_down;
        let body = self.bodies.get_mut(*self.by_id.get(&id)?)?;
        body.rates.basis = basis;
        body.rates.orientation = quaternion_from_basis(basis);
        body.rates.world_inverse_inertia =
            world_inverse_inertia(basis, body.inertia.inverse_tensor);
        body.rates.position = add(origin, mul_basis(basis, body.local_center));
        body.rates.linear_velocity = Vector3::ZERO;
        body.rates.angular_velocity = Vector3::ZERO;
        body.rates.kinetic_energy = 0.0;
        body.rates.cool_down = cool_down;
        body.asleep = true;
        Some(body.instance)
    }

    /// Ids of bodies whose pose differs from the authored spawn pose, in id order.
    /// Retail offers the per-object reset (cMsgResetDMO) only for a moved object
    /// (sub_826666A8 gate); this is the engine-side list behind "reset moved objects".
    pub(crate) fn moved_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .bodies
            .iter()
            .filter(|b| {
                let o = b.origin();
                let d = sub(o, b.spawn_origin);
                d.x * d.x + d.y * d.y + d.z * d.z > SPAWN_POSE_EPSILON * SPAWN_POSE_EPSILON
                    || b.rates.basis.columns.iter().zip(b.spawn_basis.columns.iter()).any(|(a, s)| {
                        a.iter().zip(s.iter()).any(|(x, y)| (x - y).abs() > SPAWN_POSE_EPSILON)
                    })
            })
            .map(|b| b.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Authored spawn pose of one body (template origin, basis).
    pub(crate) fn spawn_pose(&self, id: u32) -> Option<(Vector3, Basis3)> {
        let body = self.bodies.get(*self.by_id.get(&id)?)?;
        Some((body.spawn_origin, body.spawn_basis))
    }

    /// Return one body to its authored spawn pose, at rest and asleep (the
    /// receiving end of retail's cMsgResetDMO; how retail moves it, teleport or
    /// fade, is not decoded yet: NOT RETAIL YET, an instant teleport). Returns the
    /// collision instance so the caller can rebake its triangles.
    pub(crate) fn reset_to_spawn(&mut self, id: u32) -> Option<usize> {
        let (origin, basis) = self.spawn_pose(id)?;
        if self.held == Some(id) {
            self.set_held(None);
        }
        self.teleport(id, origin, basis)
    }

    /// Advance awake bodies one tick; wake bodies the skater touches. Moved
    /// instances re-bake their triangles in the collision layer afterwards.
    pub(crate) fn step(
        &mut self,
        world: &BoardWorld,
        layer: &mut crate::skate_world::PropCollisionLayer,
        skater_volumes: &[BoardWorldVolume],
    ) {
        self.step_with_actors(world, layer, skater_volumes, &[]);
    }

    /// [`Self::step`] with other skaters' volumes too (NPC skaters, doc 26 fix 19): retail NPC
    /// skaters are full skaters, so their board and body push a DMO by the same contact rule as
    /// the player's. `others` = (stable actor id, volume), sorted by actor id; the local
    /// player's volumes go first ([`LOCAL_PUSHER`]), so the push order is deterministic.
    pub(crate) fn step_with_actors(
        &mut self,
        world: &BoardWorld,
        layer: &mut crate::skate_world::PropCollisionLayer,
        skater_volumes: &[BoardWorldVolume],
        others: &[(u64, BoardWorldVolume)],
    ) {
        self.stats = PropStepStats::default();
        let time_step = self.step_simulation().time_step;
        self.apply_upright(time_step);
        for index in 0..self.bodies.len() {
            let held = self.is_held(index);
            // Skater push: cheap bounds reject, then the retail pair query.
            // The carried prop is velocity-driven by the carrier; letting the
            // skater push it (or be pushed by it) fights the drag.
            if !held {
                let body_bounds = self.bodies[index].bounds();
                let mut overlapping = false;
                let mut closing = false;
                let actors = skater_volumes
                    .iter()
                    .map(|v| (LOCAL_PUSHER, v))
                    .chain(others.iter().map(|(actor, v)| (*actor, v)));
                for (actor, volume) in actors {
                    let Some(volume_bounds) = volume_bounds(volume.primitive) else {
                        continue;
                    };
                    if !body_bounds.overlaps(volume_bounds) {
                        continue;
                    }
                    let box_primitive = self.bodies[index].box_primitive();
                    let Some(manifold) = primitive_pair_contacts(
                        volume.primitive,
                        box_primitive,
                        self.pair_for(index),
                    ) else {
                        continue;
                    };
                    let pushes = self.stats.skater_pushes;
                    let contact = self.push_from_skater(index, volume, &manifold);
                    if self.stats.skater_pushes > pushes {
                        self.bodies[index].pushed_by = Some(actor);
                    }
                    match contact {
                        SkaterContact::Closing => closing = true,
                        SkaterContact::OverlapOnly => overlapping = true,
                        SkaterContact::Separate => {}
                    }
                }
                // A skater volume parked inside the box (the board under a
                // bench seat, inside the render AABB) would otherwise nudge
                // the prop every tick forever: it never sleeps and rebakes
                // its triangles every tick. Count those ticks; past
                // `stuck_release_ticks` only real hits push it.
                let body = &mut self.bodies[index];
                body.stuck_ticks = if closing {
                    0
                } else if overlapping {
                    body.stuck_ticks.saturating_add(1)
                } else {
                    0
                };
            }
        }
        let solver = self.tuning.solver;
        let simulation = self.step_simulation();
        // Awake bodies in body order (deterministic; plain indices).
        let awake: Vec<usize> = (0..self.bodies.len()).filter(|&i| !self.bodies[i].asleep).collect();
        self.stats.awake = awake.len() as u32;
        // Parameter block swap (82C53EF8): a body commanded since its last
        // step runs on the commanded block, otherwise on its free block;
        // the switch happens on the step the commanded bit changes. The
        // command flag lasts one step (retail shifts 0x02 -> 0x01).
        let mut commanded_now = vec![false; self.bodies.len()];
        for &index in &awake {
            let body = &mut self.bodies[index];
            commanded_now[index] = body.commanded;
            body.commanded_block = body.commanded;
            body.commanded = false;
        }
        // Retail: every awake body's contact rows go through one shared row
        // solve (contact stage 82DC30A8 -> 82AE27D0, island +176 iterations)
        // and only then does any body integrate (BatchIntegrator).
        let rows = solver.row_solver.then(|| self.row_corrections(world, &awake, solver.iterations));
        let mut sleeps = 0u32;
        for &index in &awake {
            let held = self.is_held(index);
            let commanded = commanded_now[index];
            // The held prop is a normal dynamic body: pitch, roll, gravity
            // and contacts stay with the simulation (retail Move Object sends
            // only a horizontal + yaw command, 82D45318), so it can tip and
            // the floor holds it.
            let corrections = match &rows {
                Some(rows) => rows[index],
                None => self.contact_corrections(index, world),
            };
            // A commanded (or held) body never snaps to rest or sleeps.
            // Sleep: retail clears the sleep counter on every command
            // (82ADF7B8 in both slot 9 sinks), so a commanded body cannot
            // freeze. Held covers placement (`carry_to`, NOT RETAIL).
            let body_sleep_capable = self.bodies[index].enable_sleep && !held && !commanded;
            // Rest snap (NOT RETAIL, mod option `rest_snap`): only while
            // something is touching the body, so it never zeroes a fall.
            let resting = solver.rest_snap
                && body_sleep_capable
                && (dot(corrections.linear_displacement, corrections.linear_displacement)
                    > 0.0
                    || dot(corrections.position_displacement, corrections.position_displacement)
                        > 0.0
                    || dot(corrections.angular_displacement, corrections.angular_displacement)
                        > 0.0);
            let body = &mut self.bodies[index];
            // Integrator 82AE6590: E = |v|^2 + s m^-1 |w|^2 after damping and
            // the caps; counter = 0 when E >= island +172, else +1 if E did
            // not rise, capped at island +168.
            let step = integrate_body_rates(body.rates, body.inertia, simulation, corrections);
            body.rates = step.state;
            if resting && body.rates.kinetic_energy < simulation.minimum_energy {
                body.rates.linear_velocity = Vector3::ZERO;
                body.rates.angular_velocity = Vector3::ZERO;
                body.rates.kinetic_energy = 0.0;
                // The snap zeroes the energy the integrator compares against,
                // so its counter stalls; count snapped resting ticks here.
                body.rates.cool_down = (body.rates.cool_down + 1).min(simulation.cool_down);
            }
            // Sleep pass 82DC3130: counter >= sim +204 (= island +168) moves
            // the body to the sleeping list, at most 100 bodies per call.
            if body_sleep_capable
                && body.rates.cool_down >= solver.sleep_frames
                && sleeps < solver.max_sleeps_per_step
            {
                sleeps += 1;
                body.asleep = true;
                body.rates.cool_down = simulation.cool_down;
                body.rest_y = body.rates.position.y;
            }
            // The held prop's triangles stay parked (set_held/HELD_PARK) so
            // skater queries never see them while carrying.
            if held {
                continue;
            }
            // An unchanged pose would rebake identical triangles (and rebuild
            // the layer's query index for nothing); skip it.
            let body = &self.bodies[index];
            let pose = (body.origin(), body.rates.basis);
            if body.baked.is_some_and(|baked| same_pose(baked, pose)) {
                continue;
            }
            if let Err(error) = layer.rebake(body.instance, pose.1.columns, pose.0) {
                warn!("SKATE_PROP_DYNAMICS: rebake instance {}: {error}", body.instance);
            }
            self.bodies[index].baked = Some(pose);
            self.stats.rebakes += 1;
        }
        self.log_diagnostics(world);
        self.tick += 1;
    }

    /// Skater volumes treat the prop as a pushable weight: the prop receives a
    /// fraction of the closing speed through the reduced mass of the pair, as
    /// an impulse at the contact point. The skater's own response still comes
    /// from the exact triangle layer.
    fn push_from_skater(
        &mut self,
        index: usize,
        volume: &BoardWorldVolume,
        manifold: &PrimitiveContactManifold,
    ) -> SkaterContact {
        let tuning = self.bodies[index].tuning;
        // The overlap nudge stops once the volume has sat inside for
        // `stuck_release_ticks`; from then on slow drift (solver jitter of a
        // parked board) does not push either, only a real hit does.
        let stuck = self.bodies[index].stuck_ticks >= tuning.stuck_release_ticks;
        let nudge = if stuck { 0.0 } else { tuning.penetration_push_speed };
        let hit_floor = if stuck { tuning.penetration_push_speed } else { 0.0 };
        // The manifold normal points from the prop (B) toward the skater (A).
        let push = scale(manifold.normal, -1.0);
        let mut strongest = 0.0_f32;
        let mut point = self.bodies[index].rates.position;
        let mut any_closing = false;
        let mut any_overlap = false;
        for pair in &manifold.points[..manifold.count] {
            let closing = dot(sub(volume.motion.linear_velocity, self.bodies[index].velocity_at(pair.b)), push);
            let closing = if closing > hit_floor { closing } else { 0.0 };
            let penetration = dot(sub(pair.a, pair.b), manifold.normal);
            any_closing |= closing > 0.0;
            any_overlap |= penetration < 0.0;
            let drive = closing.max(if penetration < 0.0 { nudge } else { 0.0 });
            if drive > strongest {
                strongest = drive;
                point = pair.b;
            }
        }
        let contact = if any_closing {
            SkaterContact::Closing
        } else if any_overlap {
            SkaterContact::OverlapOnly
        } else {
            SkaterContact::Separate
        };
        if strongest <= 0.0 {
            return contact;
        }
        // Momentum-style transfer: the skater shares its closing speed through
        // the reduced mass of the pair, so a 20 kg box skips away while a
        // 500 kg ramp barely budges. Δv = strongest × transfer × M/(M+m).
        // Board hits carry the full transfer; body bumps are capped to a
        // nudge speed so walking into a prop cannot keep accelerating it.
        // Board and body pushes are both capped along the push direction
        // (#15 capped only body bumps), so a board held against a prop cannot
        // keep accelerating it either.
        let mass = 1.0 / self.bodies[index].inertia.inverse_mass;
        let reduced = mass * tuning.skater_push_mass / (tuning.skater_push_mass + mass);
        let mut amount = strongest * reduced * tuning.push_transfer;
        let cap = if matches!(
            volume.body,
            skate_core::physics::board_step::CollisionBody::Board(_)
        ) {
            tuning.board_push_speed
        } else {
            tuning.body_push_speed
        };
        let along = dot(self.bodies[index].rates.linear_velocity, push);
        let allowed = (cap - along).max(0.0) * mass;
        amount = amount.min(allowed);
        if amount <= 0.0 {
            return contact;
        }
        let impulse = scale(push, amount);
        self.bodies[index].apply_impulse(impulse, point);
        self.stats.skater_pushes += 1;
        contact
    }

    /// The simulation step with the live island settings (sleep energy and
    /// counter cap from [`PropSolverSettings`]).
    fn step_simulation(&self) -> RetailSimulationStep {
        let solver = self.tuning.solver;
        RetailSimulationStep {
            cool_down: solver.sleep_frames,
            minimum_energy: solver.sleep_energy,
            ..self.simulation
        }
    }

    /// Solver-side state of an awake prop (reaction slot = body index).
    fn row_body(&self, index: usize) -> RetailContactBodyState {
        let body = &self.bodies[index];
        let inertia = pack_world_inverse_inertia(body.rates.world_inverse_inertia);
        RetailContactBodyState {
            contact_body_id: index as u32,
            reaction_id: index as u32,
            center_of_mass: body.rates.position,
            inverse_inertia_full: inertia.full,
            inverse_inertia_split: inertia.split,
            inverse_mass: body.inertia.inverse_mass,
            state: ACTIVE_BODY,
            force_acceleration: body.rates.force_acceleration,
            torque_acceleration: body.rates.torque_acceleration,
            linear_velocity: body.rates.linear_velocity,
            angular_velocity: body.rates.angular_velocity,
            kinetic_energy: body.rates.kinetic_energy,
            cool_down: body.rates.cool_down,
        }
    }

    /// Solver-side state of an immovable support: the static world, or an
    /// asleep prop (inactive, so the row solver gives it no response).
    fn row_support(&self, world_reaction: usize, center: Vector3) -> RetailContactBodyState {
        RetailContactBodyState {
            contact_body_id: u32::MAX,
            reaction_id: world_reaction as u32,
            center_of_mass: center,
            inverse_inertia_full: Vector3::ZERO,
            inverse_inertia_split: Vector3::ZERO,
            inverse_mass: 0.0,
            state: 0,
            force_acceleration: Vector3::ZERO,
            torque_acceleration: Vector3::ZERO,
            linear_velocity: Vector3::ZERO,
            angular_velocity: Vector3::ZERO,
            kinetic_energy: 0.0,
            cool_down: 0,
        }
    }

    /// Retail contact solve for every awake prop at once: one row per
    /// manifold point (A = the prop, B = the triangle or the other prop,
    /// normal from B toward A as the pair queries return it), built by the
    /// retail ContactBatchBuild (82AE10C8: targets in displacement units,
    /// predicted separation v dt + separation + a dt^2, restitution -v dt e)
    /// and iterated `iterations` times by 82AE27D0 (contacts only). Returns
    /// the per-body correction buffers, indexed by body (the integrator turns
    /// the +0 / +32 pair into velocity, +16 / +48 into position only).
    /// No slop, no correction fraction, no per-tick cap (retail has none).
    fn row_corrections(
        &mut self,
        world: &BoardWorld,
        awake: &[usize],
        iterations: u32,
    ) -> Vec<RetailReactionCorrections> {
        let count = self.bodies.len();
        let world_reaction = count;
        let dt = self.simulation.time_step;
        let mut rows = Vec::new();
        let mut wake = Vec::new();
        for &index in awake {
            self.bodies[index].contacts = 0;
        }
        let push_rows = |rows: &mut Vec<RetailContactJacobian>,
                             manifold: &PrimitiveContactManifold,
                             material: RetailContactMaterial,
                             a: RetailContactBodyState,
                             b: RetailContactBodyState| {
            for pair in &manifold.points[..manifold.count] {
                let contact = generate_contact(
                    RetailContactInput {
                        position_on_a: pair.a,
                        position_on_b: pair.b,
                        normal: manifold.normal,
                        restitution: material.restitution,
                        static_friction: material.static_friction,
                        dynamic_friction: material.dynamic_friction,
                        tag: 0,
                    },
                    a,
                    b,
                );
                rows.push(build_contact_jacobian(contact, dt));
            }
        };
        for &index in awake {
            let tuning = self.bodies[index].tuning;
            let box_primitive = self.bodies[index].box_primitive();
            let world_query = self.world_query_for(index);
            let pair_settings = self.pair_for(index);
            let bounds = self.bodies[index].bounds().expanded(tuning.contact_padding + 0.05);
            let a = self.row_body(index);
            let support = self.row_support(world_reaction, Vector3::ZERO);
            for range in world.candidate_ranges(Some(bounds)) {
                for triangle in &world.triangles()[range] {
                    let Some(manifold) = primitive_triangle_world_contacts(
                        box_primitive,
                        triangle.triangle,
                        Vector3::ZERO,
                        world_query,
                    ) else {
                        continue;
                    };
                    self.bodies[index].contacts += 1;
                    let material = self.contact_material(index, triangle.material);
                    push_rows(&mut rows, &manifold, material, a, support);
                }
            }
            for other in 0..count {
                let other_awake = !self.bodies[other].asleep;
                // Each awake pair once, from its lower index.
                if other == index || (other_awake && other < index) {
                    continue;
                }
                if !self.bodies[index].bounds().overlaps(self.bodies[other].bounds().expanded(tuning.contact_padding)) {
                    continue;
                }
                let Some(manifold) = primitive_pair_contacts(
                    box_primitive,
                    self.bodies[other].box_primitive(),
                    pair_settings,
                ) else {
                    continue;
                };
                self.bodies[index].contacts += 1;
                let material = self.contact_material(index, self.body_material(other));
                let b = if other_awake {
                    self.bodies[other].contacts += 1;
                    self.row_body(other)
                } else {
                    // An asleep prop is an immovable support; a hard hit
                    // (closing faster than 1 m/s) wakes it for the next step
                    // (NOT RETAIL YET: retail merges touching bodies into the
                    // island; the wake rule is ours).
                    let closing = manifold.points[..manifold.count]
                        .iter()
                        .map(|pair| dot(self.bodies[index].velocity_at(pair.a), manifold.normal))
                        .fold(0.0_f32, f32::min);
                    if closing < -1.0 {
                        wake.push(other);
                    }
                    self.row_support(world_reaction, self.bodies[other].rates.position)
                };
                push_rows(&mut rows, &manifold, material, a, b);
            }
        }
        let mut reactions = vec![RetailReactionCorrections::default(); count + 1];
        solve_constraints(&mut rows, &mut [], &mut [], &mut reactions, iterations);
        for other in wake {
            self.bodies[other].wake();
        }
        reactions.truncate(count);
        reactions
    }

    /// Impulse and positional corrections for one awake body against the
    /// static world and every other prop box (asleep props are immovable).
    /// The engine's older pass (NOT RETAIL), used when `row_solver` is off.
    fn contact_corrections(&mut self, index: usize, world: &BoardWorld) -> RetailReactionCorrections {
        let mut corrections = RetailReactionCorrections::default();
        let box_primitive = self.bodies[index].box_primitive();
        let tuning = self.bodies[index].tuning;
        let pair_settings = self.pair_for(index);
        let world_query = self.world_query_for(index);
        let bounds = self.bodies[index].bounds().expanded(tuning.contact_padding + 0.05);
        let mut contacts = 0u32;
        // All static manifolds of this tick first: every one resolves from the
        // same pre-contact velocity, so the closing impulse is shared over
        // ALL simultaneous points, not per triangle. Per-triangle sharing
        // applied the full impulse once per touching triangle (a box edge on
        // a tiled floor touches ~10), which launched a tipping bench at
        // 15 m/s (2026-10-08 street drag trace).
        let mut manifolds = Vec::new();
        for range in world.candidate_ranges(Some(bounds)) {
            for triangle in &world.triangles()[range] {
                let Some(manifold) = primitive_triangle_world_contacts(
                    box_primitive,
                    triangle.triangle,
                    Vector3::ZERO,
                    world_query,
                ) else {
                    continue;
                };
                contacts += 1;
                manifolds.push((manifold, triangle.material));
            }
        }
        let points: usize = manifolds.iter().map(|(m, _)| m.count.max(1)).sum();
        for (manifold, triangle_material) in &manifolds {
            let material = self.contact_material(index, *triangle_material);
            self.resolve_static(index, manifold, material, points as f32, &mut corrections);
        }
        let box_primitive = self.bodies[index].box_primitive();
        for other in 0..self.bodies.len() {
            if other == index {
                continue;
            }
            if !self.bodies[index].bounds().overlaps(self.bodies[other].bounds().expanded(tuning.contact_padding)) {
                continue;
            }
            let Some(manifold) = primitive_pair_contacts(
                box_primitive,
                self.bodies[other].box_primitive(),
                pair_settings,
            ) else {
                continue;
            };
            contacts += 1;
            let material = self.contact_material(index, self.body_material(other));
            if self.bodies[other].asleep {
                // An asleep prop is an immovable support; a hard hit wakes it.
                let closing = manifold.points[..manifold.count]
                    .iter()
                    .map(|pair| {
                        dot(
                            sub(
                                self.bodies[index].velocity_at(pair.a),
                                self.bodies[other].velocity_at(pair.b),
                            ),
                            manifold.normal,
                        )
                    })
                    .fold(0.0_f32, |a, b| a.min(b));
                self.resolve_static(index, &manifold, material, manifold.count.max(1) as f32, &mut corrections);
                if closing < -1.0 {
                    self.bodies[other].wake();
                }
            } else {
                self.resolve_dynamic(index, other, &manifold, material, &mut corrections);
            }
        }
        self.bodies[index].contacts = contacts;
        // Bounded depenetration: the per-point corrections add up (every
        // triangle and prop touching a deep body contributes), so a body
        // pushed deep into geometry would otherwise jump out in one tick.
        let cap = tuning.max_depenetration_per_tick;
        let depth = length(corrections.position_displacement);
        if cap.is_finite() && cap >= 0.0 && depth > cap {
            corrections.position_displacement =
                scale(corrections.position_displacement, cap / depth);
        }
        corrections
    }

    /// Resolve contacts against an immovable surface (static world or asleep
    /// prop). The manifold normal points from the surface toward this prop.
    fn resolve_static(
        &mut self,
        index: usize,
        manifold: &PrimitiveContactManifold,
        material: RetailContactMaterial,
        shared_points: f32,
        corrections: &mut RetailReactionCorrections,
    ) {
        let dt = self.simulation.time_step;
        let tuning = self.bodies[index].tuning;
        let count = manifold.count.max(1) as f32;
        let share = shared_points.max(count);
        for pair in &manifold.points[..manifold.count] {
            let normal = manifold.normal;
            let gap = dot(sub(pair.a, pair.b), normal);
            if gap > tuning.contact_padding {
                continue;
            }
            let body = &self.bodies[index];
            let r = sub(pair.a, body.rates.position);
            let velocity = body.velocity_at(pair.a);
            let vn = dot(velocity, normal);
            let inverse_mass = body.inertia.inverse_mass;
            let angular = mul_basis(body.rates.world_inverse_inertia, cross(r, normal));
            let denominator = inverse_mass + dot(normal, cross(angular, r));
            if denominator <= 1e-9 {
                continue;
            }
            if vn < 0.0 {
                let restitution = if vn < -tuning.restitution_threshold {
                    material.restitution.max(0.0)
                } else {
                    0.0
                };
                // Each manifold point applies its share of the impulse: with
                // N simultaneous points at the same closing speed (a face
                // landing flat), the unshared impulses would sum to N× the
                // needed correction and bounce the body off the surface.
                let impulse = -(1.0 + restitution) * vn / (denominator * share);
                let mut delta = scale(normal, impulse * inverse_mass);
                let mut spin = mul_basis(
                    body.rates.world_inverse_inertia,
                    cross(r, scale(normal, impulse)),
                );
                // Retail combine friction caps the tangential impulse.
                let tangent = sub(velocity, scale(normal, vn));
                let speed = length(tangent);
                if speed > 1e-6 {
                    let limit = material.dynamic_friction.max(0.0) * impulse;
                    let friction = (-speed / denominator).clamp(-limit, limit);
                    let direction = scale(tangent, 1.0 / speed);
                    delta = add(delta, scale(direction, friction * inverse_mass));
                    spin = add(
                        spin,
                        mul_basis(
                            body.rates.world_inverse_inertia,
                            cross(r, scale(direction, friction)),
                        ),
                    );
                }
                corrections.linear_displacement =
                    add(corrections.linear_displacement, scale(delta, dt));
                corrections.angular_displacement =
                    add(corrections.angular_displacement, scale(spin, dt));
            }
            let penetration = (-gap - tuning.penetration_slop).max(0.0) * tuning.penetration_correction;
            corrections.position_displacement = add(
                corrections.position_displacement,
                scale(normal, penetration / count),
            );
        }
    }

    /// Two awake props split the impulse by their inverse masses. Positional
    /// correction is applied to this body only; the other accumulates its own
    /// when its turn comes (the pair is visited twice per tick).
    fn resolve_dynamic(
        &mut self,
        index: usize,
        other: usize,
        manifold: &PrimitiveContactManifold,
        material: RetailContactMaterial,
        corrections: &mut RetailReactionCorrections,
    ) {
        let dt = self.simulation.time_step;
        let tuning = self.bodies[index].tuning;
        let normal = manifold.normal;
        let count = manifold.count.max(1) as f32;
        for pair in &manifold.points[..manifold.count] {
            let gap = dot(sub(pair.a, pair.b), normal);
            if gap > tuning.contact_padding {
                continue;
            }
            let (body, other_body) = if index < other {
                let (a, b) = self.bodies.split_at_mut(other);
                (&a[index], &b[0])
            } else {
                let (a, b) = self.bodies.split_at_mut(index);
                (&b[0], &a[other])
            };
            let inverse_mass = body.inertia.inverse_mass + other_body.inertia.inverse_mass;
            let velocity = sub(body.velocity_at(pair.a), other_body.velocity_at(pair.b));
            let vn = dot(velocity, normal);
            if vn >= 0.0 {
                continue;
            }
            let restitution = if vn < -tuning.restitution_threshold {
                material.restitution.max(0.0)
            } else {
                0.0
            };
            // Same per-point sharing as the static contact above.
            let impulse = -(1.0 + restitution) * vn / (inverse_mass * count);
            let share = impulse * body.inertia.inverse_mass;
            corrections.linear_displacement = add(
                corrections.linear_displacement,
                scale(normal, share * dt),
            );
            let penetration = (-gap - tuning.penetration_slop).max(0.0) * tuning.penetration_correction;
            corrections.position_displacement = add(
                corrections.position_displacement,
                scale(normal, penetration * 0.5),
            );
        }
    }
}

/// How one skater volume touched a prop this tick.
enum SkaterContact {
    Separate,
    /// Inside the box but not closing on it.
    OverlapOnly,
    Closing,
}

fn same_pose(a: (Vector3, Basis3), b: (Vector3, Basis3)) -> bool {
    a.0.x.to_bits() == b.0.x.to_bits()
        && a.0.y.to_bits() == b.0.y.to_bits()
        && a.0.z.to_bits() == b.0.z.to_bits()
        && a.1
            .columns
            .iter()
            .flatten()
            .zip(b.1.columns.iter().flatten())
            .all(|(x, y)| x.to_bits() == y.to_bits())
}

fn volume_bounds(
    primitive: ContactPrimitive,
) -> Option<skate_core::physics::board_world::query_metadata::Bounds> {
    let expanded = |center: Vector3, radius: f32| {
        let r = radius.abs();
        skate_core::physics::board_world::query_metadata::Bounds::from_points([
            Vector3::new(center.x - r, center.y - r, center.z - r),
            Vector3::new(center.x + r, center.y + r, center.z + r),
        ])
    };
    match primitive {
        ContactPrimitive::Sphere(sphere) => expanded(sphere.center, sphere.radius),
        ContactPrimitive::Capsule {
            center,
            axis,
            half_length,
            radius,
        } => {
            let offset = scale(axis, half_length);
            let a = sub(center, offset);
            let b = add(center, offset);
            let lo = Vector3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
            let hi = Vector3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
            let r = radius.abs();
            skate_core::physics::board_world::query_metadata::Bounds::from_points([
                Vector3::new(lo.x - r, lo.y - r, lo.z - r),
                Vector3::new(hi.x + r, hi.y + r, hi.z + r),
            ])
        }
        _ => None,
    }
}

/// Prop tuning a mod (or the engine) sets: the default plus per prop type
/// overrides by MOBJ template name. Setting it back to `default()` restores
/// the shipped values (mod disable). Applied to the live props before each
/// physics tick and again after a map load builds new props.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct PropTuningSettings(pub PropTuningTable);

pub(crate) fn apply_prop_tuning(
    settings: Res<PropTuningSettings>,
    mut physics: ResMut<super::GamePhysics>,
) {
    if physics
        .prop_dynamics()
        .is_none_or(|dynamics| dynamics.tuning() == &settings.0)
    {
        return;
    }
    if let Some(dynamics) = physics.prop_dynamics_mut() {
        dynamics.set_tuning(settings.0.clone());
    }
}

/// Publish dynamic prop poses to the spawned Bevy entities. The component
/// transform holds the template-origin placement; scale stays as spawned.
pub(crate) fn sync_prop_transforms(
    physics: Res<super::GamePhysics>,
    mut props: Query<(&crate::skate_world::PropInstance, &mut Transform)>,
) {
    let Some(dynamics) = physics.prop_dynamics() else {
        return;
    };
    for (prop, mut transform) in &mut props {
        let Some((origin, basis)) = dynamics.pose(prop.id) else {
            continue;
        };
        let translation = Vec3::new(origin.x, origin.y, origin.z);
        let rotation = Quat::from_mat3(&Mat3::from_cols(
            Vec3::from_array(basis.columns[0]),
            Vec3::from_array(basis.columns[1]),
            Vec3::from_array(basis.columns[2]),
        ));
        if (transform.translation - translation).length_squared() > 1e-12
            || (transform.rotation - rotation).length_squared() > 1e-12
        {
            transform.translation = translation;
            transform.rotation = rotation;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::skate_world::build_prop_layer;
    use skate_core::physics::{
        board::BodyId,
        board_step::CollisionBody,
        collision::Sphere,
    };

    fn material() -> RetailContactMaterial {
        RetailContactMaterial {
            static_friction: 0.8,
            dynamic_friction: 0.6,
            restitution: 0.0,
        }
    }

    /// The static world material the game gives prop contacts
    /// (`PhysicsSettings::floor_material`, agCollision 8277C5D8 context
    /// 83034F34 / 38 / 3C): {0, 0, 1}, so the max / max / min combine keeps
    /// the prop's own block.
    fn floor_material() -> RetailContactMaterial {
        RetailContactMaterial { static_friction: 0.0, dynamic_friction: 0.0, restitution: 1.0 }
    }

    fn simulation() -> RetailSimulationStep {
        super::prop_simulation(RetailSimulationStep::fixed_60_hz(
            0,
            0.01,
            Vector3::new(0., -9.81, 0.),
        ))
    }

    /// Unit-cube template (±0.5) with a single instance placed at `origin`.
    fn fixture(
        origin: [f32; 3],
    ) -> (
        BoardWorld,
        crate::skate_world::PropCollisionLayer,
        PropDynamics,
    ) {
        box_fixture(origin, [0.5, 0.5, 0.5])
    }

    /// Box template with half extents `half` and one instance at `origin`.
    fn box_fixture(
        origin: [f32; 3],
        half: [f32; 3],
    ) -> (
        BoardWorld,
        crate::skate_world::PropCollisionLayer,
        PropDynamics,
    ) {
        let corners = [
            [-1., -1., -1.], [1., -1., -1.], [1., -1., 1.], [-1., -1., 1.],
            [-1., 1., -1.], [1., 1., -1.], [1., 1., 1.], [-1., 1., 1.],
        ]
        .map(|c: [f32; 3]| [c[0] * half[0], c[1] * half[1], c[2] * half[2]]);
        let faces = [
            [4, 7, 6], [4, 6, 5], // +Y top
            [0, 1, 2], [0, 2, 3], // -Y bottom
            [1, 5, 6], [1, 6, 2], // +X
            [0, 7, 4], [0, 3, 7], // -X
            [3, 2, 6], [3, 6, 7], // +Z
            [0, 5, 1], [0, 4, 5], // -Z
        ];
        let vertex = |position| skate_data::skate_map::Vertex {
            position,
            normal: [0., 1., 0.],
            uv: [0.; 2],
            lightmap_uv: [0.; 2],
            material: 1,
            decal_uv: None,
            tangent_frame: None,
        };
        let map = skate_data::skate_map::SkateMap {
            version: 14,
            name: "props".into(),
            spawn: [0.; 3],
            heading: 0.,
            environment: vec![0.; 45],
            materials: vec![skate_data::skate_map::Material {
                name: "prop".into(),
                flags: 0,
                friction: 0.5,
                restitution: 0.1,
                color: [1.; 3],
                roughness: 0.5,
                emissive: 0.,
                textures: [0; 5],
                indirect_strength: 0.,
                alpha_mode: 0,
                alpha_cutoff: 0.5,
                audio: 3,
                physics: 1,
                pattern: 0,
                depth_layer: None,
                retail_definition: None,
            }],
            textures: vec![],
            geometry: skate_data::skate_map::Geometry {
                vertices: corners.into_iter().map(vertex).collect(),
                indices: faces.into_iter().flatten().collect(),
                collision: vec![],
            },
            rails: vec![],
            doors: vec![],
            lights: vec![],
            routes: vec![],
            extensions: vec![],
        };
        let objects = vec![skate_data::skate_map::StaticObject {
            id: 7,
            name: "template/crate".into(),
            transform: [
                1., 0., 0., 0., 1., 0., 0., 0., 1., origin[0], origin[1], origin[2],
            ],
            first_index: 0,
            index_count: 36,
            first_collision: 0,
            collision_count: 0,
            rails: vec![],
            physics: Default::default(),
        }];
        let layer = build_prop_layer(&map, &objects, floor_material()).unwrap().unwrap();
        let dynamics = PropDynamics::new(&objects, layer.instances(), simulation());
        let world = super::super::ground::Terrain::Flat.world(floor_material());
        (world, layer, dynamics)
    }

    const REST_Y: f32 = super::super::ground::HEIGHT + 0.5;

    /// A prop dropped from the air falls, settles on the floor and sleeps; its
    /// collision triangles are re-baked at the new pose.
    #[test]
    fn dropped_prop_falls_settles_and_sleeps() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y + 5., 0.]);
        dynamics.bodies[0].wake();
        for _ in 0..240 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let body = &dynamics.bodies[0];
        assert!(body.asleep, "prop should cool down to sleep: {:#?}", body.rates);
        assert!(
            (body.rates.position.y - REST_Y).abs() < 0.1,
            "resting height {}",
            body.rates.position.y
        );
        // Re-baked triangles: a probe from the drop height hits the top face.
        let hit = layer
            .world()
            .query_thin_line(Vector3::new(0., REST_Y + 5., 0.), Vector3::new(0., REST_Y - 1., 0.))
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - (REST_Y + 0.5)).abs() < 0.1);
    }

    /// A moving skater sphere wakes a resting prop and pushes it sideways.
    #[test]
    fn skater_volume_pushes_resting_prop() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 0.]);
        let start_x = dynamics.bodies[0].rates.position.x;
        assert!(dynamics.bodies[0].asleep);
        let volumes = [BoardWorldVolume {
            collision_group: 4,
            body: CollisionBody::Board(BodyId::Deck),
            primitive: ContactPrimitive::Sphere(Sphere {
                center: Vector3::new(-0.55, REST_Y, 0.),
                radius: 0.2,
            }),
            motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: Vector3::new(2., 0., 0.), ..Default::default() },
            material: material(),
        }];
        for _ in 0..30 {
            dynamics.step(&world, &mut layer, &volumes);
        }
        let body = &dynamics.bodies[0];
        assert!(
            body.rates.position.x > start_x + 0.02,
            "pushed from {start_x} to {}",
            body.rates.position.x
        );
    }

    /// A settled prop stays put and finite over long idle ticks.
    #[test]
    fn settled_prop_does_not_sink_or_diverge() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y + 5., 0.]);
        dynamics.bodies[0].wake();
        for _ in 0..240 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let settled = dynamics.bodies[0].rates.position;
        for _ in 0..120 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let body = &dynamics.bodies[0];
        let p = body.rates.position;
        assert!(p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
        assert!(p.y > REST_Y - 0.1, "sank to {}", p.y);
        assert!((p.y - settled.y).abs() < 0.02, "drifted {} -> {}", settled.y, p.y);
    }

    // Board stuck inside a prop (2026-10-05, bench near the Aletown spawn).

    /// Bench-sized box: 2.0 x 0.9 x 0.7 m, about 126 kg at the default
    /// density, resting on the flat ground.
    const BENCH_HALF: [f32; 3] = [1.0, 0.45, 0.35];
    const BENCH_REST_Y: f32 = super::super::ground::HEIGHT + 0.45;

    /// A parked deck capsule inside the bench box (under the seat, where the
    /// render mesh is open but the render-AABB contact box is solid).
    fn parked_deck() -> [BoardWorldVolume; 1] {
        [BoardWorldVolume {
            collision_group: 4,
            body: CollisionBody::Board(BodyId::Deck),
            primitive: ContactPrimitive::Capsule {
                center: Vector3::new(0.1, super::super::ground::HEIGHT + 0.1, 0.05),
                axis: Vector3::new(1., 0., 0.),
                half_length: 0.3,
                radius: 0.06,
            },
            motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: Vector3::ZERO, ..Default::default() },
            material: material(),
        }]
    }

    /// #15's push before the fix: overlap nudge forever, no board cap, no
    /// depenetration bound.
    fn legacy_tuning() -> PropTuningTable {
        PropTuningTable {
            default: PropTuning {
                stuck_release_ticks: u32::MAX,
                board_push_speed: f32::INFINITY,
                max_depenetration_per_tick: f32::INFINITY,
                ..PropTuning::default()
            },
            ..Default::default()
        }
    }

    /// Runs `ticks` steps with the deck parked inside the bench and returns
    /// (awake ticks, rebakes, skater pushes) over the last `tail` ticks plus
    /// the bench's horizontal travel.
    fn park_deck_in_bench(table: Option<PropTuningTable>, ticks: u32, tail: u32) -> (u32, u32, u32, f32) {
        let (world, mut layer, mut dynamics) = box_fixture([0., BENCH_REST_Y, 0.], BENCH_HALF);
        if let Some(table) = table {
            dynamics.set_tuning(table);
        }
        let start = dynamics.bodies[0].rates.position;
        let volumes = parked_deck();
        let (mut awake, mut rebakes, mut pushes) = (0, 0, 0);
        for tick in 0..ticks {
            dynamics.step(&world, &mut layer, &volumes);
            let stats = dynamics.last_step_stats();
            if tick >= ticks - tail {
                awake += stats.awake;
                rebakes += stats.rebakes;
                pushes += stats.skater_pushes;
            }
        }
        let end = dynamics.bodies[0].rates.position;
        let travel = ((end.x - start.x).powi(2) + (end.z - start.z).powi(2)).sqrt();
        (awake, rebakes, pushes, travel)
    }

    /// Reproduction: with #15's push a parked deck inside a heavy bench box
    /// nudges it every tick (0.5 m/s floor, close to what ground friction
    /// takes off again), so the bench never sleeps and rebakes its triangles
    /// (and the whole prop layer's query index) every tick.
    #[test]
    fn legacy_push_keeps_bench_awake_with_parked_deck() {
        let (awake, rebakes, pushes, travel) = park_deck_in_bench(Some(legacy_tuning()), 600, 300);
        println!("legacy: awake {awake}/300 rebakes {rebakes} pushes {pushes} travel {travel:.3} m");
        assert_eq!(awake, 300, "bench should stay awake every tick under #15's push");
        assert_eq!(pushes, 300, "a push every tick");
        assert!(rebakes > 250, "rebake nearly every tick: {rebakes}");
    }

    /// Fix: the overlap nudge stops after `stuck_release_ticks`, so the bench
    /// cools down and sleeps with the deck still inside; no per-tick work.
    #[test]
    fn parked_deck_inside_bench_lets_it_sleep() {
        let (awake, rebakes, pushes, travel) = park_deck_in_bench(None, 600, 300);
        println!("fixed: awake {awake}/300 rebakes {rebakes} pushes {pushes} travel {travel:.3} m");
        assert_eq!(awake, 0, "bench should be asleep");
        assert_eq!(rebakes, 0);
        assert_eq!(pushes, 0);
        assert!(travel < 0.5, "bench travelled {travel} m");
    }

    /// A real hit after the release still pushes the prop.
    #[test]
    fn released_bench_still_takes_a_real_hit() {
        let (world, mut layer, mut dynamics) = box_fixture([0., BENCH_REST_Y, 0.], BENCH_HALF);
        let parked = parked_deck();
        for _ in 0..300 {
            dynamics.step(&world, &mut layer, &parked);
        }
        assert!(dynamics.bodies[0].asleep);
        let start = dynamics.bodies[0].rates.position.z;
        let mut hit = parked;
        hit[0].primitive = ContactPrimitive::Sphere(Sphere {
            center: Vector3::new(0., BENCH_REST_Y, -0.35 - 0.15),
            radius: 0.2,
        });
        hit[0].motion.linear_velocity = Vector3::new(0., 0., 4.);
        for _ in 0..20 {
            dynamics.step(&world, &mut layer, &hit);
        }
        assert!(dynamics.bodies[0].rates.position.z > start + 0.01);
    }

    /// The board push is capped like the body bump: a board held against a
    /// light prop cannot drive it past `board_push_speed`.
    #[test]
    fn board_push_speed_is_capped() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 0.]);
        let mut table = PropTuningTable::default();
        table.default.board_push_speed = 0.8;
        dynamics.set_tuning(table);
        let mut fastest = 0.0_f32;
        for tick in 0..60 {
            let x = dynamics.bodies[0].rates.position.x - 0.55 - 0.05 + tick as f32 * 0.0;
            let volumes = [BoardWorldVolume {
                collision_group: 4,
                body: CollisionBody::Board(BodyId::Deck),
                primitive: ContactPrimitive::Sphere(Sphere {
                    center: Vector3::new(x, REST_Y, 0.),
                    radius: 0.2,
                }),
                motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: Vector3::new(10., 0., 0.), ..Default::default() },
                material: material(),
            }];
            dynamics.step(&world, &mut layer, &volumes);
            fastest = fastest.max(dynamics.bodies[0].rates.linear_velocity.x);
        }
        assert!(fastest <= 0.8 + 1e-3, "pushed to {fastest} m/s");
        assert!(fastest > 0.1, "push still applies: {fastest}");
    }

    /// Depenetration in the engine's older impulse pass (mod option
    /// `row_solver = false`) is bounded per tick: a box dropped deep into the
    /// floor rises at most `max_depenetration_per_tick` per tick from the
    /// positional correction.
    #[test]
    fn depenetration_is_bounded_per_tick() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y - 0.4, 0.]);
        dynamics.set_tuning(PropTuningTable {
            solver: PropSolverSettings { row_solver: false, ..Default::default() },
            ..Default::default()
        });
        dynamics.bodies[0].wake();
        let cap = dynamics.tuning().default.max_depenetration_per_tick;
        let mut previous = dynamics.bodies[0].rates.position.y;
        for _ in 0..30 {
            dynamics.step(&world, &mut layer, &[]);
            let y = dynamics.bodies[0].rates.position.y;
            // Velocity terms add at most a few mm on top of the correction.
            assert!(y - previous <= cap + 0.02, "rose {} m in one tick", y - previous);
            previous = y;
        }
        assert!(previous > REST_Y - 0.4 + 0.05, "the box still comes out: {previous}");
    }

    /// The retail row solver has no per-tick cap: a box 0.4 m deep in the
    /// floor comes out on the full predicted-separation target and settles.
    #[test]
    fn retail_rows_push_a_deep_box_out_and_it_settles() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y - 0.4, 0.]);
        dynamics.bodies[0].wake();
        let mut previous = dynamics.bodies[0].rates.position.y;
        let mut largest_rise = 0.0f32;
        let mut fastest = 0.0f32;
        let mut highest = previous;
        let mut slept_at = None;
        for tick in 0..240 {
            dynamics.step(&world, &mut layer, &[]);
            let body = &dynamics.bodies[0];
            let y = body.rates.position.y;
            largest_rise = largest_rise.max(y - previous);
            fastest = fastest.max(body.rates.linear_velocity.y);
            highest = highest.max(y);
            if slept_at.is_none() && body.asleep {
                slept_at = Some(tick);
            }
            previous = y;
        }
        println!("deep box: largest rise {largest_rise:.4} m/tick, fastest up {fastest:.3} m/s, highest {:.4} over rest, end {:.4} over rest, slept at {slept_at:?}", highest - REST_Y, previous - REST_Y);
        assert!((previous - REST_Y).abs() < 0.02, "the box rests on the floor: {}", previous - REST_Y);
        assert!(slept_at.is_some(), "the box sleeps");
    }

    /// Per prop type tuning: an override keyed by template name applies to
    /// that type only, a collision box override keeps the rendered pose, and
    /// reset restores the defaults.
    #[test]
    fn tuning_overrides_per_template_and_resets() {
        let (_, _, mut dynamics) = fixture([0., REST_Y, 0.]);
        let origin = dynamics.bodies[0].origin();
        let mut table = PropTuningTable::default();
        table.by_template.insert(
            "template/crate".into(),
            PropTuning {
                stuck_release_ticks: 5,
                collision_box: Some(PropBox {
                    center: Vector3::new(0., -0.25, 0.),
                    half_extents: Vector3::new(0.5, 0.25, 0.5),
                }),
                ..PropTuning::default()
            },
        );
        dynamics.set_tuning(table);
        let body = &dynamics.bodies[0];
        assert_eq!(body.tuning.stuck_release_ticks, 5);
        assert!((body.half_extents.y - 0.25).abs() < 1e-6);
        let moved = body.origin();
        assert!((moved.y - origin.y).abs() < 1e-5, "rendered pose must not move");
        dynamics.reset_tuning();
        let body = &dynamics.bodies[0];
        assert_eq!(body.tuning, PropTuning::default());
        assert!((body.half_extents.y - 0.5).abs() < 1e-6);
    }

    /// Frame-cost probe on the converted DownTown props (private assets,
    /// `SKATE3_ASSET_ROOT`): time of one prop rebake, which also rebuilds the
    /// prop layer's query index. Run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn downtown_prop_rebake_cost() {
        let root = std::path::PathBuf::from(
            std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT"),
        );
        let (mut layer, dynamics) = crate::skate_world::load_prop_layer(
            &root,
            "DownTown",
            floor_material(),
            simulation(),
        )
        .expect("DownTown props");
        let bench = dynamics
            .bodies
            .iter()
            .position(|b| b.template.to_ascii_lowercase().contains("bench"))
            .unwrap_or(0);
        let body = &dynamics.bodies[bench];
        let (origin, basis) = (body.origin(), body.rates.basis);
        let started = std::time::Instant::now();
        for _ in 0..100 {
            layer.rebake(body.instance, basis.columns, origin).unwrap();
        }
        let each = started.elapsed() / 100;
        println!(
            "DownTown: {} props, {} triangles; {} ({} tris, box {:?}) rebake {:?} each",
            dynamics.bodies.len(),
            layer.world().triangles().len(),
            body.template,
            layer.instances()[body.instance].range.len(),
            body.half_extents,
            each
        );
    }

    // Phase 3: offboard carry glue (`crate::physics::prop_carry`).

    fn carrier(state: skate_core::player::state::PhysicalStateId, z: f32) -> crate::physics::prop_carry::Carrier {
        crate::physics::prop_carry::Carrier {
            state,
            position: Vector3::new(0., super::super::ground::HEIGHT + 0.9, z),
            forward: Vector3::new(0., 0., 1.),
            time_step: simulation().time_step,
            skeleton: None,
        }
    }

    /// One carry tick with the grab button held (the carry keeps its prop).
    fn tick() -> crate::physics::prop_carry::Tick {
        crate::physics::prop_carry::Tick {
            grab: true,
            ..Default::default()
        }
    }

    /// Grab button released: drops a carry or confirms a placement.
    fn release() -> crate::physics::prop_carry::Tick {
        crate::physics::prop_carry::Tick::default()
    }

    /// Derived controller words with the given raw flag bits held this tick
    /// (word 13) and on the previous tick (word 6).
    fn controller_words(now: &[u32], before: &[u32]) -> [u32; 26] {
        let mut words = [0u32; 26];
        for bit in now {
            words[13] |= 1 << bit;
        }
        for bit in before {
            words[6] |= 1 << bit;
        }
        words
    }

    /// Video bug (2026-10-05, Aletown spawn): sprinting (A, raw bit 21) past
    /// props grabbed them and dragged them along. A is the retail sprint
    /// button, so pressing or holding it must never grab, and a prop passed
    /// at a run stays where it was.
    #[test]
    fn sprinting_past_a_prop_does_not_grab_it() {
        let (world, mut layer, mut dynamics) = fixture([0.8, REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        let buttons = carry.buttons();
        let start = dynamics.position_of(7).unwrap();
        // Run along +Z at 5 m/s past the prop, which sits 0.8 m to the side.
        let speed_per_tick = 5.0 * simulation().time_step;
        for i in 0..90 {
            let words = if i == 0 {
                controller_words(&[21], &[])
            } else {
                controller_words(&[21], &[21])
            };
            let tick = crate::physics::prop_carry::Tick::from_controller(&words, buttons, 0., 0., 0.);
            assert!(!tick.grab, "sprint (A) produced a grab on tick {i}");
            carry.update(&mut dynamics, tick, carrier(state, -1.0 + speed_per_tick * i as f32));
            assert_eq!(carry.held(), None, "sprinting grabbed the prop on tick {i}");
            dynamics.step(&world, &mut layer, &[]);
        }
        let end = dynamics.position_of(7).unwrap();
        let moved = ((end.x - start.x).powi(2) + (end.z - start.z).powi(2)).sqrt();
        assert!(moved < 0.05, "prop followed the runner: moved {moved} m");
    }

    /// The retail GrabWorld button (RB, raw bit 28) grabs while held and
    /// releasing it drops the prop.
    #[test]
    fn grab_world_button_holds_and_release_drops() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        let buttons = carry.buttons();
        let from = |now: &[u32], before: &[u32]| {
            crate::physics::prop_carry::Tick::from_controller(
                &controller_words(now, before),
                buttons,
                0.,
                0.,
                0.,
            )
        };
        carry.update(&mut dynamics, from(&[28], &[]), carrier(state, 0.));
        assert_eq!(carry.held(), Some(7));
        for i in 0..30 {
            carry.update(&mut dynamics, from(&[28], &[28]), carrier(state, 0.));
            dynamics.step(&world, &mut layer, &[]);
            assert_eq!(carry.held(), Some(7), "held grab dropped on tick {i}");
        }
        carry.update(&mut dynamics, from(&[], &[28]), carrier(state, 1.5));
        assert_eq!(carry.held(), None, "releasing the grab button must drop");
        // B is a rising edge for placement, not a level.
        assert!(from(&[20], &[]).placement);
        assert!(!from(&[20], &[20]).placement);
    }

    /// One held tick with the OB_ObjectMv intents (left stick X / Z, right stick X).
    fn stick(x: f32, z: f32, rot: f32) -> crate::physics::prop_carry::Tick {
        crate::physics::prop_carry::Tick { grab: true, object_move: [x, z, rot], ..Default::default() }
    }

    /// The skater follows the prop's grab frame (Move Object: the prop leads),
    /// as `biped_ground` does in the game; height stays the carrier's.
    fn follow(
        carry: &crate::physics::prop_carry::PropCarry,
        carrier: crate::physics::prop_carry::Carrier,
    ) -> crate::physics::prop_carry::Carrier {
        match carry.skater_target() {
            Some((p, f)) => crate::physics::prop_carry::Carrier {
                position: Vector3::new(p.x, carrier.position.y, p.z),
                forward: f,
                ..carrier
            },
            None => carrier,
        }
    }

    /// Heading in the yaw-rate sense (positive angular velocity about +Y
    /// increases it), like `HeldBody::heading`.
    fn heading_of(dynamics: &PropDynamics, id: u32) -> f32 {
        let z = dynamics.pose(id).unwrap().1.columns[2];
        z[0].atan2(z[2])
    }

    /// The axis convention the Move Object heading relies on: a positive yaw
    /// rate on a free body turns its local +Z toward world +X (right-handed).
    #[test]
    fn positive_yaw_rate_turns_local_z_toward_plus_x() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y + 20., 0.]);
        dynamics.bodies[0].wake();
        dynamics.bodies[0].rates.angular_velocity = Vector3::new(0., 1., 0.);
        for _ in 0..10 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let z = dynamics.pose(7).unwrap().1.columns[2];
        assert!(z[0] > 0.1, "local +Z after a positive yaw rate: {z:?}");
        assert!(heading_of(&dynamics, 7) > 0.1);
    }

    /// What the retail controller math alone predicts for a centre push on a
    /// free point mass (no contacts, no friction): 82D45318 steps 1 to 10
    /// (`move_object::command`) read the body velocity before the step and
    /// the slot 9 sink adds the sent command as an acceleration in the same
    /// step (`v += L dt`, spec 7.5). Returns the speed after every tick.
    fn predicted_centre_push(mass: f32, move_z: f32, ticks: usize) -> Vec<f32> {
        use skate_core::player::offboard::move_object::{command, MoveObjectController, MoveObjectInput};
        let t = crate::physics::prop_carry::CarryLocomotion::default().move_object;
        let dt = simulation().time_step;
        let mut state = MoveObjectController::default();
        let mut v = 0.0f32;
        let mut z = 1.2f32;
        (0..ticks)
            .map(|_| {
                let input = MoveObjectInput {
                    move_z,
                    move_x: 0.0,
                    move_rotation: 0.0,
                    forward: [0.0, 0.0, 1.0],
                    grip: [0.0, 0.0, z - 0.5],
                    center: [0.0, 0.0, z],
                    velocity: [0.0, 0.0, v],
                    heading: 0.0,
                    mass,
                    yaw_inertia: 1.0,
                    contact_normal: [0.0; 3],
                    record_272: false,
                };
                v += command(&t, &mut state, &input).linear[2] * dt;
                z += v * dt;
                v
            })
            .collect()
    }

    /// Object-relative stick (82D45318): with the prop grabbed by the centre
    /// of its near face, the left stick in any direction never turns it
    /// (zero lever, the left stick has no rotation share), Z pushes / pulls
    /// along the grab-edge normal and X slides along the edge, and the
    /// skater stays on the grab frame.
    #[test]
    fn move_object_left_stick_moves_the_prop_in_the_edge_frame() {
        for step in 0..8 {
            let a = step as f32 * std::f32::consts::TAU / 8.0;
            let (x, z) = (a.sin(), a.cos());
            let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
            let mut carry = crate::physics::prop_carry::PropCarry::default();
            let mut at = carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.);
            carry.update(&mut dynamics, tick(), at);
            dynamics.set_held(carry.held());
            let start = dynamics.position_of(7).unwrap();
            at.state = skate_core::player::state::PhysicalStateId::OffBoardPushing;
            for _ in 0..90 {
                at = follow(&carry, at);
                carry.update(&mut dynamics, stick(x, z, 0.0), at);
                dynamics.set_held(carry.held());
                dynamics.step(&world, &mut layer, &[]);
            }
            assert_eq!(carry.held(), Some(7), "stick ({x:.2}, {z:.2}) lost the grab");
            let end = dynamics.position_of(7).unwrap();
            let (dx, dz) = (end.x - start.x, end.z - start.z);
            let travelled = (dx * dx + dz * dz).sqrt();
            let t = crate::physics::prop_carry::CarryLocomotion::default().move_object;
            let (ex, ez) = (x * t.side_speed, z * if z > 0.0 { t.push_speed } else { t.pull_speed });
            let expected = (ex * ex + ez * ez).sqrt();
            assert!(travelled > 0.5, "stick ({x:.2}, {z:.2}) did not move the prop: {travelled}");
            let along = (dx * ex + dz * ez) / (travelled * expected);
            assert!(along > 0.97, "stick ({x:.2}, {z:.2}) moved the prop off its direction: {along}");
            assert!(heading_of(&dynamics, 7).abs() < 0.05, "left stick turned the prop: {}", heading_of(&dynamics, 7));
            let target = carry.skater_target().unwrap().0;
            let lag = ((target.x - at.position.x).powi(2) + (target.z - at.position.z).powi(2)).sqrt();
            assert!(lag < 0.2, "skater fell {lag} m behind the grab frame");
        }
    }

    /// A straight push moves the prop along the grab-edge normal at about the
    /// retail push speed, and the skater stays on the grab frame.
    #[test]
    fn dragged_prop_follows_a_straight_push() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let mut at = carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.);
        carry.update(&mut dynamics, tick(), at);
        assert_eq!(carry.held(), Some(7));
        dynamics.set_held(Some(7));
        at.state = skate_core::player::state::PhysicalStateId::OffBoardPushing;
        let mut max_side = 0.0f32;
        let mut speed = 0.0f32;
        let mut peak = 0.0f32;
        let mut up_min = 1.0f32;
        for _ in 0..180 {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 1., 0.), at);
            dynamics.set_held(carry.held());
            dynamics.step(&world, &mut layer, &[]);
            let p = dynamics.position_of(7).unwrap();
            max_side = max_side.max(p.x.abs());
            speed = dynamics.bodies[0].rates.linear_velocity.z;
            peak = peak.max(speed);
            up_min = up_min.min(dynamics.bodies[0].rates.basis.columns[1][1]);
        }
        assert_eq!(carry.held(), Some(7), "the push dropped the prop");
        assert!(max_side < 0.05, "prop wandered {max_side} m off the push line");
        let mass = 1.0 / dynamics.bodies[0].inertia.inverse_mass;
        let t = crate::physics::prop_carry::CarryLocomotion::default().move_object;
        let target = t.push_speed * t.speed_scale(mass);
        let predicted = predicted_centre_push(mass, 1.0, 180);
        let predicted_peak = predicted.iter().fold(0.0f32, |a, b| a.max(*b));
        let predicted_end = predicted[179];
        println!("straight push: speed {speed:.3} (retail math {predicted_end:.3}, target {target}), peak {peak:.3} (retail math {predicted_peak:.3}), up_y min {up_min:.4}");
        // The retail controller settles on the target; the sim must follow the
        // controller's own prediction (floor contacts only add the commanded
        // block's small friction, which the integrating controller removes).
        assert!((predicted_end - target).abs() < 0.05 * target, "retail math does not settle on the target: {predicted_end}");
        assert!((speed - predicted_end).abs() < 0.1 * target, "push speed {speed} m/s, retail math {predicted_end}");
        assert!((peak - predicted_peak).abs() < 0.15 * target, "peak {peak} m/s, retail math {predicted_peak}");
        assert!(up_min > 0.99, "a straight push tipped the cube: up_y {up_min}");
    }

    /// The right stick turns the held prop (yaw command only, retail sign)
    /// and the skater is carried round with its grab edge.
    #[test]
    fn right_stick_turns_the_held_prop_and_the_skater_follows() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let mut at = carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.);
        carry.update(&mut dynamics, tick(), at);
        dynamics.set_held(Some(7));
        at.state = skate_core::player::state::PhysicalStateId::OffBoardPushing;
        let mut turned = 0.0f32;
        let mut previous = heading_of(&dynamics, 7);
        let mut framed = previous;
        for _ in 0..90 {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 0., 1.), at);
            // The grab frame is built from the pose before this step.
            framed = heading_of(&dynamics, 7);
            dynamics.set_held(carry.held());
            dynamics.step(&world, &mut layer, &[]);
            let h = heading_of(&dynamics, 7);
            turned += (h - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            previous = h;
        }
        assert_eq!(carry.held(), Some(7));
        // 82D45318: w = -(curve x MvRot x gain), sent as (0, w, 0) about +Y
        // (right-handed, test `positive_yaw_rate_turns_local_z_toward_plus_x`),
        // so a positive MvRot turns the prop clockwise seen from above (the
        // unwrapped heading atan2(z.x, z.z) decreases).
        assert!(turned < -0.3, "positive MvRot did not turn the prop clockwise: {turned} rad");
        let up = dynamics.bodies[0].rates.basis.columns[1][1];
        assert!(up > 0.99, "a yaw command tipped the prop: up_y {up}");
        // The skater faces the turned edge (frame of the last carry update).
        let (_, facing) = carry.skater_target().unwrap();
        let off = (facing.x.atan2(facing.z) - framed + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        assert!(off.abs() < 0.05, "skater not facing the grab edge: {off} rad off");
    }

    /// Pushing at the grab point off the centre turns the prop through the
    /// lever-arm curves (a bench grabbed near its end).
    #[test]
    fn off_centre_push_turns_a_long_prop() {
        let (world, mut layer, mut dynamics) = box_fixture([0., super::super::ground::HEIGHT + 0.41, 0.], [2.3, 0.41, 0.29]);
        let id = dynamics.bodies[0].id;
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        // Behind the bench, 1.8 m right of its centre.
        let mut at = crate::physics::prop_carry::Carrier {
            position: Vector3::new(1.8, super::super::ground::HEIGHT + 0.9, -0.9),
            ..carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.)
        };
        carry.update(&mut dynamics, tick(), at);
        assert_eq!(carry.held(), Some(id));
        dynamics.set_held(Some(id));
        at.state = skate_core::player::state::PhysicalStateId::OffBoardPushing;
        let mut turned = 0.0f32;
        let mut previous = heading_of(&dynamics, id);
        for _ in 0..120 {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 1., 0.), at);
            dynamics.set_held(carry.held());
            dynamics.step(&world, &mut layer, &[]);
            let h = heading_of(&dynamics, id);
            turned += (h - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            previous = h;
        }
        assert_eq!(carry.held(), Some(id), "the push dropped the bench");
        // Push along +Z at r = (1.8, 0, -0.3) from the centre: torque about +Y
        // is -1.8 F, and retail's yaw term (82D45318) turns the same way.
        println!("off-centre push: turned {turned:.3} rad");
        assert!(turned < -0.05, "the off-centre push did not turn the bench with its torque: {turned}");
    }

    /// A held right stick turns the prop at the retail yaw-rate target and no
    /// faster. 82D45318 [code, 0x82D45BC8..0x82D45CD0]: yaw error = w - 60 x
    /// (facing change since the previous tick, +384), PID 20 / 0 / 40 with the
    /// output accumulating (integral action) and clamped to 6 rad/s^2. The
    /// retail math on a free yaw body (`yaw_rate_feedback_settles_at_the_target_rate`
    /// in skate-core) spins up at 0.1 rad/s per tick, peaks 0.6 % over |w| and
    /// settles at |w| with zero steady error; the expectation here is |w| from
    /// the command itself (curve BFB3BEF0BB2661C0(|lever|) x rot x gain +1160).
    #[test]
    fn held_right_stick_turn_rate_stays_bounded() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let mut at = carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.);
        carry.update(&mut dynamics, tick(), at);
        dynamics.set_held(Some(7));
        at.state = skate_core::player::state::PhysicalStateId::OffBoardPushing;
        let mut peak = 0.0f32;
        for _ in 0..180 {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 0., 1.), at);
            dynamics.set_held(carry.held());
            dynamics.step(&world, &mut layer, &[]);
            peak = peak.max(dynamics.bodies[0].rates.angular_velocity.y.abs());
        }
        assert_eq!(carry.held(), Some(7), "the turn dropped the prop");
        let (command, _) = carry.last_command().expect("held");
        let target = command.yaw_target.abs();
        let rate = dynamics.bodies[0].rates.angular_velocity.y.abs();
        println!("right stick turn: rate {rate:.3} rad/s, peak {peak:.3}, retail target {target:.3}");
        assert!(target > 0.5, "no yaw target from the right stick: {command:?}");
        assert!((rate - target).abs() < 0.05 * target, "yaw rate {rate} rad/s, retail target {target}");
        assert!(peak < 1.05 * target, "yaw rate overshoot {peak} rad/s for a target of {target}");
    }

    /// Grabbing the prop ahead picks it up; the stick moves it.
    #[test]
    fn grabbed_prop_moves_with_the_stick() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let mut at = carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.);
        carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, at);
        assert_eq!(carry.held(), Some(7));
        dynamics.set_held(carry.held());
        let start = dynamics.position_of(7).unwrap().z;
        for _ in 0..60 {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 1., 0.), at);
            dynamics.set_held(carry.held());
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        // Distance the retail controller math predicts in one second.
        let dt = simulation().time_step;
        let mass = 1.0 / dynamics.bodies[0].inertia.inverse_mass;
        let predicted: f32 = predicted_centre_push(mass, 1.0, 60).iter().map(|v| v * dt).sum();
        let travelled = p.z - start;
        println!("push distance: {travelled:.3} m in 1 s (retail math {predicted:.3} m)");
        assert!((travelled - predicted).abs() < 0.1 * predicted, "pushed {travelled} m, retail math {predicted} m");
        assert!(p.y > super::super::ground::HEIGHT, "carried prop underground: {p:?}");
    }

    /// Dropping releases the prop; it falls, keeps no NaN, and sleeps again.
    #[test]
    fn dropped_carry_falls_and_sleeps() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(state, 0.));
        for _ in 0..30 {
            carry.update(&mut dynamics, tick(), carrier(state, 0.));
            dynamics.step(&world, &mut layer, &[]);
        }
        carry.update(&mut dynamics, release(), carrier(state, 0.));
        assert_eq!(carry.held(), None);
        for _ in 0..300 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        assert!((p.y - REST_Y).abs() < 0.1, "resting height {}", p.y);
        assert!(dynamics.bodies[0].asleep, "dropped prop never slept");
    }

    /// The grab is ignored unless the skater is on foot.
    #[test]
    fn grab_requires_biped_ground() {
        let (_world, _layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        for state in [
            skate_core::player::state::PhysicalStateId::PhysicsGround,
            skate_core::player::state::PhysicalStateId::BipedAir,
            skate_core::player::state::PhysicalStateId::WipeoutGround,
        ] {
            carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(state, 0.));
            assert_eq!(carry.held(), None, "{state:?} must not grab");
        }
        // Grabbing, then mounting the board, drops the prop automatically.
        carry.update(
            &mut dynamics,
            crate::physics::prop_carry::Tick { grab: true, ..tick() },
            carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.),
        );
        assert_eq!(carry.held(), Some(7));
        carry.update(
            &mut dynamics,
            tick(),
            carrier(skate_core::player::state::PhysicalStateId::PhysicsGround, 0.),
        );
        assert_eq!(carry.held(), None);
    }

    /// Retail grab-object publication moves the selector to OffBoardPushing
    /// while held; the carry must not treat state 502 as leaving the ground.
    #[test]
    fn carry_survives_offboard_pushing_and_drops_afterwards() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let ground = skate_core::player::state::PhysicalStateId::BipedGround;
        let pushing = skate_core::player::state::PhysicalStateId::OffBoardPushing;
        carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(ground, 0.));
        assert_eq!(carry.held(), Some(7));
        let mut at = carrier(pushing, 0.);
        for _ in 0..30 {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 0.2, 0.), at);
            dynamics.step(&world, &mut layer, &[]);
            assert_eq!(carry.held(), Some(7), "state 502 dropped the carry");
        }
        assert!(dynamics.position_of(7).unwrap().z > 1.25, "the stick did not move the prop in 502");
        // Leaving the on-foot states still auto-drops.
        carry.update(
            &mut dynamics,
            tick(),
            carrier(skate_core::player::state::PhysicalStateId::BipedAir, 0.),
        );
        assert_eq!(carry.held(), None);
    }

    // Phase 4: placement mode and layout persistence.

    /// Placement adjusts the ghost pose; confirming drops the prop there,
    /// records the layout pose, and the prop falls and sleeps in place.
    #[test]
    fn placement_adjust_confirm_and_sleep() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        let grab = crate::physics::prop_carry::Tick { grab: true, ..tick() };
        carry.update(&mut dynamics, grab, carrier(state, 0.));
        assert_eq!(carry.held(), Some(7));
        // Enter placement; push the ghost far (distance axis) and yaw it.
        carry.update(
            &mut dynamics,
            crate::physics::prop_carry::Tick { placement: true, ..tick() },
            carrier(state, 0.),
        );
        assert!(carry.placing());
        for _ in 0..60 {
            carry.update(
                &mut dynamics,
                crate::physics::prop_carry::Tick {
                    distance_axis: 1.0,
                    yaw_axis: 0.25,
                    ..tick()
                },
                carrier(state, 0.),
            );
            dynamics.step(&world, &mut layer, &[]);
        }
        let held_pose = dynamics.position_of(7).unwrap();
        let horizontal = (held_pose.x * held_pose.x + held_pose.z * held_pose.z).sqrt();
        assert!(horizontal > 2.0, "ghost pushed out to r={horizontal}");
        let recorded_basis = dynamics.pose(7).unwrap().1;
        assert!(
            recorded_basis.columns[2][0] > 0.3,
            "ghost yaw never applied: {:?}",
            recorded_basis.columns
        );
        // Confirm (release the grab button): drop at the ghost pose; the prop
        // stays there and sleeps.
        carry.update(&mut dynamics, release(), carrier(state, 0.));
        assert_eq!(carry.held(), None);
        assert!(!carry.placing());
        let recorded = carry.layout().get(&7).copied();
        assert!(recorded.is_some(), "confirmed placement was not recorded");
        for _ in 0..300 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        assert!((p.y - REST_Y).abs() < 0.1, "placed prop rests at {}", p.y);
        let placed_horizontal = (p.x * p.x + p.z * p.z).sqrt();
        assert!(placed_horizontal > 1.5, "placed prop kept its distance: {placed_horizontal}");
        assert!(dynamics.bodies[0].asleep, "placed prop never slept");
    }

    /// Cancelling placement returns to plain carry with the prop still held.
    #[test]
    fn placement_cancel_returns_to_carry() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        let grab = crate::physics::prop_carry::Tick { grab: true, ..tick() };
        let place = crate::physics::prop_carry::Tick { placement: true, grab: true, ..tick() };
        carry.update(&mut dynamics, grab, carrier(state, 0.));
        carry.update(&mut dynamics, place, carrier(state, 0.));
        assert!(carry.placing());
        carry.update(&mut dynamics, place, carrier(state, 0.));
        assert!(!carry.placing());
        assert_eq!(carry.held(), Some(7), "cancel must keep the carry");
        assert!(carry.layout().is_empty(), "cancel must not record a pose");
        // The Move Object push still works after the cancel.
        carry.update(&mut dynamics, stick(0., 1., 0.), carrier(state, 0.));
        dynamics.step(&world, &mut layer, &[]);
        assert!(dynamics.position_of(7).unwrap().z > 1.2);
    }

    /// A saved layout teleports a fresh body to the stored pose, asleep, and
    /// the rebaked triangles follow.
    #[test]
    fn layout_teleports_fresh_body() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let basis = skate_core::math::Basis3 {
            columns: [[0., 0., -1.], [0., 1., 0.], [1., 0., 0.]],
        };
        let origin = Vector3::new(4., REST_Y + 0.5, -3.);
        let instance = dynamics.teleport(7, origin, basis).unwrap();
        layer.rebake(instance, basis.columns, origin).unwrap();
        let (pose_origin, pose_basis) = dynamics.pose(7).unwrap();
        assert_eq!(pose_origin, origin);
        assert_eq!(pose_basis.columns[2], [1., 0., 0.]);
        assert!(dynamics.bodies[0].asleep);
        // Rotated 90° about Y: the cube is symmetric, but the rebaked probe
        // confirms the range moved to the new origin.
        let hit = layer
            .world()
            .query_thin_line(
                Vector3::new(4., REST_Y + 2., -3.),
                Vector3::new(4., REST_Y - 1., -3.),
            )
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - (REST_Y + 1.)).abs() < 0.05);
        let _ = world;
    }

    /// "Reset moved objects": a moved body is listed, reset returns it to the
    /// authored pose asleep and at rest, and the list empties.
    #[test]
    fn reset_to_spawn_returns_moved_body() {
        let (_world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let (spawn_origin, spawn_basis) = dynamics.spawn_pose(7).unwrap();
        assert_eq!(dynamics.pose(7).unwrap().0, spawn_origin);
        assert!(dynamics.moved_ids().is_empty());
        let basis = skate_core::math::Basis3 {
            columns: [[0., 0., -1.], [0., 1., 0.], [1., 0., 0.]],
        };
        let instance = dynamics.teleport(7, Vector3::new(4., REST_Y + 0.5, -3.), basis).unwrap();
        layer.rebake(instance, basis.columns, Vector3::new(4., REST_Y + 0.5, -3.)).unwrap();
        assert_eq!(dynamics.moved_ids(), vec![7]);
        let instance = dynamics.reset_to_spawn(7).unwrap();
        layer.rebake(instance, spawn_basis.columns, spawn_origin).unwrap();
        let (origin, basis) = dynamics.pose(7).unwrap();
        let d = sub(origin, spawn_origin);
        assert!(d.x.abs() + d.y.abs() + d.z.abs() < 1e-5);
        assert_eq!(basis.columns, spawn_basis.columns);
        assert!(dynamics.bodies[0].asleep);
        assert_eq!(dynamics.bodies[0].rates.linear_velocity, Vector3::ZERO);
        assert!(dynamics.moved_ids().is_empty());
        assert!(dynamics.reset_to_spawn(99).is_none());
    }

    // Upright (retail cMsgUprightDMO, doc 27 "Upright").

    /// Basis rotated by `degrees` about world Z (local Y tips toward -X).
    fn tilted_about_z(degrees: f32) -> skate_core::math::Basis3 {
        let (s, c) = degrees.to_radians().sin_cos();
        skate_core::math::Basis3 { columns: [[c, s, 0.], [-s, c, 0.], [0., 0., 1.]] }
    }

    fn tilt_degrees(basis: skate_core::math::Basis3) -> f32 {
        let up = mul_basis(basis, Vector3::new(0., 1., 0.));
        (up.y / length(up)).clamp(-1., 1.).acos().to_degrees()
    }

    /// 82C573D0 values: stop under 10 deg, axis up x world-up, speed
    /// 3 x (min(tilt, 70 deg) - 5 deg) for |A| <= 1.1, command
    /// (target - w_axis - 0.1 w_perp) x 60, fallback to the body's X / Z axis
    /// above 120 deg.
    #[test]
    fn upright_command_matches_retail_constants() {
        let s = PropUprightSettings::default();
        let a = Vector3::ZERO;
        assert!(upright_command(tilted_about_z(0.), Vector3::ZERO, a, &s).is_none());
        assert!(upright_command(tilted_about_z(9.9), Vector3::ZERO, a, &s).is_none());
        // 30 deg: up = (-sin, cos, 0); up x Y = (0, 0, -sin) -> axis -Z.
        let c = upright_command(tilted_about_z(30.), Vector3::ZERO, a, &s).unwrap();
        let speed = 3.0 * (30f32.to_radians() - 5f32.to_radians());
        assert!((c.z - (-speed * 60.)).abs() < 1e-3 && c.x.abs() < 1e-5 && c.y.abs() < 1e-5, "{c:?}");
        // 90 deg: capped at 70 deg.
        let c = upright_command(tilted_about_z(90.), Vector3::ZERO, a, &s).unwrap();
        let speed = 3.0 * (70f32.to_radians() - 5f32.to_radians());
        assert!((c.z + speed * 60.).abs() < 1e-3, "{c:?}");
        // Spin: along the axis replaced, 10% of the off-axis spin removed.
        let w = Vector3::new(2.0, 0.0, -1.0);
        let c = upright_command(tilted_about_z(90.), w, a, &s).unwrap();
        assert!((c.z - (-speed - (-1.0)) * 60.).abs() < 1e-3, "{c:?}");
        assert!((c.x - (-0.1 * 2.0 * 60.)).abs() < 1e-3, "{c:?}");
        // Gain blend: |A| >= 2.1 -> gain 5.
        let c = upright_command(tilted_about_z(30.), Vector3::ZERO, Vector3::new(0., 3., 0.), &s).unwrap();
        let speed = 5.0 * (30f32.to_radians() - 5f32.to_radians());
        assert!((c.z + speed * 60.).abs() < 1e-3, "{c:?}");
        // Above 120 deg: the body's own Z axis (A.x <= A.z) or X axis (A.x > A.z).
        let basis = tilted_about_z(150.);
        let c = upright_command(basis, Vector3::ZERO, a, &s).unwrap();
        assert!(c.x.abs() < 1e-5 && c.y.abs() < 1e-5 && c.z > 0., "{c:?}");
        let c = upright_command(basis, Vector3::ZERO, Vector3::new(1., 0., 0.), &s).unwrap();
        let x = mul_basis(basis, Vector3::new(1., 0., 0.));
        let along = dot(c, x) / length(c);
        assert!((along - 1.0).abs() < 1e-4, "{c:?}");
    }

    /// A box lying on its side, uprighted, turns back within the 2 s window;
    /// the window closes when the tilt drops under 10 deg.
    #[test]
    fn upright_rights_a_tipped_box_within_the_window() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 0.]);
        let instance = dynamics.teleport(7, Vector3::new(0., REST_Y + 0.05, 0.), tilted_about_z(90.)).unwrap();
        layer.rebake(instance, tilted_about_z(90.).columns, Vector3::new(0., REST_Y + 0.05, 0.)).unwrap();
        assert!(!dynamics.upright(99));
        assert!(dynamics.upright(7));
        assert!(dynamics.is_uprighting(7));
        let mut closed_at = None;
        for step in 1..=120 {
            dynamics.step(&world, &mut layer, &[]);
            if !dynamics.is_uprighting(7) {
                closed_at = Some(step);
                break;
            }
        }
        let tilt = tilt_degrees(dynamics.bodies[0].rates.basis);
        let step = closed_at.expect("window should close by tilt before the 2 s timeout");
        assert!(tilt < 10.0, "closed at step {step} with tilt {tilt}");
        for _ in 0..240 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let tilt = tilt_degrees(dynamics.bodies[0].rates.basis);
        assert!(tilt < 10.0, "settled tilt {tilt}");
    }

    /// With no righting gain the window times out after 2.0 s of 1/60 s
    /// updates (82C56780: open while timer <= 2.0).
    #[test]
    fn upright_window_times_out_at_two_seconds() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 0.]);
        dynamics.teleport(7, Vector3::new(0., REST_Y, 0.), tilted_about_z(90.)).unwrap();
        let mut table = PropTuningTable::default();
        table.upright.gain_min = 0.0;
        table.upright.gain_max = 0.0;
        dynamics.set_tuning(table);
        let s = PropUprightSettings::default();
        let mut expected = 0u32;
        let mut t = 0f32;
        while t <= s.window_seconds {
            t += s.tick_seconds;
            expected += 1;
        }
        assert!((120..=121).contains(&expected));
        assert!(dynamics.upright(7));
        let mut closed_at = None;
        for step in 1..=200 {
            dynamics.step(&world, &mut layer, &[]);
            if !dynamics.is_uprighting(7) {
                closed_at = Some(step);
                break;
            }
        }
        assert_eq!(closed_at, Some(expected));
        assert!(tilt_degrees(dynamics.bodies[0].rates.basis) > 10.0);
    }

    /// 82C52E68 refuses Move Object yaw while the window is open; the linear
    /// command still applies. After the window closes yaw applies again.
    #[test]
    fn upright_blocks_move_object_yaw_during_the_window() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 0.]);
        dynamics.teleport(7, Vector3::new(0., REST_Y, 0.), tilted_about_z(90.)).unwrap();
        assert!(dynamics.upright(7));
        let dt = dynamics.step_simulation().time_step;
        let w0 = dynamics.bodies[0].rates.angular_velocity;
        let v0 = dynamics.bodies[0].rates.linear_velocity;
        assert!(dynamics.apply_move_command(7, Vector3::new(3., 0., 0.), 5.0, Vector3::ZERO, dt));
        assert_eq!(dynamics.bodies[0].rates.angular_velocity, w0, "yaw refused");
        assert!((dynamics.bodies[0].rates.linear_velocity.x - (v0.x + 3. * dt)).abs() < 1e-6, "linear applied");
        // Mod knob: block_yaw off lets the yaw through.
        let mut table = PropTuningTable::default();
        table.upright.block_yaw = false;
        dynamics.set_tuning(table);
        assert!(dynamics.apply_move_command(7, Vector3::ZERO, 5.0, Vector3::ZERO, dt));
        assert!((dynamics.bodies[0].rates.angular_velocity.y - (w0.y + 5. * dt)).abs() < 1e-6);
        dynamics.set_tuning(PropTuningTable::default());
        // An upright body closes the window on its first update; yaw applies again.
        let (world2, mut layer2, mut upright) = fixture([0., REST_Y, 0.]);
        assert!(upright.upright(7));
        upright.step(&world2, &mut layer2, &[]);
        assert!(!upright.is_uprighting(7));
        let w = upright.bodies[0].rates.angular_velocity;
        assert!(upright.apply_move_command(7, Vector3::ZERO, 5.0, Vector3::ZERO, dt));
        assert!((upright.bodies[0].rates.angular_velocity.y - (w.y + 5. * dt)).abs() < 1e-6);
        let _ = (world, &mut layer);
    }

    // NPC skaters against props (doc 26, fix 19).

    /// One NPC skater rolling along +X (its +Z forward turned to +X) at `speed`, at `x`.
    fn npc_sample(x: f32, speed: f32) -> skate_core::living_world::replay::ReplaySample {
        let q = bevy::prelude::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        skate_core::living_world::replay::ReplaySample {
            line: [0; 16],
            node: 0,
            position: [x, super::super::ground::HEIGHT, 0.0],
            velocity: [speed, 0.0, 0.0],
            heading: std::f32::consts::FRAC_PI_2,
            board: [q.x, q.y, q.z, q.w],
            skater: [q.x, q.y, q.z, q.w],
            flags: 0,
            phase: skate_core::living_world::replay::ReplayPhase::Rolling,
            jump: None,
            phase_frames: 0,
            previous_phase: None,
            previous_phase_frames: 0,
            sub_frame: 0.0,
            fakie: false,
        }
    }

    const NPC: skate_core::living_world::LivingWorldId =
        skate_core::living_world::LivingWorldId { kind: skate_core::living_world::Kind::Skater, serial: 7 };

    /// Run an NPC line along +X through a prop centred at x = 0 for `ticks` 60 Hz ticks.
    /// Returns the prop centre x per tick and the NPC x per tick.
    fn npc_run(
        dynamics: &mut PropDynamics,
        world: &BoardWorld,
        layer: &mut crate::skate_world::PropCollisionLayer,
        start_x: f32,
        speed: f32,
        ticks: usize,
        push: bool,
    ) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for tick in 0..ticks {
            let x = start_x + speed * tick as f32 / 60.0;
            let volumes = if push {
                crate::living_world::npc_skaters::prop_volumes(NPC, &npc_sample(x, speed)).to_vec()
            } else {
                Vec::new()
            };
            dynamics.step_with_actors(world, layer, &[], &volumes);
            out.push((dynamics.bodies[0].rates.position.x, x));
        }
        out
    }

    /// A bin-sized prop resting on an NPC skater's line is knocked ahead of it (the player's push
    /// rule), the NPC owns the moved prop, and the bin never ends behind the NPC's board.
    #[test]
    fn npc_skater_knocks_a_bin_on_its_line_out_of_the_way() {
        let half = [0.3, 0.45, 0.3];
        let rest = super::super::ground::HEIGHT + 0.45;
        let (world, mut layer, mut dynamics) = box_fixture([0., rest, 0.], half);
        let control = {
            let (world, mut layer, mut dynamics) = box_fixture([0., rest, 0.], half);
            npc_run(&mut dynamics, &world, &mut layer, -3.0, 5.0, 120, false)
        };
        assert!(control.iter().all(|(p, _)| p.abs() < 1e-4), "nothing touches the control bin");
        let run = npc_run(&mut dynamics, &world, &mut layer, -3.0, 5.0, 120, true);
        let (end_prop, end_npc) = *run.last().unwrap();
        assert!(end_prop > 1.0, "bin pushed ahead along the line: x {end_prop}");
        assert_eq!(dynamics.pushed_by(dynamics.bodies[0].id), Some(crate::living_world::npc_skaters::PROXY_ID_TAG | NPC.to_u64()));
        // No pass-through: once the board reached the bin, the bin's centre stays ahead of the
        // board's nose (0.4 m) minus the bin's own half depth, or it was knocked clear sideways.
        for &(prop, npc) in &run {
            assert!(prop - npc > -(0.4 + half[0]) - 0.05 || dynamics.bodies[0].rates.position.z.abs() > 0.6, "bin {prop} behind npc {npc}");
        }
        let _ = end_npc;
    }

    /// A bench (126 kg) on the line is pushed by the same rule (weight-scaled, board-capped);
    /// the result is identical bit for bit on a second run (seeded, ordered by actor id).
    #[test]
    fn npc_skater_bench_push_is_deterministic() {
        let runs: Vec<Vec<(f32, f32)>> = (0..2)
            .map(|_| {
                let (world, mut layer, mut dynamics) = box_fixture([0., BENCH_REST_Y, 0.], BENCH_HALF);
                npc_run(&mut dynamics, &world, &mut layer, -3.0, 5.0, 120, true)
            })
            .collect();
        assert_eq!(runs[0], runs[1]);
        let (end_prop, _) = *runs[0].last().unwrap();
        assert!(end_prop > 0.5, "bench pushed along the line: x {end_prop}");
    }

    /// NPC volumes and the local player's both push in one step; the last actually pushing actor
    /// owns the prop, and a far NPC does not touch it.
    #[test]
    fn far_npc_leaves_props_alone_and_local_push_owns_it() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 0.]);
        let far = crate::living_world::npc_skaters::prop_volumes(NPC, &npc_sample(-20.0, 5.0));
        let local = [BoardWorldVolume {
            collision_group: 4,
            body: CollisionBody::Board(BodyId::Deck),
            primitive: ContactPrimitive::Sphere(Sphere { center: Vector3::new(-0.55, REST_Y, 0.), radius: 0.2 }),
            motion: skate_core::physics::board_world::VolumeMotion { linear_velocity: Vector3::new(2., 0., 0.), ..Default::default() },
            material: material(),
        }];
        dynamics.step_with_actors(&world, &mut layer, &[], &far);
        assert!(dynamics.bodies[0].asleep);
        assert_eq!(dynamics.pushed_by(dynamics.bodies[0].id), None);
        dynamics.step_with_actors(&world, &mut layer, &local, &far);
        assert_eq!(dynamics.pushed_by(dynamics.bodies[0].id), Some(LOCAL_PUSHER));
    }

    // Dragged props sink through the floor (2026-10-07, DownTown, doc 26).

    /// DownTown-like sidewalk height (the log's props rest with bottoms at 12.6).
    const SIDEWALK_Y: f32 = 12.6;
    /// Street one curb lower, from z = CURB_Z on.
    const STREET_Y: f32 = SIDEWALK_Y - 0.15;
    const CURB_Z: f32 = 6.0;

    /// A map collision world like a city street: 1 m one-sided floor tiles
    /// (many interior edges, as the district meshes), a sidewalk, a curb face
    /// and the street below it.
    fn street_world() -> BoardWorld {
        let mut collision = Vec::new();
        let mut push = |points: [[f32; 3]; 3]| {
            collision.push(skate_data::skate_map::Collision { points, surface: 0, material: 1, native_edges: None });
        };
        for xi in -8..8 {
            for zi in -6..18 {
                let (x0, x1, z0, z1) = (xi as f32, xi as f32 + 1.0, zi as f32, zi as f32 + 1.0);
                let y = if z0 < CURB_Z { SIDEWALK_Y } else { STREET_Y };
                push([[x0, y, z0], [x0, y, z1], [x1, y, z1]]);
                push([[x0, y, z0], [x1, y, z1], [x1, y, z0]]);
            }
            let (x0, x1) = (xi as f32, xi as f32 + 1.0);
            push([[x0, STREET_Y, CURB_Z], [x1, STREET_Y, CURB_Z], [x1, SIDEWALK_Y, CURB_Z]]);
            push([[x0, STREET_Y, CURB_Z], [x1, SIDEWALK_Y, CURB_Z], [x0, SIDEWALK_Y, CURB_Z]]);
        }
        let map = skate_data::skate_map::SkateMap {
            version: 14,
            name: "street".into(),
            spawn: [0.; 3],
            heading: 0.,
            environment: vec![0.; 45],
            materials: vec![skate_data::skate_map::Material {
                name: "concrete".into(),
                flags: 0,
                friction: 0.5,
                restitution: 0.1,
                color: [1.; 3],
                roughness: 0.5,
                emissive: 0.,
                textures: [0; 5],
                indirect_strength: 0.,
                alpha_mode: 0,
                alpha_cutoff: 0.5,
                audio: 3,
                physics: 1,
                pattern: 0,
                depth_layer: None,
                retail_definition: None,
            }],
            textures: vec![],
            geometry: skate_data::skate_map::Geometry { vertices: vec![], indices: vec![], collision },
            rails: vec![],
            doors: vec![],
            lights: vec![],
            routes: vec![],
            extensions: vec![],
        };
        crate::skate_world::collision_world(&map, floor_material()).unwrap()
    }

    /// Box sizes of the props the user dragged in the 2026-10-07 session
    /// (half extents from the PED_OBSTACLE lines: bench, bin, vending, rail).
    const DRAGGED_TEMPLATES: [(&str, [f32; 3]); 4] = [
        ("bench", [2.30, 0.41, 0.29]),
        ("bin", [0.28, 0.41, 0.27]),
        ("vending", [0.50, 0.95, 0.45]),
        ("rail", [2.00, 0.30, 0.12]),
    ];

    struct DragRun {
        /// Lowest box bottom minus floor seen over the whole run.
        worst_gap: f32,
        /// Smallest up-axis Y while held (1 = upright).
        held_up_min: f32,
        /// Horizontal distance from the spawn at release and at the end.
        away_at_release: f32,
        away_at_end: f32,
        end: PropGroundProbe,
        asleep: bool,
    }

    /// Grab the prop from behind, push it `seconds` along +Z (full stick) across
    /// the curb with the skater following the grab frame, release, then let it
    /// settle 3 s. The held id is fed back through `set_held` like
    /// `GamePhysics::update_prop_carry`.
    fn drag_and_release(half: [f32; 3], seconds: f32) -> DragRun {
        let world = street_world();
        let (_, mut layer, mut dynamics) = box_fixture([0., SIDEWALK_Y + half[1], 0.], half);
        let id = dynamics.bodies[0].id;
        let spawn = dynamics.position_of(id).unwrap();
        let dt = simulation().time_step;
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        use skate_core::player::state::PhysicalStateId::{BipedGround, OffBoardPushing};
        let mut at = crate::physics::prop_carry::Carrier {
            state: BipedGround,
            position: Vector3::new(0., SIDEWALK_Y + 0.9, -(half[2] + 0.6)),
            forward: Vector3::new(0., 0., 1.),
            time_step: dt,
            skeleton: None,
        };
        carry.update(&mut dynamics, tick(), at);
        assert_eq!(carry.held(), Some(id), "grab failed");
        dynamics.set_held(carry.held());
        at.state = OffBoardPushing;
        let mut worst_gap = f32::INFINITY;
        let mut held_up_min = 1.0f32;
        let track = |dynamics: &PropDynamics, worst: &mut f32| {
            let probe = dynamics.ground_probe(id, &world).unwrap();
            if let Some(gap) = probe.gap() {
                *worst = worst.min(gap);
            }
            probe
        };
        let ticks = (seconds / dt) as u32;
        for _ in 1..=ticks {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 1., 0.), at);
            dynamics.set_held(carry.held());
            dynamics.step(&world, &mut layer, &[]);
            track(&dynamics, &mut worst_gap);
            held_up_min = held_up_min.min(dynamics.bodies[0].rates.basis.columns[1][1]);
        }
        assert_eq!(carry.held(), Some(id), "the push dropped the prop");
        carry.update(&mut dynamics, release(), at);
        dynamics.set_held(carry.held());
        let flat = |p: Vector3| ((p.x - spawn.x).powi(2) + (p.z - spawn.z).powi(2)).sqrt();
        let away_at_release = flat(dynamics.position_of(id).unwrap());
        let mut end = track(&dynamics, &mut worst_gap);
        for _ in 0..180 {
            dynamics.step(&world, &mut layer, &[]);
            end = track(&dynamics, &mut worst_gap);
        }
        DragRun {
            worst_gap,
            held_up_min,
            away_at_release,
            away_at_end: flat(end.center),
            end,
            asleep: dynamics.bodies[0].asleep,
        }
    }

    /// Prop sinking (2026-10-07): every dragged template stays on the floor
    /// (box bottom never more than a few cm into it), rests on the street
    /// after release and does not slide back toward its spawn. Tipping is
    /// allowed (retail leaves pitch / roll to the physics).
    #[test]
    fn dragged_props_rest_on_the_floor_after_release() {
        let mut failures = Vec::new();
        for (name, half) in DRAGGED_TEMPLATES {
            let run = drag_and_release(half, 5.0);
            println!(
                "{name}: worst gap {:.3} m, held up_y min {:.4}, away {:.2} -> {:.2} m, end {:?}, asleep {}",
                run.worst_gap, run.held_up_min, run.away_at_release, run.away_at_end, run.end, run.asleep
            );
            let rest = run.end.gap();
            let checks = [
                (run.worst_gap > -0.08, format!("{name} sank {} m into the floor", -run.worst_gap)),
                (!run.end.below_ground(), format!("{name} ended below the floor: {:?}", run.end)),
                (rest.is_some_and(|g| g.abs() < 0.08), format!("{name} not resting on the street: gap {rest:?}")),
                (run.away_at_end > run.away_at_release - 0.1, format!("{name} slid back toward its spawn: {} -> {}", run.away_at_release, run.away_at_end)),
                // Moved at all (a full push may tip a prop over; tipping is allowed).
                (run.away_at_release > 0.5, format!("{name} was not dragged: {}", run.away_at_release)),
            ];
            failures.extend(checks.into_iter().filter(|c| !c.0).map(|c| c.1));
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// Tipping stays (user: "props should still be able to tip.. they do in
    /// retail"): a tall prop pushed off-centre from the street into the curb
    /// face tips (the command has no pitch / roll term, the contacts decide),
    /// and never ends under the floor.
    #[test]
    fn pushing_a_tall_prop_into_the_curb_can_tip_it() {
        let world = street_world();
        let half = [0.50, 0.95, 0.45];
        let (_, mut layer, mut dynamics) = box_fixture([0., STREET_Y + half[1], CURB_Z + 1.2], half);
        let id = dynamics.bodies[0].id;
        let dt = simulation().time_step;
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        use skate_core::player::state::PhysicalStateId::{BipedGround, OffBoardPushing};
        let mut at = crate::physics::prop_carry::Carrier {
            state: BipedGround,
            position: Vector3::new(0.3, STREET_Y + 0.9, CURB_Z + 1.2 + half[2] + 0.6),
            forward: Vector3::new(0., 0., -1.),
            time_step: dt,
            skeleton: None,
        };
        carry.update(&mut dynamics, tick(), at);
        assert_eq!(carry.held(), Some(id), "grab failed");
        dynamics.set_held(carry.held());
        at.state = OffBoardPushing;
        let mut up_min = 1.0f32;
        let mut worst_gap = f32::INFINITY;
        for _ in 0..180 {
            at = follow(&carry, at);
            carry.update(&mut dynamics, stick(0., 1., 0.), at);
            dynamics.set_held(carry.held());
            dynamics.step(&world, &mut layer, &[]);
            up_min = up_min.min(dynamics.bodies[0].rates.basis.columns[1][1]);
            if let Some(gap) = dynamics.ground_probe(id, &world).unwrap().gap() {
                worst_gap = worst_gap.min(gap);
            }
            if carry.held().is_none() {
                break;
            }
        }
        println!("tall prop into the curb: up_y min {up_min:.3}, worst gap {worst_gap:.3}");
        assert!(up_min < 0.97, "the prop never tipped against the curb: up_y min {up_min}");
        assert!(!dynamics.ground_probe(id, &world).unwrap().below_ground(), "the tipped prop went under the floor");
    }

    /// Retail props do not tip from a push (spec 7.4): the command acts at the
    /// centre of mass with no pitch / roll term and the held prop runs on the
    /// commanded block, so a straight full push on flat ground keeps a 1 m
    /// cube and the bin upright (tipping comes from obstacles, test above).
    #[test]
    fn straight_push_on_flat_ground_does_not_tip_a_cube_or_the_bin() {
        for (name, half) in [("cube", [0.5f32, 0.5, 0.5]), ("bin", [0.28, 0.41, 0.27])] {
            let world = street_world();
            let (_, mut layer, mut dynamics) = box_fixture([0., SIDEWALK_Y + half[1], -4.0], half);
            let id = dynamics.bodies[0].id;
            let dt = simulation().time_step;
            let mut carry = crate::physics::prop_carry::PropCarry::default();
            use skate_core::player::state::PhysicalStateId::{BipedGround, OffBoardPushing};
            let mut at = crate::physics::prop_carry::Carrier {
                state: BipedGround,
                position: Vector3::new(0., SIDEWALK_Y + 0.9, -4.0 - (half[2] + 0.6)),
                forward: Vector3::new(0., 0., 1.),
                time_step: dt,
                skeleton: None,
            };
            carry.update(&mut dynamics, tick(), at);
            assert_eq!(carry.held(), Some(id), "{name}: grab failed");
            dynamics.set_held(carry.held());
            at.state = OffBoardPushing;
            let mut up_min = 1.0f32;
            // 2 s of full push stays on the flat sidewalk (curb at z = 6).
            for _ in 0..120 {
                at = follow(&carry, at);
                carry.update(&mut dynamics, stick(0., 1., 0.), at);
                dynamics.set_held(carry.held());
                dynamics.step(&world, &mut layer, &[]);
                up_min = up_min.min(dynamics.bodies[0].rates.basis.columns[1][1]);
            }
            let z = dynamics.position_of(id).unwrap().z;
            println!("{name}: up_y min {up_min:.4}, z {z:.2}");
            assert_eq!(carry.held(), Some(id), "{name}: the push dropped the prop");
            assert!(z < CURB_Z - half[2], "{name}: reached the curb, test not on flat ground: z {z}");
            assert!(z > -4.0 + 1.0, "{name}: barely moved: z {z}");
            assert!(up_min > 0.99, "{name}: a straight push tipped it: up_y min {up_min}");
        }
    }

    /// The commanded block (82C53EF8): a commanded body switches to it on its
    /// next step and back to its free block on the step after the commands
    /// stop. The block is the body's own material, combined with the other
    /// side by 82763078 (static max, dynamic max, restitution min); nothing
    /// replaces the combined friction. Every command wakes the body, zero or
    /// not (82ADF7B8).
    #[test]
    fn commanded_block_switches_with_the_command_and_zero_commands_wake() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        assert!(dynamics.bodies[0].asleep);
        let authored = dynamics.bodies[0].material;
        let low = RetailContactMaterial { static_friction: 0.01, dynamic_friction: 0.005, restitution: 0.9 };
        assert_eq!(dynamics.body_material(0), authored, "free block = authored material by default");
        assert!(dynamics.apply_move_command(7, Vector3::ZERO, 0.0, Vector3::ZERO, simulation().time_step));
        assert!(!dynamics.bodies[0].asleep, "a zero command must still wake the body");
        dynamics.step(&world, &mut layer, &[]);
        let held = dynamics.body_material(0);
        assert_eq!((held.static_friction, held.dynamic_friction, held.restitution), (0.03, 0.02, authored.restitution));
        // Combine, not replace: against a low-friction side the held block wins,
        // against the floor material the higher friction wins (max / max / min).
        let c = dynamics.contact_material(0, low);
        assert_eq!((c.static_friction, c.dynamic_friction, c.restitution), (0.03, 0.02, authored.restitution.min(0.9)));
        let floor = material();
        let c = dynamics.contact_material(0, floor);
        assert_eq!(c, combine_contact_materials(held, floor));
        assert_eq!(c.dynamic_friction, floor.dynamic_friction.max(0.02));
        dynamics.step(&world, &mut layer, &[]);
        assert_eq!(dynamics.body_material(0), authored, "block not restored after commands stopped");
        // Per prop type override (mod) and the old wake rule.
        let mut rules = MoveCommandRules::default();
        rules.by_template.insert(
            "template/crate".into(),
            PropMaterialBlocks { held: Some([0.4, 0.3]), free: Some([0.9, 0.8]), restitution: Some(0.2), ..Default::default() },
        );
        rules.wake_on_command = false;
        dynamics.set_move_rules(rules);
        assert_eq!(dynamics.body_material(0), RetailContactMaterial { static_friction: 0.9, dynamic_friction: 0.8, restitution: 0.2 });
        dynamics.bodies[0].asleep = true;
        dynamics.apply_move_command(7, Vector3::ZERO, 0.0, Vector3::ZERO, simulation().time_step);
        assert!(dynamics.bodies[0].asleep, "wake_on_command off: a zero command leaves the body asleep");
        dynamics.apply_move_command(7, Vector3::new(1.0, 0.0, 0.0), 0.0, Vector3::ZERO, simulation().time_step);
        dynamics.step(&world, &mut layer, &[]);
        assert_eq!(dynamics.body_material(0), RetailContactMaterial { static_friction: 0.4, dynamic_friction: 0.3, restitution: 0.2 });
    }

    /// Free pair choice (82C53EF8 / 82C54BF0 with 82C54B00): only a type with
    /// the upright flag (data +312 bit 0) uses the upright pair, and only while
    /// its up axis y > 0.65; tipped (or exactly 0.65) it uses the default pair.
    /// The held block ignores the upright test.
    #[test]
    fn free_block_follows_the_upright_test() {
        let authored = RetailContactMaterial { static_friction: 0.5, dynamic_friction: 0.5, restitution: 0.1 };
        let blocks = PropMaterialBlocks {
            free: Some([0.6, 0.5]),
            free_upright: Some([0.9, 0.7]),
            restitution: Some(0.3),
            ..Default::default()
        };
        let mut rules = MoveCommandRules::default();
        rules.by_template.insert("t".into(), blocks);
        let pair = |r: &MoveCommandRules, commanded, up_y| {
            let m = r.body_material("t", authored, commanded, up_y);
            [m.static_friction, m.dynamic_friction, m.restitution]
        };
        // Flag off: the default pair whatever the pose.
        assert_eq!(pair(&rules, false, 1.0), [0.6, 0.5, 0.3]);
        assert_eq!(pair(&rules, false, 0.0), [0.6, 0.5, 0.3]);
        rules.by_template.get_mut("t").unwrap().upright_pair = Some(true);
        assert_eq!(pair(&rules, false, 1.0), [0.9, 0.7, 0.3]);
        assert_eq!(pair(&rules, false, 0.66), [0.9, 0.7, 0.3]);
        assert_eq!(pair(&rules, false, 0.65), [0.6, 0.5, 0.3], "strict > 0.65");
        assert_eq!(pair(&rules, false, -1.0), [0.6, 0.5, 0.3]);
        assert_eq!(pair(&rules, true, 1.0), [0.03, 0.02, 0.3]);
        assert_eq!(pair(&rules, true, 0.0), [0.03, 0.02, 0.3]);
        // Threshold is data.
        rules.upright_cos = 0.9;
        assert_eq!(pair(&rules, false, 0.8), [0.6, 0.5, 0.3]);
        // No type data at all: authored material, held keeps the authored restitution.
        let plain = MoveCommandRules::default();
        assert_eq!(plain.body_material("other", authored, false, 1.0), authored);
        assert_eq!(pair(&plain, true, 1.0)[2], 0.1);
        // Deterministic: same inputs, same bits.
        assert_eq!(pair(&rules, false, 0.95).map(f32::to_bits), pair(&rules, false, 0.95).map(f32::to_bits));
    }

    /// Slot 9 sinks (82D9CC78 / 82D9CCF0): the linear command adds L dt at the
    /// centre of mass with no mass factor and no torque, the vertical part is
    /// dropped, and the yaw command adds (0, Y, 0) dt with no inertia factor.
    #[test]
    fn move_command_is_an_acceleration_at_the_centre_of_mass() {
        let (_, _, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let dt = simulation().time_step;
        let grip = Vector3::new(0.4, REST_Y, 0.7);
        dynamics.apply_move_command(7, Vector3::new(6.0, 5.0, -12.0), 3.0, grip, dt);
        let r = dynamics.bodies[0].rates;
        assert!((r.linear_velocity.x - 6.0 * dt).abs() < 1e-6 && (r.linear_velocity.z + 12.0 * dt).abs() < 1e-6);
        assert_eq!(r.linear_velocity.y, 0.0, "vertical command must be dropped");
        assert_eq!((r.angular_velocity.x, r.angular_velocity.z), (0.0, 0.0), "no lever torque at the centre of mass");
        assert!((r.angular_velocity.y - 3.0 * dt).abs() < 1e-6);
        assert_eq!(r.torque_acceleration, Vector3::ZERO);
    }

    /// Contact gap (2026-10-08 street trace): a box lying tilted on the tiled
    /// floor, a few cm into it, must still produce floor manifolds; the
    /// dragged bin lost every floor contact in this pose and fell through.
    /// Cause: the SAT axis is the tilted box face (1-8 deg off the floor),
    /// which triangle fixup sees as an edge region on a welded flat edge;
    /// the GP pair query (is_object false) rejects it on every tile, the
    /// object world query (the production path, `world_query_for`) accepts
    /// it while the tilt stays within acos(1 - convexity_epsilon) = 8.1 deg. The pair query is still checked to keep the repro honest.
    #[test]
    fn lying_tilted_box_keeps_floor_contacts() {
        let world = street_world();
        let half = [0.28f32, 0.41, 0.27];
        let (_, _, dynamics) = box_fixture([0., SIDEWALK_Y + half[1], 0.], half);
        let pair = dynamics.pair_for(0);
        let query = dynamics.world_query_for(0);
        let mut misses = Vec::new();
        let mut pair_misses = 0;
        let mut lowest_up = 1.0f32;
        for step in 0..36 {
            let roll = std::f32::consts::FRAC_PI_2 + (step as f32 - 18.0).to_radians();
            let (s, c) = roll.sin_cos();
            // Rows = local axes in world space: roll about world Z.
            let basis = Basis3 { columns: [[c, s, 0.], [-s, c, 0.], [0., 0., 1.]] };
            let down = half[0] * s.abs() + half[1] * c.abs();
            for depth in [0.0f32, 0.02, 0.05] {
                let center = Vector3::new(0.37, SIDEWALK_Y + down - depth, 2.41);
                let primitive = ContactPrimitive::RoundedBox { center, basis, half_extents: Vector3::new(half[0], half[1], half[2]), radius: 0.0 };
                let bounds = skate_core::physics::board_world::query_metadata::Bounds {
                    min: Vector3::new(center.x - 0.6, center.y - 0.6, center.z - 0.6),
                    max: Vector3::new(center.x + 0.6, center.y + 0.6, center.z + 0.6),
                };
                let mut pair_manifolds = 0;
                let mut candidates = Vec::new();
                for range in world.candidate_ranges(Some(bounds)) {
                    for triangle in &world.triangles()[range] {
                        if primitive_pair_contacts(primitive, ContactPrimitive::Triangle(triangle.triangle), pair).is_some() {
                            pair_manifolds += 1;
                        }
                        candidates.push(triangle.triangle);
                    }
                }
                let mut manifolds = 0;
                for triangle in candidates {
                    if let Some(manifold) = primitive_triangle_world_contacts(primitive, triangle, Vector3::ZERO, query) {
                        manifolds += 1;
                        lowest_up = lowest_up.min(manifold.normal.y);
                    }
                }
                if manifolds == 0 {
                    misses.push((step as i32 - 18, depth));
                }
                if pair_manifolds == 0 {
                    pair_misses += 1;
                }
            }
        }
        println!("tilted box: pair-query misses {pair_misses}, lowest floor normal up {lowest_up:.4}");
        assert!(misses.is_empty(), "no floor contact for (roll offset deg, depth m): {misses:?}");
        assert!(pair_misses > 0, "the GP pair query no longer drops these contacts: re-check the repro");
        assert!(lowest_up > 0.0, "a floor manifold pushes the box down: {lowest_up}");
    }

    /// PROP_BELOW_GROUND's test: a body pushed under the floor face is
    /// detected (the probe starts above its last rest height), one resting
    /// on it is not.
    #[test]
    fn ground_probe_flags_a_body_under_the_floor() {
        let world = street_world();
        let (_, _, mut dynamics) = box_fixture([0., SIDEWALK_Y + 0.4, 0.], [0.3, 0.4, 0.3]);
        let id = dynamics.bodies[0].id;
        let resting = dynamics.ground_probe(id, &world).unwrap();
        assert!(!resting.below_ground() && resting.gap().unwrap().abs() < 1e-3, "{resting:?}");
        dynamics.bodies[0].rates.position.y = SIDEWALK_Y - 0.6;
        let sunk = dynamics.ground_probe(id, &world).unwrap();
        assert!(sunk.below_ground(), "sunk body not flagged: {sunk:?}");
    }

    /// Real DownTown repro (private assets): drag the props the user moved on
    /// 2026-10-07 and release them. `SKATE3_ASSET_ROOT` = assets root,
    /// `SKATE3_MAP` = DownTown.skate. Run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn downtown_dragged_props_rest_on_the_floor() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT"));
        let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").expect("set SKATE3_MAP"));
        let map = skate_data::skate_map::SkateMap::parse(&std::fs::read(&map_path).unwrap()).unwrap();
        let world = crate::skate_world::collision_world(&map, floor_material()).unwrap();
        let ids = [3417526289u32, 880096370, 44597382, 3160070536, 788476715, 2198011218, 3116907260, 206220507];
        let dt = simulation().time_step;
        let mut failures = Vec::new();
        for id in ids {
            let (mut layer, mut dynamics) = crate::skate_world::load_prop_layer(&root, "DownTown", floor_material(), simulation()).expect("DownTown props");
            let Some(spawn) = dynamics.position_of(id) else { println!("{id}: not in the package"); continue };
            let index = dynamics.by_id[&id];
            let half = dynamics.bodies[index].half_extents;
            let ground = dynamics.ground_probe(id, &world).unwrap();
            let floor = ground.ground.unwrap_or(spawn.y - half.y);
            let forward = Vector3::new(0., 0., 1.);
            let reach = dynamics.bodies[index].bounds();
            let start_z = reach.min.z - 0.6;
            let at = |state, z: f32| crate::physics::prop_carry::Carrier {
                state,
                position: Vector3::new(spawn.x, floor + 0.9, z),
                forward,
                time_step: dt,
                skeleton: None,
            };
            use skate_core::player::state::PhysicalStateId::{BipedGround, OffBoardPushing};
            let mut carry = crate::physics::prop_carry::PropCarry::default();
            carry.update(&mut dynamics, tick(), at(BipedGround, start_z));
            if carry.held() != Some(id) {
                println!("{id}: grabbed {:?} instead", carry.held());
                continue;
            }
            dynamics.set_held(carry.held());
            let mut worst = f32::INFINITY;
            let mut up_min = 1.0f32;
            let mut skater = at(OffBoardPushing, start_z);
            for _ in 1..=180 {
                skater = follow(&carry, skater);
                carry.update(&mut dynamics, stick(0., 1., 0.), skater);
                dynamics.set_held(carry.held());
                dynamics.step(&world, &mut layer, &[]);
                worst = worst.min(dynamics.ground_probe(id, &world).unwrap().gap().unwrap_or(0.));
                up_min = up_min.min(dynamics.bodies[index].rates.basis.columns[1][1]);
            }
            carry.update(&mut dynamics, release(), skater);
            dynamics.set_held(carry.held());
            for _ in 0..180 {
                dynamics.step(&world, &mut layer, &[]);
                worst = worst.min(dynamics.ground_probe(id, &world).unwrap().gap().unwrap_or(0.));
            }
            let end = dynamics.ground_probe(id, &world).unwrap();
            println!(
                "{id} {}: spawn {spawn:?} floor {floor:.2} worst gap {worst:.3} held up_y min {up_min:.4} end {end:?} asleep {}",
                dynamics.bodies[index].template, dynamics.bodies[index].asleep
            );
            if end.below_ground() || worst < -0.1 {
                failures.push(id);
            }
        }
        assert!(failures.is_empty(), "props sank: {failures:?}");
    }
}
