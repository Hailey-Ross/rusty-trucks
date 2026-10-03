//! End-to-end render of the native player audio (MixMap inputs → components → AEMS voices →
//! granular bed → 6-channel bus) for the scripted per-frame situations of
//! `tools/audio-e2e/scenarios.py`, headless. The same scripts can drive another renderer (e.g. the
//! PoC's oracle probe); `tools/audio-e2e/compare.py` compares two renders.
//!
//!   set E2E_DIR=...\.local\audio-re\e2e   (optional E2E_ONLY=roll20,grind_metal)
//!   cargo test -p skate-game --release --bin skate3rust -- --ignored e2e_render --nocapture
//!
//! Writes `<name>.ours.f32` (raw f32, 6 channels in the PoC's order L, R, C, LFE, Ls, Rs) and
//! `<name>.ours.voices.tsv` (per frame: bank, slot, gain, pitch of every sounding voice; grain
//! voices as `grain`).
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use skate_audio::formats::{Bank, Project};
use skate_audio::mixmap::{MixMap, keys};
use skate_audio::player::AudioState;
use skate_audio::player::state::material_of_tag;
use skate_audio::runtime::Runtime;

use super::player_audio::{BANKS, PlayerAudio};
use super::skate_events::Riding;

struct Row(HashMap<String, f64>);
impl Row {
    fn f(&self, k: &str) -> f32 {
        self.0.get(k).copied().unwrap_or(0.0) as f32
    }
    fn i(&self, k: &str) -> i64 {
        self.0.get(k).copied().unwrap_or(0.0) as i64
    }
}

fn read(path: &std::path::Path) -> Vec<Row> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let cols: Vec<String> = lines.next().unwrap().split('\t').map(str::to_owned).collect();
    lines.map(|l| Row(cols.iter().cloned().zip(l.split('\t').map(|v| v.parse::<f64>().unwrap())).collect())).collect()
}

/// What `skate_events::observe` / `audio_state` would publish for this row.
struct Script {
    x: f32,
    air_time: f32,
    pushes: u32,
    family: Option<i32>,
    material: Option<u32>,
    jump_velocity: f32,
    /// Last row's push plant (the edge `+335`).
    planted: bool,
    /// The bridge's `+212` (|COM v|) of the last row and the last row's COM position (the
    /// fallback below), for this row's `+216`.
    com_212: f32,
    com_at: Option<[f32; 3]>,
}

