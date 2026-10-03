//! Evaluator tests on hand-built banks: posting, payload delivery, the player's open / push /
//! query / restart edge, release → destroy, capacity, and the function → global round trip with
//! its walk-order rule (spec §2, §5).
use super::*;
use crate::formats::Project;
use crate::formats::csi::Symbol;

/// One module of a synthetic bank.
struct Spec {
    max: i16,
    globals: u16,
    functions: u16,
    destructor: bool,
    class_data: bool,
    players: Vec<u32>,
    template: Vec<u8>,
    /// (opcode, pairs, block offset); advances are derived from the next op's block.
    program: Vec<(u8, Vec<(i32, i32)>, u32)>,
    /// Template offsets that must hold the bank offset of the shared sample group.
    group_ptrs: Vec<u32>,
}

/// An export: kind (0 global, 1 class, 2 function), name id, name, and where the handle goes:
/// None = the module's class handle, Some(off) = template offset.
struct Ex {
    module: usize,
    kind: u32,
    name_id: u16,
    name: &'static str,
    at: Option<u32>,
}

const PROJECT: u16 = 0x1234;

fn project() -> Project {
    let s = |name: &str, id: u16, default: i32| Symbol { name: name.into(), name_id: id, default };
    Project {
        name: "test.csi".into(),
        id: PROJECT,
        tables: [
            vec![s("f_msg", 1, 0)],
            vec![s("c_test", 1, 0), s("c_util", 2, 0), s("c_req", 3, 0)],
            vec![s("g_snd", 1, 77)],
        ],
    }
}

fn be(v: &mut Vec<u8>, w: u32) {
    v.extend_from_slice(&w.to_be_bytes());
}

