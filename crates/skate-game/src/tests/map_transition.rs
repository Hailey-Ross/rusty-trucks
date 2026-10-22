//! Construction/lifetime checks only. Never advance physics or drive gameplay.
use super::*;

#[test]
fn a_pending_request_cannot_be_replaced_by_another_menu_action() {
    let mut transition = MapTransition::default();
    transition.request(Entry { label: "first".into(), path: None });
    transition.request(Entry { label: "second".into(), path: Some("other.skate".into()) });
    assert!(transition.busy());
    assert!(matches!(&transition.phase, Phase::Requested(entry) if entry.label == "first"));
}

#[test]
fn a_failed_change_reloads_the_live_map_once() {
    let live = || Some(Entry { label: "DownTown".into(), path: Some("maps/DownTown.skate".into()) });
    let mut restoring = None;
    let reload = after_failure(&mut restoring, live(), "University", "bad block".into()).unwrap();
    assert_eq!(reload.label, "DownTown");
    assert_eq!(reload.path.as_deref(), Some(std::path::Path::new("maps/DownTown.skate")));
    assert_eq!(restoring.as_deref(), Some("Could not load University: bad block"));
    // Reloading the live map failed as well: nothing is left to go back to.
    let message = after_failure(&mut restoring, live(), "DownTown", "also bad".into()).unwrap_err();
    assert_eq!(message, "Could not load University: bad block\nReloading DownTown failed too: also bad");
    assert!(restoring.is_none());
}

#[test]
fn a_failed_start_map_with_nothing_to_go_back_to_leaves_the_error() {
    // The Switch's empty start scene: no live map, and no test world to reload instead.
    let mut restoring = None;
    let message = after_failure(&mut restoring, None, "DownTown", "bad block".into()).unwrap_err();
    assert_eq!(message, "Could not load DownTown: bad block");
    assert!(restoring.is_none());
}

