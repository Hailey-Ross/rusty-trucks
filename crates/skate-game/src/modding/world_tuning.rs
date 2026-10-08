//! World tuning writes (capability `world_tuning` = 1, schema `skate_mods::world_tuning`): the
//! same entry for mods (`sdk.world.set_tuning(domain, patch)`) and engine systems ([`set`]).
//!
//! Patches are held per owner in [`WorldTuning`] (serialisable JSON, in arrival order). After every
//! change the domain is rebuilt from the shipped values plus the merged patches (first writer wins
//! per field) and written into the one authority resource of that domain:
//! - `living_world` -> `LivingWorldSettings` (rebuilt with `reset_mod_overrides`, so the player's
//!   menu draw distance comes back when no mod sets one),
//! - `props` -> `PropTuningSettings`,
//! - `carry` -> `CarrySettings`,
//! - `shadows` -> `retail_render::WorldShadowSettings` (dynamic shadow floor on the baked world).
//! A mod that stops, fails or reloads loses its patches ([`clear_owner`]); [`clear_all`] when every
//! mod goes.

use bevy::prelude::*;
use serde_json::{json, Value};
use skate_core::math::Vector3;
use skate_mods::world_tuning::{parse, CarryPatch, LivingWorldPatch, Merge, Patch, PropTuningPatch, PropsPatch, DOMAINS};

use crate::living_world::LivingWorldSettings;
use crate::retail_render::{WorldShadowSettings, RETAIL_WORLD_SHADOW_FLOOR};
use crate::physics::prop_carry::{CarryButtons, CarrySettings, LocomotionOverrides};
use crate::physics::prop_dynamics::{PropBox, PropTuning, PropTuningSettings, PropTuningTable};

/// Per-owner patches, in arrival order (a re-set keeps the owner's place).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct WorldTuning {
    pub entries: Vec<(String, String, Value)>,
}

impl WorldTuning {
    fn merged<T: Merge + Default>(&self, domain: &str, pick: impl Fn(Patch) -> Option<T>) -> T {
        let mut out: Option<T> = None;
        for (_, d, v) in &self.entries {
            let Some(p) = (d == domain).then(|| parse(d, v)).flatten().and_then(&pick) else { continue };
            match out.as_mut() {
                Some(o) => o.merge(&p),
                None => out = Some(p),
            }
        }
        out.unwrap_or_default()
    }
}

/// Set (or with `None` remove) `owner`'s patch of `domain`, then rebuild that domain.
pub(crate) fn set(world: &mut World, owner: &str, domain: &str, patch: Option<Value>) -> Result<(), String> {
    if !DOMAINS.contains(&domain) {
        return Err(format!("unknown world tuning domain {domain}"));
    }
    if let Some(p) = &patch {
        parse(domain, p).ok_or_else(|| format!("invalid {domain} tuning patch"))?;
    }
    let mut t = world.remove_resource::<WorldTuning>().unwrap_or_default();
    let at = t.entries.iter().position(|(o, d, _)| o == owner && d == domain);
    match (at, patch) {
        (Some(i), Some(p)) => t.entries[i].2 = p,
        (None, Some(p)) => t.entries.push((owner.to_owned(), domain.to_owned(), p)),
        (Some(i), None) => {
            t.entries.remove(i);
        }
        (None, None) => {}
    }
    rebuild(world, &t, domain);
    world.insert_resource(t);
    Ok(())
}

/// A mod stopped, failed or reloaded: its patches go and the domains it touched are rebuilt.
pub(crate) fn clear_owner(world: &mut World, owner: &str) {
    let Some(mut t) = world.remove_resource::<WorldTuning>() else { return };
    let touched: Vec<String> = t.entries.iter().filter(|(o, _, _)| o == owner).map(|(_, d, _)| d.clone()).collect();
    t.entries.retain(|(o, _, _)| o != owner);
    for d in &touched {
        rebuild(world, &t, d);
    }
    world.insert_resource(t);
}

/// Every mod went: every domain back to the shipped values (and the player's own choices).
pub(crate) fn clear_all(world: &mut World) {
    let had = world.remove_resource::<WorldTuning>().is_some_and(|t| !t.entries.is_empty());
    let empty = WorldTuning::default();
    if had {
        for d in DOMAINS {
            rebuild(world, &empty, d);
        }
    }
    world.insert_resource(empty);
}

fn rebuild(world: &mut World, t: &WorldTuning, domain: &str) {
    match domain {
        "living_world" => {
            let p = t.merged(domain, |p| if let Patch::LivingWorld(p) = p { Some(p) } else { None });
            if let Some(mut s) = world.get_resource_mut::<LivingWorldSettings>() {
                apply_living_world(&mut s, &p);
            }
        }
        "props" => {
            let p = t.merged(domain, |p| if let Patch::Props(p) = p { Some(p) } else { None });
            let table = props_table(&p);
            match world.get_resource_mut::<PropTuningSettings>() {
                Some(mut s) => s.0 = table,
                None => world.insert_resource(PropTuningSettings(table)),
            }
        }
        "carry" => {
            let p = t.merged(domain, |p| if let Patch::Carry(p) = p { Some(p) } else { None });
            let c = carry_settings(&p);
            match world.get_resource_mut::<CarrySettings>() {
                Some(mut s) => *s = c,
                None => world.insert_resource(c),
            }
        }
        "shadows" => {
            let p = t.merged(domain, |p| if let Patch::Shadows(p) = p { Some(p) } else { None });
            let s = WorldShadowSettings { floor: p.world_floor.map_or(RETAIL_WORLD_SHADOW_FLOOR, Vec3::from_array) };
            match world.get_resource_mut::<WorldShadowSettings>() {
                Some(mut r) => *r = s,
                None => world.insert_resource(s),
            }
        }
        _ => {}
    }
}

