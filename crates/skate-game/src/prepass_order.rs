//! `SKATE_PREPASS_ORDER=1`: the view's opaque depth prepass draws the world's leaves nearest
//! first (render_options.rs). Bevy keeps a phase's bins from frame to frame, in the order they
//! were filled, sorts only its batch sets (bevy_render `sort_binned_render_phase`), and draws
//! each batch set as one multi-draw, its bins in order (`batch_and_prepare_binned_render_phase`).
//! Each frame, once Bevy has sorted the sets, this puts every set's bins in order of the
//! distance from the view to each leaf's box, and when the frame is drawn puts them back, so
//! Bevy's own order never changes for good and a step without the option draws as Bevy would.
//! Only the bins' order changes: the same draws, in the same multi-draws.
use bevy::asset::UntypedAssetId;
use bevy::camera::primitives::Aabb;
use bevy::core_pipeline::prepass::Opaque3dPrepass;
use bevy::math::Vec3A;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::batching::sort_binned_render_phase;
use bevy::render::render_phase::ViewBinnedRenderPhases;
use bevy::render::view::{ExtractedView, RetainedViewEntity};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

use crate::render_options::RenderOptions;
use crate::skate_world::LeafTriangles;

pub(crate) struct PrepassOrderPlugin;

impl Plugin for PrepassOrderPlugin {
    fn build(&self, app: &mut App) {
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render
            .init_resource::<LeafBoxes>()
            .init_resource::<Moved>()
            .add_systems(ExtractSchedule, extract_leaf_boxes)
            .add_systems(
                Render,
                (
                    order_prepass
                        .in_set(RenderSystems::PhaseSort)
                        .after(sort_binned_render_phase::<Opaque3dPrepass>),
                    restore_prepass_order.in_set(RenderSystems::Cleanup),
                ),
            );
    }
}

/// The world leaves' boxes by mesh, while `SKATE_PREPASS_ORDER` is on. Each leaf has a mesh of
/// its own and the identity transform (skate_world.rs), so its `Aabb` is in world space.
#[derive(Resource, Default)]
struct LeafBoxes {
    on: bool,
    leaves: usize,
    boxes: HashMap<UntypedAssetId, (Vec3A, Vec3A)>,
    logged: bool,
}

/// The batch sets `order_prepass` reordered this frame, for `restore_prepass_order`, and room
/// for the sort.
#[derive(Resource, Default)]
struct Moved {
    sets: Vec<MovedSet>,
    keys: Vec<(u64, u32)>,
    order: Vec<u32>,
    at: Vec<u32>,
    held: Vec<u32>,
}

struct MovedSet {
    view: RetainedViewEntity,
    /// The set's index among the view's multi-drawable batch sets.
    set: usize,
    /// For each bin, in Bevy's order, where it is now.
    now_at: Vec<u32>,
}

fn extract_leaf_boxes(
    options: Extract<Res<RenderOptions>>,
    leaves: Extract<Query<(&Mesh3d, &Aabb), With<LeafTriangles>>>,
    changed: Extract<Query<(), (With<LeafTriangles>, Or<(Changed<Mesh3d>, Changed<Aabb>)>)>>,
    mut boxes: ResMut<LeafBoxes>,
) {
    if !options.prepass_order {
        if boxes.on {
            *boxes = LeafBoxes::default();
        }
        return;
    }
    boxes.on = true;
    let count = leaves.iter().count();
    if count != boxes.leaves || !changed.is_empty() {
        boxes.leaves = count;
        boxes.boxes.clear();
        boxes.boxes.extend(
            leaves
                .iter()
                .map(|(mesh, aabb)| (UntypedAssetId::from(mesh.0.id()), (aabb.min(), aabb.max()))),
        );
    }
}