/// Build an ABKC bank: header | module records | templates | programs | sample group | S10A |
/// interface list + id records | empty rebase list. Samples: (frames, looping) at 48 kHz mono.
fn bank(modules: &[Spec], exports: &[Ex], samples: &[(u32, bool)]) -> Bank {
    let records: usize = modules.iter().map(|m| 60 + 4 * m.players.len()).sum();
    let mut at = 0x5C + records;
    let mut template_at = Vec::new();
    for m in modules {
        template_at.push(at);
        at += m.template.len();
    }
    let mut program_at = Vec::new();
    let mut programs = Vec::new();
    for m in modules {
        program_at.push(at);
        let mut p = Vec::new();
        for (i, (op, pairs, block)) in m.program.iter().enumerate() {
            let next = m.program.get(i + 1).map_or(m.template.len() as u32, |o| o.2);
            p.push(*op);
            p.push(pairs.len() as u8);
            p.extend_from_slice(&[0, 0]);
            for &(s, d) in pairs {
                be(&mut p, s as u32);
                be(&mut p, d as u32);
            }
            be(&mut p, next - block);
        }
        p.push(255);
        p.extend_from_slice(&[0, 0, 0]);
        at += p.len();
        programs.push(p);
    }
    let group_at = at;
    let mut group = Vec::new();
    be(&mut group, samples.len() as u32);
    for i in 0..samples.len() {
        group.extend_from_slice(&(i as u16).to_be_bytes());
        group.push(100);
        // Bytes 3..7 azimuths; byte 8 is the first byte of the stream offset word.
        group.extend_from_slice(&[0, 0, 0, 0, (i + 1) as u8]);
        be(&mut group, u32::MAX);
    }
    at += group.len();
    let s10a_at = at;
    let mut s10a = b"S10A".to_vec();
    be(&mut s10a, 0);
    be(&mut s10a, samples.len() as u32);
    let mut bodies = Vec::new();
    let table = 12 + 4 * samples.len();
    for &(frames, looping) in samples {
        be(&mut s10a, (table + bodies.len()) as u32);
        be(&mut bodies, (3 << 24) | 48000);
        be(&mut bodies, (u32::from(looping) << 29) | frames);
        be(&mut bodies, 0);
    }
    s10a.extend_from_slice(&bodies);
    at += s10a.len();
    let interface_at = at;
    let mut iface = Vec::new();
    be(&mut iface, exports.len() as u32);
    let ids_at = interface_at + 4 + 12 * exports.len();
    let mut ids = Vec::new();
    let mut module_at = Vec::new();
    let mut r = 0x5C;
    for m in modules {
        module_at.push(r);
        r += 60 + 4 * m.players.len();
    }
    for e in exports {
        let handle = match e.at {
            None => module_at[e.module] + 4,
            Some(off) => template_at[e.module] + off as usize,
        };
        be(&mut iface, handle as u32);
        be(&mut iface, (ids_at + ids.len()) as u32);
        be(&mut iface, e.kind << 24);
        ids.extend_from_slice(&PROJECT.to_be_bytes());
        ids.extend_from_slice(&e.name_id.to_be_bytes());
        ids.extend_from_slice(e.name.as_bytes());
        ids.push(0);
        while ids.len() % 4 != 0 {
            ids.push(0);
        }
    }
    at = ids_at + ids.len();
    let rebase_at = at;

    let mut d = vec![0u8; 0x5C];
    d[..4].copy_from_slice(b"ABKC");
    d[0x0A..0x0C].copy_from_slice(&(modules.len() as u16).to_be_bytes());
    d[0x18..0x1C].copy_from_slice(&(s10a_at as u32).to_be_bytes());
    d[0x1C..0x20].copy_from_slice(&0x5Cu32.to_be_bytes());
    d[0x20..0x24].copy_from_slice(&(s10a_at as u32).to_be_bytes());
    d[0x34..0x38].copy_from_slice(&(rebase_at as u32).to_be_bytes());
    d[0x38..0x3C].copy_from_slice(&(interface_at as u32).to_be_bytes());
    for (i, m) in modules.iter().enumerate() {
        let mut rec = vec![0u8; 60];
        rec[0x1E..0x20].copy_from_slice(&m.max.to_be_bytes());
        rec[0x20..0x22].copy_from_slice(&m.globals.to_be_bytes());
        rec[0x22..0x24].copy_from_slice(&m.functions.to_be_bytes());
        rec[0x24] = m.players.len() as u8;
        rec[0x25] = u8::from(m.destructor);
        rec[0x26] = u8::from(m.class_data);
        rec[0x28..0x2C].copy_from_slice(&(program_at[i] as u32).to_be_bytes());
        rec[0x2C..0x30].copy_from_slice(&(template_at[i] as u32).to_be_bytes());
        rec[0x30..0x34].copy_from_slice(&(m.template.len() as u32).to_be_bytes());
        rec[0x34..0x38].copy_from_slice(&(m.template.len() as u32 - 16).to_be_bytes());
        for p in &m.players {
            be(&mut rec, *p);
        }
        d.extend_from_slice(&rec);
    }
    for m in modules {
        let mut t = m.template.clone();
        for &g in &m.group_ptrs {
            put_u32(&mut t, g as usize, group_at as u32);
        }
        d.extend_from_slice(&t);
    }
    for p in &programs {
        d.extend_from_slice(p);
    }
    d.extend_from_slice(&group);
    d.extend_from_slice(&s10a);
    d.extend_from_slice(&iface);
    d.extend_from_slice(&ids);
    be(&mut d, 0); // empty rebase list
    Bank::parse("test.abk", d).expect("synthetic bank")
}

/// Template words helper.
fn words(n: usize) -> Vec<u8> {
    vec![0u8; 4 * n]
}

/// A c_test module: destructor @24, ClassData (2 values) @44, Create @72, Player @76 (one input:
/// pitch), Destroy @124. Payload w0 → playcontrol, w1 → pitch, w2 → sample select.
fn player_module(max: i16) -> Spec {
    let mut t = words(35); // 140 bytes
    put_u8(&mut t, 44 + 16, 3); // ClassData: 3 values ((3 + 5)·4 = 32 B → @44..76)
    // Create @76
    put_i32(&mut t, 76, 1);
    // Player @80: n = 1 input, update outputs
    let p = 80usize;
    put_u8(&mut t, p + 14, 1);
    put_u8(&mut t, p + 15, 1);
    put_u8(&mut t, p + 28, 0); // input id 0 (pitch)
    put_i32(&mut t, p + 32, -1); // applied
    // Destroy @124 (datasize 140 − 16)
    Spec {
        max,
        globals: 0,
        functions: 0,
        destructor: true,
        class_data: true,
        players: vec![80],
        template: t,
        program: vec![
            (0, vec![(-1, 124 + 12 - 24)], 24),
            // ClassData @44: values at +20,+24,+28 → playcontrol (+24 of player), pitch value
            // (+36), sample select (+20).
            (1, vec![(20, 80 + 24 - 44), (24, 80 + 36 - 44), (28, 80 + 20 - 44)], 44),
            (3, vec![], 76),
            (27, vec![], 80),
            (4, vec![], 124),
        ],
        group_ptrs: vec![80 + 4],
    }
}

