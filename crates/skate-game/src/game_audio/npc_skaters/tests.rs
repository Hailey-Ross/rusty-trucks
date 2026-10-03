use std::collections::{BTreeMap, HashMap};

use skate_audio::formats::{Bank, Project};
use skate_audio::mixmap::{MixMap, keys};
use skate_audio::player::AudioState;
use skate_audio::player::collision::CollisionManager;
use skate_audio::player::components::{Command, Slot};
use skate_audio::player::objpos::Listener;
use skate_audio::player::state::material_of_tag;
use skate_audio::runtime::Runtime;
use skate_audio::splice::MIXER_BANK_BASE;
use skate_audio::world::skaters::{AUDIO_RADIUS, NpcSkater, Parts, Slots, Tuning, component_state};

use super::NpcHost;
use super::super::Library;
use super::super::player_audio::{BANKS, OPTIONAL_BANKS, SPLICE_BANKS};

#[test]
fn nothing_published_means_no_work() {
    let host = NpcHost::default();
    let published = super::NpcSkaters::default();
    assert!(published.skaters.is_empty() && host.objects.is_empty() && host.nodes.is_empty());
    assert_eq!(host.slots.holders().count(), 0);
}

fn install() -> Option<(Library, Vec<u8>)> {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let library = Library::load(root).ok()?;
    let mxb = library.aems().mixmap.clone()?;
    let bytes = library.read(&mxb).ok()?;
    Some((library, bytes))
}

/// A runtime as the game builds it for the local player: every project, the boot utilities, the
/// player banks, the optional rolling / rattle banks and the Splice banks; the buses at the
/// default preset.
fn runtime(library: &Library) -> (Runtime, HashMap<usize, &'static str>, Parts) {
    let mut rt = Runtime::new();
    for file in &library.aems().projects {
        rt.install_project(&Project::parse(file, &library.read(file).unwrap()).unwrap());
    }
    let mut names = HashMap::new();
    let mut load = |rt: &mut Runtime, stem: &'static str| -> bool {
        let Some(file) = library.aems().banks.get(stem) else { return false };
        let id = rt.load_bank(Bank::parse(stem, library.read(file).unwrap()).unwrap(), library.bank_pcm(stem));
        names.insert(id, stem);
        true
    };
    for stem in std::iter::once("emitter_utility").chain(BANKS.iter().copied()) {
        assert!(load(&mut rt, stem), "{stem}");
    }
    rt.post(rt.eval.class_id("c_emitter_utility").unwrap(), &[]);
    use skate_audio::player::seams::{UTILITY, UTILITY_BANK};
    if load(&mut rt, UTILITY_BANK) {
        rt.post(rt.eval.class_id(UTILITY).unwrap(), &[]);
    }
    let mut parts = Parts::default();
    for (k, banks) in OPTIONAL_BANKS.iter().take(2).enumerate() {
        for (i, stem) in banks.iter().enumerate() {
            let loaded = load(&mut rt, stem);
            if i == 0 && loaded {
                if k == 0 {
                    parts.rolling = true;
                } else {
                    parts.rattle = true;
                }
            }
        }
    }
    parts.contacts = true;
    for (i, stem) in SPLICE_BANKS.iter().enumerate() {
        match library.splice_bank(stem) {
            Some((bank, pcm)) => {
                let r = &mut rt;
                let index = r.splice.load_bank(stem, bank, pcm, &mut r.mixer);
                names.insert(MIXER_BANK_BASE + index, stem);
            }
            None => parts.contacts &= i > 0,
        }
    }
    let (presets, eq) = library.bus_tuning();
    rt.mixer.buses.env.presets = presets;
    rt.mixer.buses.eq.set_records(&eq);
    rt.mixer.buses.env.request(skate_audio::bus::env::DEFAULT_PRESET);
    (rt, names, parts)
}

fn globals(m: &mut MixMap) {
    for id in 1..=4 {
        m.set_input(keys::MASTER, id, 32767);
    }
    for id in [1, 2, 5] {
        m.set_input(keys::MUSIC, id, 32767);
    }
    m.set_input(keys::REVERB, 5, 32767);
}

/// The NPC's published state at time `t` of the pass: rolling along +x at `V` m/s, `LANE` m in
/// front of the camera, on concrete with the sidewalk seam pattern; an ollie onto a metal rail
/// (grind family 1) for x in −6..6, landing after it.
const V: f32 = 7.0;
const LANE: f32 = -8.0;
const START_X: f32 = -45.0;

