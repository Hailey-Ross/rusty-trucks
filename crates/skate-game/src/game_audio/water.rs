//! Water sounds on the map's water. When a map loads, its retail water
//! collision (surface type 12) is grouped into bodies; each body gets a sound
//! by size — moving fountain water, canal lapping, pond lapping — played from
//! the point of that water nearest the camera and fading out with distance.
//! Retail places water emitters by hand in its per-map `.ems` files; this is
//! our own placement from the geometry (docs/hails-additions/11-audio.md).
use super::{Category, Library, Play, Voices, library::Clip, voices::VoiceId};
use crate::physics::GamePhysics;
use bevy::prelude::*;
use skate_core::physics::board_world::is_water_tag;
use std::collections::HashMap;

/// Banks and samples the water emitters play (preloaded at startup).
pub(super) const PRELOAD: [(&str, &[usize]); 3] = [
    ("water_fountain", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
    ("water_lapping", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
    ("water_lapping_pond", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
];

/// Surface sample spacing (m) for finding the nearest water.
const GRID: f32 = 6.0;
/// Positions closer than this (m) count as the same vertex when grouping.
const WELD: f32 = 0.05;
/// At most this many water bodies sound at once (the closest).
const MAX_BODIES: usize = 3;
/// Emitters are placed at most this far from the listener, in the direction
/// of the water: Bevy's attenuation (scale 0.1) is then 1, and the distance
/// fade below is the only one, while panning still points at the water.
const PAN_DISTANCE: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Fountain,
    Canal,
    Lake,
}
impl Kind {
    fn of(area: f32) -> Self {
        if area < 400.0 { Kind::Fountain } else if area < 30_000.0 { Kind::Canal } else { Kind::Lake }
    }
    /// (level, audible radius in m)
    fn level_and_reach(self) -> (f32, f32) {
        match self {
            Kind::Fountain => (0.45, 25.0),
            Kind::Canal => (0.4, 40.0),
            Kind::Lake => (0.45, 60.0),
        }
    }
}

struct Body {
    kind: Kind,
    points: Vec<Vec3>,
}

/// Group water triangles into bodies (shared vertex positions) and sample each
/// on a GRID. Pure: tested below.
fn bodies(triangles: &[[Vec3; 3]]) -> Vec<(f32, Vec<Vec3>)> {
    let mut parent: Vec<usize> = (0..triangles.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut first = HashMap::<[i32; 3], usize>::new();
    for (t, triangle) in triangles.iter().enumerate() {
        for p in triangle {
            let key = [p.x, p.y, p.z].map(|c| (c / WELD).round() as i32);
            let other = *first.entry(key).or_insert(t);
            let (a, b) = (root(&mut parent, t), root(&mut parent, other));
            parent[a] = b;
        }
    }
    let mut groups = HashMap::<usize, (f32, HashMap<(i32, i32), Vec3>)>::new();
    for (t, [a, b, c]) in triangles.iter().enumerate() {
        let r = root(&mut parent, t);
        let entry = groups.entry(r).or_default();
        entry.0 += 0.5 * (*b - *a).cross(*c - *a).length();
        let centre = (*a + *b + *c) / 3.0;
        // One sample per grid cell, plus the vertices of large triangles.
        for p in [centre, *a, *b, *c] {
            entry.1.entry(((p.x / GRID).floor() as i32, (p.z / GRID).floor() as i32)).or_insert(p);
        }
    }
    groups.into_values().map(|(area, cells)| (area, cells.into_values().collect())).collect()
}

/// Distance fade: full near the water, silent at `reach`.
fn fade(distance: f32, reach: f32) -> f32 {
    (1.0 - distance / reach).clamp(0.0, 1.0).powf(1.5)
}

#[derive(Default)]
struct Emitter {
    voice: Option<(VoiceId, Clip)>,
    next: f32,
}

#[derive(Default)]
pub(super) struct State {
    map: Option<(String, u64)>,
    bodies: Vec<Body>,
    emitters: HashMap<usize, Emitter>,
    rng: u32,
}

fn random(rng: &mut u32) -> f32 {
    if *rng == 0 {
        *rng = 0x9e37_79b9;
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    (*rng >> 8) as f32 / (1u32 << 24) as f32
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut state: Local<State>,
    mut commands: Commands,
    map: Res<crate::map_transition::CurrentMap>,
    physics: Res<GamePhysics>,
    library: Option<ResMut<Library>>,
    mut voices: ResMut<Voices>,
    mut assets: ResMut<Assets<AudioSource>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
) {
    let Some(mut library) = library else { return };
    let identity = (map.name.clone(), map.generation);
    if state.map.as_ref() != Some(&identity) {
        for (_, emitter) in state.emitters.drain() {
            if let Some((id, _)) = emitter.voice { voices.stop(id, 1.0); }
        }
        let water: Vec<[Vec3; 3]> = physics.world().triangles().iter().filter(|t| is_water_tag(t.tag))
            .map(|t| t.triangle.vertices.map(|v| Vec3::new(v.x, v.y, v.z))).collect();
        state.bodies = bodies(&water).into_iter().map(|(area, points)| Body { kind: Kind::of(area), points }).collect();
        let count = |k| state.bodies.iter().filter(|b| b.kind == k).count();
        info!("Water audio: {} bodies ({} fountains, {} canals, {} lakes)", state.bodies.len(),
            count(Kind::Fountain), count(Kind::Canal), count(Kind::Lake));
        state.map = Some(identity);
    }
    let Ok(ear) = listener.single().map(GlobalTransform::translation) else { return };
    if super::silenced(menu.as_deref(), &replay) {
        return;
    }
    let dt = time.delta_secs().clamp(0.0, 0.25);
    let now = time.elapsed_secs_f64();
    // Nearest point of each body; keep the closest audible ones.
    let mut audible: Vec<(usize, f32, Vec3)> = state.bodies.iter().enumerate().filter_map(|(i, body)| {
        let nearest = body.points.iter().min_by(|a, b| a.distance_squared(ear).total_cmp(&b.distance_squared(ear)))?;
        let d = nearest.distance(ear);
        (d < body.kind.level_and_reach().1).then_some((i, d, *nearest))
    }).collect();
    audible.sort_by(|a, b| a.1.total_cmp(&b.1));
    // Only the nearest body of each kind: neighbouring fountains would play
    // copies of the same loop, which phase against each other.
    let mut kinds = Vec::new();
    audible.retain(|(i, ..)| {
        let kind = state.bodies[*i].kind;
        let first = !kinds.contains(&kind);
        kinds.push(kind);
        first
    });
    audible.truncate(MAX_BODIES);

    let state = &mut *state;
    let mut rng = state.rng;
    // Stop bodies that went out of reach.
    state.emitters.retain(|i, emitter| {
        let keep = audible.iter().any(|(j, ..)| j == i);
        if !keep {
            if let Some((id, _)) = emitter.voice.take() { voices.stop(id, 0.8); }
        }
        keep
    });
    for (index, distance, nearest) in audible {
        let kind = state.bodies[index].kind;
        let (level, reach) = kind.level_and_reach();
        let volume = level * fade(distance, reach);
        let towards = (nearest - ear).normalize_or_zero() * distance.min(PAN_DISTANCE);
        let at = ear + towards;
        let emitter = state.emitters.entry(index).or_default();
        if emitter.voice.as_ref().is_some_and(|(id, _)| !voices.playing(*id)) {
            emitter.voice = None;
        }
        let mut play = Play { category: Category::Ambience, volume, pitch: 1.0, position: Some(at), looping: false, fade_in: 0.4, envelope: None };
        // Short pieces, overlapping with soft fade-ins, so the water keeps
        // moving: fountains splash and flow (not still-water lapping, which
        // the user rejected for fountains), canals and lakes lap.
        let bank = match kind {
            Kind::Fountain => "water_fountain",
            Kind::Canal => "water_lapping",
            Kind::Lake => "water_lapping_pond",
        };
        emitter.next -= dt;
        if emitter.next <= 0.0 {
            let index = (random(&mut rng) * 10.0) as usize % 10;
            if let Some(clip) = library.sample(&mut assets, bank, index) {
                play.pitch = 0.95 + 0.1 * random(&mut rng);
                voices.play(&mut commands, &clip, play, now);
            }
            emitter.next = if kind == Kind::Fountain { 0.6 + 0.5 * random(&mut rng) } else { 0.7 + 0.6 * random(&mut rng) };
        }
    }
    state.rng = rng;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(x0: f32, size: f32) -> [[Vec3; 3]; 2] {
        let p = |x: f32, z: f32| Vec3::new(x0 + x, 1.0, z);
        [[p(0., 0.), p(size, 0.), p(size, size)], [p(0., 0.), p(size, size), p(0., size)]]
    }

    #[test]
    fn separate_water_is_separate_bodies_with_samples() {
        let mut triangles = quad(0.0, 10.0).to_vec();
        triangles.extend(quad(500.0, 200.0));
        let mut found = bodies(&triangles);
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(found.len(), 2);
        assert!((found[0].0 - 100.0).abs() < 0.01);
        assert!((found[1].0 - 40_000.0).abs() < 1.0);
        assert_eq!(Kind::of(found[0].0), Kind::Fountain);
        assert_eq!(Kind::of(found[1].0), Kind::Lake);
        assert!(!found[0].1.is_empty() && !found[1].1.is_empty());
    }

    #[test]
    fn fade_is_full_near_and_silent_at_reach() {
        assert_eq!(fade(0.0, 40.0), 1.0);
        assert_eq!(fade(40.0, 40.0), 0.0);
        assert_eq!(fade(80.0, 40.0), 0.0);
        assert!(fade(20.0, 40.0) > 0.3 && fade(20.0, 40.0) < 0.4);
    }
}
