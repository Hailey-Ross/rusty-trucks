//! The simple lane follower of milestone V3: census cars drive their lanes, take connectors by
//! retail's least-loaded rule, stop at the stop line when the V1 junction query says so and keep
//! a gap to the car ahead. The full retail driver (look-ahead `sub_82C412D8`, speed planner
//! `sub_82C3FA08` with its lead / obstacle cases, horn and manoeuvre deciders, the state graph)
//! is milestone V4; this module is the part of it V3 needs to put moving, light-obeying traffic on
//! screen, with every simplification named below.
//!
//! Retail [code, TU3; addresses are evidence only, behaviour re-implemented, not copied]:
//! - integrator `sub_82C3FF38`: `speed (+3412) += accel (+3408) x dt`; the accel is forced to 0
//!   while the speed is above the cap `+3688` (= `+3680` (+ `+3684` while skitched) x f2); speed
//!   and accel are zeroed when both are tiny. [`integrate`] is that step.
//! - the cap: the lane's speed limit (14.167 / 13.889 m/s [data]); the recomp shows `+3688` =
//!   14.167 on a 51 km/h road and 17.0 while skitched (`+3684` = 0.2) [trace, `npc-livingworld-re`
//!   §7d]. V3 uses the segment's limit on a lane and the exit segment's limit (at most the
//!   junction speed) on a connector.
//! - stop line (`sub_82C3FA08`): `accel = -v^2 / (2 (d - f2) + 0.001)` with `d` the distance to
//!   the line and `f2` the stop distance [code]. [`stop_accel`].
//! - following a lead (`sub_82C3FA08`, `+4403` bit 0x10, both speeds above
//!   `follow_min_speed_kmh` 20): `accel = (max(v_lead - margin x 0.2778, 0)^2 - v^2) /
//!   (2 gap + 0.001)` with `margin` = `follow_speed_margin_kmh` 20 [code + data]; kept as
//!   [`retail_follow_accel`] for V4, V3 uses the plain [`follow_accel`].
//! - the junction query (`sub_82E11E90`, V1 [`junction_entry`]) runs once the stop line is within
//!   `look_ahead + speed`; Go enters, Approach slows to the connector's entry speed, Signal /
//!   Yield / Blocked hold the car at the line [code].
//! - connector choice: least loaded exit lane per metre on every car (`sub_82C376E8`, V1
//!   [`choose_connector`]) [code]; the load is the number of cars on the exit lane (the retail
//!   per-lane vec4 is not read yet, V1 open item).
//! - acceleration ramps 0.2 m/s^2 per ~0.25 s up to 2.3-3.4 m/s^2 from a stop [trace 47CAEDA0,
//!   proof1]; hard stops reach about -7.3 m/s^2 [trace 47CAEDA0, 164620].
//!
//! V3 simplifications (until V4): no look-ahead obstacles other than cars (skater, NPCs and peds
//! are V4 / V5); the look-ahead distance is the comfortable stopping distance plus one second of
//! speed (retail `+3516` is not read); the following rule below 20 km/h and the minimum gap are a
//! plain "stop `min_gap` behind the car ahead"; a car that got Go and can no longer stop
//! comfortably commits to the junction (amber dilemma zone); a car at a dead end stops there (the
//! engine despawns it); no lane changes, overtakes, horns, parking or skids. One engine-side
//! safeguard that is not retail: [`lead`] treats a car inside the junction on another connector
//! into the same exit lane, nearer the exit, as the car ahead (two lanes of one approach merging
//! into one; retail's junction query does not cover that case, its look-ahead does, V4).
//!
//! Multiplayer seams: [`step`] is a pure function of the road network, the signal clock, the cars
//! (sorted by key) and `dt`; cars update in key order and see the cars before them already moved
//! (deterministic). A client that runs the same spawn records from the same tick with the same
//! signal tick count gets the same motion; see doc 26 V3.
//!
//! Moddability: every number is a field of [`FollowParams`] (defaults from the vehicle spec
//! records or the trace values named on each field; a mod overrides per car); the connector
//! choice is the V1 [`ConnectorChoice`].

use std::collections::BTreeMap;

