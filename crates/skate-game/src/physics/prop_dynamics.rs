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
//! skater's board/skeleton volumes for pushes. Contact response is a compact
//! impulse pass producing `RetailReactionCorrections`; the retail compiled-row
//! contact solver is explicitly not gameplay-ready (`build_contact_jacobian`).
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
        contact::{RetailContactMaterial, combine_contact_materials},
        mass::{RETAIL_UNBOUNDED_VELOCITY, primitive_mass_properties},
        rigid_body::{
            RetailInertiaDynamics, RetailQuaternion, RetailReactionCorrections, RetailBodyRates,
            RetailSimulationStep, integrate_body_rates, world_inverse_inertia,
        },
        world_contact::{
            ContactPrimitive, PrimitiveContactManifold, PrimitivePairSettings,
            primitive_pair_contacts,
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
    /// Penetration ignored by the positional correction (m).
    pub penetration_slop: f32,
    /// Fraction of the penetration removed per tick (Baumgarte).
    pub penetration_correction: f32,
    /// Upper bound on the positional correction of one body in one tick (m).
    /// A body deep inside geometry comes out over several ticks instead of
    /// being thrown out in one.
    pub max_depenetration_per_tick: f32,
    /// Restitution applies only above this closing speed (m/s); below it
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

/// Props get their own simulation step: the board's simulation carries
/// cool_down = 0 (the host never sleeps it), which would freeze props after a
/// single tick, and its FreezingEnergy threshold is tuned for a ~kg-scale
/// board, while props are density-100 boxes (energy scales with mass, so
/// resting contact jitter alone keeps a prop above the board's threshold).
pub(crate) fn prop_simulation(
    base: skate_core::physics::rigid_body::RetailSimulationStep,
) -> skate_core::physics::rigid_body::RetailSimulationStep {
    skate_core::physics::rigid_body::RetailSimulationStep {
        cool_down: 30,
        minimum_energy: 0.5,
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
        }
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
        self.held = held;
    }

    fn is_held(&self, index: usize) -> bool {
        self.held == Some(self.bodies[index].id)
    }

    /// Skate 3 style drag: the prop stays on the ground and is pulled
    /// horizontally toward `target` (its Y is untouched, so gravity and
    /// ground contacts keep working). Rotation stays frozen. Returns false if
    /// the id is unknown.
    pub(crate) fn drag_to(
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
        let flat = Vector3::new(delta.x, 0.0, delta.z);
        let distance = dot(flat, flat).sqrt();
        let speed = (distance / time_step).min(max_speed);
        body.rates.linear_velocity = if distance > 1e-6 {
            let pulled = scale(flat, speed / distance);
            Vector3::new(pulled.x, body.rates.linear_velocity.y, pulled.z)
        } else {
            Vector3::new(0.0, body.rates.linear_velocity.y, 0.0)
        };
        body.rates.angular_velocity = Vector3::ZERO;
        true
    }

    /// Turn the dragged body about world +Y at `rate` rad/s (positive turns
    /// +Z toward +X), so a held prop turns with its carrier. Call after
    /// [`Self::drag_to`], which freezes rotation. Returns false if the id is
    /// unknown.
    pub(crate) fn set_yaw_rate(&mut self, id: u32, rate: f32) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let rate = if rate.is_finite() { rate } else { 0.0 };
        self.bodies[index].rates.angular_velocity = Vector3::new(0.0, rate, 0.0);
        true
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
        let cool_down = self.simulation.cool_down;
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
            if self.bodies[index].asleep {
                continue;
            }
            self.stats.awake += 1;
            let corrections = self.contact_corrections(index, world);
            let body_sleep_capable = self.bodies[index].enable_sleep;
            // Snap to rest below the sleep threshold, but only while something
            // is actually touching the body: without the contact gate the snap
            // zeroes the first ticks of a fall (g·dt is far below the sleep
            // threshold) and the prop descends at g·dt² per tick forever.
            let resting = body_sleep_capable
                && (dot(corrections.linear_displacement, corrections.linear_displacement)
                    > 0.0
                    || dot(corrections.position_displacement, corrections.position_displacement)
                        > 0.0
                    || dot(corrections.angular_displacement, corrections.angular_displacement)
                        > 0.0);
            let body = &mut self.bodies[index];
            let step = integrate_body_rates(body.rates, body.inertia, self.simulation, corrections);
            body.rates = step.state;
            if resting && body.rates.kinetic_energy < self.simulation.minimum_energy {
                body.rates.linear_velocity = Vector3::ZERO;
                body.rates.angular_velocity = Vector3::ZERO;
                body.rates.kinetic_energy = 0.0;
                // The snap zeroes the energy the integrator compares against
                // its previous value, so its own cool-down counter stalls
                // (post-gravity energy is always greater than zero). Count
                // snapped resting ticks here instead.
                body.rates.cool_down =
                    (body.rates.cool_down + 1).min(self.simulation.cool_down);
            }
            if body_sleep_capable && body.rates.cool_down >= self.simulation.cool_down {
                body.asleep = true;
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
            let closing = dot(sub(volume.linear_velocity, self.bodies[index].velocity_at(pair.b)), push);
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

    /// Impulse and positional corrections for one awake body against the
    /// static world and every other prop box (asleep props are immovable).
    fn contact_corrections(&mut self, index: usize, world: &BoardWorld) -> RetailReactionCorrections {
        let mut corrections = RetailReactionCorrections::default();
        let box_primitive = self.bodies[index].box_primitive();
        let tuning = self.bodies[index].tuning;
        let pair_settings = self.pair_for(index);
        let bounds = self.bodies[index].bounds().expanded(tuning.contact_padding + 0.05);
        for range in world.candidate_ranges(Some(bounds)) {
            for triangle in &world.triangles()[range] {
                let Some(manifold) = primitive_pair_contacts(
                    box_primitive,
                    ContactPrimitive::Triangle(triangle.triangle),
                    pair_settings,
                ) else {
                    continue;
                };
                let material =
                    combine_contact_materials(self.bodies[index].material, triangle.material);
                self.resolve_static(index, &manifold, material, &mut corrections);
            }
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
            let material = combine_contact_materials(
                self.bodies[index].material,
                self.bodies[other].material,
            );
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
                self.resolve_static(index, &manifold, material, &mut corrections);
                if closing < -1.0 {
                    self.bodies[other].wake();
                }
            } else {
                self.resolve_dynamic(index, other, &manifold, material, &mut corrections);
            }
        }
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
        corrections: &mut RetailReactionCorrections,
    ) {
        let dt = self.simulation.time_step;
        let tuning = self.bodies[index].tuning;
        let count = manifold.count.max(1) as f32;
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
                let impulse = -(1.0 + restitution) * vn / (denominator * count);
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
        let layer = build_prop_layer(&map, &objects, material()).unwrap().unwrap();
        let dynamics = PropDynamics::new(&objects, layer.instances(), simulation());
        let world = super::super::ground::Terrain::Flat.world(material());
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
            body: CollisionBody::Board(BodyId::Deck),
            primitive: ContactPrimitive::Sphere(Sphere {
                center: Vector3::new(-0.55, REST_Y, 0.),
                radius: 0.2,
            }),
            linear_velocity: Vector3::new(2., 0., 0.),
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
            body: CollisionBody::Board(BodyId::Deck),
            primitive: ContactPrimitive::Capsule {
                center: Vector3::new(0.1, super::super::ground::HEIGHT + 0.1, 0.05),
                axis: Vector3::new(1., 0., 0.),
                half_length: 0.3,
                radius: 0.06,
            },
            linear_velocity: Vector3::ZERO,
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
        hit[0].linear_velocity = Vector3::new(0., 0., 4.);
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
                body: CollisionBody::Board(BodyId::Deck),
                primitive: ContactPrimitive::Sphere(Sphere {
                    center: Vector3::new(x, REST_Y, 0.),
                    radius: 0.2,
                }),
                linear_velocity: Vector3::new(10., 0., 0.),
                material: material(),
            }];
            dynamics.step(&world, &mut layer, &volumes);
            fastest = fastest.max(dynamics.bodies[0].rates.linear_velocity.x);
        }
        assert!(fastest <= 0.8 + 1e-3, "pushed to {fastest} m/s");
        assert!(fastest > 0.1, "push still applies: {fastest}");
    }

    /// Depenetration is bounded per tick: a box dropped deep into the floor
    /// rises at most `max_depenetration_per_tick` per tick from the
    /// positional correction.
    #[test]
    fn depenetration_is_bounded_per_tick() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y - 0.4, 0.]);
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
            material(),
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
            carry.update(&mut dynamics, from(&[28], &[28]), carrier(state, 0.05 * i as f32));
            dynamics.step(&world, &mut layer, &[]);
            assert_eq!(carry.held(), Some(7), "held grab dropped on tick {i}");
        }
        carry.update(&mut dynamics, from(&[], &[28]), carrier(state, 1.5));
        assert_eq!(carry.held(), None, "releasing the grab button must drop");
        // B is a rising edge for placement, not a level.
        assert!(from(&[20], &[]).placement);
        assert!(!from(&[20], &[20]).placement);
    }

    /// Video bug (2026-10-05, cart and bin, hold RB): the skater and the held
    /// prop spun round each other. Move Object locomotion must never turn the
    /// pair from the left stick (retail 8259C4B0: the left-stick rotation
    /// curve is all zero), whatever the stick direction, so a held stick
    /// cannot chase itself round. Only the right stick turns, at most
    /// `turn_rate`.
    #[test]
    fn move_object_left_stick_never_turns_the_pair() {
        use crate::physics::prop_carry::{object_move_motion, yaw_row, CarryLocomotion};
        let locomotion = CarryLocomotion::default();
        let dt = simulation().time_step;
        for step in 0..16 {
            let a = step as f32 * std::f32::consts::TAU / 16.0;
            let (x, z) = (a.sin(), a.cos());
            let mut right = [1.0, 0.0, 0.0, 0.0];
            let mut forward = [0.0, 0.0, 1.0, 0.0];
            let mut position = [0.0f32; 2];
            let mut yaw = 0.0f32;
            for _ in 0..240 {
                let (velocity, rate) = object_move_motion(right, forward, x, z, 0.0, locomotion);
                assert_eq!(rate, 0.0, "left stick ({x:.2}, {z:.2}) turned the pair");
                right = yaw_row(right, rate * dt);
                forward = yaw_row(forward, rate * dt);
                yaw += rate * dt;
                position[0] += velocity[0] * dt;
                position[1] += velocity[2] * dt;
            }
            assert_eq!(yaw, 0.0);
            // The pair travels on a straight line in the stick's direction in
            // the skater frame (side and push/pull speeds scale the axes).
            let travelled = (position[0] * position[0] + position[1] * position[1]).sqrt();
            assert!(travelled > 0.5, "stick ({x:.2}, {z:.2}) did not move the pair");
            let along_speed = if z >= 0.0 { locomotion.push_speed } else { locomotion.pull_speed };
            let (ex, ez) = (x * locomotion.side_speed, z * along_speed);
            let expected = (ex * ex + ez * ez).sqrt();
            let along = (position[0] * ex + position[1] * ez) / (travelled * expected);
            assert!(along > 0.99, "stick ({x:.2}, {z:.2}) moved the pair off its direction: {along}");
        }
        // Right stick: bounded turn, either way.
        for rot in [-1.0f32, -0.5, 0.5, 1.0, 4.0] {
            let (_, rate) = object_move_motion([1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], 0.0, 0.0, rot, locomotion);
            assert!(rate.abs() <= locomotion.turn_rate + 1e-6 && rate.signum() == rot.signum());
        }
    }

    /// A straight push keeps the held prop on a straight line at its grab
    /// offset in front of the carrier.
    #[test]
    fn dragged_prop_follows_a_straight_push() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::OffBoardPushing;
        let mut grab = carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.);
        carry.update(&mut dynamics, tick(), grab);
        assert_eq!(carry.held(), Some(7));
        dynamics.set_held(Some(7));
        let speed = crate::physics::prop_carry::CarryLocomotion::default().push_speed;
        let dt = simulation().time_step;
        let mut max_side = 0.0f32;
        for i in 1..=180 {
            grab = carrier(state, speed * dt * i as f32);
            carry.update(&mut dynamics, tick(), grab);
            dynamics.step(&world, &mut layer, &[]);
            let p = dynamics.position_of(7).unwrap();
            max_side = max_side.max(p.x.abs());
        }
        assert_eq!(carry.held(), Some(7), "the push dropped the prop");
        let p = dynamics.position_of(7).unwrap();
        let ahead = p.z - grab.position.z;
        assert!(max_side < 0.05, "prop wandered {max_side} m off the push line");
        assert!((ahead - 0.9).abs() < 0.15, "prop not held at its grab offset: {ahead} m ahead");
    }

    /// Turning (right stick) swings the held prop round with the carrier: it
    /// stays in front instead of staying on its old world bearing.
    #[test]
    fn turning_carrier_swings_the_held_prop_with_it() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::OffBoardPushing;
        let base = carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.);
        carry.update(&mut dynamics, tick(), base);
        dynamics.set_held(Some(7));
        let ticks = 90;
        let mut facing = base.forward;
        for i in 1..=ticks {
            let yaw = std::f32::consts::FRAC_PI_2 * i as f32 / ticks as f32;
            facing = Vector3::new(yaw.sin(), 0.0, yaw.cos());
            carry.update(&mut dynamics, tick(), crate::physics::prop_carry::Carrier { state, forward: facing, ..base });
            dynamics.step(&world, &mut layer, &[]);
        }
        for _ in 0..30 {
            carry.update(&mut dynamics, tick(), crate::physics::prop_carry::Carrier { state, forward: facing, ..base });
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        let (dx, dz) = (p.x - base.position.x, p.z - base.position.z);
        let bearing = dx.atan2(dz).to_degrees();
        assert!((bearing - 90.0).abs() < 10.0, "prop did not turn with the carrier: bearing {bearing} deg");
    }

    /// Grabbing the prop ahead picks it up; it follows as the carrier moves.
    #[test]
    fn grabbed_prop_follows_carrier() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(state, 0.));
        assert_eq!(carry.held(), Some(7));
        for i in 0..60 {
            carry.update(&mut dynamics, tick(), carrier(state, 0.05 * i as f32));
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        assert!(p.z > 2.0, "prop followed to z={}", p.z);
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
        for i in 0..30 {
            carry.update(&mut dynamics, tick(), carrier(pushing, 0.05 * i as f32));
            dynamics.step(&world, &mut layer, &[]);
            assert_eq!(carry.held(), Some(7), "state 502 dropped the carry");
        }
        assert!(dynamics.position_of(7).unwrap().z > 1.5, "prop did not follow in 502");
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
        // Carry follow still works after the cancel.
        carry.update(&mut dynamics, tick(), carrier(state, 1.0));
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
            body: CollisionBody::Board(BodyId::Deck),
            primitive: ContactPrimitive::Sphere(Sphere { center: Vector3::new(-0.55, REST_Y, 0.), radius: 0.2 }),
            linear_velocity: Vector3::new(2., 0., 0.),
            material: material(),
        }];
        dynamics.step_with_actors(&world, &mut layer, &[], &far);
        assert!(dynamics.bodies[0].asleep);
        assert_eq!(dynamics.pushed_by(dynamics.bodies[0].id), None);
        dynamics.step_with_actors(&world, &mut layer, &local, &far);
        assert_eq!(dynamics.pushed_by(dynamics.bodies[0].id), Some(LOCAL_PUSHER));
    }
}
