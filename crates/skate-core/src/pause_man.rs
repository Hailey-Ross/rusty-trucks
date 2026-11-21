//! Retail's `Sk8::ThePauseMan` (830CFE24, research b1053): a stack of pause modes; the top mode's mask (table
//! 821D98D8) says which clients are paused. A client asks with its bit (82562148): paused when the request word and
//! the mask both hold it. Offline every request bit is set (the constructor's 0xFFFFFFFF), so the mask decides.

/// The mask of each pause mode (821D98D8, modes 0..=21). A mode past the table pauses nothing.
pub const MODE_MASKS: [u32; 22] = [
    0x0000_0000, 0xFFFF_FFFD, 0xFFFF_FFE5, 0xFFFF_FFE5, 0xFFFF_FFEC, 0xFFFF_FFE1, 0xFFFF_FFE1, 0xFFFF_FFE5,
    0xFFFF_FFE1, 0xFFFF_FFE5, 0xFFFF_FFE5, 0xFFFF_FFF5, 0xFFFF_FFF1, 0xFFFF_FFF1, 0xFFFF_FFE5, 0xFFFF_FFF5,
    0xFFFF_FFF5, 0xFFFF_FFF5, 0xFFFF_FFF5, 0xFFFF_FFE5, 0xFFFF_FFE5, 0xFFFF_FFFD,
];

/// The modes our hosts push.
pub mod mode {
    /// LuaBind_PauseSimulation (828409C8).
    pub const SIMULATION: u32 = 5;
    /// The replay NIS (8285EC08 pushes it after `NIS`).
    pub const REPLAY_NIS: u32 = 8;
    /// Any NIS while it plays (basic NIS start 8285E968, replay NIS enter 8285EC08).
    pub const NIS: u32 = 12;
}

/// The client bits.
pub mod bit {
    /// GameSession_StepSimulation (8285C928): the worlds step only while this is not paused.
    pub const WORLD: u32 = 1;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseMan {
    modes: Vec<u32>,
    /// +0x104: the requests (all set offline; the online flow clears and sets bits).
    pub requests: u32,
}

impl Default for PauseMan {
    fn default() -> Self {
        Self { modes: Vec::new(), requests: u32::MAX }
    }
}

impl PauseMan {
    /// 826DB498.
    pub fn push(&mut self, mode: u32) {
        self.modes.push(mode);
    }

    /// 826DB530: every entry of the mode goes.
    pub fn pop(&mut self, mode: u32) {
        self.modes.retain(|&m| m != mode);
    }

    /// +0x100: the top mode's mask (0 with no mode).
    pub fn mask(&self) -> u32 {
        self.modes.last().map_or(0, |&m| MODE_MASKS.get(m as usize).copied().unwrap_or(0))
    }

    /// 82562148 negated: the client of `bit` is paused.
    pub fn paused(&self, bit: u32) -> bool {
        self.requests & self.mask() & bit != 0
    }

    pub fn modes(&self) -> &[u32] {
        &self.modes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_mode_decides_and_a_pop_removes_every_entry() {
        let mut pm = PauseMan::default();
        assert!(!pm.paused(bit::WORLD), "no mode: nothing paused");
        pm.push(4);
        assert!(!pm.paused(bit::WORLD), "mode 4's mask lacks bit 0");
        pm.push(mode::SIMULATION);
        assert!(pm.paused(bit::WORLD));
        assert!(!pm.paused(2) && !pm.paused(4) && !pm.paused(8) && !pm.paused(16), "bits 1..4 run on under mode 5");
        assert!(pm.paused(32));
        pm.push(mode::NIS);
        pm.push(mode::REPLAY_NIS);
        pm.pop(mode::REPLAY_NIS);
        pm.pop(mode::NIS);
        assert!(pm.paused(bit::WORLD), "mode 5 still holds");
        pm.push(mode::SIMULATION);
        pm.pop(mode::SIMULATION);
        assert_eq!(pm.modes(), [4], "both mode 5 entries went");
        assert!(!pm.paused(bit::WORLD));
        pm.requests = 0;
        pm.push(mode::NIS);
        assert!(!pm.paused(bit::WORLD), "no request, no pause (online)");
    }
}