use super::cursor::{choose_connector, ConnectorChoice, LaneCursor, Place};
use super::graph::RoadNetwork;
use super::junction::{junction_entry, query_due, Entry, EntryQuery, Occupancy, VehicleKey, VehicleSnapshot};
use super::signals::SignalClock;
use crate::living_world::rng::Rng;

/// The follower's numbers. Every field is data a mod may override per car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FollowParams {
    /// Largest pull-away acceleration, m/s^2. Default: the spec field `Hash_328B9F4685A14018`
    /// (2.0 default, 3.0 family, 3.1 minivan) [data; that it is the max accel is the V0 layout
    /// candidate, open], matching the 2.3-3.4 m/s^2 the recomp reaches [trace].
    pub accel_max: f32,
    /// Acceleration ramp, m/s^3: 0.2 m/s^2 per 0.25 s [trace proof1, 47CAEDA0].
    pub jerk: f32,
    /// Comfortable braking for the look-ahead and stop planning, m/s^2. Default: the spec field
    /// `Hash_758229215579C6D1` (2.5-3.0) [data; meaning a candidate, open].
    pub plan_decel: f32,
    /// Hardest braking, m/s^2: -7.25 / -7.31 seen [trace 47CAEDA0, 164620].
    pub hard_brake: f32,
    /// Distance kept to the car ahead when stopped (rear to front), m. Engine value (retail
    /// `+3520` not read): 2.0.
    pub min_gap: f32,
    /// Stop distance before the line (`f2` of the stop-line rule), m. Engine value: 0.5.
    pub stop_margin: f32,
    /// Following rule gate and margin (spec `follow_min_speed_kmh`, `follow_speed_margin_kmh`,
    /// 20 / 20 km/h) [data + code `sub_82C3FA08`], in m/s.
    pub follow_min_speed: f32,
    pub follow_margin: f32,
    /// Multiplier on the lane cap (`f2` of the integrator's cap; 1.0 = retail; a mod or the
    /// skitch milestone raises it).
    pub cap_scale: f32,
    /// The driver record's horn values (`livingworld_vehicle_drivers`).
    pub horn: super::horn::HornParams,
}

impl Default for FollowParams {
    /// The `default` spec record's values (`livingworld_vehicle_characteristics/default`) and
    /// the trace values; the engine fills real cars from their spec record.
    fn default() -> Self {
        Self {
            accel_max: 2.0,
            jerk: 0.8,
            plan_decel: 3.0,
            hard_brake: 7.3,
            min_gap: 2.0,
            stop_margin: 0.5,
            follow_min_speed: 20.0 / 3.6,
            follow_margin: 20.0 / 3.6,
            cap_scale: 1.0,
            horn: super::horn::HornParams::default(),
        }
    }
}

/// One car of the follower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Car {
    pub key: VehicleKey,
    pub cursor: LaneCursor,
    /// m/s (retail `+3412`).
    pub speed: f32,
    /// m/s^2 (retail `+3408`): the commanded acceleration of the last step.
    pub accel: f32,
    /// Length along the lane (m); the cursor is the car's centre.
    pub length: f32,
    pub params: FollowParams,
    /// The last junction answer for the chosen connector (retail junction state `+4392`).
    pub entry: Option<Entry>,
    /// The car got Go and can no longer stop comfortably: it goes through.
    pub committed: bool,
    /// An actor (the skater) hit the car ahead of it (retail `+4401` bit 0x20, `sub_82C3C150`): the
    /// planner brakes hard (`sub_82C3FA08` at `0x82C3FE44`: accel = -speed) until the car stands,
    /// then the integrator clears it (`sub_82C3FF38`).
    pub hit_brake: bool,
    /// The nearest obstacle in the look-ahead (`obstacles::nearest`; set by the host each frame, retail
    /// `sub_82C40B70` -> `+3584..+3620`).
    pub obstacle: Option<super::horn::ObstacleHit>,
    /// The rolled driver bits (`+4401` 0x01 / 0x02; the host rolls them at spawn).
    pub driver: super::horn::DriverBits,
    /// The limiter kind of the last step (`+4392`, [`super::horn::limiter`]).
    pub limiter: u8,
    pub horn_timers: super::horn::HornTimers,
    /// The horn state of the last step (`+3420`, 0 = silent) and its honked-at target (kind 2, a ped).
    pub horn: u8,
    pub honk_target: Option<u64>,
}

