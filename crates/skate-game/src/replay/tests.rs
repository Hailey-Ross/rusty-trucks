use super::*;

fn pose(x: f32) -> Snapshot {
    Snapshot {
        root: Transform::from_xyz(x, 0., 0.),
        bones: vec![Mat4::from_translation(Vec3::X * x)],
        camera: Transform::from_xyz(x, 2., 3.),
        fov: 1.0,
    }
}

#[test]
fn rolling_window_is_the_retail_ninety_seconds_with_variable_cadence() {
    let mut replay = Replay::default();
    for i in 0..4000 {
        replay.record(
            pose(i as f32),
            if i % 2 == 0 { 1. / 60. } else { 1. / 30. },
            false,
        );
    }
    let (start, end) = replay.bounds();
    // 5400 frames at retail's 60 per second (b345).
    assert!((end - start - 90.).abs() < 1e-6);
    assert!(replay.frames.len() <= 3602);
    replay.enter();
    replay.seek(-100.);
    assert_eq!(replay.cursor, start);
    replay.seek(1000.);
    assert_eq!(replay.cursor, end);
}

#[test]
fn scrubbing_interpolates_shared_pose_and_camera_without_recording() {
    let mut replay = Replay::default();
    replay.enter();
    assert!(!replay.active);
    replay.record(pose(0.), 0.1, false);
    replay.record(pose(1.), 0.1, false);
    replay.enter();
    replay.record(pose(99.), 0.1, false);
    assert_eq!(replay.frames.len(), 2);
    replay.seek(0.05);
    let (a, b, alpha) = replay.sample().unwrap();
    assert_eq!(alpha, 0.5);
    assert_eq!(blend(a.root, b.root, alpha).translation.x, 0.5);
    assert_eq!(blend(a.camera, b.camera, alpha).translation.x, 0.5);
    replay.toggle_camera();
    let free = replay.free_camera.unwrap();
    replay.seek(0.1);
    assert_eq!(replay.free_camera.unwrap(), free);
    replay.toggle_camera();
    assert!(replay.free_camera.is_none());
    replay.exit();
    assert!(replay.sample().is_none());
    replay.record(pose(2.), 0.1, false);
    assert_eq!(replay.frames.back().unwrap().pose.root.translation.x, 2.);
}

#[test]
fn frame_steps_and_cuts_do_not_blend_through_teleports() {
    let mut replay = Replay::default();
    replay.record(pose(0.), 0.1, false);
    replay.record(pose(100.), 0.1, true);
    replay.record(pose(101.), 0.1, false);
    replay.enter();
    replay.seek(0.05);
    let (a, b, _) = replay.sample().unwrap();
    assert_eq!(a.root, b.root);
    assert_eq!(a.root.translation.x, 0.);
    replay.step(1);
    assert_eq!(replay.sample().unwrap().0.root.translation.x, 100.);
    replay.step(-1);
    assert_eq!(replay.cursor, 0.);
}

#[test]
fn replay_gates_all_fixed_gameplay_sets() {
    #[derive(Resource, Default)]
    struct Counts([usize; 3]);
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<Counts>()
        .configure_sets(
            FixedUpdate,
            (
                SimulationSet::Input,
                SimulationSet::Controls,
                SimulationSet::Physics,
            )
                .run_if(live),
        )
        .add_systems(
            FixedUpdate,
            (
                (|mut c: ResMut<Counts>| c.0[0] += 1).in_set(SimulationSet::Input),
                (|mut c: ResMut<Counts>| c.0[1] += 1).in_set(SimulationSet::Controls),
                (|mut c: ResMut<Counts>| c.0[2] += 1).in_set(SimulationSet::Physics),
            ),
        );
    app.world_mut().run_schedule(FixedUpdate);
    app.world_mut().resource_mut::<Replay>().active = true;
    for _ in 0..120 {
        app.world_mut().run_schedule(FixedUpdate);
    }
    assert_eq!(app.world().resource::<Counts>().0, [1; 3]);
    app.world_mut().resource_mut::<Replay>().exit();
    app.world_mut().run_schedule(FixedUpdate);
    assert_eq!(app.world().resource::<Counts>().0, [2; 3]);
}

