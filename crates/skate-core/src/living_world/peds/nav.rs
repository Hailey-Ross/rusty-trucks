//! The pedestrian navmesh (doc 26, peds milestone M3): retail's NavPower nav graphs as polygons
//! with neighbours, point location, reachability and polygon paths.
//!
//! Retail [code, TU3; addresses are evidence only]: every ped owns a NavPower bot (`ped+5940`)
//! that its mover (`ped+2224`, vtable `0x8232C708`) drives: the mover hands the bot a destination
//! (`sub_82C47378`) and reads its steering direction back (`sub_82C47198`). The wander goal
//! probes destinations with a nearest-polygon search (`sub_82C46348`, radius 0.5 `0x8209975C`)
//! and a reachability test (`sub_82C464F0` -> `sub_82926BA8`, a per-graph connectivity bitset),
//! not with a straight ray. The polygons come from the `0x00EB0027` objects
//! (`tools/asset_pipeline/living_world_navmesh.py` documents the layout) [data].
//!
//! What is ours (stated simplifications): NavPower's path search and path following are not
//! decoded, so a path is an A* over polygon neighbours (portal midpoints, equal cost per metre
//! unless [`NavRules::area_cost`] says otherwise) smoothed by the funnel algorithm; reachability
//! is a connected-component label; polygons of [`NavRules::blocked_areas`] (default the 0xF1
//! area, where no recomp ped position ever lies [trace]) are not walkable.
//!
//! Pure and deterministic: no hash-map iteration, ties broken by polygon index, so every machine
//! that loads the same mesh plans the same paths.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

use crate::living_world::Vec3;

/// Area byte of road carriageway polygons [data: 98 % of DownTown road piece centres].
pub const AREA_ROAD: u8 = 0xA1;
/// Area byte of pavement / default polygons [data: 91-92 % of recomp ped positions, trace].
pub const AREA_DEFAULT: u8 = 0x11;
/// Area byte no recomp ped stands on [trace: 0 of ~8100 PEDXYZ samples]; meaning open.
pub const AREA_OTHER: u8 = 0xF1;

/// One polygon as loaded (vertices in order; `neighbours[k]` = the polygon across the edge from
/// `verts[k]` to `verts[k + 1]`, `None` at a boundary).
#[derive(Clone, Debug, PartialEq)]
pub struct NavPolyInput {
    pub verts: Vec<Vec3>,
    pub neighbours: Vec<Option<u32>>,
    pub area: u8,
}

/// A navmesh as plain records (the export's `navmesh.bin` district, or a mod's walk areas).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavMeshInput {
    /// NavPower header values: cell, agent radius, step, agent height (0.12 / 0.35 / 0.2 / 1.6
    /// [data], meanings unverified).
    pub agent: [f32; 4],
    pub polygons: Vec<NavPolyInput>,
}

/// Which polygons peds may use and what crossing them costs. Data defaults; a mod overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct NavRules {
    pub blocked_areas: Vec<u8>,
    /// Cost per metre by area (absent = 1). Retail costs are not read; empty by default.
    pub area_cost: Vec<(u8, f32)>,
    /// Horizontal snap radius of a point query [code `0x8209975C` = 0.5 in `sub_82C46348`].
    pub locate_radius: f32,
    /// Vertical window of a point query (ours: the agent height plus a slope allowance).
    pub locate_height: f32,
    /// Polygons A* may expand per query (ours, a cost cap).
    pub max_expansions: usize,
}

impl Default for NavRules {
    fn default() -> Self {
        Self { blocked_areas: vec![AREA_OTHER], area_cost: Vec::new(), locate_radius: 0.5, locate_height: 4.0, max_expansions: 6000 }
    }
}

impl NavRules {
    pub fn walkable(&self, area: u8) -> bool {
        !self.blocked_areas.contains(&area)
    }
    pub fn cost(&self, area: u8) -> f32 {
        self.area_cost.iter().find(|(a, _)| *a == area).map_or(1.0, |(_, c)| *c)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavPoly {
    pub verts: Vec<Vec3>,
    pub neighbours: Vec<Option<u32>>,
    pub area: u8,
    pub centre: Vec3,
    pub min: [f32; 2],
    pub max: [f32; 2],
}

/// The built mesh: polygons, a uniform grid for point queries, component labels.
#[derive(Clone, Debug, PartialEq)]
pub struct NavMesh {
    pub agent: [f32; 4],
    pub polys: Vec<NavPoly>,
    pub rules: NavRules,
    cell: f32,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
    component: Vec<u32>,
}

/// A located point: polygon and the point on it (snapped when it was outside).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavPoint {
    pub poly: u32,
    pub position: Vec3,
}

fn sub2(a: Vec3, b: Vec3) -> [f32; 2] {
    [a[0] - b[0], a[2] - b[2]]
}
fn cross2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}
pub(crate) fn dist_xz(a: Vec3, b: Vec3) -> f32 {
    let d = sub2(a, b);
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

/// Closest point to `p` on segment `a`-`b` (xz) and its squared distance.
fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> (Vec3, f32) {
    let ab = sub2(b, a);
    let ap = sub2(p, a);
    let len = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1]) / len).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    let d = sub2(p, q);
    (q, d[0] * d[0] + d[1] * d[1])
}