impl Script {
    /// The audio state's `+216` for this row (last row's `+212`), then this row's `+212`: the
    /// logged |COM v| (`com_speed`, logs since 2026-10-03 afternoon); for older logs |Δ COM
    /// position| × 60 from the logged COM positions (`com_x/y/z`, the reckoning's followed point at
    /// 4 decimals, so ±0.006 m/s; the graph saturates from 0.95 m/s); without either 0 (graph 1.0:
    /// the impacts as logged).
    fn com_216(&mut self, r: &Row) -> f32 {
        let previous = self.com_212;
        self.com_212 = if r.0.contains_key("com_speed") {
            r.f("com_speed")
        } else if r.0.contains_key("com_x") {
            let at = [r.f("com_x"), r.f("com_y"), r.f("com_z")];
            let v = self.com_at.map_or(0.0, |p| {
                let d = [at[0] - p[0], at[1] - p[1], at[2] - p[2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * 60.0
            });
            self.com_at = Some(at);
            v
        } else {
            0.0
        };
        previous
    }

    fn riding(&mut self, r: &Row) -> Riding {
        let speed = r.f("speed");
        let com_speed_216 = self.com_216(r);
        self.x += speed / 60.0;
        let state = r.i("state") as u32;
        // As `skate_events::observe`: an unridden board reports no contacts (logs recorded before
        // that gate still carry them).
        let unridden = super::skate_events::board_unridden(state);
        let mask = if unridden { 0 } else { r.i("wheels") as u32 };
        let contact: [bool; 4] = std::array::from_fn(|i| mask & (1 << i) != 0);
        let wheels = contact.iter().filter(|c| **c).count() as u32;
        // E2E_CONTACT_MATERIALS=1: the materials gated by contact, as the game did before 20:15.
        let lines = if std::env::var("E2E_CONTACT_MATERIALS").is_ok_and(|v| v == "1") {
            mask
        } else if unridden {
            0
        } else if r.0.contains_key("lines") {
            r.i("lines") as u32
        } else if mask != 0 {
            15
        } else {
            0
        };
        let tag = if unridden { 0 } else { r.i("tag") as u32 };
        let airborne = (200..300).contains(&state) && wheels == 0;
        // `+236` as `skate_events::audio_state`; E2E_AIR_TIME_STATE=0: the old count (air flag only).
        let follow = !std::env::var("E2E_AIR_TIME_STATE").is_ok_and(|v| v == "0");
        let air_words = !std::env::var("E2E_AIR_WORDS").is_ok_and(|v| v == "0");
        self.air_time = super::skate_events::air_time_236(self.air_time, state, airborne, 1.0 / 60.0, follow);
        let grinding = r.i("grinding") != 0;
        if grinding {
            self.family = Some(r.i("family") as i32);
            // The log's grind tag → `+692` (tag − 1, 0 → 143), as `skate_events::audio_state`.
            self.material = Some(material_of_tag(r.i("grind_tag") as u32));
        }
        let push = r.i("push") != 0;
        // The push plant (`skate_events::push_plant`, session review #1): logs since 2026-10-03 carry
        // State55 (`plant`), and the plant and its rise drive `+333 || +334` / `+335` and the bed's
        // push count. E2E_PUSH_PLANT=0: the animation's push edge (`push`) as before. Older logs keep
        // that unless E2E_PLANT_FROM_FEET=1, which takes the plant from their foot-down bits on the
        // ground states without the brake (State55 && (State57 || State56), the bridge's
        // `+333 || +334`; a proof aid, not the game's input).
        let plant_on = !std::env::var("E2E_PUSH_PLANT").is_ok_and(|v| v == "0");
        let new_log = r.0.contains_key("plant");
        let planted = if plant_on && new_log {
            Some(r.i("plant") != 0)
        } else if plant_on && std::env::var("E2E_PLANT_FROM_FEET").is_ok_and(|v| v == "1") && r.0.contains_key("foot_down") {
            Some(r.i("foot_down") & 3 != 0 && r.i("brake") == 0 && (100..200).contains(&state))
        } else {
            None
        };
        let (push_planted, push_trigger) = match planted {
            Some(now) => (now, now && !self.planted),
            None => (push, push),
        };
        self.planted = planted.unwrap_or(false);
        self.pushes += u32::from(push_trigger);
        let scorable = r.i("scorable");
        let jv = r.f("jv");
        if jv != 0.0 {
            self.jump_velocity = skate_audio::player::state::jump_velocity(jv);
        }
        let feet = r.i("feet") as u32;
        let h = if new_log { Self::harness(r) } else { AudioState::default() };
        let audio = AudioState {
            dt: 1.0 / 60.0,
            ground_speed: speed,
            com_velocity: [speed, 0.0, 0.0],
            com_position: [self.x, 1.0, 0.0],
            board_position: [self.x, 0.1, 0.0],
            board_velocity: [speed, 0.0, 0.0],
            wheel_count: wheels,
            wheel_contact: contact,
            // The materials come from the wheel lines (`skate_events::audio_state`): logs since 20:15
            // carry their hit mask (`lines`); older logs and the scripts take every line as hitting
            // while any wheel is down (retail keeps wheel 0's material on 100 % of 3-wheel frames).
            wheel_material: std::array::from_fn(|i| if lines & (1 << i) != 0 { material_of_tag(tag) } else { 143 }),
            // Wheel 0's pattern for the front pair, wheel 3's for the rear (logs since the seams
            // port); scripted scenarios have none.
            seam_pattern: if unridden { [0; 4] } else {
                let (f, b) = (r.i("seam0") as u32, r.i("seam3") as u32);
                std::array::from_fn(|i| if lines & (1 << i) == 0 { 0 } else if i < 2 { f } else { b })
            },
            // The wheels around wheel 0's logged position along the deck heading (track 0.2 m,
            // wheelbase 0.6 m — the engine's board; only the grid crossings read them).
            wheel_position: {
                let (x0, z0, h) = (r.f("wheel_x"), r.f("wheel_z"), r.f("heading"));
                let (ax, az, rx, rz) = (h.sin(), h.cos(), h.cos(), -h.sin());
                let offs = [(0.0, 0.0), (-0.2, 0.0), (0.0, -0.6), (-0.2, -0.6)];
                std::array::from_fn(|i| [x0 + rx * offs[i].0 + ax * offs[i].1, 0.0, z0 + rz * offs[i].0 + az * offs[i].1])
            },
            turn: r.f("turn"),
            slope: r.f("slope"),
            airborne,
            air_time: self.air_time,
            brake: r.i("brake") != 0,
            manual_brake: r.i("manual") != 0,
            balance: r.i("balance") != 0,
            grinding,
            trick_active: scorable != -1,
            hippy_jump: airborne && scorable == 234,
            bail: h.bail,
            bail_end: h.bail_end,
            // The bail, on-foot / off-board flags, the deck spin and the body regions come from the
            // logs since 2026-10-03 (session review #10); older logs: off, as before.
            on_foot: new_log && state == 500,
            soft_wheels: h.soft_wheels,
            push_planted,
            push_trigger,
            feet_in_deck_box: [feet & 1 != 0, feet & 2 != 0],
            grind_family: self.family.unwrap_or(-1),
            // `+692` = Grinds+216 − 1 (the packer `sub_827A1B78`), as the game publishes it.
            grind_material: self.material.unwrap_or(143),
            local: true,
            jump_velocity: self.jump_velocity,
            audio_trick: -1,
            scorable: scorable as i32,
            // The scenario's slip is the sine of the deck's turn off the roll (lateral / speed).
            slip: if wheels > 0 { skate_audio::player::state::slip(r.f("slip") * speed) } else { 0.0 },
            revert: if new_log { r.i("revert") != 0 } else { state == 102 },
            offboard_308: h.offboard_308,
            deck_tilt: r.f("tilt"),
            deck_spin: h.deck_spin,
            // `+240` / `+260` (Class_Treatment w8 / w9) from the rows' to_land / jump_height (both
            // scenario kinds carry them); E2E_AIR_WORDS=0: 0 as before 2026-10-02 (no Treatments).
            air_until_landing: if air_words { r.f("to_land") } else { 0.0 },
            jump_height: if air_words { r.f("jump_height") } else { 0.0 },
            // Real-play logs carry these (state_log.rs, appended columns); scripted scenarios
            // leave them 0.
            grind_impact: r.f("grind_impact"),
            deck_impact: if unridden { 0.0 } else { r.f("deck_impact") },
            deck_material: if unridden { 143 } else { material_of_tag(r.i("deck_tag") as u32) },
            foot_speed_y: [r.f("foot_y0"), r.f("foot_y1")],
            foot_speed_xz: [r.f("foot_xz0"), r.f("foot_xz1")],
            // Off-board inputs (logs since 21:00; older logs: none).
            foot_down: [r.i("foot_down") & 1 != 0, r.i("foot_down") & 2 != 0],
            foot_material: if r.0.contains_key("foot_tag_a") { [r.i("foot_tag_a") as u32, r.i("foot_tag_b") as u32] } else { [143; 2] },
            hands_on_deck: [r.i("hands") & 1 != 0, r.i("hands") & 2 != 0],
            footstep_strength: r.f("strength"),
            foot_vertical_speed: [r.f("foot_vy_a"), r.f("foot_vy_b")],
            // The skeleton inputs (logs since the skeleton-inputs stage; older logs: the defaults).
            step_code: if r.0.contains_key("step") { r.i("step") as i32 } else { 1 },
            body_speed: r.f("body"),
            limb_speed: r.f("limb"),
            com_speed_216,
            ..h
        };
        Riding {
            board: bevy::math::Vec3::new(self.x, 0.1, 0.0),
            speed,
            surface: tag,
            grinding,
            braking: r.i("brake") != 0 && speed > 0.5,
            wheels,
            pushes: self.pushes,
            audio,
            // The loose-board inputs (logs since the skeleton-inputs stage; older: upright, no contact).
            deck_up: if r.0.contains_key("deck_up") { r.f("deck_up") } else { 1.0 },
            deck_contact: r.i("deck_contact") != 0,
            deck_material: material_of_tag(r.i("deck_tag") as u32),
        }
    }
}

impl Script {
    /// The state-log columns appended 2026-10-03 (session review #10).
    fn harness(r: &Row) -> AudioState {
        AudioState {
            push_stroke: r.i("stroke") != 0,
            deck_spin: r.f("deck_spin"),
            deck_spin_xy: [r.f("spin_x"), r.f("spin_y")],
            bail: r.i("bail") != 0,
            bail_end: r.i("bail_end") != 0,
            offboard_308: r.i("held") != 0,
            offboard_air: r.i("offboard_air") != 0,
            footplant: r.i("footplant") != 0,
            soft_wheels: r.i("soft") != 0,
            body_slide_flag: r.i("face") != 0,
            body_impact: std::array::from_fn(|i| r.f(&format!("rimp{i}"))),
            body_slide: std::array::from_fn(|i| r.f(&format!("rslide{i}"))),
            body_tag: std::array::from_fn(|i| r.i(&format!("rtag{i}")) as u32),
            ..AudioState::default()
        }
    }
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

#[test]
#[ignore = "headless render for the PoC comparison; needs E2E_DIR and the install"]
fn e2e_render() {
    let Some(dir) = std::env::var_os("E2E_DIR").map(std::path::PathBuf::from) else { return eprintln!("E2E_DIR not set") };
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let library = super::Library::load(root).expect("install");
    let mxb = library.read(library.aems().mixmap.as_ref().expect("MixMap")).unwrap();
    let only = std::env::var("E2E_ONLY").ok();
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "tsv") && !p.file_stem().unwrap().to_string_lossy().contains('.')).collect();
    paths.sort();
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !o.split(',').any(|x| x == name)) {
            continue;
        }
        let rows = read(&path);
        let mut rt = Runtime::new();
        for file in &library.aems().projects {
            rt.install_project(&Project::parse(file, &library.read(file).unwrap()).unwrap());
        }
        let mut names = HashMap::new();
        for stem in std::iter::once("emitter_utility").chain(BANKS.iter().copied()) {
            let file = &library.aems().banks[stem];
            let bank = Bank::parse(stem, library.read(file).unwrap()).unwrap();
            let id = rt.load_bank(bank, library.bank_pcm(stem));
            names.insert(id, stem);
        }
        let utility = rt.eval.class_id("c_emitter_utility").unwrap();
        rt.post(utility, &[]);
        let off = |var: &str| std::env::var(var).is_ok_and(|v| v == "0");
        // Retail's second boot utility (the Seams program's sample shuffles); E2E_SEAM_UTILITY=0:
        // without it, as before Listening test 8.
        if !off("E2E_SEAM_UTILITY") {
            use skate_audio::player::seams::{UTILITY, UTILITY_BANK};
            if let Some(file) = library.aems().banks.get(UTILITY_BANK) {
                let id = rt.load_bank(Bank::parse(UTILITY_BANK, library.read(file).unwrap()).unwrap(), Vec::new());
                names.insert(id, UTILITY_BANK);
                rt.post(rt.eval.class_id(UTILITY).unwrap(), &[]);
            }
        }
        // The optional components (rolling layers, rattle, board slide, tricks, treatment) run when
        // their first bank is in the install, as in the game; E2E_<NAME>=0 turns one off.
        let off = |var: &str| std::env::var(var).is_ok_and(|v| v == "0");
        let mut optional = [false; 5];
        for (k, (banks, var)) in super::player_audio::OPTIONAL_BANKS.iter().zip(["E2E_ROLLING", "E2E_RATTLE", "E2E_SLIDE", "E2E_TRICKS", "E2E_TREATMENT"]).enumerate() {
            if off(var) {
                continue;
            }
            for (i, stem) in banks.iter().enumerate() {
                let Some(file) = library.aems().banks.get(*stem) else { continue };
                let bank = Bank::parse(stem, library.read(file).unwrap()).unwrap();
                let id = rt.load_bank(bank, library.bank_pcm(stem));
                names.insert(id, stem);
                optional[k] |= i == 0;
            }
        }
        if optional[3] {
            if let Some(id) = rt.eval.class_id(skate_audio::player::tricks::FOLEY_UTILITY) {
                rt.post(id, &[]);
            }
        }
        // E2E_CHAIN=0: the bed's chain as before the full graph-1/2/3 port.
        rt.grains.chain_extras = !off("E2E_CHAIN");
        let mut contacts = true;
        for (i, stem) in super::player_audio::SPLICE_BANKS.iter().enumerate() {
            match library.splice_bank(stem) {
                Some((bank, pcm)) => {
                    let r = &mut rt;
                    r.splice.load_bank(stem, bank, pcm, &mut r.mixer);
                }
                None => contacts &= i > 0,
            }
        }
        let streams: Vec<_> = super::player_audio::WHEEL_STREAMS.iter().map(|n| library.wheels_pcm(n)).collect();
        let wheels = streams.iter().all(Option::is_some);
        rt.load_streams(streams);
        // The buses (E2E_BUSES=0: dry, as before the bus port): the reverb network at the default
        // preset (no map → reverb01) and the eEQChain buses.
        if std::env::var("E2E_BUSES").map_or(true, |v| v != "0") {
            let (presets, eq) = library.bus_tuning();
            rt.mixer.buses.env.presets = presets;
            rt.mixer.buses.eq.set_records(&eq);
            // E2E_PRESET=<16 hex digits>: a region's preset key instead (e.g. University's
            // 407AFA1D6C7CEAD8, DownTown's 68A9E6020DF2076D).
            let preset = std::env::var("E2E_PRESET").ok().and_then(|k| u64::from_str_radix(&k, 16).ok());
            rt.mixer.buses.env.request(preset.unwrap_or(skate_audio::bus::env::DEFAULT_PRESET));
            // The FlangeSub returns (E2E_FLANGE=0: off).
            if let Some([a, b]) = library.flange_presets().filter(|_| !off("E2E_FLANGE")) {
                rt.mixer.buses.flange.set_presets(a, b);
            }
        }
        // The FootStep SubMix graphs (E2E_FOOTSTEP_SUBMIX=0: the foot voices straight into SFX Master).
        rt.mixer.buses.submix.enabled = !off("E2E_FOOTSTEP_SUBMIX");
        // Reverb.in0..6 from the preset and the env scale (E2E_REVERB_INPUTS=0: in5 = 32767, no scale).
        let reverb_inputs = !off("E2E_REVERB_INPUTS");
        let shared = Arc::new(Mutex::new(rt));
        let mut m = MixMap::from_bytes(&mxb).unwrap();
        let mut p = PlayerAudio::new(library.player_tuning(), true);
        p.contacts_on = contacts && std::env::var("E2E_CONTACTS").map_or(true, |v| v != "0");
        p.contact_tuning = library.contacts_tuning();
        p.footsteps_on = p.contacts_on && !off("E2E_FOOTSTEPS");
        // The session-review ports (E2E_PLANT_LIFT / E2E_BODY_IMPACTS / E2E_GRIND_ONOFF=0: off).
        p.set_review_ports(!off("E2E_PLANT_LIFT"), !off("E2E_BODY_IMPACTS"), !off("E2E_GRIND_ONOFF"));
        // The body poster on the console cadence (with the MixMap's; E2E_BODY_CONSOLE=0: per call).
        p.body_console = !off("E2E_BODY_CONSOLE");
        // The deck poster too (E2E_DECK_CONSOLE=0: per call).
        p.deck_console = !off("E2E_DECK_CONSOLE");
        // The bridge's speed graph on the body impacts (E2E_BODY_CURVE=0: the logged impacts).
        p.set_body_curve(!off("E2E_BODY_CURVE"));
        // E2E_BODY_LOG=1: every body-poster message (`<name>.ours.bodymsg.tsv`).
        let body_log = std::env::var("E2E_BODY_LOG").is_ok_and(|v| v == "1");
        p.set_body_log(body_log);
        p.set_footstep_materials(library.footstep_materials());
        p.wheels_on = wheels && std::env::var("E2E_WHEELS").map_or(true, |v| v != "0");
        (p.rolling_on, p.rattle_on, p.slide_on, p.tricks_on, p.treatment_on) = (optional[0], optional[1], optional[2], optional[3], optional[4]);
        let mut bed = super::grain_bed::Bed::new(&library).expect("grain bed data");
        // The bed's turn / brake slews once per console evaluation (with the MixMap's cadence;
        // E2E_SLEW_CONSOLE=0: per call, dt-scaled, as before).
        bed.slew_console = !off("E2E_SLEW_CONSOLE");
        // E2E_SLEW_LOG=1: the bed's turn input, |COM v|, special, I and brake per call
        // (`<name>.ours.slew.tsv`).
        let mut slew_log = std::env::var("E2E_SLEW_LOG").is_ok_and(|v| v == "1").then(|| {
            let mut w = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.slew.tsv"))).unwrap());
            writeln!(w, "frame	eval	turn_in	com_speed	turn_I	turn_signed	braking	brake").unwrap();
            w
        });
        let seams = skate_audio::player::tuning::PlayerTuning { seam_wobbles: p.tuning.seam_wobbles.clone(), ..Default::default() };
        let mut script = Script { x: 0.0, air_time: 0.0, pushes: 0, family: None, material: None, jump_velocity: 0.0, planted: false, com_212: 0.0, com_at: None };
        let mut out = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.f32"))).unwrap());
        let mut voices = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.voices.tsv"))).unwrap());
        writeln!(voices, "frame\tbank\tslot\tgain\tpitch").unwrap();
        // The body poster's messages: the row where its count changed, the count and the digest.
        let mut body = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.body.tsv"))).unwrap());
        writeln!(body, "frame\tposts\tdigest").unwrap();
        let mut body_seen = p.body_trace();
        let mut bodymsg = body_log.then(|| {
            let mut w = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.bodymsg.tsv"))).unwrap());
            writeln!(w, "frame\trow\tregion\timpact\tcom216\tmat_a\tmat_b\ttier_a\ttier_b\tlevel_a\tlevel_b").unwrap();
            w
        });
        // The deck poster's messages, the same way.
        let mut deck = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.deck.tsv"))).unwrap());
        writeln!(deck, "frame\tposts\tdigest").unwrap();
        let mut deck_seen = p.deck_trace();
        let (mut owed, blocks_per_frame) = (0.0f64, 48_000.0 / 256.0 / 60.0);
        let order: Vec<usize> = std::iter::repeat(0).take(60).chain(0..rows.len()).collect();
        // E2E_TIMING=1: per-frame cost of the game-thread side and per-block cost of the render.
        let timing = std::env::var("E2E_TIMING").is_ok_and(|v| v == "1");
        let (mut game_us, mut block_us): (Vec<(f64, usize, [f64; 5])>, Vec<f64>) = (Vec::new(), Vec::new());
        // E2E_SUBSTEPS=N: N audio-manager calls per 60 Hz physics row (the recomp calls it per
        // rendered frame, ~5 per physics step: the state, wheel positions included, changes only on
        // the first). Diagnostic for cadence-dependent components (Class_Seams' one-call w7 pulse);
        // 1 (default) = the game's 60 Hz host.
        let substeps: usize = std::env::var("E2E_SUBSTEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
        let sub_dt = 1.0 / 60.0 / substeps as f32;
        // E2E_CALLS=N: the game's host at N rendered frames per 60 Hz tick (353 fps ≈ 6): the full
        // tick on the first call; Class_Seams on its console cadence (`PlayerAudio::seam_frame`) on
        // every call, at the wheel positions interpolated from the previous row like the rendered
        // board; the blocks spread evenly. 1 (default) = one call per row, the seams per tick at the
        // physics positions, as before.
        let calls: usize = std::env::var("E2E_CALLS").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
        let per_row = substeps * calls;
        // E2E_FPS=f (overrides E2E_CALLS): the game's host at a rendered frame rate f, as
        // `native::mixmap_frame` runs it: f/60 calls per physics row on average (the first call of a
        // row ticks; every call runs `seam_frame`); below 60 fps a call ticks the MixMap once per
        // elapsed 60 Hz step and rows without a call only render.
        let fps: Option<f64> = std::env::var("E2E_FPS").ok().and_then(|v| v.parse().ok());
        let console = fps.is_some() || calls > 1;
        // The MixMap's console cadence (`native::mixmap_frame`, `skate_audio::mixmap::cadence`): one
        // evaluation per two 60 Hz rows with dt 1/30, the Jitter stepped and the eEQChain cleared
        // there, the flag inputs held in between. On with E2E_FPS (the game's host) unless
        // E2E_MIX_CONSOLE=0; E2E_MIX_CONSOLE=1 turns it on for the per-row renders too.
        let mix_console = match std::env::var("E2E_MIX_CONSOLE").ok().as_deref() {
            Some("0") => false,
            Some("1") => true,
            _ => fps.is_some(),
        };
        let mut cadence = skate_audio::mixmap::cadence::Cadence::default();
        if mix_console {
            super::native::hold_flag_inputs(&mut m);
        }
        let (mut fps_acc, mut rows_since_call) = (0.0f64, 0usize);
        for (frame, &i) in order.iter().enumerate() {
          let row_riding = script.riding(&rows[i]);
          rows_since_call += 1;
          let (n_row, calls) = match fps {
              Some(f) => {
                  fps_acc += f / 60.0;
                  let n = fps_acc.floor() as usize;
                  fps_acc -= n as f64;
                  (n, n.max(1))
              }
              None => (per_row, calls),
          };
          let per_row = n_row.max(1);
          if n_row == 0 {
              owed += blocks_per_frame;
              let mut rt = shared.lock().unwrap();
              while owed >= 1.0 {
                  owed -= 1.0;
                  let bus = rt.render_block();
                  for k in 0..skate_audio::BLOCK {
                      for ch in [0, 2, 1, 5, 3, 4] {
                          out.write_all(&bus[ch][k].to_le_bytes()).unwrap();
                      }
                  }
              }
          }
          for call in 0..n_row {
            // The console seams cadence (Listening test 9) whenever the host runs other than one
            // call per row: every call is a rendered frame of 1/f s.
            if console {
                p.seam_console = true;
                p.seam_alpha = Some((call % calls) as f32 / calls as f32);
                let dt = fps.map_or(1.0 / (60.0 * calls as f64), |f| 1.0 / f) as f32;
                p.seam_frame(&m, &row_riding.audio, dt, &mut shared.lock().unwrap());
            }
            if call % calls != 0 {
                owed += blocks_per_frame / per_row as f64;
                let mut rt = shared.lock().unwrap();
                while owed >= 1.0 {
                    owed -= 1.0;
                    let bus = rt.render_block();
                    for k in 0..skate_audio::BLOCK {
                        for ch in [0, 2, 1, 5, 3, 4] {
                            out.write_all(&bus[ch][k].to_le_bytes()).unwrap();
                        }
                    }
                }
                continue;
            }
            let t0 = std::time::Instant::now();
            let riding = row_riding;
            let mut s = riding.audio;
            if substeps > 1 {
                s.dt = sub_dt;
            }
            globals(&mut m);
            if reverb_inputs {
                let v = shared.lock().unwrap().mixer.buses.env.reverb_inputs();
                for (id, x) in v.into_iter().enumerate() {
                    m.set_input(keys::REVERB, id, x);
                }
            }
            // Below 60 fps one call ticks once per elapsed 60 Hz step (`mixmap_frame`'s ticks loop),
            // and the listener's velocity and the bed's frame span those steps (the game passes the
            // real frame time). Before 2026-10-03 both took one row's dt there: E2E_FPS < 60 renders
            // ran the camera velocity (Doppler) and the bed's clocks off by the rows per call.
            let ticks = if fps.is_some_and(|f| f < 60.0) { rows_since_call } else { 1 };
            let call_dt = sub_dt * ticks as f32;
            let l = p.listener([script.x - 3.0, 2.5, 0.0], [1.0, 0.0, 0.0], call_dt, &s);
            let mix_calls = if mix_console { cadence.advance(ticks) } else { 0 };
            p.jitter_steps = mix_console.then_some(mix_calls);
            // As `mixmap_frame`: sub_82491180 (half 1) before the inputs, with the last walk's values.
            if mix_console && mix_calls > 0 {
                shared.lock().unwrap().mixer.buses.eq.clear(p.eq_jitter());
            }
            p.write_inputs(&mut m, &s, Some(&l));
            bed.write_inputs(&mut m, &s, !p.rolling_on);
            let speed_scale = bed.push_scale();
            let loose = PlayerAudio::loose_board(&s, &riding);
            let t1 = std::time::Instant::now();
            p.process(&mut m, &s, &mut shared.lock().unwrap(), speed_scale, loose);
            let t2 = std::time::Instant::now();
            if p.body_trace() != body_seen {
                body_seen = p.body_trace();
                writeln!(body, "{frame}\t{}\t{:016x}", body_seen.0, body_seen.1).unwrap();
            }
            if let Some(w) = bodymsg.as_mut() {
                for (region, impact, m) in p.take_body_log() {
                    writeln!(w, "{frame}\t{i}\t{region}\t{impact}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", s.com_speed_216, m.material[0], m.material[1], m.tier[0], m.tier[1], m.level[0], m.level[1]).unwrap();
                }
            }
            if p.deck_trace() != deck_seen {
                deck_seen = p.deck_trace();
                writeln!(deck, "{frame}\t{}\t{:016x}", deck_seen.0, deck_seen.1).unwrap();
            }
            if mix_console {
                for _ in 0..mix_calls {
                    m.tick(skate_audio::mixmap::cadence::CONSOLE_DT);
                }
            } else {
                for _ in 0..ticks {
                    m.tick(sub_dt);
                }
            }
            rows_since_call = 0;
            if !mix_console && frame % 2 == 1 {
                shared.lock().unwrap().mixer.buses.eq.clear(p.eq_jitter());
            }
            let t3 = std::time::Instant::now();
            p.update(&m, &s, &mut shared.lock().unwrap(), speed_scale, loose);
            shared.lock().unwrap().mixer.buses.flange.frame(std::array::from_fn(|i| m.level(keys::REVERB, i)));
            if reverb_inputs {
                shared.lock().unwrap().mixer.buses.env.scale_frame(m.level(keys::REVERB, 4));
            }
            let t4 = std::time::Instant::now();
            let routed = p.rolling_on.then(|| (std::mem::take(&mut p.routed.grains), p.routed.primary));
            bed.slew_calls = mix_console.then_some(mix_calls);
            super::grain_bed::step(&mut bed, &library, &m, &shared, &riding, call_dt, &seams, routed);
            if let Some(w) = slew_log.as_mut() {
                let (turn, brake) = bed.slews();
                writeln!(w, "{frame}	{mix_calls}	{}	{}	{}	{turn}	{}	{brake}", s.turn, s.com_speed(), turn.abs(), u8::from(riding.braking)).unwrap();
            }
            let t5 = std::time::Instant::now();
            if timing && frame >= 60 {
                let us = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1e6;
                game_us.push((us(t0, t5), frame - 60, [us(t0, t1), us(t1, t2), us(t2, t3), us(t3, t4), us(t4, t5)]));
            }
            owed += blocks_per_frame / per_row as f64;
            let mut rt = shared.lock().unwrap();
            while owed >= 1.0 {
                owed -= 1.0;
                let tb = std::time::Instant::now();
                let bus = rt.render_block();
                if timing && frame >= 60 {
                    block_us.push(tb.elapsed().as_secs_f64() * 1e6);
                }
                // Ours: L, C, R, Ls, Rs, LFE → the PoC's L, R, C, LFE, Ls, Rs.
                for k in 0..skate_audio::BLOCK {
                    for ch in [0, 2, 1, 5, 3, 4] {
                        out.write_all(&bus[ch][k].to_le_bytes()).unwrap();
                    }
                }
            }
            drop(rt);
          }
            let rt = shared.lock().unwrap();
            if frame >= 60 {
                for v in rt.mixer.snapshot() {
                    let bank = names.get(&v.bank).copied().unwrap_or("?");
                    let out = match v.output {
                        skate_audio::bus::Output::Master => 8,
                        skate_audio::bus::Output::Eq(i) => i,
                        // A FootStep SubMix graph (sends on into the env bus and SFX Master).
                        skate_audio::bus::Output::Submix(i) => 20 + i,
                    };
                    writeln!(voices, "{}\t{bank}\t{}\t{:.5}\t{:.4}\t{:.5}\t{out}", frame - 60, v.slot, v.gain, v.pitch, v.send).unwrap();
                }
                for (t, truck) in rt.grains.trucks.iter().enumerate() {
                    for (k, player) in truck.players.iter().enumerate() {
                        if player.voices() > 0 {
                            let r = player.record;
                            writeln!(voices, "{}\tgrain{t}{}\t{}\t{:.5}\t{:.4}\t{:.4}", frame - 60, ["A", "B"][k], player.starts, r.gain, r.pitch, r.position).unwrap();
                        }
                    }
                }
                for (bank, record, sample, gain, pitch) in rt.splice.voices() {
                    writeln!(voices, "{}	splice:{bank}:{record}	{sample}	{gain:.5}	{pitch:.4}", frame - 60).unwrap();
                }
                if rt.grains.rocket.voices() > 0 {
                    let r = rt.grains.rocket.record;
                    writeln!(voices, "{}\trocket\t{}\t{:.5}\t{:.4}\t{:.4}", frame - 60, rt.grains.rocket.starts, r.gain, r.pitch, r.position).unwrap();
                }
            }
        }
        let (hits, transitions) = p.seam_hits();
        eprintln!("{name}: {} frames (+60 settle); Class_Seams hits {hits} ({transitions} material changes)", rows.len());
        if timing && !game_us.is_empty() {
            // The raw times for the optimisation bench (`tools/audio-bench/`):
            // one line per measured game-thread call and per rendered block.
            let mut raw = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.ours.timing.tsv"))).unwrap());
            writeln!(raw, "kind\tus").unwrap();
            for (total, _, _) in &game_us {
                writeln!(raw, "frame\t{total:.2}").unwrap();
            }
            for us in &block_us {
                writeln!(raw, "block\t{us:.2}").unwrap();
            }
            drop(raw);
            let pct = |v: &mut Vec<f64>, q: f64| {
                v.sort_by(f64::total_cmp);
                v[((v.len() - 1) as f64 * q) as usize]
            };
            let mut g: Vec<f64> = game_us.iter().map(|x| x.0).collect();
            eprintln!("  game thread per frame (us): p50 {:.0} p99 {:.0} max {:.0}", pct(&mut g, 0.5), pct(&mut g, 0.99), pct(&mut g, 1.0));
            let mut slow: Vec<(usize, f64)> = block_us.iter().copied().enumerate().collect();
            slow.sort_by(|a, b| b.1.total_cmp(&a.1));
            eprintln!("  slowest blocks (index, us): {:?}", &slow[..slow.len().min(4)]);
            eprintln!("  render per 256-frame block (us, budget 5333): p50 {:.0} p99 {:.0} max {:.0}", pct(&mut block_us, 0.5), pct(&mut block_us, 0.99), pct(&mut block_us, 1.0));
            game_us.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (total, f, parts) in game_us.iter().take(8) {
                eprintln!("  frame {f}: {total:.0} us = inputs {:.0} process {:.0} tick {:.0} update {:.0} bed {:.0}", parts[0], parts[1], parts[2], parts[3], parts[4]);
            }
        }
    }
}