/// Shipped values (player's choices kept) plus the patch. Negative times / alphas clamp to 0.
pub(crate) fn apply_living_world(s: &mut LivingWorldSettings, p: &LivingWorldPatch) {
    s.reset_mod_overrides();
    if let Some(m) = p.npc_draw_distance {
        s.npc_draw_distance = skate_core::living_world::DrawDistance::new(m).multiplier();
    }
    if let Some(f) = &p.skater_fade {
        let c = &mut s.skater_fade;
        c.fade_in_seconds = f.fade_in_seconds.map_or(c.fade_in_seconds, |v| v.max(0.0));
        c.fade_seconds = f.fade_seconds.map_or(c.fade_seconds, |v| v.max(0.0));
        c.despawn_alpha = f.despawn_alpha.map_or(c.despawn_alpha, |v| v.clamp(0.0, 1.0));
    }
    if let Some(f) = &p.skater_line_chain {
        let c = &mut s.skater_line_chain;
        c.radius = f.radius.map_or(c.radius, |v| v.max(0.0));
        c.max_candidates = f.max_candidates.map_or(c.max_candidates, |v| v as usize);
        c.blend_seconds = f.blend_seconds.map_or(c.blend_seconds, |v| v.max(0.0));
        c.keep_facing = f.keep_facing.unwrap_or(c.keep_facing);
        c.facing_rule = f.facing_rule.as_deref().and_then(skate_core::living_world::replay::FacingRule::from_name).unwrap_or(c.facing_rule);
        c.steer_dead_zone_deg = f.steer_dead_zone_deg.map_or(c.steer_dead_zone_deg, |v| v.max(0.0));
        c.steer_full_deg = f.steer_full_deg.map_or(c.steer_full_deg, |v| v.max(0.0));
        c.fakie.high_speed = f.fakie_high_speed.map_or(c.fakie.high_speed, |v| v.max(0.0));
        c.fakie.low_speed = f.fakie_low_speed.map_or(c.fakie.low_speed, |v| v.max(0.0));
        c.fakie.slowly_backwards_seconds = f.fakie_slow_seconds.map_or(c.fakie.slowly_backwards_seconds, |v| v.max(0.0));
        c.fakie.after_teleport_seconds = f.fakie_spawn_seconds.map_or(c.fakie.after_teleport_seconds, |v| v.max(0.0));
    }
    if let Some(f) = &p.ped_fade {
        let c = &mut s.ped_fade;
        c.distance = f.distance.map_or(c.distance, |[a, b]| [a.max(0.0), b.max(0.0)]);
        c.fade_in_seconds = f.fade_in_seconds.map_or(c.fade_in_seconds, |v| v.max(0.0));
        c.enabled = f.enabled.unwrap_or(c.enabled);
    }
    if let Some(f) = &p.ped_obstacles {
        let c = &mut s.ped_obstacles;
        c.enabled = f.enabled.unwrap_or(c.enabled);
        c.min_half_extent = f.min_half_extent.map_or(c.min_half_extent, |v| v.max(0.0));
        c.moving_speed = f.moving_speed.map_or(c.moving_speed, |v| v.max(0.0));
        c.recut_fraction = f.recut_fraction.map_or(c.recut_fraction, |v| v.max(0.0));
        c.detour_margin = f.detour_margin.map_or(c.detour_margin, |v| v.max(0.0));
        c.step_height = f.step_height.map_or(c.step_height, |v| v.max(0.0));
        c.held_is_obstacle = f.held_is_obstacle.unwrap_or(c.held_is_obstacle);
        c.moving_solid = f.moving_solid.unwrap_or(c.moving_solid);
    }
    if let Some(f) = &p.ped_vehicle_contact {
        let c = &mut s.ped_vehicle_contact;
        c.enabled = f.enabled.unwrap_or(c.enabled);
        c.push = f.push.unwrap_or(c.push);
    }
    if let Some(f) = &p.npc_skater_props {
        s.npc_skater_props.enabled = f.enabled.unwrap_or(s.npc_skater_props.enabled);
    }
    if let Some(m) = &p.skater_clips {
        s.skater_clips = m.clone();
    }
    if let Some(m) = &p.skater_blend_seconds {
        s.skater_blend_seconds = m.iter().map(|(k, v)| (k.clone(), v.max(0.0))).collect();
    }
}

fn prop_tuning(base: &PropTuning, p: &PropTuningPatch) -> PropTuning {
    let f = |v: Option<f32>, d: f32| v.map_or(d, |v| v.max(0.0));
    PropTuning {
        contact_padding: f(p.contact_padding, base.contact_padding),
        penetration_slop: f(p.penetration_slop, base.penetration_slop),
        penetration_correction: f(p.penetration_correction, base.penetration_correction),
        max_depenetration_per_tick: f(p.max_depenetration_per_tick, base.max_depenetration_per_tick),
        restitution_threshold: f(p.restitution_threshold, base.restitution_threshold),
        skater_push_mass: f(p.skater_push_mass, base.skater_push_mass),
        push_transfer: f(p.push_transfer, base.push_transfer),
        body_push_speed: f(p.body_push_speed, base.body_push_speed),
        board_push_speed: f(p.board_push_speed, base.board_push_speed),
        penetration_push_speed: f(p.penetration_push_speed, base.penetration_push_speed),
        stuck_release_ticks: p.stuck_release_ticks.unwrap_or(base.stuck_release_ticks),
        collision_box: p.collision_box.map_or(base.collision_box, |b| {
            Some(PropBox { center: Vector3::new(b.center[0], b.center[1], b.center[2]), half_extents: Vector3::new(b.half_extents[0], b.half_extents[1], b.half_extents[2]) })
        }),
    }
}

