//! Ped conversations (doc 26 "Ped conversations"): the conversation object a ped spawns with
//! SpawnConversationArea. Retail (TU3, evidence only; re-implemented;
//! `.local/research/peds/b23-ped-plugins-conversations.md`, `b24-conversation-object.md`, main
//! checked the vtable, the completion state and the spawn constants):
//! - a conversation is a waypoint plugin entity (ctor `82E1CEC0`, interface vtable `0x8232B868`)
//!   with up to 3 member slots (participant, in-position bit) and a state (`D+1172`);
//! - gathering (vf4) while it has room and the state is 0 or 1; a ped may join (vf8) when its
//!   entity type fills a free slot of the chosen row, or of any candidate row before one is chosen;
//! - 3 waypoints (`82E1D620`) on a 1.5 m circle 120 deg apart from a random start in [0, pi);
//!   a ped locks the nearest free one (vfunc 144 `82E1CBC0`);
//! - when every member signalled "in position" (vf44) it starts (`82E1EBC0`): fewer than 2
//!   members ends it, else a candidate row is picked at random, the per-row value list gives one
//!   value, state 2, the first member speaks;
//! - each turn lasts the speaker's 3.0 s ConversationSpeak timer, then vf48 advances (`82E1ECB0`):
//!   state + 1, the next occupied member speaks; state 7 is complete (vf36): 5 turns.
//! The line id per state (`82E1EA98`: 2 -> 0 or 1, 3 -> 3, 4 -> 5, 5 -> 6, 6 -> 7) is kept for the
//! speech side; its meaning is open.

use super::super::Vec3;

/// Member slots (`D+1040`, 3 inline).
pub const MAX_MEMBERS: usize = 3;
/// The state at which a conversation is complete (vf36).
pub const COMPLETE: u8 = 7;

/// Retail values (data-driven; world tuning later).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConversationParams {
    /// Waypoint circle radius, m, and waypoint count (`82E1D620`).
    pub radius: f32,
    pub waypoints: usize,
    /// No new conversation within this distance of another, m (`0x8220E13C`).
    pub exclusion: f32,
    /// The area goes this far ahead of the starter, m (`0x821DBCEC`).
    pub ahead: f32,
    /// A speaker's turn, s (ConversationSpeak timer 30).
    pub turn_seconds: f32,
}

impl Default for ConversationParams {
    fn default() -> Self {
        Self { radius: 1.5, waypoints: 3, exclusion: 50.0, ahead: 3.0, turn_seconds: 3.0 }
    }
}

/// One conversation row (`livingworld_conversations`): the entity types of its participants
/// (`7819`) and its value list (`36F1`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConversationRow {
    pub name: String,
    pub participants: Vec<String>,
    pub values: Vec<i32>,
}

/// A member slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Member {
    pub ped: u64,
    pub in_position: bool,
}

/// One conversation (host-owned plain data).
#[derive(Clone, Debug, PartialEq)]
pub struct Conversation {
    pub id: u64,
    pub center: Vec3,
    /// Waypoint positions and who holds each.
    pub waypoints: Vec<(Vec3, Option<u64>)>,
    pub members: Vec<Member>,
    /// Candidate rows (indices into the host's row list) and the chosen one.
    pub candidates: Vec<usize>,
    pub row: Option<usize>,
    pub value: Option<i32>,
    pub state: u8,
    pub speaker: usize,
}

/// Uniform [0, 1) from the host's seeded generator.
pub type Rand<'a> = &'a mut dyn FnMut() -> f32;

impl Conversation {
    /// `82E1D620`: the area at `center` with its waypoints.
    pub fn new(id: u64, center: Vec3, candidates: Vec<usize>, params: &ConversationParams, rand: Rand) -> Self {
        let start = rand() * std::f32::consts::PI;
        let step = std::f32::consts::TAU / params.waypoints.max(1) as f32;
        let waypoints = (0..params.waypoints)
            .map(|i| {
                let a = start + step * i as f32;
                ([center[0] + a.sin() * params.radius, center[1], center[2] + a.cos() * params.radius], None)
            })
            .collect();
        Self { id, center, waypoints, members: Vec::new(), candidates, row: None, value: None, state: 0, speaker: 0 }
    }

    /// vf4.
    pub fn is_gathering(&self) -> bool {
        self.members.len() < MAX_MEMBERS && self.state <= 1
    }

    /// vf8: `entity` fills a free slot of the chosen row, or of a candidate row before one is chosen.
    pub fn allowed_to_join(&self, entity: &str, member_types: &[String], rows: &[ConversationRow]) -> bool {
        let fits = |r: &ConversationRow| {
            let mut free = r.participants.clone();
            for t in member_types {
                if let Some(i) = free.iter().position(|p| p == t) {
                    free.remove(i);
                }
            }
            free.iter().any(|p| p == entity)
        };
        match self.row {
            Some(r) => rows.get(r).is_some_and(fits),
            None => self.candidates.iter().filter_map(|&r| rows.get(r)).any(fits),
        }
    }

    pub fn join(&mut self, ped: u64) -> bool {
        if !self.is_gathering() || self.members.iter().any(|m| m.ped == ped) {
            return false;
        }
        self.members.push(Member { ped, in_position: false });
        true
    }