#[test]
#[ignore = "requires private installed assets; CPU lifecycle only, no gameplay"]
fn installed_worlds_prepare_commit_and_retire_without_simulation() {
    let root = PathBuf::from(std::env::var("SKATE_TRANSITION_TEST_ASSETS").unwrap()).canonicalize().unwrap();
    let manifest = skate_data::GameAssets::load(&root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(&root, &manifest).unwrap();
    let difficulty = crate::difficulty::Difficulty::Hardcore;
    let physics = GamePhysics::load_with_difficulty(&root, None, difficulty).unwrap();
    let skater = SkaterRuntime::load(&root, &graphs, &physics, difficulty.key()).unwrap();
    let source = skater.animation.source.clone();
    let evaluator = skater.animation.evaluator.clone();
    let mut controls = PlayerControls::load(&root).unwrap();
    controls.preferences = skate_core::input::riding_intentions::PushPreferences {
        automatic_push_enabled: true, automatic_push_right: true
    };
    let mut world = World::new();
    world.insert_resource(Config { asset_root: root.clone(), verification_capture: None,
        map: None, map_path: None, boot_map: None, difficulty, check_assets: false, validate_maps: false, start_paused: false, teleport: None, mute: false, multiplayer: Default::default(), map_fingerprint: crate::config::map_fingerprint(None).unwrap() });
    world.insert_resource(CurrentMap::from_package(None, None));
    world.insert_resource(graphs);
    world.insert_resource(physics);
    world.insert_resource(skater);
    world.insert_resource(controls);
    world.init_resource::<ButtonInput<KeyCode>>();
    world.init_resource::<ButtonInput<MouseButton>>();
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<Assets<StandardMaterial>>();
    world.init_resource::<Assets<crate::retail_render::WorldMaterial>>();
    world.init_resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>();
    world.init_resource::<Assets<crate::retail_sky::SkyMaterial>>();
    let character = world.spawn((crate::world::PlayerRoot, Transform::default())).id();
    let mut initial = PreparedScene::new(&world);
    initial.prepare(None, &root);
    initial.publish(&mut world);
    let procedural_meshes = world.resource::<Assets<Mesh>>().len();
    let procedural_entities = world.query_filtered::<Entity, With<crate::map_render::MapEntity>>().iter(&world).count();
    let course_triangles = world.resource::<GamePhysics>().world_triangles().len();

    for (iteration, file) in [Some("BlackBoxPark.skate"), None, Some("University.skate"), None,
        Some("BlackBoxPark.skate"), None].into_iter().enumerate() {
        let entry = Entry { label: file.unwrap_or("Test world").into(),
            path: file.map(|name| root.parent().unwrap().join("maps").join(name)) };
        let old: Vec<_> = world.query_filtered::<Entity, With<crate::map_render::MapEntity>>().iter(&world).collect();
        world.resource_mut::<GamePhysics>().contact_count = 123;
        world.resource_mut::<SkaterRuntime>().pose_generation = 999;
        let Phase::Loading { job, .. } = start(&mut world, entry).unwrap() else { panic!("expected worker"); };
        let prepared = job.join().unwrap().unwrap();
        // The transition releases the live world's visuals before reading the new
        // map, so the new map's decode and pages never stack on the old world
        // (doc 58 §3.3). Preparation itself must still add nothing to the live
        // world: it stages against reserved handles and only inserts on publish.
        assert!(old.iter().all(|&id| world.get_entity(id).is_err()), "the transition should have retired the live world's visuals before loading");
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 0, "preparation inserted into the live world's assets");
        // The live map's collision goes too, for the test course's, while its
        // simulation state stays as it was.
        assert_eq!(world.resource::<GamePhysics>().world_triangles().len(), course_triangles,
            "the transition should have released the live map's collision before loading");
        assert_eq!(world.resource::<GamePhysics>().contact_count, 123);
        let triangles = prepared.physics.world_triangles().len();
        let spawn = prepared.metadata.spawn;
        assert!(Arc::ptr_eq(&prepared.skater.animation.source, &source));
        assert!(Arc::ptr_eq(&prepared.skater.animation.evaluator, &evaluator));
        commit(&mut world, prepared);
        assert!(old.into_iter().all(|id| world.get_entity(id).is_err()));
        assert!(world.get_entity(character).is_ok());
        assert_eq!(world.resource::<CurrentMap>().generation, iteration as u64 + 1);
        assert_eq!(world.resource::<CurrentMap>().spawn, spawn);
        let physics = world.resource::<GamePhysics>();
        assert_eq!(physics.world_triangles().len(), triangles);
        assert_eq!(physics.contact_count, 0);
        assert_eq!(physics.ticks, 0);
        assert!(!physics.failed);
        assert_eq!(physics.difficulty_index(), difficulty as u32);
        assert_eq!(world.resource::<SkaterRuntime>().pose_generation, 0);
        assert_eq!(world.resource::<PlayerControls>().ticks, 0);
        assert!(world.resource::<PlayerControls>().preferences.automatic_push_enabled);
        assert!(world.resource::<PlayerControls>().preferences.automatic_push_right);
        assert!(world.resource::<crate::presentation::Presentation>().pair().is_none());
        assert!(!world.resource::<crate::replay::Replay>().active);
        assert!(world.resource::<crate::camera::CameraRuntime>().frame.is_none());
        if file.is_none() {
            assert_eq!(world.resource::<Assets<Mesh>>().len(), procedural_meshes);
            assert_eq!(world.resource::<Assets<Image>>().len(), 0);
            assert_eq!(world.resource::<Assets<crate::retail_render::WorldMaterial>>().len(), 0);
            // Material tables are storage buffers now, so they are part of what
            // retirement has to release (RFC 1 D2).
            assert_eq!(world.resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>().len(), 0);
            assert_eq!(world.query_filtered::<Entity, With<crate::map_render::MapEntity>>().iter(&world).count(), procedural_entities);
        }
        eprintln!("LIFECYCLE_CHECK generation={} map={file:?} triangles={triangles} entities={} meshes={} images={}",
            iteration + 1, world.query_filtered::<Entity, With<crate::map_render::MapEntity>>().iter(&world).count(), world.resource::<Assets<Mesh>>().len(), world.resource::<Assets<Image>>().len());
    }
    let missing = Entry { label: "missing".into(), path: Some(root.join("nonexistent-transition-check.skate")) };
    let Phase::Loading { job, .. } = start(&mut world, missing).unwrap() else { panic!("expected worker"); };
    assert!(job.join().unwrap().is_err());
    assert_eq!(world.resource::<CurrentMap>().generation, 6);
    // The old world's visuals and collision were already released to make room
    // for the map that failed; `poll` then reloads the map that was live. Driven
    // directly like this, the character and the generation are what's left.
    assert_eq!(world.resource::<Assets<Mesh>>().len(), 0);
    assert_eq!(world.resource::<GamePhysics>().world_triangles().len(), course_triangles);
    assert!(world.get_entity(character).is_ok());
}