impl Car {
    pub fn new(key: VehicleKey, cursor: LaneCursor, length: f32, params: FollowParams) -> Self {
        Car { key, cursor, speed: 0.0, accel: 0.0, length, params, entry: None, committed: false, hit_brake: false, obstacle: None, driver: super::horn::DriverBits { horn: true, blocked_long: true }, limiter: 0, horn_timers: Default::default(), horn: 0, honk_target: None }
    }

    /// Look-ahead distance (m): comfortable stopping distance (V3 stand-in for `+3516`).
    pub fn look_ahead(&self) -> f32 {
        self.speed * self.speed / (2.0 * self.params.plan_decel.max(0.1))
    }

    pub fn snapshot(&self) -> VehicleSnapshot {
        VehicleSnapshot {
            id: self.key,
            length: self.length,
            speed: self.speed,
            place: self.cursor.place,
            distance: self.cursor.distance,
            look_ahead: self.look_ahead(),
            min_gap: self.params.min_gap,
            flagged: false,
        }
    }
}

/// What one step did to a car (engine events, mod events, logs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FollowEvent {
    /// The junction answer changed (`None` = left the approach).
    Junction { key: VehicleKey, connector: usize, entry: Entry },
    /// The car entered a connector (it is inside the junction).
    EnteredJunction { key: VehicleKey, connector: usize },
    /// The car entered a lane from a connector.
    EnteredLane { key: VehicleKey, segment: usize, lane: u8 },
    /// The car stands at the end of a lane with nowhere to go.
    DeadEnd { key: VehicleKey },
}

/// `sub_82C3FF38`: one integrator step. Returns the new (speed, accel).
pub fn integrate(speed: f32, accel: f32, cap: f32, dt: f32) -> (f32, f32) {
    let accel = if speed > cap && accel > 0.0 { 0.0 } else { accel };
    let mut v = speed + accel * dt;
    if v < 0.0 {
        v = 0.0;
    }
    if accel.abs() < 1e-3 && v < 1e-3 {
        return (0.0, 0.0);
    }
    (v, accel)
}

/// Stop-line braking (`sub_82C3FA08`): `-v^2 / (2 (d - f2) + 0.001)`; `d - f2 <= 0` asks for an
/// instant stop (`-v / dt` in the integrator's terms; the caller clamps).
pub fn stop_accel(speed: f32, distance: f32, margin: f32) -> f32 {
    let d = distance - margin;
    if d <= 0.0 {
        return f32::NEG_INFINITY;
    }
    -(speed * speed) / (2.0 * d + 0.001)
}

/// Braking to reach `target` m/s at `distance` (the Approach answer and the following rule).
pub fn reach_accel(speed: f32, target: f32, distance: f32) -> f32 {
    if speed <= target {
        return f32::INFINITY;
    }
    if distance <= 0.0 {
        return f32::NEG_INFINITY;
    }
    (target * target - speed * speed) / (2.0 * distance + 0.001)
}

/// Following a lead `gap` metres ahead (rear of the lead to the front of this car): brake to the
/// lead's speed by `min_gap` behind it. V3 simplification: retail's following rule (lead speed
/// minus the 20 km/h margin, above 20 km/h, over the gap fields `+3756` / `+3760` / `+3728`
/// that are not read yet) is V4; [`retail_follow_accel`] holds the formula for it.
pub fn follow_accel(p: &FollowParams, speed: f32, lead_speed: f32, gap: f32) -> f32 {
    let gap = gap - p.min_gap;
    if gap <= 0.0 {
        return f32::NEG_INFINITY;
    }
    reach_accel(speed, lead_speed, gap)
}

/// Retail's following term (`sub_82C3FA08`): `(max(v_lead - margin, 0)^2 - v^2) / (2 gap +
/// 0.001)` when both speeds exceed `follow_min_speed`; `None` otherwise. Not used by the V3
/// follower (its gap input is V4 work); kept so the V4 planner and its tests start from it.
pub fn retail_follow_accel(p: &FollowParams, speed: f32, lead_speed: f32, gap: f32) -> Option<f32> {
    (speed > p.follow_min_speed && lead_speed > p.follow_min_speed)
        .then(|| ((lead_speed - p.follow_margin).max(0.0).powi(2) - speed * speed) / (2.0 * gap + 0.001))
}