fn npc_state(t: f32) -> AudioState {
    let x = START_X + V * t;
    let concrete = material_of_tag(3);
    let metal = material_of_tag(9);
    let air = (-8.8..-6.0).contains(&x) || (6.0..8.8).contains(&x);
    let grinding = (-6.0..6.0).contains(&x);
    let down = !air && !grinding;
    let air_start = if x < 0.0 { -8.8 } else { 6.0 };
    AudioState {
        dt: 1.0 / 30.0,
        ground_speed: V,
        com_velocity: [V, 0.0, 0.0],
        com_position: [x, if grinding { 1.6 } else { 1.0 }, LANE],
        board_position: [x, if grinding { 0.7 } else { 0.1 }, LANE],
        board_velocity: [V, 0.0, 0.0],
        wheel_count: if down { 4 } else { 0 },
        wheel_contact: [down; 4],
        wheel_material: [if down { concrete } else { skate_audio::player::state::NO_MATERIAL }; 4],
        seam_pattern: [if down { 11 } else { 0 }; 4],
        wheel_position: [[x + 0.3, 0.0, LANE + 0.1], [x + 0.3, 0.0, LANE - 0.1], [x - 0.3, 0.0, LANE + 0.1], [x - 0.3, 0.0, LANE - 0.1]],
        airborne: air,
        air_time: if air { (x - air_start) / V } else { 0.0 },
        grinding,
        grind_family: if grinding || x > 0.0 { 1 } else { -1 },
        grind_material: if grinding || x > 0.0 { metal } else { skate_audio::player::state::NO_MATERIAL },
        grind_impact: if grinding { 2.5 } else { 0.0 },
        slip: if down { skate_audio::player::state::slip(0.0) } else { 0.0 },
        audio_trick: -1,
        audio_trick_2: -1,
        scorable: -1,
        time_scale: 1.0,
        ..Default::default()
    }
}