/// Orders each batch set's bins by the distance from the view to the leaf's box, 0 inside it,
/// and then to its centre; bins that aren't a world leaf's go last, in Bevy's order.
fn order_prepass(
    mut boxes: ResMut<LeafBoxes>,
    mut moved: ResMut<Moved>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3dPrepass>>,
    views: Query<&ExtractedView>,
) {
    if !boxes.on {
        return;
    }
    let Moved {
        sets,
        keys,
        order,
        at,
        held,
    } = &mut *moved;
    let (mut set_count, mut bin_count, mut known, mut reordered, mut batchable) = (0, 0, 0, 0, 0);
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let eye = view.world_from_view.translation_vec3a();
        set_count += phase.multidrawable_meshes.len();
        batchable += phase.batchable_meshes.len();
        for (set, bins) in phase.multidrawable_meshes.values_mut().enumerate() {
            bin_count += bins.len();
            keys.clear();
            keys.extend(bins.keys().zip(0..).map(|(key, i)| {
                let near = boxes
                    .boxes
                    .get(&key.asset_id)
                    .map_or(u64::MAX, |&(min, max)| {
                        let outside = (min - eye).max(eye - max).max(Vec3A::ZERO);
                        let centre = (min + max) * 0.5 - eye;
                        u64::from(outside.length_squared().to_bits()) << 32
                            | u64::from(centre.length_squared().to_bits())
                    });
                (near, i)
            }));
            known += keys.iter().filter(|(near, _)| *near != u64::MAX).count();
            if keys.is_sorted() {
                continue;
            }
            keys.sort_unstable();
            order.clear();
            order.extend(keys.iter().map(|&(_, i)| i));
            permute(order, at, held, |a, b| bins.swap_indices(a, b));
            let mut now_at = vec![0; order.len()];
            for (i, &from) in order.iter().enumerate() {
                now_at[from as usize] = i as u32;
            }
            sets.push(MovedSet {
                view: view.retained_view_entity,
                set,
                now_at,
            });
            reordered += 1;
        }
    }
    if !boxes.logged && bin_count > 0 && !boxes.boxes.is_empty() {
        boxes.logged = true;
        info!(
            "[Skate] SKATE_PREPASS_ORDER: {} leaf boxes; {bin_count} bins in {set_count} batch \
             sets, {known} with a box, {reordered} sets reordered; {batchable} batchable bins \
             left in Bevy's order",
            boxes.boxes.len()
        );
    }
}

/// Puts back the order `order_prepass` changed, once the frame is drawn.
fn restore_prepass_order(
    mut moved: ResMut<Moved>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3dPrepass>>,
) {
    let Moved { sets, at, held, .. } = &mut *moved;
    for moved_set in sets.drain(..) {
        let Some((_, bins)) = phases
            .get_mut(&moved_set.view)
            .and_then(|phase| phase.multidrawable_meshes.get_index_mut(moved_set.set))
        else {
            continue;
        };
        if bins.len() != moved_set.now_at.len() {
            warn_once!("[Skate] SKATE_PREPASS_ORDER: a batch set changed while drawn; left as is");
            continue;
        }
        permute(&moved_set.now_at, at, held, |a, b| bins.swap_indices(a, b));
    }
}

/// Moves the entry at `order[i]` to `i`, for every `i`, through `swap`; `order` must be a
/// permutation of `0..order.len()`. `at` and `held` are room for it.
fn permute(
    order: &[u32],
    at: &mut Vec<u32>,
    held: &mut Vec<u32>,
    mut swap: impl FnMut(usize, usize),
) {
    let n = order.len() as u32;
    // Where each entry is now, and which entry each place holds.
    at.clear();
    at.extend(0..n);
    held.clear();
    held.extend(0..n);
    for (i, &want) in order.iter().enumerate() {
        let from = at[want as usize] as usize;
        if from != i {
            swap(i, from);
            let displaced = held[i];
            held[i] = want;
            held[from] = displaced;
            at[want as usize] = i as u32;
            at[displaced as usize] = from as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::permute;

    fn apply(items: &mut Vec<char>, order: &[u32]) {
        let (mut at, mut held) = (Vec::new(), Vec::new());
        permute(order, &mut at, &mut held, |a, b| items.swap(a, b));
    }

    #[test]
    fn permute_puts_each_entry_where_the_order_says_and_back() {
        let start: Vec<char> = "abcdefg".chars().collect();
        for order in [
            vec![0, 1, 2, 3, 4, 5, 6],
            vec![6, 5, 4, 3, 2, 1, 0],
            vec![3, 0, 6, 1, 5, 2, 4],
            vec![1, 2, 3, 4, 5, 6, 0],
        ] {
            let mut items = start.clone();
            apply(&mut items, &order);
            let expected: Vec<char> = order.iter().map(|&i| start[i as usize]).collect();
            assert_eq!(items, expected, "{order:?}");
            let mut now_at = vec![0; order.len()];
            for (i, &from) in order.iter().enumerate() {
                now_at[from as usize] = i as u32;
            }
            apply(&mut items, &now_at);
            assert_eq!(items, start, "{order:?}");
        }
    }
}