#[derive(Clone, Copy, PartialEq)]
struct Open {
    f: f32,
    poly: u32,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        // Min-heap on f, then on the polygon index (deterministic).
        o.f.total_cmp(&self.f).then_with(|| o.poly.cmp(&self.poly))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl NavMesh {
    pub fn build(input: &NavMeshInput, rules: NavRules) -> Self {
        let cell = 8.0f32;
        let n = input.polygons.len() as u32;
        let polys: Vec<NavPoly> = input
            .polygons
            .iter()
            .map(|p| {
                let k = p.verts.len().max(1) as f32;
                let mut c = [0.0f32; 3];
                let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
                for v in &p.verts {
                    for i in 0..3 {
                        c[i] += v[i] / k;
                    }
                    min = [min[0].min(v[0]), min[1].min(v[2])];
                    max = [max[0].max(v[0]), max[1].max(v[2])];
                }
                let neighbours = p.neighbours.iter().map(|x| x.filter(|&q| q < n)).collect();
                NavPoly { verts: p.verts.clone(), neighbours, area: p.area, centre: c, min, max }
            })
            .collect();
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for (k, p) in polys.iter().enumerate() {
            if p.verts.is_empty() {
                continue;
            }
            for i in (p.min[0] / cell).floor() as i32..=(p.max[0] / cell).floor() as i32 {
                for j in (p.min[1] / cell).floor() as i32..=(p.max[1] / cell).floor() as i32 {
                    grid.entry((i, j)).or_default().push(k as u32);
                }
            }
        }
        // Components over walkable polygons (an edge counts when either side links the other).
        let mut adjacency: Vec<Vec<u32>> = vec![Vec::new(); polys.len()];
        for (k, p) in polys.iter().enumerate() {
            for q in p.neighbours.iter().flatten() {
                adjacency[k].push(*q);
                adjacency[*q as usize].push(k as u32);
            }
        }
        let mut component = vec![u32::MAX; polys.len()];
        let mut next = 0;
        for start in 0..polys.len() {
            if component[start] != u32::MAX || !rules.walkable(polys[start].area) {
                continue;
            }
            let mut stack = vec![start as u32];
            component[start] = next;
            while let Some(k) = stack.pop() {
                for &q in &adjacency[k as usize] {
                    if component[q as usize] == u32::MAX && rules.walkable(polys[q as usize].area) {
                        component[q as usize] = next;
                        stack.push(q);
                    }
                }
            }
            next += 1;
        }
        Self { agent: input.agent, polys, rules, cell, grid, component }
    }

    pub fn is_empty(&self) -> bool {
        self.polys.is_empty()
    }

    fn contains_xz(&self, poly: u32, x: f32, z: f32) -> bool {
        let p = &self.polys[poly as usize];
        if x < p.min[0] || x > p.max[0] || z < p.min[1] || z > p.max[1] {
            return false;
        }
        let mut inside = false;
        let n = p.verts.len();
        for i in 0..n {
            let (a, b) = (p.verts[i], p.verts[(i + 1) % n]);
            if (a[2] > z) != (b[2] > z) && x < (b[0] - a[0]) * (z - a[2]) / (b[2] - a[2]) + a[0] {
                inside = !inside;
            }
        }
        inside
    }

    /// Height of polygon `poly` at (x, z): the triangle of its fan that holds the point (centroid
    /// height outside every triangle).
    pub fn height_at(&self, poly: u32, x: f32, z: f32) -> f32 {
        let p = &self.polys[poly as usize];
        let v0 = p.verts[0];
        for i in 1..p.verts.len().saturating_sub(1) {
            let (v1, v2) = (p.verts[i], p.verts[i + 1]);
            let d = (v1[2] - v2[2]) * (v0[0] - v2[0]) + (v2[0] - v1[0]) * (v0[2] - v2[2]);
            if d.abs() < 1e-9 {
                continue;
            }
            let a = ((v1[2] - v2[2]) * (x - v2[0]) + (v2[0] - v1[0]) * (z - v2[2])) / d;
            let b = ((v2[2] - v0[2]) * (x - v2[0]) + (v0[0] - v2[0]) * (z - v2[2])) / d;
            let c = 1.0 - a - b;
            if a >= -1e-4 && b >= -1e-4 && c >= -1e-4 {
                return a * v0[1] + b * v1[1] + c * v2[1];
            }
        }
        p.centre[1]
    }

    fn candidates(&self, x: f32, z: f32) -> &[u32] {
        self.grid.get(&((x / self.cell).floor() as i32, (z / self.cell).floor() as i32)).map_or(&[], |v| v.as_slice())
    }

    /// The walkable polygon under `p` (closest in height within [`NavRules::locate_height`]), or
    /// the closest walkable polygon edge point within [`NavRules::locate_radius`].
    pub fn locate(&self, p: Vec3) -> Option<NavPoint> {
        let mut best: Option<(f32, u32)> = None;
        for &k in self.candidates(p[0], p[2]) {
            if !self.rules.walkable(self.polys[k as usize].area) || !self.contains_xz(k, p[0], p[2]) {
                continue;
            }
            let dy = (self.height_at(k, p[0], p[2]) - p[1]).abs();
            if dy <= self.rules.locate_height && best.is_none_or(|(d, _)| dy < d) {
                best = Some((dy, k));
            }
        }
        if let Some((_, k)) = best {
            return Some(NavPoint { poly: k, position: [p[0], self.height_at(k, p[0], p[2]), p[2]] });
        }
        let r = self.rules.locate_radius;
        let mut snap: Option<(f32, u32, Vec3)> = None;
        let (i0, i1) = (((p[0] - r) / self.cell).floor() as i32, ((p[0] + r) / self.cell).floor() as i32);
        let (j0, j1) = (((p[2] - r) / self.cell).floor() as i32, ((p[2] + r) / self.cell).floor() as i32);
        for i in i0..=i1 {
            for j in j0..=j1 {
                for &k in self.grid.get(&(i, j)).map_or(&[][..], |v| v.as_slice()) {
                    let poly = &self.polys[k as usize];
                    if !self.rules.walkable(poly.area) || (poly.centre[1] - p[1]).abs() > self.rules.locate_height {
                        continue;
                    }
                    let n = poly.verts.len();
                    for e in 0..n {
                        let (q, d2) = closest_on_segment(p, poly.verts[e], poly.verts[(e + 1) % n]);
                        if d2 <= r * r && snap.is_none_or(|(b, bk, _)| d2 < b || (d2 == b && k < bk)) {
                            snap = Some((d2, k, q));
                        }
                    }
                }
            }
        }
        snap.map(|(_, k, q)| NavPoint { poly: k, position: q })
    }

    /// Whether `b` can be reached from `a` (same component; retail: the graph's connectivity
    /// bitset, `sub_82926AC0`).
    pub fn reachable(&self, a: u32, b: u32) -> bool {
        let (ca, cb) = (self.component[a as usize], self.component[b as usize]);
        ca != u32::MAX && ca == cb
    }

    /// The portal between `p` and its neighbour `q` across edge `e` of `p`: `p`'s edge clipped
    /// to the part `q`'s matching edge covers (T-junctions), else `p`'s whole edge.
    fn portal(&self, p: u32, e: usize, q: u32) -> (Vec3, Vec3) {
        let pp = &self.polys[p as usize];
        let (a, b) = (pp.verts[e], pp.verts[(e + 1) % pp.verts.len()]);
        let qq = &self.polys[q as usize];
        let back = qq.neighbours.iter().position(|&x| x == Some(p));
        let Some(f) = back else { return (a, b) };
        let (c, d) = (qq.verts[f], qq.verts[(f + 1) % qq.verts.len()]);
        let ab = sub2(b, a);
        let len = ab[0] * ab[0] + ab[1] * ab[1];
        if len <= 0.0 {
            return (a, b);
        }
        let t = |v: Vec3| {
            let av = sub2(v, a);
            (av[0] * ab[0] + av[1] * ab[1]) / len
        };
        let (t0, t1) = (t(c).min(t(d)).max(0.0), t(c).max(t(d)).min(1.0));
        if t1 - t0 < 1e-4 {
            return (a, b);
        }
        let lerp = |s: f32| [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s];
        (lerp(t0), lerp(t1))
    }

    /// Path corners from `from` to `to` (both located), or `None` when unreachable or the search
    /// cap is hit. The first corner is the first turn (not the start), the last is `to`.
    pub fn find_path(&self, from: NavPoint, to: NavPoint) -> Option<Vec<Vec3>> {
        if !self.reachable(from.poly, to.poly) {
            return None;
        }
        if from.poly == to.poly {
            return Some(vec![to.position]);
        }
        let n = self.polys.len();
        let mut g = vec![f32::INFINITY; n];
        let mut came: Vec<Option<(u32, u8)>> = vec![None; n];
        let mut entry = vec![from.position; n];
        let mut heap = BinaryHeap::new();
        g[from.poly as usize] = 0.0;
        heap.push(Open { f: dist_xz(from.position, to.position), poly: from.poly });
        let mut expansions = 0;
        let mut found = false;
        while let Some(Open { poly, .. }) = heap.pop() {
            if poly == to.poly {
                found = true;
                break;
            }
            expansions += 1;
            if expansions > self.rules.max_expansions {
                return None;
            }
            let p = &self.polys[poly as usize];
            for (e, nb) in p.neighbours.iter().enumerate() {
                let Some(q) = *nb else { continue };
                if !self.rules.walkable(self.polys[q as usize].area) {
                    continue;
                }
                let (a, b) = self.portal(poly, e, q);
                let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5];
                let step = dist_xz(entry[poly as usize], mid) * self.rules.cost(p.area);
                let tentative = g[poly as usize] + step;
                if tentative < g[q as usize] {
                    g[q as usize] = tentative;
                    came[q as usize] = Some((poly, e as u8));
                    entry[q as usize] = mid;
                    heap.push(Open { f: tentative + dist_xz(mid, to.position), poly: q });
                }
            }
        }
        if !found {
            return None;
        }
        let mut chain = Vec::new();
        let mut at = to.poly;
        while let Some((prev, e)) = came[at as usize] {
            chain.push((prev, e as usize, at));
            at = prev;
        }
        chain.reverse();
        let portals: Vec<(Vec3, Vec3)> = chain
            .iter()
            .map(|&(p, e, q)| {
                let (a, b) = self.portal(p, e, q);
                // Orient as (left, right) seen along the travel from p to q (x right of +z,
                // the funnel's convention: right = larger cross(v - centre, travel)).
                let c = self.polys[p as usize].centre;
                let dir = sub2(self.polys[q as usize].centre, c);
                if cross2(sub2(a, c), dir) <= cross2(sub2(b, c), dir) { (a, b) } else { (b, a) }
            })
            .collect();
        Some(funnel(from.position, to.position, &portals))
    }
}

