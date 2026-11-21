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

/// `stock` plus the mods' roads. A road without two distinct points or a node with more than four road ends is left
/// out (named in the returned warnings). Where a node has roads arriving and leaving, an unsignalled junction joins
/// every arriving lane to each leaving road (lane for lane, the last lane where it has fewer), except straight back
/// along the same road; a node with one road end is a dead end (cars leave there, as at a map edge).
pub fn with_mod_roads(stock: Option<&RoadInput>, roads: &[ModRoad]) -> (RoadInput, Vec<String>) {
    let mut out = stock.cloned().unwrap_or(RoadInput { segments: Vec::new(), junctions: Vec::new() });
    let mut warnings = Vec::new();
    // Each node's ends in road order: (road name, the road's end at this node).
    let mut ends: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    struct Arm { node: u64, end: u8, lanes: u8, points: Vec<Vec3>, arriving: bool, road: String, speed: f32 }
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
        let (a, b) = (node_id(&road.from), node_id(&road.to));
        let free = |ends: &BTreeMap<u64, Vec<String>>, n: u64| ends.get(&n).map_or(0, Vec::len) < ENDS;
        if !free(&ends, a) || !free(&ends, b) {
            warnings.push(format!("road {}: a node has four roads already", road.name));
            continue;
        }
        let mut take = |n: u64| {
            let list = ends.entry(n).or_default();
            list.push(road.name.clone());
            (list.len() - 1) as u8
        };
        let (ea, eb) = (take(a), take(b));
        let id = segment_id(&road.name, false);
        let inner = if road.two_way { 0.0 } else { -road.lane_width * f32::from(road.lanes) * 0.5 };
        let (seg, starts, ends_at) = segment(road, &points, id, (a, ea), (b, eb), inner);
        arms.push(Arm { node: a, end: ea, lanes: road.lanes, points: starts, arriving: false, road: road.name.clone(), speed: road.speed_limit });
        arms.push(Arm { node: b, end: eb, lanes: road.lanes, points: ends_at, arriving: true, road: road.name.clone(), speed: road.speed_limit });
        out.segments.push(seg);
        if road.two_way {
            let back: Vec<Vec3> = points.iter().rev().copied().collect();
            let bid = segment_id(&road.name, true);
            let (seg, starts, ends_at) = segment(road, &back, bid, (b, eb), (a, ea), 0.0);
            arms.push(Arm { node: b, end: eb, lanes: road.lanes, points: starts, arriving: false, road: road.name.clone(), speed: road.speed_limit });
            arms.push(Arm { node: a, end: ea, lanes: road.lanes, points: ends_at, arriving: true, road: road.name.clone(), speed: road.speed_limit });
            out.segments.push(seg);
        }
    }
    for (&node, _) in &ends {
        let here: Vec<&Arm> = arms.iter().filter(|a| a.node == node).collect();
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
