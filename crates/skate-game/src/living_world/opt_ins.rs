//! The living world features still being tested, off by default (opt-in): zombie mode, NPC skater physics,
//! skitching and prop grab. The player switches them in the PC overlay (Graphics > Living World), in effect at
//! once and saved in `settings/graphics.json`; `SKATE_ZOMBIE=1`, `SKATE_NPC_SIM=1`, `SKATE_SKITCH=1` and
//! `SKATE_PROP_GRAB=1` turn one on for a run. A mod may still change zombie mode and NPC skater physics
//! (`world.set_tuning("living_world", …)`); when it stops, the player's choice is back (`reset_mod_overrides`).
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptIn {
    Zombie,
    NpcPhysics,
    Skitch,
    PropGrab,
}

impl OptIn {
    pub(crate) const ALL: [OptIn; 4] = [OptIn::Zombie, OptIn::NpcPhysics, OptIn::Skitch, OptIn::PropGrab];

    /// The variable that turns it on for a run.
    pub(crate) fn env(self) -> &'static str {
        match self {
            OptIn::Zombie => "SKATE_ZOMBIE",
            OptIn::NpcPhysics => "SKATE_NPC_SIM",
            OptIn::Skitch => "SKATE_SKITCH",
            OptIn::PropGrab => "SKATE_PROP_GRAB",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            OptIn::Zombie => "Zombie Mode",
            OptIn::NpcPhysics => "NPC Skater Physics",
            OptIn::Skitch => "Skitching",
            OptIn::PropGrab => "Prop Grab",
        }
    }
    /// The overlay's status line for the row (what it does and that it is still being tested).
    pub(crate) fn hint(self) -> &'static str {
        match self {
            OptIn::Zombie => "Zombie mode (the retail cheat, until the cheat screen exists): yellow screen, no traffic, every ped comes after you.",
            OptIn::NpcPhysics => "NPC skaters near you run full skater physics and bail (still being tested).",
            OptIn::Skitch => "Skitching on traffic cars: ride up behind a car and hold grab (still being tested).",
            OptIn::PropGrab => "Carry props by their real handles, hands on the edge (still being tested).",
        }
    }
    fn env_on(self) -> bool {
        std::env::var(self.env()).is_ok_and(|v| v == "1")
    }
    /// Session log key.
    fn log_key(self) -> &'static str {
        match self {
            OptIn::Zombie => "zombie",
            OptIn::NpcPhysics => "npc_physics",
            OptIn::Skitch => "skitch",
            OptIn::PropGrab => "prop_grab",
        }
    }
}

/// The player's choices (all off = retail as far as it is ported).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct OptIns {
    pub zombie: bool,
    pub npc_physics: bool,
    pub skitch: bool,
    pub prop_grab: bool,
}

impl OptIns {
    /// The `SKATE_*=1` variables of this run.
    pub(crate) fn from_env() -> Self {
        let mut s = Self::default();
        for o in OptIn::ALL {
            s.set(o, o.env_on());
        }
        s
    }
    pub(crate) fn get(&self, o: OptIn) -> bool {
        match o {
            OptIn::Zombie => self.zombie,
            OptIn::NpcPhysics => self.npc_physics,
            OptIn::Skitch => self.skitch,
            OptIn::PropGrab => self.prop_grab,
        }
    }
    pub(crate) fn set(&mut self, o: OptIn, on: bool) {
        match o {
            OptIn::Zombie => self.zombie = on,
            OptIn::NpcPhysics => self.npc_physics = on,
            OptIn::Skitch => self.skitch = on,
            OptIn::PropGrab => self.prop_grab = on,
        }
    }
    /// Either one on (saved choice or this run's variable).
    pub(crate) fn or(self, other: Self) -> Self {
        let mut s = self;
        for o in OptIn::ALL {
            s.set(o, self.get(o) || other.get(o));
        }
        s
    }
}

/// What is on right now (the player's choice, or a mod's value while it runs).
pub(crate) fn in_effect(settings: &super::LivingWorldSettings) -> OptIns {
    OptIns {
        zombie: settings.zombie,
        npc_physics: settings.npc_simulated.enabled,
        skitch: crate::physics::skitch_enabled(),
        prop_grab: crate::physics::prop_dynamics::prop_grab_enabled(),
    }
}

/// Session log: an opt-in switching (start, overlay or mod), so a report shows which ones were on.
pub(crate) fn log_opt_ins(settings: Res<super::LivingWorldSettings>, mut last: Local<Option<OptIns>>) {
    let now = in_effect(&settings);
    if *last != Some(now) {
        let on: Vec<&str> = OptIn::ALL.iter().filter(|o| now.get(**o)).map(|o| o.log_key()).collect();
        info!("OPT_IN {}", if on.is_empty() { "none".to_owned() } else { on.join(" ") });
        *last = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_and_default_off() {
        assert_eq!(serde_json::from_str::<OptIns>("{}").unwrap(), OptIns::default());
        let mut s = OptIns::default();
        for o in OptIn::ALL {
            assert!(!s.get(o));
            s.set(o, true);
            assert!(s.get(o));
        }
        let back: OptIns = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(OptIns::default().or(OptIns { skitch: true, ..Default::default() }).skitch, true);
    }

    /// The player's choice is in effect at once, a mod may change it while it runs, and undoing the mod brings the
    /// choice back (skitch and prop grab stay off here: they are process-wide switches other tests read).
    #[test]
    fn mod_reset_restores_the_players_choice() {
        let mut s = super::super::LivingWorldSettings::default();
        s.set_user_opt_ins(OptIns { zombie: true, npc_physics: true, ..Default::default() });
        assert!(s.zombie && s.npc_simulated.enabled);
        s.zombie = false;
        s.npc_simulated.enabled = false;
        s.reset_mod_overrides();
        assert!(s.zombie && s.npc_simulated.enabled);
        assert_eq!(in_effect(&s).zombie, true);
    }
}