/// The simple stupid funnel over (left, right) portals in xz; returns the corners after `start`.
pub fn funnel(start: Vec3, goal: Vec3, portals: &[(Vec3, Vec3)]) -> Vec<Vec3> {
    let mut pts: Vec<(Vec3, Vec3)> = portals.to_vec();
    pts.push((goal, goal));
    let mut out = Vec::new();
    let mut apex = start;
    let (mut left, mut right) = (start, start);
    let (mut li, mut ri) = (0usize, 0usize);
    let mut i = 0usize;
    // Recast's triarea2 in xz.
    let area = |a: Vec3, b: Vec3, c: Vec3| cross2(sub2(c, a), sub2(b, a));
    let eq = |a: Vec3, b: Vec3| dist_xz(a, b) < 1e-5;
    let mut guard = 0;
    while i < pts.len() {
        guard += 1;
        if guard > 4 * pts.len() + 16 {
            break;
        }
        let (pl, pr) = pts[i];
        // Right side (Recast's simple stupid funnel).
        if area(apex, right, pr) <= 0.0 {
            if eq(apex, right) || area(apex, left, pr) > 0.0 {
                right = pr;
                ri = i;
            } else {
                out.push(left);
                apex = left;
                ri = li;
                i = li + 1;
                left = apex;
                right = apex;
                continue;
            }
        }
        // Left side.
        if area(apex, left, pl) >= 0.0 {
            if eq(apex, left) || area(apex, right, pl) < 0.0 {
                left = pl;
                li = i;
            } else {
                out.push(right);
                apex = right;
                li = ri;
                i = ri + 1;
                left = apex;
                right = apex;
                continue;
            }
        }
        i += 1;
    }
    if out.last().is_none_or(|l| !eq(*l, goal)) {
        out.push(goal);
    }
    out.retain(|p| !eq(*p, start));
    if out.is_empty() {
        out.push(goal);
    }
    out
}