#[derive(Default)]
struct Log {
    calls: Vec<String>,
    next: u32,
    alive: HashMap<u32, bool>,
}

impl VoiceHost for Log {
    fn open(&mut self, r: &OpenRequest) -> Option<u32> {
        self.next += 1;
        self.alive.insert(self.next, true);
        self.calls.push(format!("open v{} slot {}", self.next, r.slot));
        Some(self.next)
    }
    fn release(&mut self, v: u32) {
        self.calls.push(format!("release v{v}"));
    }
    fn pause(&mut self, v: u32) {
        self.calls.push(format!("pause v{v}"));
    }
    fn resume(&mut self, v: u32) {
        self.calls.push(format!("resume v{v}"));
    }
    fn set(&mut self, v: u32, id: u8, value: i32) {
        self.calls.push(format!("set v{v} {id}={value}"));
    }
    fn set_azimuth(&mut self, v: u32, value: i32) {
        self.calls.push(format!("az v{v} {value}"));
    }
    fn query(&mut self, v: u32) -> VoiceStatus {
        let alive = self.alive.get(&v).copied().unwrap_or(false);
        VoiceStatus { alive, remaining_ms: if alive { 500 } else { 0 }, elapsed_ms: 7 }
    }
}

fn setup(modules: Vec<Spec>, exports: Vec<Ex>) -> Evaluator {
    let mut e = Evaluator::new();
    e.install_project(&project());
    e.load_bank(bank(&modules, &exports, &[(48000, false), (24000, false)]));
    e
}

fn walk(e: &mut Evaluator, log: &mut Log) {
    for _ in 0..BLOCKS_PER_WALK {
        e.block(log);
    }
}

#[test]
fn walks_every_sixth_block_starting_on_the_sixth() {
    let mut e = setup(vec![player_module(2)], vec![Ex { module: 0, kind: 1, name_id: 1, name: "c_test", at: None }]);
    let mut log = Log::default();
    let walked: Vec<bool> = (0..13).map(|_| e.block(&mut log)).collect();
    assert_eq!(walked.iter().positions(), vec![5, 11]);
}

trait Positions {
    fn positions(self) -> Vec<usize>;
}
impl<'a, I: Iterator<Item = &'a bool>> Positions for I {
    fn positions(self) -> Vec<usize> {
        self.enumerate().filter(|e| *e.1).map(|e| e.0).collect()
    }
}

