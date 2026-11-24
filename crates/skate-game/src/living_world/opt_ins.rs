//! Living world features still in test, off by default: zombie mode, grab visualizer, ped vehicle flee. NPC skater
//! physics is part of the living world, not an opt-in (`npc_sim`, USER-ASKS 720).
//! Switches and mod reach: docs/hails-additions/26a-living-world-overview.md#living-world-opt-ins
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptIn {
    Zombie,
    /// Grab cues in the world and grab availability log lines (`grab_view`); a viewer aid, nothing changes in play.
    VisualizeGrabs,
    /// Peds flee a car moving along the line to them, either way (retail's `82E2C0C8` test, W8; still in test).
    VehicleFlee,
    /// The retail challenges (own the spot): discovery, the sign-up prompt, runs, the HUD and the saved records
    /// (docs/hails-additions/42-challenges.md, still being built).
    Challenges,
    /// Create-a-Skater's Remove on a top or pants leaves the base sports bra (bare torso) / short shorts on, the parts
    /// the tattoo editors wear (user ask 713; ours, not retail).
    BaseOutfit,
}

impl OptIn {
    pub(crate) const ALL: [OptIn; 5] = [OptIn::Zombie, OptIn::VisualizeGrabs, OptIn::VehicleFlee, OptIn::Challenges, OptIn::BaseOutfit];

    /// The variable that turns it on for a run.
    pub(crate) fn env(self) -> &'static str {
        match self {
            OptIn::Zombie => "SKATE_ZOMBIE",
            OptIn::VisualizeGrabs => "SKATE_VISUALIZE_GRABS",
            OptIn::VehicleFlee => "SKATE_PED_VEHICLE_FLEE",
            OptIn::Challenges => "SKATE_CHALLENGES",
            OptIn::BaseOutfit => "SKATE_BASE_OUTFIT",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            OptIn::Zombie => "Zombie Mode",
            OptIn::VisualizeGrabs => "Visualize Grabs",
            OptIn::VehicleFlee => "Ped Vehicle Flee",
            OptIn::Challenges => "Career and Challenges",
            OptIn::BaseOutfit => "Base Outfit",
        }
    }
    /// The overlay's status line for the row (what it does and that it is still being tested).
    pub(crate) fn hint(self) -> &'static str {
        match self {
            OptIn::Zombie => "Zombie mode (the retail cheat, until the cheat screen exists): yellow screen, no traffic, every ped comes after you.",
            OptIn::VisualizeGrabs => "Shows the props and car spots in grab range (green = grab now) and logs grab availability.",
            OptIn::VehicleFlee => "Peds run from a car within 10 m that drives toward or away from them (still being tested).",
            OptIn::Challenges => "Career mode, the Skate 101 tutorials and own the spot challenges: discovery, the D-pad Up sign-up, runs, their HUD and saved records (still being built).",
            OptIn::BaseOutfit => "In Create-a-Skater, removing a top or pants leaves the base sports bra or short shorts on.",
        }
    }
    fn env_on(self) -> bool {
        let named = |v: &str| std::env::var(v).is_ok_and(|v| !v.is_empty());
        std::env::var(self.env()).is_ok_and(|v| v == "1")
            || (self == OptIn::Challenges
                && ["SKATE_CHALLENGE", "SKATE_VERIFY_CHALLENGE", "SKATE_CONTEST", "SKATE_RACE", "SKATE_FTL", "SKATE_FILM", "SKATE_HOM"].into_iter().any(named))
    }
    /// Session log key.
    fn log_key(self) -> &'static str {
        match self {
            OptIn::Zombie => "zombie",
            OptIn::VisualizeGrabs => "visualize_grabs",
            OptIn::VehicleFlee => "ped_vehicle_flee",
            OptIn::Challenges => "challenges",
            OptIn::BaseOutfit => "base_outfit",
        }
    }
}

/// The player's choices. Career and Challenges is on by default (2026-11-20); the others are off. Files from before
/// USER-ASKS 720 may hold `npc_physics`; unknown keys are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct OptIns {
    pub zombie: bool,
    pub visualize_grabs: bool,
    pub vehicle_flee: bool,
    pub challenges: bool,
    pub base_outfit: bool,
}

impl Default for OptIns {
    fn default() -> Self {
        Self { zombie: false, visualize_grabs: false, vehicle_flee: false, challenges: true, base_outfit: false }
    }
}