/// The speed cap of the car's current place.
pub fn cap(net: &RoadNetwork, place: Place) -> f32 {
    match place {
        Place::Lane { segment, .. } => net.segments[segment].speed_limit,
        Place::Connector { connector } => {
            let c = &net.connectors[connector];
            let j = net.junctions[c.junction].speed;
            match net.connector_exit(connector) {
                Some(s) => net.segments[s].speed_limit.min(if j > 0.0 { j } else { f32::MAX }),
                None => j,
            }
        }
    }
}

/// Cars per place, sorted by distance (rearmost first). Rebuilt each step so the occupancy lists
/// follow the cars' order on the lane (retail keeps them newest first, which is the same order
/// for cars that entered from the lane start).
fn places(cars: &[Car]) -> BTreeMap<Place, Vec<(f32, usize)>> {
    let mut m: BTreeMap<Place, Vec<(f32, usize)>> = BTreeMap::new();
    for (i, c) in cars.iter().enumerate() {
        m.entry(c.cursor.place).or_default().push((c.cursor.distance, i));
    }
    for v in m.values_mut() {
        v.sort_by(|a, b| a.0.total_cmp(&b.0).then(cars[a.1].key.cmp(&cars[b.1].key)));
    }
    m
}

/// The occupancy lists of the cars (lane and connector lists, rearmost first).
pub fn occupancy(cars: &[Car]) -> Occupancy {
    let mut occ = Occupancy::default();
    // enter_* insert at the front: feed front-most first so the rearmost ends up first.
    for (place, list) in places(cars) {
        for &(_, i) in list.iter().rev() {
            match place {
                Place::Lane { segment, lane } => occ.enter_lane(segment, lane, cars[i].key),
                Place::Connector { connector } => occ.enter_connector(connector, cars[i].key),
            }
        }
    }
    occ
}

/// The car ahead of car `i` along its path (same place, then its chosen connector, then that
/// connector's exit lane; within `range` m): (index, gap rear-to-front, lead speed).
pub fn lead(net: &RoadNetwork, cars: &[Car], i: usize, range: f32) -> Option<(usize, f32, f32)> {
    let me = &cars[i];
    let front = me.cursor.distance + me.length * 0.5;
    let scan = |place: Place, offset: f32, same: bool| -> Option<(usize, f32, f32)> {
        let mut best: Option<(usize, f32, f32)> = None;
        for (j, o) in cars.iter().enumerate() {
            if j == i || o.cursor.place != place {
                continue;
            }
            if same && !(o.cursor.distance > me.cursor.distance || (o.cursor.distance == me.cursor.distance && o.key > me.key)) {
                continue;
            }
            let gap = offset + o.cursor.distance - o.length * 0.5 - front;
            if gap <= range && best.is_none_or(|b| gap < b.1) {
                best = Some((j, gap, o.speed));
            }
        }
        best
    };
    let span = me.cursor.span(net);
    let ahead = scan(me.cursor.place, 0.0, true).or_else(|| match me.cursor.place {
        Place::Lane { .. } => {
            let c = me.cursor.next?;
            scan(Place::Connector { connector: c }, span, false).or_else(|| {
                let exit = net.connector_exit(c)?;
                scan(Place::Lane { segment: exit, lane: net.connectors[c].to_lane }, span + net.connectors[c].length(), false)
            })
        }
        Place::Connector { connector } => {
            let exit = net.connector_exit(connector)?;
            scan(Place::Lane { segment: exit, lane: net.connectors[connector].to_lane }, span, false)
        }
    });
    // Merging: a car inside the junction on another connector into the same exit lane that is
    // closer to the exit point goes first. ENGINE SAFEGUARD, NOT RETAIL: retail's junction query
    // never scans the car's own approach (`sub_82E11E90` passes only the ends from_end + 1, + 2
    // and + 3 (r28 / [r1+80] / r16) to the merge scan `sub_82E11C78` and the crossing scan
    // `sub_82E11980`; its sibling check covers connectors of the same lane only) [code], so two
    // lanes of one approach merging into one exit lane are spaced by the look-ahead / planner
    // (`sub_82C412D8`, `sub_82C3FA08`), which is V4. Replace this with the ported look-ahead then.
    let mine = match me.cursor.place {
        Place::Lane { .. } => me.cursor.next.map(|c| (c, span - me.cursor.distance + net.connectors[c].length())),
        Place::Connector { connector } => Some((connector, net.connectors[connector].length() - me.cursor.distance)),
    };
    let mut merge: Option<(usize, f32, f32)> = None;
    if let Some((c, my_left)) = mine {
        let k = &net.connectors[c];
        for (j, o) in cars.iter().enumerate() {
            let Place::Connector { connector: oc } = o.cursor.place else { continue };
            let ok = &net.connectors[oc];
            if j == i || oc == c || ok.junction != k.junction || ok.to_end != k.to_end || ok.to_lane != k.to_lane {
                continue;
            }
            let theirs = ok.length() - o.cursor.distance;
            if theirs < my_left || (theirs == my_left && o.key < me.key) {
                let gap = my_left - theirs - o.length * 0.5 - me.length * 0.5;
                if gap <= range && merge.is_none_or(|b| gap < b.1) {
                    merge = Some((j, gap, o.speed));
                }
            }
        }
    }
    match (ahead, merge) {
        (Some(a), Some(m)) => Some(if m.1 < a.1 { m } else { a }),
        (a, m) => a.or(m),
    }
}