#[test]
fn post_opens_pushes_restarts_on_an_edge_and_release_destroys() {
    let mut e = setup(vec![player_module(1)], vec![Ex { module: 0, kind: 1, name_id: 1, name: "c_test", at: None }]);
    let class = e.class_id("c_test").unwrap();
    let mut log = Log::default();
    let node = e.post(class, &[1, 4096, 1]);
    assert_eq!(e.instances().len(), 1);
    // refcount: poster + destructor client + ClassData client.
    assert_eq!(e.node_refcount(node), Some(3));
    // Capacity 1: a second post creates nothing but still succeeds.
    let extra = e.post(class, &[1, 4096, 0]);
    assert_eq!(e.instances().len(), 1);
    e.release(extra);
    assert_eq!(e.node_refcount(extra), None);

    walk(&mut e, &mut log);
    assert_eq!(log.calls, ["open v1 slot 1", "set v1 0=4096"]);
    // Outputs: time left / current written in the same walk.
    let (id, ..) = e.instances()[0];
    let mem = e.instance_memory(id).unwrap();
    assert_eq!((i32_at(mem, 80 + 40), i32_at(mem, 80 + 44)), (500, 7));

    // A payload change pushes only the changed input, and clamps (0..65535).
    log.calls.clear();
    e.redeliver(node, &[1, 99999, 1]);
    walk(&mut e, &mut log);
    assert_eq!(log.calls, ["set v1 0=65535"]);

    // The voice ends: released, outputs cleared; play control is still 1 → no restart.
    log.calls.clear();
    log.alive.insert(1, false);
    walk(&mut e, &mut log);
    walk(&mut e, &mut log);
    assert_eq!(log.calls, ["release v1"]);
    let mem = e.instance_memory(id).unwrap();
    assert_eq!((i32_at(mem, 80 + 40), u32_at(mem, 80 + 8)), (0, 0));

    // A 0 → 1 edge restarts, and the open applies the full input set again.
    log.calls.clear();
    e.redeliver(node, &[0, 4096, 0]);
    walk(&mut e, &mut log);
    e.redeliver(node, &[1, 4096, 0]);
    walk(&mut e, &mut log);
    assert_eq!(log.calls, ["open v2 slot 0", "set v2 0=4096"]);

    // Release: the destructor pulse reaches Destroy in the same walk; the voice is released and
    // the node freed once every client is gone.
    log.calls.clear();
    e.release(node);
    assert_eq!(e.node_refcount(node), Some(2));
    walk(&mut e, &mut log);
    assert_eq!(log.calls, ["release v2"]);
    assert!(e.instances().is_empty());
    assert_eq!(e.node_refcount(node), None);
    // Capacity is free again.
    e.post(class, &[0, 0, 0]);
    assert_eq!(e.instances().len(), 1);
}

#[test]
fn pause_and_resume_follow_the_play_control() {
    let mut e = setup(vec![player_module(1)], vec![Ex { module: 0, kind: 1, name_id: 1, name: "c_test", at: None }]);
    let class = e.class_id("c_test").unwrap();
    let mut log = Log::default();
    let node = e.post(class, &[1, 4096, 0]);
    walk(&mut e, &mut log);
    log.calls.clear();
    e.redeliver(node, &[2, 4096, 0]);
    walk(&mut e, &mut log);
    e.redeliver(node, &[1, 4096, 0]);
    walk(&mut e, &mut log);
    assert_eq!(log.calls, ["pause v1", "resume v1"]);
    // A voice that died while paused is not restarted by 2 → 1.
    log.calls.clear();
    e.redeliver(node, &[2, 4096, 0]);
    walk(&mut e, &mut log);
    log.alive.insert(1, false);
    let (id, ..) = e.instances()[0];
    // Simulate the device dropping the handle while paused (the program sees no voice).
    {
        let i = e.instance_mut(id).unwrap();
        put_u32(&mut i.mem, 80 + 8, 0);
    }
    e.redeliver(node, &[1, 4096, 0]);
    walk(&mut e, &mut log);
    assert_eq!(log.calls, ["pause v1"]);
}

