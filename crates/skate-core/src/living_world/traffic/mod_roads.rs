//! A mod's own roads (ask 611, "routes for mods"): polylines with lanes, joined at named nodes, added to the map's
//! road input before the network is built. Ours, not retail: retail's roads come only from the map data.
//! Details: docs/hails-additions/26h-living-world-traffic.md#routes-for-mods

use super::graph::{ConnectorInput, Curve, EndInput, JunctionId, JunctionInput, PieceInput, RoadInput, SegmentId, SegmentInput, ENDS};
use crate::living_world::Vec3;
use std::collections::BTreeMap;

/// One road as a mod describes it: a center line from node `from` to node `to`.
#[derive(Clone, Debug, PartialEq)]
pub struct ModRoad {
    pub name: String,
    pub from: String,
    pub to: String,
    /// The center line, at least two points, in world meters.
    pub points: Vec<Vec3>,
    /// Lanes in each direction.
    pub lanes: u8,
    pub lane_width: f32,
    /// m/s.
    pub speed_limit: f32,
    /// Lanes both ways (a second segment from `to` back to `from`, on the other side of the line).
    pub two_way: bool,
    /// Cars the road keeps of its own (none: only the census's cars reach it).
    pub traffic: Option<ModRoadTraffic>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModRoadTraffic {
    pub cars: u32,
    /// Census vehicle category names (empty: any).
    pub categories: Vec<String>,
}

/// The population's view of the roads that ask for cars (both directions of a two-way road).
pub fn traffic(roads: &[ModRoad]) -> Vec<crate::living_world::ModTraffic> {
    roads
        .iter()
        .filter_map(|r| {
            let t = r.traffic.as_ref().filter(|t| t.cars > 0)?;
            let mut segments = vec![segment_id(&r.name, false)];
            if r.two_way {
                segments.push(segment_id(&r.name, true));
            }
            Some(crate::living_world::ModTraffic { name: r.name.clone(), segments, cars: t.cars, categories: t.categories.clone() })
        })
        .collect()
}

/// The stable id a mod name gets (FNV-1a 64 over "mod_road:<kind>:<name>"), apart from the disc's ids by the prefix.
fn id(kind: &str, name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in "mod_road:".bytes().chain(kind.bytes()).chain(b":".iter().copied()).chain(name.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn segment_id(road: &str, back: bool) -> SegmentId {
    SegmentId(id(if back { "back" } else { "road" }, road))
}

pub fn node_id(node: &str) -> u64 {
    id("node", node)
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn along(a: Vec3, d: Vec3, s: f32) -> Vec3 {
    [a[0] + d[0] * s, a[1] + d[1] * s, a[2] + d[2] * s]
}
fn length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
/// The right of a direction on the ground (the network's convention: lane 0 on the left).
fn right_of(d: Vec3) -> Vec3 {
    let l = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-6);
    [-d[2] / l, 0.0, d[0] / l]
}

/// A segment along `points` with `lanes` lanes between the line offset `inner` and `inner + lanes x width` to the
/// right; its lane centers at the first and last point.
fn segment(road: &ModRoad, points: &[Vec3], id: SegmentId, from: (u64, u8), to: (u64, u8), inner: f32) -> (SegmentInput, Vec<Vec3>, Vec<Vec3>) {
    let width = road.lane_width * f32::from(road.lanes);
    let mut pieces = Vec::new();
    let mut distance = 0.0;
    for w in points.windows(2) {
        let r = right_of(sub(w[1], w[0]));
        let len = length(sub(w[1], w[0]));
        if len < 1e-3 {
            continue;
        }
        distance += len;
        pieces.push(PieceInput {
            end_distance: distance,
            centre: Curve::straight(along(w[0], r, inner + width * 0.5), along(w[1], r, inner + width * 0.5)),
            left_start: along(w[0], r, inner),
            right_start: along(w[0], r, inner + width),
            left_end: along(w[1], r, inner),
            right_end: along(w[1], r, inner + width),
        });
    }
    let lane_at = |a: Vec3, b: Vec3, at: Vec3| {
        let r = right_of(sub(b, a));
        (0..road.lanes).map(|l| along(at, r, inner + road.lane_width * (f32::from(l) + 0.5))).collect::<Vec<_>>()
    };
    let n = points.len();
    let starts = lane_at(points[0], points[1], points[0]);
    let ends = lane_at(points[n - 2], points[n - 1], points[n - 1]);
    let seg = SegmentInput {
        id, from_node: from.0, from_end: from.1, to_node: to.0, to_end: to.1, length: distance,
        speed_limit: road.speed_limit, lanes: road.lanes, manoeuvres: 0x02, district: 0, pieces,
    };
    (seg, starts, ends)
}

/// How far (m, on the ground) `map:nearest` looks for a map junction from the road's end point.
pub const MAP_JOIN_RADIUS: f32 = 40.0;

/// A node name that joins a junction of the map's own network: `map:nearest` (the closest free one within
/// [`MAP_JOIN_RADIUS`] of the road's end) or `map:<junction id>` (hex with `0x`, or decimal, as the roads inspect lists).
pub fn map_node(name: &str) -> Option<&str> {
    name.strip_prefix("map:")
}

/// The ground position of a junction: the mean of its connectors' ends (or of the ends of the segments meeting it).
pub fn junction_position(input: &RoadInput, j: &JunctionInput) -> Option<Vec3> {
    let mut points: Vec<Vec3> = j.connectors.iter().flat_map(|c| [c.curve.start, c.curve.end]).collect();
    if points.is_empty() {
        for s in &input.segments {
            if s.to_node == j.id.0 {
                points.extend(s.pieces.last().map(|p| p.centre.end));
            }
            if s.from_node == j.id.0 {
                points.extend(s.pieces.first().map(|p| p.centre.start));
            }
        }
    }
    (!points.is_empty()).then(|| {
        let n = points.len() as f32;
        let sum = points.iter().fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
        [sum[0] / n, sum[1] / n, sum[2] / n]
    })
}

/// The lane centers of a segment at its start (`end` false) or its end.
fn lane_points(s: &SegmentInput, end: bool) -> Vec<Vec3> {
    let piece = if end { s.pieces.last() } else { s.pieces.first() };
    let Some(p) = piece else { return Vec::new() };
    let (l, r) = if end { (p.left_end, p.right_end) } else { (p.left_start, p.right_start) };
    (0..s.lanes).map(|i| along(l, sub(r, l), (f32::from(i) + 0.5) / f32::from(s.lanes.max(1)))).collect()
}

/// A map junction and a free end of it for `map:<key>` at the road end `at`.
fn map_end(out: &RoadInput, positions: &[Option<Vec3>], taken: &BTreeMap<usize, Vec<u8>>, key: &str, at: Vec3) -> Result<(usize, u8), String> {
    let free = |ji: usize| -> Option<u8> {
        let j = &out.junctions[ji];
        (0..ENDS as u8).find(|&e| j.approaches[usize::from(e)].is_none() && j.exits[usize::from(e)].is_none()
            && !taken.get(&ji).is_some_and(|t| t.contains(&e)))
    };
    let ji = if key == "nearest" {
        let mut best: Option<(usize, f32)> = None;
        for (ji, p) in positions.iter().enumerate() {
            let Some(p) = p else { continue };
            let d = ((p[0] - at[0]).powi(2) + (p[2] - at[2]).powi(2)).sqrt();
            if d <= MAP_JOIN_RADIUS && !out.junctions[ji].signalled && free(ji).is_some() && best.is_none_or(|(_, b)| d < b) {
                best = Some((ji, d));
            }
        }
        best.map(|(ji, _)| ji).ok_or_else(|| format!("no free unsignalled map junction within {MAP_JOIN_RADIUS} m"))?
    } else {
        let id = key.strip_prefix("0x").map_or_else(|| key.parse::<u64>().ok(), |h| u64::from_str_radix(h, 16).ok())
            .ok_or_else(|| format!("bad map junction id {key}"))?;
        out.junctions.iter().position(|j| j.id.0 == id).ok_or_else(|| format!("no map junction {key}"))?
    };
    if out.junctions[ji].signalled {
        return Err(format!("map junction {:#x} has traffic lights", out.junctions[ji].id.0));
    }
    let e = free(ji).ok_or_else(|| format!("map junction {:#x} has no free end", out.junctions[ji].id.0))?;
    Ok((ji, e))
}

/// Give back a refused road's map slot.
fn release(taken: &mut BTreeMap<usize, Vec<u8>>, n: (u64, u8, Option<usize>)) {
    if let Some(ji) = n.2 {
        if let Some(t) = taken.get_mut(&ji) {
            if let Some(i) = t.iter().position(|&e| e == n.1) {
                t.remove(i);
            }
        }
    }
}
/// `stock` plus the mods' roads. A road without two distinct points or a node with more than four road ends is left
/// out (named in the returned warnings). Where a node has roads arriving and leaving, an unsignalled junction joins
/// every arriving lane to each leaving road (lane for lane, the last lane where it has fewer), except straight back
/// along the same road; a node with one road end is a dead end (cars leave there, as at a map edge). A `map:` node
/// ([`map_node`]) joins one of the map's own unsignalled junctions at a free end: its lanes get connectors to every
/// exit of the junction and every approach gets connectors onto it, lane for lane as above.
pub fn with_mod_roads(stock: Option<&RoadInput>, roads: &[ModRoad]) -> (RoadInput, Vec<String>) {
    let mut out = stock.cloned().unwrap_or(RoadInput { segments: Vec::new(), junctions: Vec::new() });
    let mut warnings = Vec::new();
    let positions: Vec<Option<Vec3>> = out.junctions.iter().map(|j| junction_position(&out, j)).collect();
    // Each mod node's ends in road order; each map junction's end slots taken by mod roads.
    let mut ends: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    let mut taken: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    struct Arm { node: u64, end: u8, lanes: u8, points: Vec<Vec3>, arriving: bool, road: String, speed: f32, map: Option<usize> }
    let mut arms: Vec<Arm> = Vec::new();
    for road in roads {
        let points: Vec<Vec3> = road.points.iter().copied().fold(Vec::new(), |mut v, p| {
            if v.last().is_none_or(|q| length(sub(p, *q)) >= 1e-3) {
                v.push(p);
            }
            v
        });
        if points.len() < 2 || road.lanes == 0 || road.from == road.to {
            warnings.push(format!("road {}: needs two points, a lane and two different nodes", road.name));
            continue;
        }
        // A map end claims a junction's free slot at once, so the road's other end sees it taken.
        let mut claim = |name: &str, at: Vec3, taken: &mut BTreeMap<usize, Vec<u8>>| -> Result<(u64, u8, Option<usize>), String> {
            match map_node(name) {
                Some(key) => {
                    let (ji, e) = map_end(&out, &positions, taken, key, at)?;
                    taken.entry(ji).or_default().push(e);
                    Ok((out.junctions[ji].id.0, e, Some(ji)))
                }
                None => Ok((node_id(name), u8::MAX, None)),
            }
        };
        let (first, last) = (points[0], points[points.len() - 1]);
        let a = match claim(&road.from, first, &mut taken) {
            Ok(a) => a,
            Err(e) => {
                warnings.push(format!("road {}: {e}", road.name));
                continue;
            }
        };
        let b = match claim(&road.to, last, &mut taken) {
            Ok(b) if b.2.is_some() && b.2 == a.2 => {
                release(&mut taken, a);
                release(&mut taken, b);
                warnings.push(format!("road {}: both ends at one map junction", road.name));
                continue;
            }
            Ok(b) => b,
            Err(e) => {
                release(&mut taken, a);
                warnings.push(format!("road {}: {e}", road.name));
                continue;
            }
        };
        let full = |ends: &BTreeMap<u64, Vec<String>>, n: (u64, u8, Option<usize>)| n.2.is_none() && ends.get(&n.0).map_or(0, Vec::len) >= ENDS;
        if full(&ends, a) || full(&ends, b) {
            release(&mut taken, a);
            release(&mut taken, b);
            warnings.push(format!("road {}: a node has four roads already", road.name));
            continue;
        }
        let mut slot = |n: (u64, u8, Option<usize>)| -> u8 {
            if n.2.is_some() {
                return n.1;
            }
            let list = ends.entry(n.0).or_default();
            list.push(road.name.clone());
            (list.len() - 1) as u8
        };
        let (ea, eb) = (slot(a), slot(b));
        let (a, b) = ((a.0, a.2), (b.0, b.2));
        let id = segment_id(&road.name, false);
        let inner = if road.two_way { 0.0 } else { -road.lane_width * f32::from(road.lanes) * 0.5 };
        let (seg, starts, ends_at) = segment(road, &points, id, (a.0, ea), (b.0, eb), inner);
        arms.push(Arm { node: a.0, end: ea, lanes: road.lanes, points: starts, arriving: false, road: road.name.clone(), speed: road.speed_limit, map: a.1 });
        arms.push(Arm { node: b.0, end: eb, lanes: road.lanes, points: ends_at, arriving: true, road: road.name.clone(), speed: road.speed_limit, map: b.1 });
        out.segments.push(seg);
        if road.two_way {
            let back: Vec<Vec3> = points.iter().rev().copied().collect();
            let bid = segment_id(&road.name, true);
            let (seg, starts, ends_at) = segment(road, &back, bid, (b.0, eb), (a.0, ea), 0.0);
            arms.push(Arm { node: b.0, end: eb, lanes: road.lanes, points: starts, arriving: false, road: road.name.clone(), speed: road.speed_limit, map: b.1 });
            arms.push(Arm { node: a.0, end: ea, lanes: road.lanes, points: ends_at, arriving: true, road: road.name.clone(), speed: road.speed_limit, map: a.1 });
            out.segments.push(seg);
        }
    }
    for (&node, _) in &ends {
        let here: Vec<&Arm> = arms.iter().filter(|a| a.node == node && a.map.is_none()).collect();
        let (arriving, leaving): (Vec<&Arm>, Vec<&Arm>) = here.iter().partition(|a| a.arriving);
        if arriving.is_empty() || leaving.is_empty() {
            continue;
        }
        let mut approaches: [Option<EndInput>; ENDS] = Default::default();
        let mut exits: [Option<EndInput>; ENDS] = Default::default();
        let mut into: BTreeMap<(u8, u8), Vec<u32>> = BTreeMap::new();
        let mut connectors = Vec::new();
        for a in &arriving {
            let mut per_lane = vec![Vec::new(); usize::from(a.lanes)];
            for b in leaving.iter().filter(|b| b.road != a.road) {
                for lane in 0..a.lanes {
                    let to_lane = lane.min(b.lanes - 1);
                    let index = connectors.len() as u32;
                    connectors.push(ConnectorInput {
                        index, entry_speed: a.speed.min(b.speed), from_end: a.end, from_lane: lane, to_end: b.end, to_lane,
                        curve: Curve::straight(a.points[usize::from(lane)], b.points[usize::from(to_lane)]),
                    });
                    per_lane[usize::from(lane)].push(index);
                    into.entry((b.end, to_lane)).or_default().push(index);
                }
            }
            approaches[usize::from(a.end)] = Some(EndInput { lanes: a.lanes, lane_connectors: per_lane });
        }
        for b in &leaving {
            let lanes = (0..b.lanes).map(|l| into.get(&(b.end, l)).cloned().unwrap_or_default()).collect();
            exits[usize::from(b.end)] = Some(EndInput { lanes: b.lanes, lane_connectors: lanes });
        }
        let speed = here.iter().map(|a| a.speed).fold(f32::MAX, f32::min);
        out.junctions.push(JunctionInput { id: JunctionId(node), signalled: false, speed, approaches, exits, connectors });
    }
    // Mod roads at the map's junctions: (end, lanes, lane points, speed, road) per side; the map's own ends from the
    // segments meeting the junction.
    for (&ji, _) in &taken {
        let jid = out.junctions[ji].id.0;
        let speed = out.junctions[ji].speed;
        let mut map_in: Vec<(u8, u8, Vec<Vec3>, f32, String)> = Vec::new();
        let mut map_out: Vec<(u8, u8, Vec<Vec3>, f32, String)> = Vec::new();
        for e in 0..ENDS as u8 {
            if out.junctions[ji].approaches[usize::from(e)].is_some() {
                if let Some(s) = out.segments.iter().find(|s| s.to_node == jid && s.to_end == e) {
                    map_in.push((e, s.lanes, lane_points(s, true), s.speed_limit, format!("map:{}", s.id.0)));
                }
            }
            if out.junctions[ji].exits[usize::from(e)].is_some() {
                if let Some(s) = out.segments.iter().find(|s| s.from_node == jid && s.from_end == e) {
                    map_out.push((e, s.lanes, lane_points(s, false), s.speed_limit, format!("map:{}", s.id.0)));
                }
            }
        }
        let here: Vec<&Arm> = arms.iter().filter(|a| a.map == Some(ji)).collect();
        let mod_in: Vec<(u8, u8, Vec<Vec3>, f32, String)> = here.iter().filter(|a| a.arriving).map(|a| (a.end, a.lanes, a.points.clone(), a.speed, a.road.clone())).collect();
        let mod_out: Vec<(u8, u8, Vec<Vec3>, f32, String)> = here.iter().filter(|a| !a.arriving).map(|a| (a.end, a.lanes, a.points.clone(), a.speed, a.road.clone())).collect();
        let j = &mut out.junctions[ji];
        for (e, lanes, _, _, _) in &mod_in {
            j.approaches[usize::from(*e)] = Some(EndInput { lanes: *lanes, lane_connectors: vec![Vec::new(); usize::from(*lanes)] });
        }
        for (e, lanes, _, _, _) in &mod_out {
            j.exits[usize::from(*e)] = Some(EndInput { lanes: *lanes, lane_connectors: vec![Vec::new(); usize::from(*lanes)] });
        }
        // New pairs: a mod approach into every exit, a map approach into the mod exits; never back along the same road.
        let pairs = mod_in.iter().flat_map(|a| map_out.iter().chain(&mod_out).map(move |b| (a, b)))
            .chain(map_in.iter().flat_map(|a| mod_out.iter().map(move |b| (a, b))));
        for (a, b) in pairs {
            if a.4 == b.4 || a.0 == b.0 || a.2.is_empty() || b.2.is_empty() {
                continue;
            }
            for lane in 0..a.1 {
                let to_lane = lane.min(b.1 - 1);
                let index = j.connectors.len() as u32;
                let (Some(from), Some(to)) = (a.2.get(usize::from(lane)), b.2.get(usize::from(to_lane))) else { continue };
                j.connectors.push(ConnectorInput {
                    index, entry_speed: a.3.min(b.3).min(speed), from_end: a.0, from_lane: lane, to_end: b.0, to_lane,
                    curve: Curve::straight(*from, *to),
                });
                if let Some(end) = j.approaches[usize::from(a.0)].as_mut() {
                    if let Some(l) = end.lane_connectors.get_mut(usize::from(lane)) {
                        l.push(index);
                    }
                }
                if let Some(end) = j.exits[usize::from(b.0)].as_mut() {
                    if let Some(l) = end.lane_connectors.get_mut(usize::from(to_lane)) {
                        l.push(index);
                    }
                }
            }
        }
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::living_world::traffic::RoadNetwork;

    fn road(name: &str, from: &str, to: &str, points: Vec<Vec3>, two_way: bool) -> ModRoad {
        ModRoad { name: name.into(), from: from.into(), to: to.into(), points, lanes: 1, lane_width: 3.5, speed_limit: 11.0, two_way, traffic: None }
    }

    #[test]
    fn a_mod_road_builds_on_its_own_and_lanes_sit_right_of_the_line() {
        let (input, warnings) = with_mod_roads(None, &[road("lane", "a", "b", vec![[0.0, 0.0, 0.0], [0.0, 0.0, 50.0], [20.0, 0.0, 80.0]], false)]);
        assert!(warnings.is_empty());
        let net = RoadNetwork::build(&input).expect("builds");
        assert_eq!(net.segments.len(), 1);
        let s = &net.segments[0];
        assert!((s.length - (50.0 + 1300f32.sqrt())).abs() < 1e-3);
        assert_eq!(s.pieces.len(), 2);
        // Heading +z, a one-way lane is centred on the line: edges at x -1.75 / +1.75 (right of +z is -x in the
        // network's convention right_of = [-z, 0, x]).
        let p = &s.pieces[0];
        assert!((p.left_start[0] - 1.75).abs() < 1e-4 && (p.right_start[0] + 1.75).abs() < 1e-4, "{p:?}");
    }

    #[test]
    fn roads_meeting_at_a_node_get_a_junction_without_u_turns() {
        let roads = [
            road("in", "a", "hub", vec![[0.0, 0.0, -100.0], [0.0, 0.0, 0.0]], true),
            road("left", "hub", "b", vec![[0.0, 0.0, 0.0], [-100.0, 0.0, 0.0]], false),
            road("right", "hub", "c", vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], true),
        ];
        let (input, warnings) = with_mod_roads(None, &roads);
        assert!(warnings.is_empty());
        assert_eq!(input.segments.len(), 5, "two two-way roads and a one-way");
        let hub = input.junctions.iter().find(|j| j.id == JunctionId(node_id("hub"))).expect("hub junction");
        // Arriving at the hub: "in" (forward) and "right" (back); leaving: "in" (back), "left", "right" (forward).
        // Each arriving road may take the two other roads, never its own way back.
        assert_eq!(hub.connectors.len(), 4);
        assert!(hub.connectors.iter().all(|c| c.from_end != c.to_end));
        assert!(input.junctions.iter().all(|j| j.id != JunctionId(node_id("b"))), "a one-road node is a dead end");
        RoadNetwork::build(&input).expect("the merged input builds");
    }

    /// `map:` nodes join the map's own junctions: a free end, connectors from the new road into every exit and from
    /// every approach onto it, never back along the same road; `nearest` and an id both work; a signalled junction or
    /// one with no free end refuses.
    #[test]
    fn a_mod_road_joins_a_junction_of_the_maps_network() {
        // The "map": a hub with two two-way roads (ends 0 and 1 taken).
        let map = [
            road("north", "a", "hub", vec![[0.0, 0.0, -100.0], [0.0, 0.0, 0.0]], true),
            road("east", "hub", "c", vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], true),
        ];
        let (stock, w) = with_mod_roads(None, &map);
        assert!(w.is_empty());
        let hub = JunctionId(node_id("hub"));
        let before = stock.junctions.iter().find(|j| j.id == hub).unwrap().connectors.len();
        let joined = [road("west", "map:nearest", "far", vec![[-5.0, 0.0, 0.0], [-100.0, 0.0, 0.0]], true)];
        let (input, warnings) = with_mod_roads(Some(&stock), &joined);
        assert!(warnings.is_empty(), "{warnings:?}");
        let j = input.junctions.iter().find(|j| j.id == hub).unwrap();
        assert!(j.approaches[2].is_some() && j.exits[2].is_some(), "the free end 2");
        // west in -> north out, east out; north in, east in -> west out: 4 new one-lane connectors.
        assert_eq!(j.connectors.len(), before + 4);
        assert!(j.connectors.iter().all(|c| c.from_end != c.to_end));
        let west = input.segments.iter().find(|s| s.id == segment_id("west", false)).unwrap();
        assert_eq!((west.from_node, west.from_end), (hub.0, 2));
        RoadNetwork::build(&input).expect("the joined network builds");
        // By id (decimal or hex) too.
        let by_id = [road("west", &format!("map:{:#x}", hub.0), "far", vec![[-5.0, 0.0, 0.0], [-100.0, 0.0, 0.0]], true)];
        assert!(with_mod_roads(Some(&stock), &by_id).1.is_empty());
        // Too far, signalled, or full.
        let far = [road("west", "map:nearest", "far", vec![[-500.0, 0.0, 0.0], [-600.0, 0.0, 0.0]], true)];
        assert_eq!(with_mod_roads(Some(&stock), &far).1.len(), 1);
        let mut lights = stock.clone();
        lights.junctions.iter_mut().for_each(|j| j.signalled = true);
        assert!(with_mod_roads(Some(&lights), &by_id).1[0].contains("traffic lights"));
        let three: Vec<ModRoad> = (0..3).map(|i| road(&format!("w{i}"), "map:nearest", &format!("f{i}"), vec![[-5.0, 0.0, i as f32], [-100.0, 0.0, i as f32 * 20.0]], true)).collect();
        assert_eq!(with_mod_roads(Some(&stock), &three).1.len(), 1, "two free ends: the third road is refused");
    }

    #[test]
    fn bad_roads_are_left_out_and_named() {
        let mut roads = vec![road("short", "a", "b", vec![[0.0; 3], [0.0; 3]], false), road("loop", "a", "a", vec![[0.0; 3], [5.0, 0.0, 0.0]], false)];
        for i in 0..5 {
            roads.push(road(&format!("spoke{i}"), "hub", &format!("n{i}"), vec![[0.0; 3], [i as f32 + 1.0, 0.0, 10.0]], false));
        }
        let (input, warnings) = with_mod_roads(None, &roads);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert_eq!(input.segments.len(), 4, "four ends per node");
        assert_ne!(segment_id("x", false), segment_id("x", true));
    }
}
