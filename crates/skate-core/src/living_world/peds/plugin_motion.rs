//! The ped motion graph's plugin states (`MotionGraph_Pedestrian.xml` includes `motiongraph_sit.xml`,
//! `motiongraph_atm.xml`, `motiongraph_vend.xml`, `motiongraph_waterfountain.xml`, `motiongraph_newspaperbox.xml`)
//! [data, b84 / b85]: entered on the monitored packet's name, each state plays its logical clips in order
//! (`PlayRemappedAnimation`, blend 0.1, the next on `WillExpire 0.01`); a looping clip holds until the packet's next
//! stage intent appears (`StandUp`, `FinishDrinking`); the last step ends the packet (`MajorIntentComplete`, after
//! `IntentStageComplete decrement` on the machines). The clip behind each logical name is the ped's anim set remap.
//!
//! NOT RETAIL YET: a step table in place of the motion graph itself (the full graph is a later milestone); the hand
//! props the vending machine and the newspaper box spawn in their branch window (`SpawnInteractionBasedHandProp`), the
//! sit-avoid states and the Collision exits are not ported.

/// One clip step; `hold_until` = a looping clip held until the packet's current stage intent is that name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionStep {
    pub anim: &'static str,
    pub hold_until: Option<&'static str>,
}

/// One plugin state: the packet it runs for, its steps and whether its end steps the packet back
/// (`IntentStageComplete decrement="true"`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PluginMotion {
    pub packet: &'static str,
    pub steps: &'static [MotionStep],
    pub decrement: bool,
}

/// Every step's blend (`blendTime="0.1"`).
pub const BLEND: f32 = 0.1;

const fn clip(anim: &'static str) -> MotionStep {
    MotionStep { anim, hold_until: None }
}

const fn hold(anim: &'static str, until: &'static str) -> MotionStep {
    MotionStep { anim, hold_until: Some(until) }
}

pub const MOTIONS: &[PluginMotion] = &[
    PluginMotion { packet: "Sit", steps: &[clip("Stand2Sit"), hold("SitIdleCyc", "StandUp"), clip("Sit2Stand")], decrement: false },
    PluginMotion { packet: "UseATM", steps: &[clip("ATMInsertCard"), clip("ATMMakeSelection"), clip("ATMCollectMoney"), clip("ATMCollectCard")], decrement: true },
    PluginMotion { packet: "UseVendingMachine", steps: &[clip("VendInsert"), clip("VendSelect"), clip("VendCollect")], decrement: true },
    PluginMotion { packet: "UseWaterFountain", steps: &[clip("WaterFountainInto"), hold("WaterFountainCyc", "FinishDrinking"), clip("WaterFountainOut")], decrement: true },
    PluginMotion { packet: "UseNewspaperBox", steps: &[clip("NewspaperCollect")], decrement: true },
];

/// The plugin state for a packet name.
pub fn motion_for(packet: &str) -> Option<&'static PluginMotion> {
    MOTIONS.iter().find(|m| m.packet == packet)
}

/// The logical names the plugin states play (for the anim set loader).
pub fn names() -> impl Iterator<Item = &'static str> {
    MOTIONS.iter().flat_map(|m| m.steps.iter().map(|s| s.anim))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plugin_state_is_listed_and_its_names_load() {
        assert_eq!(motion_for("Sit").unwrap().steps[1], MotionStep { anim: "SitIdleCyc", hold_until: Some("StandUp") });
        assert!(motion_for("UseATM").unwrap().decrement && !motion_for("Sit").unwrap().decrement);
        assert!(motion_for("Converse").is_none());
        for n in names() {
            assert!(super::super::anim::names::ALL.contains(&n), "{n} not in names::ALL");
        }
    }
}