/// Shipped table plus the patch; a template entry starts from the (patched) default.
pub(crate) fn props_table(p: &PropsPatch) -> PropTuningTable {
    let mut t = PropTuningTable::default();
    if let Some(d) = &p.default {
        t.default = prop_tuning(&t.default, d);
    }
    for (name, patch) in &p.by_template {
        let entry = prop_tuning(&t.default, patch);
        t.by_template.insert(name.clone(), entry);
    }
    if let Some(v) = &p.solver {
        let d = t.solver;
        t.solver = crate::physics::prop_dynamics::PropSolverSettings {
            row_solver: v.row_solver.unwrap_or(d.row_solver),
            iterations: v.iterations.map_or(d.iterations, |n| n.clamp(1, 256)),
            sleep_energy: v.sleep_energy.map_or(d.sleep_energy, |e| e.max(0.0)),
            sleep_frames: v.sleep_frames.map_or(d.sleep_frames, |n| n.max(1)),
            max_sleeps_per_step: v.max_sleeps_per_step.map_or(d.max_sleeps_per_step, |n| n.max(1)),
            rest_snap: v.rest_snap.unwrap_or(d.rest_snap),
        };
    }
    if let Some(v) = &p.upright {
        let d = t.upright;
        t.upright = crate::physics::prop_dynamics::PropUprightSettings {
            window_seconds: v.window_seconds.unwrap_or(d.window_seconds),
            tick_seconds: v.tick_seconds.unwrap_or(d.tick_seconds),
            stop_angle_deg: v.stop_angle_deg.unwrap_or(d.stop_angle_deg),
            max_angle_deg: v.max_angle_deg.unwrap_or(d.max_angle_deg),
            dead_band_deg: v.dead_band_deg.unwrap_or(d.dead_band_deg),
            gain_min: v.gain_min.unwrap_or(d.gain_min),
            gain_max: v.gain_max.unwrap_or(d.gain_max),
            gain_blend_start: v.gain_blend_start.unwrap_or(d.gain_blend_start),
            off_axis_spin: v.off_axis_spin.unwrap_or(d.off_axis_spin),
            command_rate: v.command_rate.unwrap_or(d.command_rate),
            fallback_angle_deg: v.fallback_angle_deg.unwrap_or(d.fallback_angle_deg),
            block_yaw: v.block_yaw.unwrap_or(d.block_yaw),
        };
    }
    t
}

pub(crate) fn carry_settings(p: &CarryPatch) -> CarrySettings {
    let d = CarrySettings::default();
    CarrySettings {
        buttons: CarryButtons { grab_bit: p.grab_bit.unwrap_or(d.buttons.grab_bit), placement_bit: p.placement_bit.unwrap_or(d.buttons.placement_bit) },
        grab_range: p.grab_range.filter(|r| *r > 0.0).unwrap_or(d.grab_range),
        locomotion: LocomotionOverrides {
            push_speed: p.push_speed,
            pull_speed: p.pull_speed,
            side_speed: p.side_speed,
            turn_rate: p.turn_rate,
            grip_reach: p.grip_reach,
            linear_clamp: p.linear_clamp,
            yaw_clamp: p.yaw_clamp,
            relatch: p.relatch,
            slew_per_tick: p.slew_per_tick,
            yaw_rate_feedback: p.yaw_rate_feedback,
            linear_controller: p.linear_controller,
            yaw_controller: p.yaw_controller,
            lever_rotation: p.lever_rotation,
            lever_yaw: p.lever_yaw,
            mass_speed: p.mass_speed,
            inertia_yaw_gain: p.inertia_yaw_gain,
            let_go_distance: p.let_go_distance,
            drop_board: p.drop_board,
            follow_step: p.follow_step,
            hold_angle_limit: p.hold_angle_limit,
            hold_max_angle_to_horizontal: p.hold_max_angle_to_horizontal,
            hold_box_extents: p.hold_box_extents,
            record_272_speed_scale: p.record_272_speed_scale,
        },
        move_rules: {
            let r = crate::physics::prop_dynamics::MoveCommandRules::default();
            crate::physics::prop_dynamics::MoveCommandRules {
                commanded_material: p.commanded_material.unwrap_or(r.commanded_material),
                upright_cos: p.upright_cos.unwrap_or(r.upright_cos),
                by_template: p
                    .by_template
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            crate::physics::prop_dynamics::PropMaterialBlocks {
                                held: v.material_held,
                                free: v.material_free,
                                free_upright: v.material_free_upright,
                                upright_pair: v.upright_pair,
                                restitution: v.restitution,
                                record_272: v.record_272,
                                linear_drag: v.linear_drag,
                                angular_drag: v.angular_drag,
                                mass: v.mass,
                                maximum_linear_velocity: v.maximum_linear_velocity,
                                maximum_angular_velocity: v.maximum_angular_velocity,
                                inertia_scale: v.inertia_scale,
                                inertia_offset: v.inertia_offset,
                            },
                        )
                    })
                    .collect(),
                apply_at_com: p.apply_at_com.unwrap_or(r.apply_at_com),
                yaw_replaces_torque: p.yaw_replaces_torque.unwrap_or(r.yaw_replaces_torque),
                ignore_vertical: p.ignore_vertical.unwrap_or(r.ignore_vertical),
                wake_on_command: p.wake_on_command.unwrap_or(r.wake_on_command),
            }
        },
    }
}