#[derive(Default)]
struct Pass {
    /// Per 0.5 s window: (t, distance, held, per bank (voices opened, max gain), RMS dBFS).
    rows: Vec<(f32, f32, bool, BTreeMap<&'static str, (usize, f32)>, f32)>,
    /// (time, distance) of the claim and of the release.
    claim: Option<(f32, f32)>,
    release: Option<(f32, f32)>,
    /// Classes posted (with the soft word where the class has one: seams w11, skid w8).
    posts: BTreeMap<&'static str, usize>,
    seam_soft: Vec<i32>,
    /// Voices opened per bank while held / after the release.
    opened: BTreeMap<&'static str, usize>,
    late_opens: usize,
}

/// The NPC skater rolls past the listener through the real banks and MixMap, at the console's 30
/// Hz evaluation cadence; the local player stands 3 m in front of the camera with hard wheels.
fn pass(library: &Library, mxb: &[u8], local_soft: bool) -> Pass {
    let (mut rt, names, parts) = runtime(library);
    let tuning = library.player_tuning();
    let contact_tuning = library.contacts_tuning();
    let t = Tuning { player: &tuning, contacts: &contact_tuning };
    let mut m = MixMap::from_bytes(mxb).unwrap();
    let mut slots = Slots::default();
    let mut host = NpcHost::default();
    let mut npc: Option<NpcSkater> = None;
    let mut collision = CollisionManager::default();
    collision.submix = true;
    let camera = [0.0, 1.8, 0.0];
    let l = Listener { camera, view: [0.0, 0.0, -1.0], camera_velocity: [0.0; 3], followed: [0.0, 1.0, -3.0], facing: [0.0, 0.0, -1.0], followed_velocity: [0.0; 3] };
    let dt = 1.0 / 30.0;
    let frames = ((-START_X * 2.0 / V) / dt) as usize;
    let mut out = vec![0.0f32; 2 * 1600];
    let mut p = Pass::default();
    let mut seen = std::collections::HashSet::new();
    let (mut window, mut sum, mut n) = (BTreeMap::<&'static str, (usize, f32)>::new(), 0.0f64, 0usize);
    let mut held_any = false;
    let mut released_at = None;
    for f in 0..frames {
        let time = f as f32 * dt;
        let published = npc_state(time);
        let d = super::distance(published.com_position, camera);
        let a = slots.assign(&[(7, d)]);
        for _ in a.released {
            if let Some(mut x) = npc.take() {
                x.deactivate(&mut m, &l);
            }
            host.release_all(&mut rt, 7);
            p.release = Some((time, d));
            released_at = Some(f);
        }
        for (_, g) in a.claimed {
            npc = Some(NpcSkater::new(g as u32, parts, true, true, true));
            p.claim = Some((time, d));
        }
        globals(&mut m);
        let s = component_state(&published, local_soft);
        if let Some(x) = npc.as_mut() {
            x.write_inputs(&mut m, &s, &l, [0.0; 3], &tuning);
            let cmds = x.process(&mut m, &s, t, &mut rt.splice_host());
            for c in &cmds {
                if let Command::Post { class, words, slot } = c {
                    *p.posts.entry(class).or_default() += 1;
                    if matches!(slot, Slot::Seam(_)) {
                        p.seam_soft.push(words[11]);
                    }
                }
            }
            host.apply(&mut rt, 7, cmds);
            x.routed.grains.clear();
            let mut sh = rt.splice_host();
            for msg in x.take_collisions() {
                collision.post(msg, &mut sh);
            }
        }
        collision.process(&mut m, Some(&l));
        m.tick(dt);
        if let Some(x) = npc.as_mut() {
            let cmds = x.update(&m, &s, t, &mut rt.splice_host());
            host.apply(&mut rt, 7, cmds);
        }
        collision.update(&m, &tuning.collision, dt, &mut rt.splice_host());
        rt.fill_stereo(&mut out);
        held_any |= npc.is_some();
        for v in rt.mixer.snapshot() {
            let Some(&stem) = names.get(&v.bank) else { continue };
            let e = window.entry(stem).or_default();
            e.1 = e.1.max(v.gain);
            if seen.insert((v.bank, v.id)) {
                e.0 += 1;
                if npc.is_some() {
                    *p.opened.entry(stem).or_default() += 1;
                } else if released_at.is_some_and(|r| f > r + 30) {
                    p.late_opens += 1;
                }
            }
        }
        sum += out.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>();
        n += out.len();
        if f % 15 == 14 {
            let rms = (10.0 * (sum / n.max(1) as f64).max(1e-12).log10()) as f32;
            p.rows.push((time, d, npc.is_some(), std::mem::take(&mut window), rms));
            (sum, n) = (0.0, 0);
        }
    }
    assert!(held_any, "the skater never got an instance");
    p
}

/// A synthetic NPC skater rolls past the listener 8 m in front of the camera at 7 m/s, ollies onto
/// a metal rail, grinds it and lands, through the real banks, MixMap, Splice banks and buses
/// (`--nocapture` prints the per-0.5 s table). Checks the mechanism: the instance is taken inside
/// 30 m and given back at 30 m; seams, the grind and its on / off and landing sounds play only
/// while it is held; the seams are louder near the camera than at 25–30 m; the soft word is the
/// local player's inverse (hard local → 1).
///
/// The recomp (all_20261002_164620, user's board hard) for orientation, GAIN × SEND p50 / p90 of the
/// NPC's soft grain members (not rendered here: no per-owner bed yet): concrete_smooth_soft 0.026 /
/// 0.172, concrete_aggregate_soft 0.012 / 0.090, asphalt_rough_soft 0.002 / 0.106 — against the
/// local's concrete_smooth_hard 0.238 / 0.737; and the burst windows of the second Contacts object
/// play Skate_Collisions, Seams_Bank, Skate_Metal, GRINDS and WHEEL_SKID_BANK above their session
/// rates (`.local/research/npc-skater-audio/instance1_voices.py`).
#[test]
fn npc_skater_rolls_and_grinds_past_the_listener() {
    let Some((library, mxb)) = install() else { return eprintln!("skipped: no install with a MixMap") };
    if !BANKS.iter().all(|b| library.aems().banks.contains_key(*b)) || library.player_tuning().seam_patterns.len() < 16 {
        return eprintln!("skipped: the install lacks the player banks or the seam patterns");
    }
    let p = pass(&library, &mxb, false);
    println!("    t     dist  held  RMS dBFS  banks (voices opened, max gain)");
    for (t, d, held, banks, rms) in &p.rows {
        let b: Vec<String> = banks.iter().map(|(k, (n, g))| format!("{k} {n}/{g:.3}")).collect();
        println!("{t:6.2} {d:7.1}  {:4}  {rms:7.1}  {}", if *held { "yes" } else { "-" }, b.join(", "));
    }
    println!("posts {:?}\nopened while held {:?}", p.posts, p.opened);
    let (claim_t, claim_d) = p.claim.expect("claimed");
    let (release_t, release_d) = p.release.expect("released");
    println!("claim at {claim_t:.2} s ({claim_d:.2} m), release at {release_t:.2} s ({release_d:.2} m)");
    assert!(claim_d < AUDIO_RADIUS && claim_d > AUDIO_RADIUS - V * 0.1, "claimed as it crossed 30 m");
    assert!(release_d >= AUDIO_RADIUS && release_d < AUDIO_RADIUS + V * 0.1, "released as it crossed 30 m");
    for bank in ["Seams_Bank", "GRINDS"] {
        assert!(p.opened.get(bank).copied().unwrap_or(0) > 0, "{bank}: no voice while held");
    }
    assert!(p.posts.get("Class_grind").copied().unwrap_or(0) >= 1, "grind posted");
    assert!(p.opened.keys().any(|k| *k == "Skate_Collisions" || *k == "Skate_Metal"), "contact / grind on-off sounds");
    assert!(!p.seam_soft.is_empty() && p.seam_soft.iter().all(|&w| w == 1), "hard local wheels → the NPC's soft word 1");
    assert_eq!(p.late_opens, 0, "nothing new opens a second after the release");
    // Distance: the seams' loudest voice near the closest approach against 22..30 m while held.
    let max = |near: bool| {
        p.rows
            .iter()
            .filter(|r| r.2 && if near { r.1 < 12.0 } else { r.1 > 22.0 })
            .filter_map(|r| r.3.get("Seams_Bank").map(|x| x.1))
            .fold(0.0f32, f32::max)
    };
    let (near, far) = (max(true), max(false));
    println!("Seams_Bank max gain: near {near:.3}, 22–30 m {far:.3}");
    assert!(near > far, "seams louder near the camera ({near} vs {far})");
    assert!(near <= 1.0);
    // The local player's soft wheels flip the NPC's word.
    let q = pass(&library, &mxb, true);
    assert!(!q.seam_soft.is_empty() && q.seam_soft.iter().all(|&w| w == 0), "soft local wheels → 0");
}