#[test]
fn controller_replay_scrub_camera_and_exit_do_not_leak_gameplay_input() {
    use crate::input::ControllerInput;
    use skate_core::input::xbox::XboxState;
    let mut app = App::new();
    app.init_resource::<Replay>()
        .init_resource::<ControllerInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .init_resource::<Time<Real>>()
        .add_systems(Update, controls);
    for i in 0..121 {
        app.world_mut()
            .resource_mut::<Replay>()
            .record(pose(i as f32), 1. / 60., false);
    }
    let tick = |app: &mut App, buttons, triggers, left| {
        app.world_mut()
            .resource_mut::<ControllerInput>()
            .sample_raw_for_test(XboxState {
                buttons,
                triggers,
                left,
                right: [0; 2],
            });
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs_f64(1. / 60.));
        app.world_mut().run_schedule(Update);
    };
    tick(&mut app, SELECT, [0; 2], [0; 2]);
    assert!(app.world().resource::<Replay>().active);
    tick(&mut app, SELECT, [0; 2], [0; 2]);
    assert!(
        app.world().resource::<Replay>().active,
        "held View must not toggle repeatedly"
    );
    let end = app.world().resource::<Replay>().cursor;
    tick(&mut app, 0, [255, 0], [0; 2]);
    let rewound = app.world().resource::<Replay>().cursor;
    assert!(rewound < end);
    tick(&mut app, Y, [0; 2], [0; 2]);
    let camera = app.world().resource::<Replay>().free_camera.unwrap();
    tick(&mut app, 0, [0, 128], [32767, 0]);
    let replay = app.world().resource::<Replay>();
    assert!(replay.cursor > rewound);
    assert_ne!(replay.free_camera.unwrap(), camera);
    tick(&mut app, B, [255, 0], [32767, 0]);
    assert!(!app.world().resource::<Replay>().active);
    assert_eq!(
        app.world().resource::<ControllerInput>().mapped_actions,
        [[0.; 18]; 4]
    );
    tick(&mut app, 0, [255, 0], [32767, 0]);
    assert!(app.world().resource::<Replay>().release_controls);
    assert_eq!(
        app.world().resource::<ControllerInput>().mapped_actions,
        [[0.; 18]; 4]
    );
    tick(&mut app, 0, [0; 2], [0; 2]);
    assert!(!app.world().resource::<Replay>().release_controls);
    tick(&mut app, A, [0; 2], [0; 2]);
    assert_ne!(
        app.world().resource::<ControllerInput>().mapped_actions,
        [[0.; 18]; 4]
    );
}

#[test]
fn world_actors_play_back_from_the_recording_and_return_to_live_on_exit() {
    use super::world::ReplayActor;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    let joint = app.world_mut().spawn(Transform::from_xyz(0., 1., 0.)).id();
    let car = app.world_mut().spawn((ReplayActor { id: 9 }, super::world::ReplayJoints(vec![joint]),
        Transform::from_xyz(0., 0., 0.), Visibility::Inherited)).id();
    let step = |app: &mut App, x: f32| {
        app.world_mut().resource_mut::<Replay>().record(pose(x), 1. / 60., false);
        app.update();
    };
    step(&mut app, 0.);
    app.world_mut().entity_mut(car).insert(Transform::from_xyz(2., 0., 0.));
    app.world_mut().entity_mut(joint).insert(Transform::from_xyz(0., 1., 0.).with_rotation(Quat::from_rotation_x(1.0)));
    step(&mut app, 1.);
    // An actor that appeared after the first frame is not drawn before it.
    let late = app.world_mut().spawn((ReplayActor { id: 10 }, Transform::from_xyz(5., 0., 0.), Visibility::Inherited)).id();
    step(&mut app, 2.);
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(0.5 / 60.);
    }
    app.update();
    let world = app.world();
    assert!((world.get::<Transform>(car).unwrap().translation.x - 1.).abs() < 1e-4);
    let angle = world.get::<Transform>(joint).unwrap().rotation.angle_between(Quat::from_rotation_x(0.5));
    assert!(angle < 2e-3, "{angle}");
    assert_eq!(world.get::<Visibility>(late), Some(&Visibility::Hidden));
    assert_eq!(world.get::<Visibility>(car), Some(&Visibility::Inherited));
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    let world = app.world();
    assert_eq!(world.get::<Transform>(car).unwrap().translation.x, 2.);
    assert_eq!(world.get::<Visibility>(late), Some(&Visibility::Inherited));
}