/// One follower step of every car (`cars` sorted by key; the caller keeps them so). Cars update
/// in that order. Returns the events.
pub fn step(net: &RoadNetwork, signals: &SignalClock, cars: &mut [Car], dt: f32, choice: ConnectorChoice, rng: &mut Rng) -> Vec<FollowEvent> {
    let mut events = Vec::new();
    if !(dt > 0.0) {
        return events;
    }
    for i in 0..cars.len() {
        let occ = occupancy(cars);
        let snaps: BTreeMap<VehicleKey, VehicleSnapshot> = cars.iter().map(|c| (c.key, c.snapshot())).collect();
        let me = cars[i];
        let p = me.params;
        let cap_now = cap(net, me.cursor.place) * p.cap_scale;
        // Free road: ramp up to accel_max, never past the cap.
        let ramp = (me.accel.max(0.0) + p.jerk * dt).min(p.accel_max);
        let mut accel = ramp.min((cap_now - me.speed) / dt);
        // Stop line / junction.
        let mut hold_at: Option<f32> = None;
        // The limiter kind and the nearest limit (`+4392`, best distance; horn.rs).
        let mut kind = super::horn::limiter::FREE;
        let mut best = f32::INFINITY;
        let mut entry = me.entry;
        let mut committed = me.committed;
        if let (Place::Lane { segment, .. }, Some(c)) = (me.cursor.place, me.cursor.next) {
            let to_line = net.segments[segment].length - (me.cursor.distance + me.length * 0.5);
            let snap = me.snapshot();
            if query_due(&snap, to_line) || to_line <= p.min_gap {
                let info = junction_entry(&EntryQuery { net, signals, occupancy: &occ, vehicles: &snaps, me: &snap, connector: c, check_lights: true });
                if entry != Some(info.entry) {
                    events.push(FollowEvent::Junction { key: me.key, connector: c, entry: info.entry });
                }
                entry = Some(info.entry);
                // FollowingLane stores the answer in the same field as the limiter kind (`+4392`: 1 signal,
                // 2 approach, 3 yield, 4 blocked, 5 a yield to a flagged car); a car behind one waiting at a light
                // (1) or a flagged yield (5) counts as waiting itself and does not get the blocked horn.
                if info.entry != Entry::Go {
                    kind = if info.blocker_flagged { super::horn::limiter::JUNCTION_WAIT } else { info.entry as u8 };
                    best = to_line;
                }
                let comfortable = me.speed * me.speed / (2.0 * p.plan_decel.max(0.1));
                let hard = me.speed * me.speed / (2.0 * p.hard_brake.max(0.1));
                match info.entry {
                    Entry::Go => {
                        if comfortable >= to_line - p.stop_margin {
                            committed = true;
                        }
                    }
                    Entry::Approach if !committed => {
                        accel = accel.min(reach_accel(me.speed, net.connectors[c].entry_speed, to_line - p.stop_margin));
                    }
                    _ if !committed => {
                        if hard > to_line - p.stop_margin + 0.25 && me.speed > 0.5 {
                            // Cannot stop any more: go through (dilemma zone).
                            committed = true;
                        } else {
                            accel = accel.min(stop_accel(me.speed, to_line, p.stop_margin));
                            hold_at = Some(net.segments[segment].length - me.length * 0.5 - p.stop_margin.min(to_line.max(0.0)));
                        }
                    }
                    _ => {}
                }
            }
        }
        // The car ahead.
        let range = me.look_ahead() + me.speed + p.min_gap + 10.0;
        let mut limit: Option<f32> = None; // max distance the centre may reach along the current place
        let mut lead_close = None;
        if let Some((l, gap, lead_speed)) = lead(net, cars, i, range) {
            accel = accel.min(follow_accel(&p, me.speed, lead_speed, gap));
            // `sub_82C41120`: inside one second of travel plus the standoff the lead limits; "close" = inside
            // the standoff (ours: our follower settles at `min_gap`, so the stop margin is the tolerance).
            lead_close = Some(gap <= p.min_gap + p.stop_margin);
            if gap <= me.speed + p.min_gap && gap < best {
                best = gap;
                use super::horn::limiter::{BEHIND_LEAD, BEHIND_WAITING_LEAD, JUNCTION_WAIT};
                kind = if matches!(cars[l].limiter, BEHIND_WAITING_LEAD | JUNCTION_WAIT) { BEHIND_WAITING_LEAD } else { BEHIND_LEAD };
            }
            // Never closer than half the minimum gap (no overlaps whatever the braking).
            limit = Some(me.cursor.distance + (gap - p.min_gap * 0.5).max(0.0));
        }
        // The obstacle ahead (`sub_82C412D8` -> `sub_82C3FA08`, standoff = the car's min gap).
        let accel = match me.obstacle.and_then(|o| super::obstacles::obstacle_accel(me.speed, o.distance, p.min_gap)) {
            Some(a) => accel.min(a),
            None => accel,
        };
        if me.obstacle.is_some_and(|o| o.distance < best) {
            kind = super::horn::limiter::OBSTACLE;
        }
        // The horn timers and decider (`sub_82C41120`, `sub_82C412D8`, `sub_82C40660`).
        let mut timers = me.horn_timers;
        timers.update(&p.horn, kind, lead_close, me.obstacle.is_some(), me.speed, dt);
        let (horn, honk_target) = super::horn::decide(&p.horn, me.driver, kind, &timers, me.obstacle, me.speed);
        let accel = if me.hit_brake { accel.min(-me.speed) } else { accel };
        let accel = accel.max(-p.hard_brake * 4.0).max(-me.speed / dt);
        let (mut speed, accel) = integrate(me.speed, accel, cap_now, dt);
        let mut ds = speed * dt;
        let mut target = me.cursor.distance + ds;
        if let Some(h) = hold_at {
            if target > h {
                target = h.max(me.cursor.distance);
            }
        }
        if let Some(l) = limit {
            if target > l {
                target = l.max(me.cursor.distance);
            }
        }
        if target < me.cursor.distance + ds {
            ds = (target - me.cursor.distance).max(0.0);
            if ds <= speed * dt * 0.5 {
                speed = (ds / dt).min(speed);
            }
        }
        let car = &mut cars[i];
        car.speed = speed;
        car.accel = accel;
        if speed <= 0.0 {
            car.hit_brake = false;
        }
        car.entry = entry;
        car.committed = committed;
        car.limiter = kind;
        car.horn_timers = timers;
        car.horn = horn;
        car.honk_target = honk_target;
        let loads = &occ;
        let mut choose = |n: &RoadNetwork, s: usize, l: u8| choose_connector(n, s, l, choice, &|seg, lane| loads.lane_load(seg, lane), rng);
        let adv = car.cursor.advance(net, ds, &mut choose);
        for place in adv.entered {
            match place {
                Place::Connector { connector } => {
                    car.entry = None;
                    car.committed = false;
                    events.push(FollowEvent::EnteredJunction { key: car.key, connector });
                }
                Place::Lane { segment, lane } => events.push(FollowEvent::EnteredLane { key: car.key, segment, lane }),
            }
        }
        if adv.at_dead_end {
            car.speed = 0.0;
            car.accel = 0.0;
            events.push(FollowEvent::DeadEnd { key: car.key });
        }
    }
    events
}

#[cfg(test)]
#[path = "follow_tests.rs"]
mod tests;