/// Throwaway measurement, not a permanent test: the CPU-side peak of a University to
/// DownTown map change through the real transition worker, with the progressive texture
/// release in place. RSS is sampled from a side thread every 50 ms.
#[test]
#[ignore = "requires private installed assets; CPU lifecycle only, no gameplay"]
fn measure_university_to_downtown_peak() {
    let root = PathBuf::from(std::env::var("SKATE_TRANSITION_TEST_ASSETS").unwrap()).canonicalize().unwrap();
    let manifest = skate_data::GameAssets::load(&root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(&root, &manifest).unwrap();
    let difficulty = crate::difficulty::Difficulty::Hardcore;
    let physics = GamePhysics::load_with_difficulty(&root, None, difficulty).unwrap();
    let skater = SkaterRuntime::load(&root, &graphs, &physics, difficulty.key()).unwrap();
    let mut controls = PlayerControls::load(&root).unwrap();
    let mut world = World::new();
    world.insert_resource(Config { asset_root: root.clone(), verification_capture: None,
        map: None, map_path: None, boot_map: None, difficulty, check_assets: false, validate_maps: false, start_paused: false, teleport: None, mute: false, multiplayer: Default::default(), map_fingerprint: crate::config::map_fingerprint(None).unwrap() });
    world.insert_resource(CurrentMap::from_package(None, None));
    world.insert_resource(graphs);
    world.insert_resource(physics);
    world.insert_resource(skater);
    world.insert_resource(controls);
    world.init_resource::<ButtonInput<KeyCode>>();
    world.init_resource::<ButtonInput<MouseButton>>();
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<Assets<StandardMaterial>>();
    world.init_resource::<Assets<crate::retail_render::WorldMaterial>>();
    world.init_resource::<Assets<bevy::render::storage::ShaderStorageBuffer>>();
    world.init_resource::<Assets<crate::retail_sky::SkyMaterial>>();
    world.spawn((crate::world::PlayerRoot, Transform::default()));
    let mut initial = PreparedScene::new(&world);
    initial.prepare(None, &root);
    initial.publish(&mut world);

    let rss = || -> usize {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        status.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()).unwrap_or(0)
    };
    let mib = |kib: usize| kib as f64 / 1024.0;

    // Cold-boot University, the live world the map change stacks onto.
    let entry = Entry { label: "University".into(), path: Some(root.parent().unwrap().join("maps").join("University.skate")) };
    let Phase::Loading { job, .. } = start(&mut world, entry).unwrap() else { panic!("expected worker") };
    commit(&mut world, job.join().unwrap().unwrap());
    let live = rss();
    eprintln!("PROBE live University RSS={:.1} MiB", mib(live));

    // Sample RSS while the DownTown transition runs.
    let peak = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(live));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = {
        let (peak, stop) = (peak.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let r = rss();
                peak.fetch_max(r, std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
    };
    let entry = Entry { label: "DownTown".into(), path: Some(root.parent().unwrap().join("maps").join("DownTown.skate")) };
    let Phase::Loading { job, .. } = start(&mut world, entry).unwrap() else { panic!("expected worker") };
    let prepared = job.join().unwrap().unwrap();
    let after_prepare = rss();
    commit(&mut world, prepared);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    sampler.join().unwrap();
    let after_commit = rss();
    eprintln!("PROBE university_to_downtown after_prepare={:.1} MiB after_commit={:.1} MiB peak={:.1} MiB (live was {:.1})",
        mib(after_prepare), mib(after_commit), mib(peak.load(std::sync::atomic::Ordering::Relaxed)), mib(live));
}