    /// vfunc 144: the nearest free waypoint, locked for `ped`.
    pub fn lock_closest_waypoint(&mut self, ped: u64, at: Vec3) -> Option<Vec3> {
        if let Some(w) = self.waypoints.iter().find(|w| w.1 == Some(ped)) {
            return Some(w.0);
        }
        let d2 = |p: Vec3| (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2);
        let i = self.waypoints.iter().enumerate().filter(|w| w.1 .1.is_none()).min_by(|a, b| d2(a.1 .0).total_cmp(&d2(b.1 .0))).map(|w| w.0)?;
        self.waypoints[i].1 = Some(ped);
        Some(self.waypoints[i].0)
    }

    pub fn unlock_waypoint(&mut self, ped: u64) {
        for w in &mut self.waypoints {
            if w.1 == Some(ped) {
                w.1 = None;
            }
        }
    }

    pub fn waypoint_of(&self, ped: u64) -> Option<Vec3> {
        self.waypoints.iter().find(|w| w.1 == Some(ped)).map(|w| w.0)
    }

    /// vf44, then `82E1EBC0` when every member is in position.
    pub fn signal_in_position(&mut self, ped: u64, rows: &[ConversationRow], rand: Rand) {
        if let Some(m) = self.members.iter_mut().find(|m| m.ped == ped) {
            m.in_position = true;
        }
        if self.state <= 1 && self.members.iter().all(|m| m.in_position) {
            self.start(rows, rand);
        }
    }

    fn start(&mut self, rows: &[ConversationRow], rand: Rand) {
        if self.members.len() < 2 {
            // vf52 (body not read): the conversation ends.
            self.state = COMPLETE;
            return;
        }
        if self.row.is_none() && !self.candidates.is_empty() {
            let i = ((rand() * self.candidates.len() as f32) as usize).min(self.candidates.len() - 1);
            self.row = Some(self.candidates[i]);
        }
        if self.value.is_none() {
            let list = self.row.and_then(|r| rows.get(r)).map(|r| r.values.as_slice()).unwrap_or(&[]);
            self.value = Some(if list.is_empty() { 6 } else { list[((rand() * list.len() as f32) as usize).min(list.len() - 1)] });
        }
        self.state = 2;
        self.speaker = 0;
    }

    /// vf12: the speaking member.
    pub fn speaker(&self) -> Option<u64> {
        (self.state >= 2 && self.state < COMPLETE).then(|| self.members.get(self.speaker).map(|m| m.ped)).flatten()
    }

    /// vf48 / `82E1ECB0`: the turn passes.
    pub fn pass_turn(&mut self) {
        if self.state < 2 || self.state >= COMPLETE {
            return;
        }
        self.state += 1;
        if !self.members.is_empty() {
            self.speaker = (self.speaker + 1) % self.members.len();
        }
    }

    /// vf36.
    pub fn is_complete(&self) -> bool {
        self.state >= COMPLETE
    }

    /// `82E1EA98`: the line id for the current state (`rand` for state 2's 0 / 1).
    pub fn line_id(&self, rand: Rand) -> Option<u8> {
        Some(match self.state {
            2 => u8::from(rand() >= 0.5),
            3 => 3,
            4 => 5,
            5 => 6,
            6 => 7,
            _ => return None,
        })
    }

    pub fn leave(&mut self, ped: u64) {
        self.unlock_waypoint(ped);
        if let Some(i) = self.members.iter().position(|m| m.ped == ped) {
            self.members.remove(i);
            if self.speaker >= self.members.len() {
                self.speaker = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversation_gathers_starts_and_runs_five_turns() {
        let rows = vec![ConversationRow { name: "r".into(), participants: vec!["adult_male".into(), "teen_male".into()], values: vec![4] }];
        let mut seq = [0.0f32, 0.0, 0.0, 0.0].into_iter().cycle();
        let mut rand = || seq.next().unwrap();
        let mut c = Conversation::new(1, [0.0; 3], vec![0], &ConversationParams::default(), &mut rand);
        assert_eq!(c.waypoints.len(), 3);
        assert!((c.waypoints[0].0[2] - 1.5).abs() < 1e-5, "start angle 0 -> +z");
        assert!(c.allowed_to_join("teen_male", &["adult_male".into()], &rows));
        assert!(!c.allowed_to_join("adult_male", &["adult_male".into()], &rows), "no free adult_male slot");
        assert!(c.join(10) && c.join(11));
        let a = c.lock_closest_waypoint(10, [0.0, 0.0, 5.0]).unwrap();
        assert_eq!(a, c.waypoints[0].0);
        assert_ne!(c.lock_closest_waypoint(11, [0.0, 0.0, 5.0]).unwrap(), a, "a locked waypoint is skipped");
        c.signal_in_position(10, &rows, &mut rand);
        assert_eq!(c.state, 0, "waits for every member");
        c.signal_in_position(11, &rows, &mut rand);
        assert_eq!((c.state, c.row, c.value, c.speaker()), (2, Some(0), Some(4), Some(10)));
        let mut speakers = Vec::new();
        while !c.is_complete() {
            speakers.push(c.speaker().unwrap());
            c.pass_turn();
        }
        assert_eq!(speakers, vec![10, 11, 10, 11, 10]);
        assert!(!c.is_gathering());
    }

    #[test]
    fn a_lone_member_ends_the_conversation() {
        let mut rand = || 0.5f32;
        let mut c = Conversation::new(1, [0.0; 3], vec![], &ConversationParams::default(), &mut rand);
        c.join(10);
        c.signal_in_position(10, &[], &mut rand);
        assert!(c.is_complete());
    }
}
