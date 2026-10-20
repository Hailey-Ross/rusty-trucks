//! Every drawn actor in the replay: NPC skaters, peds, traffic cars and props are recorded as drawn (root and joint
//! local transforms, after their presenters) and played back from the recording instead of freezing, as retail's
//! DataTracker records the census actors and skaters every frame (b364, `sub_827BF090`). Mods reach it through the
//! components: any entity with a [`ReplayActor`] (and optionally [`ReplayJoints`]) is recorded.
use super::Replay;
use bevy::prelude::*;
use skate_core::replay_buffer::{ActorPose, JointPose};
use std::collections::HashMap;

/// Id tags in the top byte, after `LivingWorldId::to_u64` (0 skater, 1 pedestrian, 2 vehicle).
pub(crate) const PROP_TAG: u64 = 3 << 56;

/// A recorded actor: a stable id (network and mod safe) for its track.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReplayActor {
    pub id: u64,
}

/// The joints recorded with the actor's root, in a fixed order (their local transforms).
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub(crate) struct ReplayJoints(pub Vec<Entity>);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<LiveState>().add_systems(
        PostUpdate,
        (tag_actors, refresh_joints, record, play_back).chain().before(bevy::transform::TransformSystems::Propagate),
    );
}

fn tag_actors(
    mut commands: Commands,
    npcs: Query<(Entity, &crate::living_world::npc_skaters::NpcSkater), Without<ReplayActor>>,
    peds: Query<(Entity, &crate::living_world::peds::Pedestrian), Without<ReplayActor>>,
    cars: Query<(Entity, &crate::living_world::vehicles::TrafficCar), Without<ReplayActor>>,
    props: Query<(Entity, &crate::skate_world::PropInstance), Without<ReplayActor>>,
) {
    let ids = npcs.iter().map(|(e, n)| (e, n.id.to_u64()))
        .chain(peds.iter().map(|(e, p)| (e, p.id.to_u64())))
        .chain(cars.iter().map(|(e, c)| (e, c.id.to_u64())))
        .chain(props.iter().map(|(e, p)| (e, PROP_TAG | u64::from(p.id))));
    for (entity, id) in ids {
        commands.entity(entity).insert(ReplayActor { id });
    }
}

/// Joint lists from each kind's bindings (they change when a look loads).
fn refresh_joints(
    mut commands: Commands,
    npcs: Query<(Entity, &crate::living_world::npc_skaters::NpcPuppet, Option<&ReplayJoints>)>,
    peds: Query<(Entity, &crate::living_world::peds::PedPuppet, Option<&ReplayJoints>)>,
    cars: Query<(Entity, &crate::living_world::vehicles::CarLook, Option<&ReplayJoints>)>,
) {
    let lists = npcs.iter().map(|(e, p, j)| (e, p.joints(), j))
        .chain(peds.iter().map(|(e, p, j)| (e, p.bindings.as_ref().map(|b| b.joints()).unwrap_or_default(), j)))
        .chain(cars.iter().map(|(e, c, j)| (e, c.wheel_joints(), j)));
    for (entity, joints, current) in lists {
        if current.is_none_or(|c| c.0 != joints) {
            commands.entity(entity).insert(ReplayJoints(joints));
        }
    }
}

fn joint(t: &Transform) -> JointPose {
    JointPose { translation: t.translation.to_array(), rotation: t.rotation.to_array(), scale: t.scale.to_array() }
}

fn transform(p: &JointPose) -> Transform {
    Transform {
        translation: Vec3::from_array(p.translation),
        rotation: Quat::from_array(p.rotation),
        scale: Vec3::from_array(p.scale),
    }
}

/// One frame per new player frame (one per sim step), after every presenter drew this frame.
#[allow(clippy::type_complexity)]
fn record(
    mut replay: ResMut<Replay>,
    mut ids: Local<HashMap<Entity, u64>>,
    mut gone: Local<Vec<u64>>,
    mut removed: RemovedComponents<ReplayActor>,
    actors: Query<(Entity, Ref<ReplayActor>, Ref<Transform>, Option<&ReplayJoints>, Option<Ref<Visibility>>)>,
    joints: Query<&Transform>,
) {
    gone.extend(removed.read().filter_map(|e| ids.remove(&e)));
    if replay.active {
        return;
    }
    let Some(now) = replay.frames.back().map(|f| f.time) else { return };
    if replay.world_time == Some(now) {
        return;
    }
    replay.world_time = Some(now);
    replay.world.begin_frame(now);
    for id in gone.drain(..) {
        replay.world.end(id);
    }
    for (entity, actor, root, list, visibility) in &actors {
        // A jointless actor (a prop) whose root and visibility did not change keeps its last key.
        let changed = actor.is_added() || root.is_changed() || visibility.as_ref().is_some_and(|v| v.is_changed());
        if list.is_none() && !changed {
            continue;
        }
        ids.insert(entity, actor.id);
        let visibility = visibility.map(|v| *v);
        let pose = ActorPose {
            root: joint(&root),
            joints: list.map(|l| l.0.iter().filter_map(|j| joints.get(*j).ok()).map(joint).collect()).unwrap_or_default(),
            hidden: visibility == Some(Visibility::Hidden),
        };
        replay.world.record(actor.id, &pose);
    }
}

/// Live transforms and visibility of every actor the playback touched, put back on exit.
#[derive(Resource, Default)]
struct LiveState {
    saved: Option<HashMap<Entity, (Transform, Option<Visibility>)>>,
}

fn play_back(
    replay: Res<Replay>,
    mut live: ResMut<LiveState>,
    actors: Query<(Entity, &ReplayActor, Option<&ReplayJoints>)>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut commands: Commands,
) {
    if !replay.active {
        for (entity, (t, v)) in live.saved.take().into_iter().flatten() {
            if let Ok(mut current) = transforms.get_mut(entity) {
                *current = t;
            }
            match (v, visibility.get_mut(entity)) {
                (Some(v), Ok(mut current)) => *current = v,
                (None, Ok(_)) => {
                    commands.entity(entity).remove::<Visibility>();
                }
                _ => {}
            }
        }
        return;
    }
    let saved = live.saved.get_or_insert_with(HashMap::new);
    let keep = |saved: &mut HashMap<_, _>, entity: Entity, transforms: &Query<&mut Transform>, visibility: &Query<&mut Visibility>| {
        *saved.entry(entity).or_insert_with(|| {
            (transforms.get(entity).copied().unwrap_or_default(), visibility.get(entity).ok().copied())
        })
    };
    let time = replay.cursor;
    for (entity, actor, list) in &actors {
        let (_, live_visibility) = keep(saved, entity, &transforms, &visibility);
        let sample = replay.world.sample(actor.id, time);
        let shown = sample.as_ref().is_some_and(|p| !p.hidden);
        if let Ok(mut v) = visibility.get_mut(entity) {
            *v = if shown { live_visibility.filter(|v| *v != Visibility::Hidden).unwrap_or(Visibility::Inherited) } else { Visibility::Hidden };
        } else if !shown {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        let Some(pose) = sample else { continue };
        if let Ok(mut t) = transforms.get_mut(entity) {
            *t = transform(&pose.root);
        }
        let Some(list) = list.filter(|l| l.0.len() == pose.joints.len()) else { continue };
        for (j, p) in list.0.iter().zip(&pose.joints) {
            keep(saved, *j, &transforms, &visibility);
            if let Ok(mut t) = transforms.get_mut(*j) {
                *t = transform(p);
            }
        }
    }
}