#[test]
fn mod_markers_and_limits_are_owned_and_go_with_their_mod() {
    let mut replay = Replay::default();
    assert!(replay.add_marker("a", "early".into()).is_err(), "nothing recorded yet");
    replay.record(pose(0.), 1. / 60., false);
    replay.add_marker("a", "kickflip".into()).unwrap();
    replay.add_marker("b", "bail".into()).unwrap();
    for _ in 2..MAX_MARKERS {
        replay.add_marker("b", "x".into()).unwrap();
    }
    assert!(replay.add_marker("a", "one too many".into()).is_err());
    replay.set_limits("a", Some(1800), None);
    assert_eq!(replay.world.limits.max_frames, 1800);
    assert!((replay.window() - 30.).abs() < 1e-9);
    // Another mod's reset does not drop a's limits; a's disable does.
    replay.set_limits("b", None, None);
    assert_eq!(replay.world.limits.max_frames, 1800);
    replay.clear_owner("a");
    assert_eq!(replay.world.limits.max_frames, 5400);
    let state = replay.state();
    assert_eq!(state["markers"].as_array().unwrap().len(), MAX_MARKERS - 1);
    assert_eq!(state["max_frames"], 5400);
    replay.clear_mods();
    assert_eq!(replay.state()["markers"].as_array().unwrap().len(), 0);
    // Markers leave with the window.
    replay.add_marker("a", "old".into()).unwrap();
    for _ in 0..(91 * 60) {
        replay.record(pose(1.), 1. / 60., false);
    }
    assert_eq!(replay.state()["markers"].as_array().unwrap().len(), 0);
}

#[test]
fn a_despawned_actor_plays_back_as_a_ghost_until_it_leaves_the_window() {
    use super::world::{ReplayActor, ReplayGhost, ReplayJoints};
    use bevy::ecs::system::RunSystemOnce;
    let mut app = App::new();
    app.init_resource::<Replay>();
    app.world_mut().resource_mut::<Replay>().world.limits.max_frames = 60;
    super::world::plugin(&mut app);
    let ped = app.world_mut().spawn((Transform::from_xyz(3., 0., 0.), Visibility::Inherited)).id();
    let scene = app.world_mut().spawn((ChildOf(ped), Transform::default())).id();
    let joint = app.world_mut().spawn((ChildOf(scene), Transform::from_xyz(0., 1., 0.))).id();
    app.world_mut().entity_mut(ped).insert((ReplayActor { id: 1 << 56 | 5 }, ReplayJoints(vec![joint])));
    let step = |app: &mut App| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        app.update();
    };
    step(&mut app);
    step(&mut app);
    app.world_mut().run_system_once(move |mut commands: Commands| super::world::retire(&mut commands, ped)).unwrap();
    assert!(app.world().get_entity(ped).is_err());
    let ghost = app.world_mut().query_filtered::<Entity, With<ReplayGhost>>().single(app.world()).unwrap();
    assert_eq!(app.world().get::<ChildOf>(scene).unwrap().parent(), ghost);
    step(&mut app);
    step(&mut app);
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(0.);
    }
    app.update();
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<Transform>(ghost).unwrap().translation.x, 3.);
    app.world_mut().resource_mut::<Replay>().seek(3. / 60.);
    app.update();
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden));
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    assert_eq!(app.world().get::<Visibility>(ghost), Some(&Visibility::Hidden), "hidden while live");
    for _ in 0..200 {
        step(&mut app);
    }
    assert!(app.world().get_entity(ghost).is_err() && app.world().get_entity(joint).is_err());
}

#[test]
fn playback_publishes_the_recorded_audio_steps_the_cursor_crosses() {
    use crate::game_audio::skate_events::Riding;
    let mut replay = Replay::default();
    for i in 0..30 {
        replay.record(pose(i as f32), 1. / 60., false);
        let mut riding = Riding { speed: i as f32, ..Riding::default() };
        riding.audio.push_trigger = i == 3;
        replay.attach_riding(riding);
        // A second attach in the same frame does not replace the frame's step.
        replay.attach_riding(Riding { speed: -1., ..Riding::default() });
    }
    replay.enter();
    replay.seek(0.);
    assert!(replay.audio_steps().is_empty(), "entering publishes nothing");
    assert!(replay.jumped() && !replay.audible());
    replay.seek(4. / 60.);
    let steps = replay.audio_steps();
    assert_eq!(steps.iter().map(|r| r.speed).collect::<Vec<_>>(), [1., 2., 3., 4.]);
    assert!(steps[2].audio.push_trigger && replay.audible() && !replay.jumped());
    assert!(replay.audio_steps().is_empty() && !replay.audible(), "paused is silent");
    // Rewind: the step at the cursor, without its push.
    replay.seek(3. / 60.);
    let back = replay.audio_steps();
    assert_eq!(back.len(), 1);
    assert!(back[0].speed == 3. && !back[0].audio.push_trigger && replay.jumped() && !replay.audible());
    // A fast scrub keeps the newest steps and the skipped push.
    replay.seek(2. / 60.);
    replay.audio_steps();
    replay.seek(20. / 60.);
    let fast = replay.audio_steps();
    assert_eq!(fast.len(), MAX_FED_STEPS);
    assert_eq!(fast.last().unwrap().speed, 20.);
    assert!(fast[0].audio.push_trigger && replay.jumped());
    replay.exit();
    assert!(replay.audio_steps().is_empty() && !replay.audible());
}