/// Requester (c_req): Create → op 5 calls f_msg with param 5; reads g_snd through a GlobalVariable
/// state into a word we inspect. Utility (c_util): op 37 on f_msg → op 39 publishes the parameter
/// into g_snd.
fn round_trip_modules() -> (Vec<Spec>, Vec<Ex>) {
    // Requester: destructor @24 (20 B), global state @44 (28 B), Create @72, op 5 @76 (handle 8 B,
    // flag 0, n 1 → inputs {trig @88, param @92}), sink word @96, Destroy @100, size 116.
    let mut t = words(29);
    put_i32(&mut t, 72, 1);
    put_u8(&mut t, 76 + 9, 1);
    put_i32(&mut t, 92, 5);
    let requester = Spec {
        max: 4,
        globals: 1,
        functions: 0,
        destructor: true,
        class_data: false,
        players: vec![],
        template: t,
        program: vec![
            (0, vec![(-1, 100 + 12 - 24)], 24),
            (2, vec![(-1, 96 - 44)], 44),
            (3, vec![(-1, 88 - 72)], 72),
            (5, vec![], 76),
            (4, vec![], 100),
        ],
        group_ptrs: vec![],
    };
    // Utility: function state @24 (n 1 → (1 + 7)·4 = 32 B → @24..56), op 39 @56 (handle, min,
    // max, prev, value → 24 B), Destroy @80, size 96.
    let mut t = words(24);
    put_u8(&mut t, 24 + 24, 1);
    put_i32(&mut t, 56 + 8, 0);
    put_i32(&mut t, 56 + 12, 1000);
    put_i32(&mut t, 56 + 16, 0x7FFF_FFFE);
    put_i32(&mut t, 56 + 20, 0x7FFF_FFFE);
    let utility = Spec {
        max: 1,
        globals: 0,
        functions: 1,
        destructor: false,
        class_data: false,
        players: vec![],
        template: t,
        // op 37: on a call, copy the parameter (+28) into op 39's value (+20).
        program: vec![(37, vec![(28, 56 + 20 - 24)], 24), (39, vec![], 56), (4, vec![], 80)],
        group_ptrs: vec![],
    };
    let exports = vec![
        Ex { module: 0, kind: 1, name_id: 3, name: "c_req", at: None },
        Ex { module: 0, kind: 0, name_id: 1, name: "g_snd", at: Some(44) },
        Ex { module: 0, kind: 2, name_id: 1, name: "f_msg", at: Some(76) },
        Ex { module: 1, kind: 1, name_id: 2, name: "c_util", at: None },
        Ex { module: 1, kind: 2, name_id: 1, name: "f_msg", at: Some(24) },
        Ex { module: 1, kind: 0, name_id: 1, name: "g_snd", at: Some(56) },
    ];
    (vec![requester, utility], exports)
}

#[test]
fn function_call_and_global_publish_follow_walk_order() {
    let (modules, exports) = round_trip_modules();
    let mut e = setup(modules, exports);
    let mut log = Log::default();
    let g = e.global_id("g_snd").unwrap();
    assert_eq!(e.global(g), Some(77)); // csi default
    e.post(e.class_id("c_util").unwrap(), &[]);
    let req = e.post(e.class_id("c_req").unwrap(), &[]);
    let (rid, ..) = e.instances()[0]; // newest first: the requester runs first
    // A new subscriber copies the global's current value at creation.
    assert_eq!(i32_at(e.instance_memory(rid).unwrap(), 44 + 24), 77);
    walk(&mut e, &mut log);
    // Walk 1: the requester read the old value (77), then called f_msg; the utility (older, later
    // in the walk) published 5 in the same walk, delivered synchronously into the requester's state.
    let mem = e.instance_memory(rid).unwrap();
    assert_eq!(i32_at(mem, 96), 77);
    assert_eq!(i32_at(mem, 44 + 24), 5);
    assert_eq!(e.global(g), Some(5));
    walk(&mut e, &mut log);
    assert_eq!(i32_at(e.instance_memory(rid).unwrap(), 96), 5);
    // Release: the requester ends; its subscription is gone.
    e.release(req);
    walk(&mut e, &mut log);
    assert_eq!(e.instances().len(), 1);
    assert!(e.registry.globals[g].subscribers.is_empty());
    // Setting the same value notifies nobody; a different one is stored.
    e.set_global(g, 5);
    e.set_global(g, 9);
    assert_eq!(e.global(g), Some(9));
}

#[test]
fn unload_bank_destroys_its_instances_and_stops_answering_posts() {
    let mut e = setup(vec![player_module(2)], vec![Ex { module: 0, kind: 1, name_id: 1, name: "c_test", at: None }]);
    let class = e.class_id("c_test").unwrap();
    let mut log = Log::default();
    e.post(class, &[1, 4096, 0]);
    walk(&mut e, &mut log);
    e.unload_bank(0, &mut log);
    assert!(e.instances().is_empty());
    assert_eq!(log.calls.last().map(String::as_str), Some("release v1"));
    e.post(class, &[1, 4096, 0]);
    assert!(e.instances().is_empty());
}