impl OptIns {
    /// The `SKATE_*=1` variables of this run.
    pub(crate) fn from_env() -> Self {
        let mut s = Self { zombie: false, visualize_grabs: false, vehicle_flee: false, challenges: false, base_outfit: false };
        for o in OptIn::ALL {
            s.set(o, o.env_on());
        }
        s
    }
    pub(crate) fn get(&self, o: OptIn) -> bool {
        match o {
            OptIn::Zombie => self.zombie,
            OptIn::VisualizeGrabs => self.visualize_grabs,
            OptIn::VehicleFlee => self.vehicle_flee,
            OptIn::Challenges => self.challenges,
            OptIn::BaseOutfit => self.base_outfit,
        }
    }
    pub(crate) fn set(&mut self, o: OptIn, on: bool) {
        match o {
            OptIn::Zombie => self.zombie = on,
            OptIn::VisualizeGrabs => self.visualize_grabs = on,
            OptIn::VehicleFlee => self.vehicle_flee = on,
            OptIn::Challenges => self.challenges = on,
            OptIn::BaseOutfit => self.base_outfit = on,
        }
    }
    /// Either one on (saved choice or this run's variable).
    /// An opt-in whose `SKATE_*` variable is "0" is off for this run (an automated run without challenges).
    pub(crate) fn without_env_offs(self) -> Self {
        let mut s = self;
        for o in OptIn::ALL {
            if std::env::var(o.env()).is_ok_and(|v| v == "0") {
                s.set(o, false);
            }
        }
        s
    }

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
        // The player's own choice only: a viewer aid, not a mod tuning.
        visualize_grabs: settings.user_opt_ins.visualize_grabs,
        vehicle_flee: settings.ped_brain.vehicle_flee.enabled,
        // The player's own choice only (mods use sdk.challenges, which works only while it is on).
        challenges: settings.user_opt_ins.challenges,
        // The player's own choice only.
        base_outfit: settings.user_opt_ins.base_outfit,
    }
}

/// Session log line for one opt-in switching: `OPT_IN_CHANGED <name> off -> on by player`. `by` is `player` (the
/// overlay, or the player's choice back in effect) or `mod`. Every opt-in uses it, also the overlay's audio and
/// water ones, so one search finds every toggle of a session.
pub(crate) fn opt_in_changed_line(name: &str, old: bool, new: bool, by: &str) -> String {
    let word = |on: bool| if on { "on" } else { "off" };
    format!("OPT_IN_CHANGED {name} {} -> {} by {by}", word(old), word(new))
}
pub(crate) fn log_opt_in_changed(name: &str, old: bool, new: bool, by: &str) {
    if old != new {
        info!("{}", opt_in_changed_line(name, old, new, by));
    }
}

/// The OPT_IN_CHANGED lines from `old` to `now`; a value that differs from the player's choice is a mod's.
fn changes(old: OptIns, now: OptIns, user: OptIns) -> Vec<String> {
    OptIn::ALL.iter().filter(|o| old.get(**o) != now.get(**o))
        .map(|o| opt_in_changed_line(o.log_key(), old.get(*o), now.get(*o), if now.get(*o) == user.get(*o) { "player" } else { "mod" }))
        .collect()
}

/// Session log: an opt-in switching (start, overlay or mod), so a report shows which ones were on and when each one
/// was toggled.
pub(crate) fn log_opt_ins(settings: Res<super::LivingWorldSettings>, mut last: Local<Option<OptIns>>) {
    let now = in_effect(&settings);
    if *last != Some(now) {
        if let Some(old) = *last {
            for line in changes(old, now, settings.user_opt_ins) {
                info!("{line}");
            }
        }
        let on: Vec<&str> = OptIn::ALL.iter().filter(|o| now.get(**o)).map(|o| o.log_key()).collect();
        info!("OPT_IN {}", if on.is_empty() { "none".to_owned() } else { on.join(" ") });
        *last = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_and_default_challenges_on() {
        assert_eq!(serde_json::from_str::<OptIns>("{}").unwrap(), OptIns::default());
        assert_eq!(OptIns::default(), OptIns { zombie: false, visualize_grabs: false, vehicle_flee: false, challenges: true, base_outfit: false }, "challenges on by default (2026-11-20)");
        assert_eq!(serde_json::from_str::<OptIns>(r#"{"npc_physics":false}"#).unwrap(), OptIns::default(), "a file from before USER-ASKS 720 still loads");
        let mut s = OptIns { challenges: false, ..OptIns::default() };
        for o in OptIn::ALL {
            assert!(!s.get(o));
            s.set(o, true);
            assert!(s.get(o));
        }
        let back: OptIns = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(OptIns::default().or(OptIns { zombie: true, ..Default::default() }).zombie, true);
    }

    /// The player's choice is in effect at once, a mod may change it while it runs, and undoing the mod brings the
    /// choice back.
    #[test]
    fn mod_reset_restores_the_players_choice() {
        let mut s = super::super::LivingWorldSettings::default();
        s.set_user_opt_ins(OptIns { zombie: true, ..Default::default() });
        assert!(s.zombie && s.npc_simulated.enabled);
        s.zombie = false;
        s.npc_simulated.enabled = false;
        s.reset_mod_overrides();
        assert!(s.zombie && s.npc_simulated.enabled, "NPC skater physics is always on again once the mod stops");
        assert_eq!(in_effect(&s).zombie, true);
    }

    /// One OPT_IN_CHANGED line per opt-in that switched, naming who switched it.
    #[test]
    fn changes_name_each_toggle_and_its_source() {
        let off = OptIns { challenges: false, ..OptIns::default() };
        let zombie = OptIns { zombie: true, ..off };
        assert_eq!(changes(off, zombie, zombie), vec!["OPT_IN_CHANGED zombie off -> on by player"]);
        assert_eq!(changes(zombie, off, zombie), vec!["OPT_IN_CHANGED zombie on -> off by mod"]);
        assert!(changes(zombie, zombie, off).is_empty());
        let both = OptIns { zombie: true, visualize_grabs: true, ..off };
        assert_eq!(changes(off, both, both), vec![
            "OPT_IN_CHANGED zombie off -> on by player",
            "OPT_IN_CHANGED visualize_grabs off -> on by player",
        ]);
    }
}