#[test]
fn world_actor_audio_state_follows_the_recording_and_returns_on_exit() {
    use super::world::ReplayActor;
    use crate::world_audio::AudioVelocity;
    let mut app = App::new();
    app.init_resource::<Replay>();
    super::world::plugin(&mut app);
    super::components::track::<AudioVelocity>(&mut app);
    let car = app.world_mut().spawn((ReplayActor { id: 2 << 56 | 7 }, Transform::default(), AudioVelocity(Vec3::X))).id();
    let step = |app: &mut App| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        app.update();
    };
    step(&mut app);
    step(&mut app);
    app.world_mut().entity_mut(car).insert(AudioVelocity(Vec3::Y * 5.));
    step(&mut app);
    // Unchanged values are not stored again.
    assert_eq!(super::components::value_at::<AudioVelocity>(app.world(), 2 << 56 | 7, 1.5 / 60.), Some(AudioVelocity(Vec3::X)));
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(1. / 60.);
    }
    app.update();
    assert_eq!(app.world().get::<AudioVelocity>(car), Some(&AudioVelocity(Vec3::X)));
    app.world_mut().resource_mut::<Replay>().seek(2. / 60.);
    app.update();
    assert_eq!(app.world().get::<AudioVelocity>(car), Some(&AudioVelocity(Vec3::Y * 5.)));
    app.world_mut().resource_mut::<Replay>().seek(0.);
    app.update();
    app.world_mut().resource_mut::<Replay>().exit();
    app.update();
    assert_eq!(app.world().get::<AudioVelocity>(car), Some(&AudioVelocity(Vec3::Y * 5.)), "live value back");
}

#[test]
fn one_shot_sounds_fire_again_when_playback_crosses_them() {
    use crate::world_audio::VehicleHorn;
    #[derive(Resource, Default)]
    struct Heard(Vec<u8>);
    let mut app = App::new();
    app.init_resource::<Replay>().init_resource::<Heard>().add_message::<VehicleHorn>();
    super::messages::track::<VehicleHorn>(&mut app);
    app.add_systems(Update, |mut r: MessageReader<VehicleHorn>, mut heard: ResMut<Heard>| {
        heard.0.extend(r.read().map(|h| h.kind));
    });
    let step = |app: &mut App, horn: Option<u8>| {
        app.world_mut().resource_mut::<Replay>().record(pose(0.), 1. / 60., false);
        if let Some(kind) = horn {
            app.world_mut().write_message(VehicleHorn { vehicle: Entity::PLACEHOLDER, kind, seconds: 1. });
        }
        app.update();
    };
    step(&mut app, None);
    step(&mut app, Some(4));
    step(&mut app, None);
    app.world_mut().resource_mut::<Heard>().0.clear();
    {
        let mut replay = app.world_mut().resource_mut::<Replay>();
        replay.enter();
        replay.seek(0.);
    }
    app.update();
    app.world_mut().resource_mut::<Replay>().seek(2. / 60.);
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Heard>().0, [4], "once, when the cursor passed it");
    app.world_mut().resource_mut::<Replay>().seek(0.);
    app.update();
    assert_eq!(app.world().resource::<Heard>().0, [4], "not on a rewind");
}

#[test]
fn free_camera_stays_within_retail_forty_metres_of_the_player() {
    let player = Vec3::new(10., 0., 0.);
    assert_eq!(leashed(Vec3::new(20., 0., 0.), player, FREE_CAMERA_LEASH), Vec3::new(20., 0., 0.));
    let far = leashed(Vec3::new(110., 0., 0.), player, FREE_CAMERA_LEASH);
    assert!((far - Vec3::new(50., 0., 0.)).length() < 1e-4);
    assert_eq!(Replay::default().free_camera_leash.unwrap_or(FREE_CAMERA_LEASH), 40.);
}