/// `sdk.engine.inspect(key, 'world_tuning:<domain>')`: the domain as the game uses it now.
pub(crate) fn read(world: &World, domain: &str) -> Value {
    let v3 = |v: Vector3| json!([v.x, v.y, v.z]);
    let tuning = |t: &PropTuning| {
        json!({
            "contact_padding": t.contact_padding, "penetration_slop": t.penetration_slop,
            "penetration_correction": t.penetration_correction, "max_depenetration_per_tick": t.max_depenetration_per_tick,
            "restitution_threshold": t.restitution_threshold, "skater_push_mass": t.skater_push_mass,
            "push_transfer": t.push_transfer, "body_push_speed": t.body_push_speed, "board_push_speed": t.board_push_speed,
            "penetration_push_speed": t.penetration_push_speed, "stuck_release_ticks": t.stuck_release_ticks,
            "collision_box": t.collision_box.map_or(Value::Null, |b| json!({"center": v3(b.center), "half_extents": v3(b.half_extents)})),
        })
    };
    match domain {
        "living_world" => world.get_resource::<LivingWorldSettings>().map_or(Value::Null, |s| {
            json!({
                "npc_draw_distance": s.npc_draw_distance,
                "user_npc_draw_distance": s.user_npc_draw_distance,
                "skater_fade": {"fade_in_seconds": s.skater_fade.fade_in_seconds, "fade_seconds": s.skater_fade.fade_seconds, "despawn_alpha": s.skater_fade.despawn_alpha},
                "skater_line_chain": {"radius": s.skater_line_chain.radius, "max_candidates": s.skater_line_chain.max_candidates, "blend_seconds": s.skater_line_chain.blend_seconds, "keep_facing": s.skater_line_chain.keep_facing, "facing_rule": s.skater_line_chain.facing_rule.name(), "steer_dead_zone_deg": s.skater_line_chain.steer_dead_zone_deg, "steer_full_deg": s.skater_line_chain.steer_full_deg, "fakie_high_speed": s.skater_line_chain.fakie.high_speed, "fakie_low_speed": s.skater_line_chain.fakie.low_speed, "fakie_slow_seconds": s.skater_line_chain.fakie.slowly_backwards_seconds, "fakie_spawn_seconds": s.skater_line_chain.fakie.after_teleport_seconds},
                "ped_fade": {"distance": s.ped_fade.distance, "fade_in_seconds": s.ped_fade.fade_in_seconds, "enabled": s.ped_fade.enabled},
                "ped_obstacles": {"enabled": s.ped_obstacles.enabled, "min_half_extent": s.ped_obstacles.min_half_extent,
                    "moving_speed": s.ped_obstacles.moving_speed, "recut_fraction": s.ped_obstacles.recut_fraction,
                    "detour_margin": s.ped_obstacles.detour_margin, "step_height": s.ped_obstacles.step_height,
                    "held_is_obstacle": s.ped_obstacles.held_is_obstacle, "moving_solid": s.ped_obstacles.moving_solid},
                "npc_skater_props": {"enabled": s.npc_skater_props.enabled},
                "ped_vehicle_contact": {"enabled": s.ped_vehicle_contact.enabled, "push": s.ped_vehicle_contact.push},
                "skater_clips": s.skater_clips,
                "skater_blend_seconds": s.skater_blend_seconds,
            })
        }),
        "props" => world.get_resource::<PropTuningSettings>().map_or(Value::Null, |s| {
            let by: serde_json::Map<String, Value> = s.0.by_template.iter().map(|(k, t)| (k.clone(), tuning(t))).collect();
            let v = s.0.solver;
            let u = s.0.upright;
            json!({"default": tuning(&s.0.default), "by_template": by, "solver": {
                "row_solver": v.row_solver, "iterations": v.iterations, "sleep_energy": v.sleep_energy,
                "sleep_frames": v.sleep_frames, "max_sleeps_per_step": v.max_sleeps_per_step, "rest_snap": v.rest_snap,
            }, "upright": {
                "window_seconds": u.window_seconds, "tick_seconds": u.tick_seconds, "stop_angle_deg": u.stop_angle_deg,
                "max_angle_deg": u.max_angle_deg, "dead_band_deg": u.dead_band_deg, "gain_min": u.gain_min, "gain_max": u.gain_max,
                "gain_blend_start": u.gain_blend_start, "off_axis_spin": u.off_axis_spin, "command_rate": u.command_rate,
                "fallback_angle_deg": u.fallback_angle_deg, "block_yaw": u.block_yaw,
            }})
        }),
        "carry" => world.get_resource::<CarrySettings>().map_or(Value::Null, |c| {
            // The tuning in effect: the live carry's base (setup data) with this patch applied.
            let base = world.get_resource::<crate::physics::GamePhysics>().map_or_else(
                crate::physics::prop_carry::CarryLocomotion::default,
                |p| p.prop_carry.base_locomotion(),
            );
            let l = c.locomotion.apply(base);
            let m = l.move_object;
            let g = |g: skate_core::player::offboard::move_object::ControllerGains| json!([g.proportional, g.filtered, g.derivative, g.filter]);
            let curve = |c: skate_core::point_graph::PointGraph<8>| json!([c.x, c.y]);
            let r = &c.move_rules;
            let by: serde_json::Map<String, Value> = r
                .by_template
                .iter()
                .map(|(k, b)| (k.clone(), json!({"material_held": b.held, "material_free": b.free, "material_free_upright": b.free_upright,
                    "upright_pair": b.upright_pair, "restitution": b.restitution, "record_272": b.record_272,
                    "linear_drag": b.linear_drag, "angular_drag": b.angular_drag, "mass": b.mass,
                    "maximum_linear_velocity": b.maximum_linear_velocity, "maximum_angular_velocity": b.maximum_angular_velocity,
                    "inertia_scale": b.inertia_scale, "inertia_offset": b.inertia_offset})))
                .collect();
            json!({"grab_bit": c.buttons.grab_bit, "placement_bit": c.buttons.placement_bit, "grab_range": c.grab_range,
                "push_speed": m.push_speed, "pull_speed": m.pull_speed, "side_speed": m.side_speed, "turn_rate": l.turn_rate,
                "grip_reach": m.follow_reach, "linear_clamp": m.linear_clamp, "yaw_clamp": m.yaw_clamp, "relatch": m.relatch,
                "slew_per_tick": m.slew_per_tick, "yaw_rate_feedback": m.yaw_rate_feedback, "linear_controller": g(m.linear_controller), "yaw_controller": g(m.yaw_controller),
                "lever_rotation": curve(m.lever_rotation), "lever_yaw": curve(m.lever_yaw), "mass_speed": curve(m.mass_speed),
                "inertia_yaw_gain": curve(m.inertia_yaw_gain), "let_go_distance": l.let_go_distance, "drop_board": l.drop_board,
                "follow_step": m.follow_step, "hold_angle_limit": m.hold_angle_limit,
                "hold_max_angle_to_horizontal": m.hold_max_angle_to_horizontal, "hold_box_extents": m.hold_box_extents,
                "record_272_speed_scale": m.record_272_speed_scale,
                "commanded_material": r.commanded_material, "upright_cos": r.upright_cos, "apply_at_com": r.apply_at_com,
                "yaw_replaces_torque": r.yaw_replaces_torque, "ignore_vertical": r.ignore_vertical,
                "wake_on_command": r.wake_on_command, "by_template": by})
        }),
        "shadows" => world.get_resource::<WorldShadowSettings>().map_or(Value::Null, |s| json!({"world_floor": s.floor.to_array()})),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let mut w = World::new();
        w.insert_resource(LivingWorldSettings::default());
        w.init_resource::<PropTuningSettings>();
        w.init_resource::<CarrySettings>();
        w.init_resource::<WorldShadowSettings>();
        w
    }

    #[test]
    fn world_shadow_floor_defaults_to_retail_set_and_reset() {
        let mut w = world();
        assert_eq!(w.resource::<WorldShadowSettings>().floor, Vec3::new(0.05, 0.09, 0.13));
        set(&mut w, "dev.a", "shadows", Some(json!({"world_floor": [0.0, 0.0, 0.0]}))).unwrap();
        set(&mut w, "dev.b", "shadows", Some(json!({"world_floor": [0.5, 0.5, 0.5]}))).unwrap();
        assert_eq!(w.resource::<WorldShadowSettings>().floor, Vec3::ZERO, "first writer wins");
        assert_eq!(read(&w, "shadows")["world_floor"], json!([0.0f32, 0.0f32, 0.0f32]));
        assert!(set(&mut w, "dev.a", "shadows", Some(json!({"world_floor": [2.0, 0.0, 0.0]}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<WorldShadowSettings>().floor, Vec3::splat(0.5));
        clear_all(&mut w);
        assert_eq!(*w.resource::<WorldShadowSettings>(), WorldShadowSettings::default(), "mod disable restores retail");
    }

    #[test]
    fn ped_vehicle_contact_is_mod_reachable_and_reset_on_disable() {
        let mut w = world();
        let retail = LivingWorldSettings::default().ped_vehicle_contact;
        assert!(retail.enabled && retail.push, "retail: cars push peds out of the way");
        assert_eq!(read(&w, "living_world")["ped_vehicle_contact"], json!({"enabled": true, "push": true}));
        set(&mut w, "dev.a", "living_world", Some(json!({"ped_vehicle_contact": {"push": false}}))).unwrap();
        let s = w.resource::<LivingWorldSettings>().ped_vehicle_contact;
        assert!(s.enabled && !s.push, "absent fields keep retail");
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"ped_vehicle_contact": {"knockdown": true}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<LivingWorldSettings>().ped_vehicle_contact, retail, "mod disable restores retail");
    }

    #[test]
    fn living_world_values_set_and_reset_on_disable() {
        let mut w = world();
        w.resource_mut::<LivingWorldSettings>().set_user_draw_distance(1.5);
        set(&mut w, "dev.a", "living_world", Some(json!({
            "npc_draw_distance": 3.0,
            "skater_fade": {"fade_in_seconds": 0.0, "fade_seconds": 2.5, "despawn_alpha": 0.05},
            "skater_line_chain": {"radius": 0.0, "blend_seconds": 0.6, "keep_facing": true, "facing_rule": "per_node", "steer_full_deg": 12.0, "fakie_high_speed": 2.5, "fakie_spawn_seconds": 0.0},
            "ped_fade": {"distance": [90.0, 110.0], "fade_in_seconds": 2.0, "enabled": false},
        }))).unwrap();
        {
            let s = w.resource::<LivingWorldSettings>();
            assert_eq!(s.npc_draw_distance, 3.0);
            assert_eq!(s.user_npc_draw_distance, 1.5, "the player's choice is kept aside");
            assert_eq!((s.skater_fade.fade_in_seconds, s.skater_fade.fade_seconds, s.skater_fade.despawn_alpha), (0.0, 2.5, 0.05));
            assert_eq!((s.ped_fade.distance, s.ped_fade.fade_in_seconds, s.ped_fade.enabled), ([90.0, 110.0], 2.0, false));
            assert_eq!((s.skater_line_chain.radius, s.skater_line_chain.max_candidates), (0.0, 16), "absent fields keep retail");
            assert_eq!(s.skater_line_chain.blend_seconds, 0.6);
            assert!(s.skater_line_chain.keep_facing, "the fix 16 option is mod-reachable");
            assert_eq!(s.skater_line_chain.facing_rule, skate_core::living_world::replay::FacingRule::PerNode, "the fix 23 rule is mod-reachable");
            assert_eq!((s.skater_line_chain.steer_dead_zone_deg, s.skater_line_chain.steer_full_deg), (2.0, 12.0));
            let f = s.skater_line_chain.fakie;
            assert_eq!((f.high_speed, f.low_speed, f.slowly_backwards_seconds, f.after_teleport_seconds), (2.5, 0.5, 0.2, 0.0), "the fakie rule is mod-reachable, absent fields keep retail");
        }
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["fakie_high_speed"], json!(2.5));
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["radius"], json!(0.0));
        assert!((read(&w, "living_world")["skater_line_chain"]["blend_seconds"].as_f64().unwrap() - 0.6).abs() < 1e-6);
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["keep_facing"], json!(true));
        assert_eq!(read(&w, "living_world")["skater_line_chain"]["facing_rule"], json!("per_node"));
        assert_eq!(read(&w, "living_world")["skater_fade"]["fade_seconds"], json!(2.5));
        clear_owner(&mut w, "dev.a");
        let s = w.resource::<LivingWorldSettings>();
        assert_eq!(s.npc_draw_distance, 1.5, "disable restores the player's menu choice");
        let retail = LivingWorldSettings::default();
        assert_eq!((s.skater_fade, s.ped_fade, s.skater_line_chain), (retail.skater_fade, retail.ped_fade, retail.skater_line_chain));
        assert!(!s.skater_line_chain.keep_facing, "disable turns the fix 16 option off");
        assert_eq!(s.skater_line_chain.facing_rule, skate_core::living_world::replay::FacingRule::RidingEntry, "disable returns to the default (retail) rule");
        assert_eq!((s.skater_line_chain.steer_dead_zone_deg, s.skater_line_chain.steer_full_deg), (2.0, 10.0));
        assert_eq!(s.skater_line_chain.fakie, skate_core::living_world::replay::retail::FAKIE, "disable restores the retail fakie rule");
    }

    #[test]
    fn first_writer_wins_and_nil_gives_fields_back() {
        let mut w = world();
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_fade": {"fade_seconds": 2.0}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_fade": {"fade_seconds": 4.0}, "npc_draw_distance": 2.0}))).unwrap();
        assert_eq!(w.resource::<LivingWorldSettings>().skater_fade.fade_seconds, 2.0);
        assert_eq!(w.resource::<LivingWorldSettings>().npc_draw_distance, 2.0);
        set(&mut w, "dev.a", "living_world", None).unwrap();
        assert_eq!(w.resource::<LivingWorldSettings>().skater_fade.fade_seconds, 4.0);
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"bogus": 1}))).is_err());
        clear_all(&mut w);
        assert_eq!(*w.resource::<LivingWorldSettings>(), LivingWorldSettings::default());
    }

    #[test]
    fn npc_skater_clips_set_merge_and_reset() {
        let mut w = world();
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_clips": {"rolling": "A_CYC"}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_clips": {"rolling": "B_CYC", "air": "C"}}))).unwrap();
        {
            let s = w.resource::<LivingWorldSettings>();
            assert_eq!((s.skater_clips["rolling"].as_str(), s.skater_clips["air"].as_str()), ("A_CYC", "C"));
        }
        assert_eq!(read(&w, "living_world")["skater_clips"]["air"], json!("C"));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"skater_clips": {"flying": "X"}}))).is_err());
        clear_all(&mut w);
        assert!(w.resource::<LivingWorldSettings>().skater_clips.is_empty());
        // The stable phase ids the mod API accepts are the engine's.
        use skate_core::living_world::replay::ReplayPhase as P;
        let names = [P::Rolling, P::Crouched, P::Air, P::AirTrick, P::GroundTrick, P::OffBoard].map(P::name);
        assert_eq!(names, skate_mods::world_tuning::NPC_SKATER_PHASES);
    }

    #[test]
    fn npc_skater_blend_seconds_set_merge_and_reset() {
        use crate::living_world::npc_skaters::{RETAIL_BLEND_SECONDS, blend_seconds};
        use skate_core::living_world::replay::ReplayPhase as P;
        let mut w = world();
        assert_eq!(blend_seconds(&w.resource::<LivingWorldSettings>().skater_blend_seconds, P::Air), RETAIL_BLEND_SECONDS);
        set(&mut w, "dev.a", "living_world", Some(json!({"skater_blend_seconds": {"air": 0.4}}))).unwrap();
        set(&mut w, "dev.b", "living_world", Some(json!({"skater_blend_seconds": {"air": 0.1, "default": 0.0}}))).unwrap();
        {
            let s = &w.resource::<LivingWorldSettings>().skater_blend_seconds;
            assert_eq!((blend_seconds(s, P::Air), blend_seconds(s, P::Rolling)), (0.4, 0.0));
        }
        assert_eq!(read(&w, "living_world")["skater_blend_seconds"]["default"], json!(0.0));
        assert!(set(&mut w, "dev.a", "living_world", Some(json!({"skater_blend_seconds": {"air": -1.0}}))).is_err());
        clear_all(&mut w);
        assert!(w.resource::<LivingWorldSettings>().skater_blend_seconds.is_empty());
    }

    #[test]
    fn prop_tuning_and_collision_box_set_and_reset() {
        let mut w = world();
        set(&mut w, "dev.a", "props", Some(json!({
            "default": {"max_depenetration_per_tick": 0.1, "stuck_release_ticks": 10},
            "by_template": {"bench01": {"push_transfer": 0.2, "collision_box": {"center": [0.0, 0.4, 0.0], "half_extents": [1.0, 0.4, 0.3]}}},
        }))).unwrap();
        {
            let t = &w.resource::<PropTuningSettings>().0;
            assert_eq!((t.default.max_depenetration_per_tick, t.default.stuck_release_ticks), (0.1, 10));
            let b = t.for_template("bench01");
            assert_eq!((b.push_transfer, b.max_depenetration_per_tick), (0.2, 0.1), "template starts from the patched default");
            let bx = b.collision_box.unwrap();
            assert_eq!((bx.center.y, bx.half_extents.x), (0.4, 1.0));
            assert_eq!(t.for_template("other").push_transfer, PropTuning::default().push_transfer);
        }
        assert_eq!(read(&w, "props")["by_template"]["bench01"]["collision_box"]["half_extents"], json!([1.0f32, 0.4f32, 0.3f32]));
        clear_owner(&mut w, "dev.a");
        assert_eq!(*w.resource::<PropTuningSettings>(), PropTuningSettings::default());
    }

    #[test]
    fn prop_solver_settings_set_validate_and_reset() {
        use crate::physics::prop_dynamics::PropSolverSettings;
        let mut w = world();
        let retail = PropSolverSettings::default();
        assert_eq!((retail.row_solver, retail.iterations, retail.sleep_energy, retail.sleep_frames, retail.max_sleeps_per_step, retail.rest_snap),
            (true, 25, 1e-5, 2, 100, false), "retail DMO island defaults");
        set(&mut w, "dev.a", "props", Some(json!({"solver": {"iterations": 10, "sleep_energy": 0.5, "sleep_frames": 30, "rest_snap": true}}))).unwrap();
        {
            let v = w.resource::<PropTuningSettings>().0.solver;
            assert_eq!((v.row_solver, v.iterations, v.sleep_energy, v.sleep_frames, v.max_sleeps_per_step, v.rest_snap), (true, 10, 0.5, 30, 100, true));
        }
        assert_eq!(read(&w, "props")["solver"]["iterations"], json!(10));
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"iterations": 0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"sleep_energy": -1.0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"sleep_frames": 0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"solver": {"unknown": 1}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<PropTuningSettings>().0.solver, retail);
    }

    #[test]
    fn prop_upright_settings_set_validate_and_reset() {
        let mut w = world();
        let retail = crate::physics::prop_dynamics::PropUprightSettings::default();
        assert_eq!((retail.window_seconds, retail.stop_angle_deg, retail.max_angle_deg, retail.dead_band_deg, retail.gain_min, retail.gain_max, retail.command_rate, retail.fallback_angle_deg, retail.block_yaw),
            (2.0, 10.0, 70.0, 5.0, 3.0, 5.0, 60.0, 120.0, true));
        set(&mut w, "dev.a", "props", Some(json!({"upright": {"window_seconds": 4.0, "block_yaw": false}}))).unwrap();
        {
            let u = w.resource::<PropTuningSettings>().0.upright;
            assert_eq!((u.window_seconds, u.block_yaw, u.gain_min), (4.0, false, 3.0));
        }
        assert_eq!(read(&w, "props")["upright"]["window_seconds"], json!(4.0));
        assert!(set(&mut w, "dev.b", "props", Some(json!({"upright": {"window_seconds": 0.0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"upright": {"gain_max": -1.0}}))).is_err());
        assert!(set(&mut w, "dev.b", "props", Some(json!({"upright": {"unknown": 1}}))).is_err());
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<PropTuningSettings>().0.upright, retail);
    }

    #[test]
    fn carry_move_object_speeds_set_and_reset() {
        use crate::physics::prop_carry::CarryLocomotion;
        let mut w = world();
        set(&mut w, "dev.a", "carry", Some(json!({"push_speed": 2.5, "turn_rate": 0.4, "yaw_controller": [10.0, 0.0, 20.0, 0.2], "yaw_rate_feedback": 0.0,
            "mass_speed": [[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0], [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]]}))).unwrap();
        let c = w.resource::<CarrySettings>().clone();
        assert_eq!((c.locomotion.push_speed, c.locomotion.turn_rate), (Some(2.5), Some(0.4)));
        assert_eq!(c.locomotion.pull_speed, None, "unset fields stay unset");
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        // Base from the setup data (a mod-changed base proves the overrides sit on top of it).
        let mut base = skate_core::player::offboard::move_object::MoveObjectTuning::default();
        base.pull_speed = 1.75;
        carry.set_base_tuning(base);
        c.apply_to(&mut carry);
        let l = carry.locomotion();
        assert_eq!((l.move_object.push_speed, l.move_object.pull_speed, l.turn_rate), (2.5, 1.75, Some(0.4)));
        assert_eq!(l.move_object.yaw_controller.derivative, 20.0);
        assert_eq!(l.move_object.yaw_rate_feedback, 0.0, "a mod may switch the yaw-rate feedback off");
        assert_eq!(l.move_object.mass_speed.y, [1.0; 8]);
        assert_eq!(read(&w, "carry")["turn_rate"], json!(0.4f32));
        clear_owner(&mut w, "dev.a");
        let reset = w.resource::<CarrySettings>().clone();
        assert_eq!(reset, CarrySettings::default(), "mod disable restores the retail tuning");
        reset.apply_to(&mut carry);
        assert_eq!(carry.locomotion(), CarryLocomotion::from_tuning(base));
        // Invalid values are rejected by the patch validation.
        assert!(set(&mut w, "dev.a", "carry", Some(json!({"linear_controller": [1.0, 0.0, 1.0, 2.0]}))).is_err());
        assert!(set(&mut w, "dev.a", "carry", Some(json!({"yaw_rate_feedback": -1.0}))).is_err());
    }

    /// The Move Object command rules (retail defaults) are mod knobs, per prop
    /// type blocks included, and go back to retail on mod disable.
    #[test]
    fn carry_move_command_rules_set_and_reset() {
        use crate::physics::prop_dynamics::{MoveCommandRules, PropMaterialBlocks, RETAIL_COMMANDED_MATERIAL};
        let mut w = world();
        assert_eq!(read(&w, "carry")["commanded_material"], json!(RETAIL_COMMANDED_MATERIAL));
        assert_eq!(read(&w, "carry")["apply_at_com"], json!(true));
        assert_eq!(read(&w, "carry")["upright_cos"], json!(0.65f32));
        set(&mut w, "dev.a", "carry", Some(json!({"commanded_material": [0.2, 0.0], "apply_at_com": false, "wake_on_command": false,
            "upright_cos": 0.8, "by_template": {"template/bin": {"material_held": [0.5, 0.0], "material_free": [0.9, 0.1],
            "material_free_upright": [1.0, 0.9], "upright_pair": true, "restitution": 0.25, "angular_drag": 0.5,
            "mass": 40.0, "maximum_angular_velocity": 20.0, "inertia_offset": [0.0, 0.1, 0.0]}}}))).unwrap();
        let r = w.resource::<CarrySettings>().move_rules.clone();
        assert_eq!(r.commanded_material, [0.2, 0.0]);
        assert!(!r.apply_at_com && !r.wake_on_command && r.yaw_replaces_torque && r.ignore_vertical);
        assert_eq!(r.upright_cos, 0.8);
        assert_eq!(
            r.by_template["template/bin"],
            PropMaterialBlocks {
                held: Some([0.5, 0.0]),
                free: Some([0.9, 0.1]),
                free_upright: Some([1.0, 0.9]),
                upright_pair: Some(true),
                restitution: Some(0.25),
                record_272: None,
                linear_drag: None,
                angular_drag: Some(0.5),
                mass: Some(40.0),
                maximum_linear_velocity: None,
                maximum_angular_velocity: Some(20.0),
                inertia_scale: None,
                inertia_offset: Some([0.0, 0.1, 0.0]),
            }
        );
        assert_eq!(read(&w, "carry")["by_template"]["template/bin"]["upright_pair"], json!(true));
        assert_eq!(read(&w, "carry")["by_template"]["template/bin"]["angular_drag"], json!(0.5f32));
        assert_eq!(read(&w, "carry")["by_template"]["template/bin"]["material_free"], json!([0.9f32, 0.1f32]));
        clear_owner(&mut w, "dev.a");
        assert_eq!(w.resource::<CarrySettings>().move_rules, MoveCommandRules::default(), "mod disable restores retail");
    }

    #[test]
    fn carry_buttons_and_grab_range_set_and_reset() {
        let mut w = world();
        set(&mut w, "dev.a", "carry", Some(json!({"grab_bit": 21, "placement_bit": 22, "grab_range": 3.5}))).unwrap();
        let c = w.resource::<CarrySettings>().clone();
        assert_eq!((c.buttons.grab_bit, c.buttons.placement_bit, c.grab_range), (21, 22, 3.5));
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        c.apply_to(&mut carry);
        assert_eq!((carry.buttons().grab_bit, carry.grab_range()), (21, 3.5));
        clear_owner(&mut w, "dev.a");
        let d = w.resource::<CarrySettings>().clone();
        assert_eq!(d, CarrySettings::default());
        d.apply_to(&mut carry);
        assert_eq!((carry.buttons(), carry.grab_range()), (CarryButtons::default(), 2.0));
    }
}
