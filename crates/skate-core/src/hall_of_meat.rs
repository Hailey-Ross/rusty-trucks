//! The Hall of Meat bail injuries (retail tracker 0x82DAF1D8, impacts 0x82DAE9A8 / 0x82DAE6F0, counts 0x82510990).
//! Tuning is `scoring_wipeouts/default` (BoneSet, impact scale); details: docs/hails-additions/31-retail-menus.md#career-stats-2026-10-25-user-asks-334

pub const BONES: usize = 25;
pub const LEVELS: usize = 6;

/// The impact each bone reads from the skeleton contact record (0x82DAE9A8): bones 0..22 part i + 1's force, bone
/// 23 part 1's specific force, bone 24 part 23's specific force. The record is Fill 0x82BD60C8's copy of the contact
/// array (+0x34 / +0x64 <- force / specific force).
pub fn part_impacts(force: &[f32; 24], specific_force: &[f32; 24]) -> [f32; BONES] {
    std::array::from_fn(|i| match i {
        23 => specific_force[1],
        24 => specific_force[23],
        _ => force[i + 1],
    })
}

/// One BoneSet entry (0x54 bytes): up to 4 links {bone, factor} that an impact spreads to, and 6 levels
/// {points, threshold} (a threshold of 0 turns the level off).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bone {
    pub links: [Option<(usize, f32)>; 4],
    pub levels: [(u32, f32); LEVELS],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    pub bones: [Bone; BONES],
    /// layout+0xBFC (`Hash_B7514DE7EBDE9D01`, retail 0.75); the part's impact float is doubled, then scaled by it.
    pub impact_scale: f32,
    /// An impact (or a spread share) must be above this to count (0x820641A8).
    pub floor: f32,
    /// The accumulator's decay per console tick (0x8208824C).
    pub decay: f32,
}

impl Tuning {
    /// The BoneSet field as big-endian words (25 x 21).
    pub fn from_words(words: &[u32], impact_scale: f32) -> Option<Self> {
        if words.len() != BONES * 21 {
            return None;
        }
        let mut bones = [Bone::default(); BONES];
        for (i, bone) in bones.iter_mut().enumerate() {
            let w = &words[i * 21..(i + 1) * 21];
            for (l, link) in bone.links.iter_mut().enumerate() {
                let id = w[1 + l * 2];
                *link = (id != u32::MAX && (id as usize) < BONES).then(|| (id as usize, f32::from_bits(w[2 + l * 2])));
            }
            for (k, level) in bone.levels.iter_mut().enumerate() {
                *level = (w[9 + k * 2], f32::from_bits(w[10 + k * 2]));
            }
        }
        Some(Self { bones, impact_scale, floor: 0.1, decay: 0.8 })
    }
}

/// One bail's bone state: the accumulator, its peak and the injury level (-1 none, 0..5).
#[derive(Clone, Debug, PartialEq)]
pub struct Injuries {
    accumulated: [f32; BONES],
    peak: [f32; BONES],
    pub level: [i8; BONES],
}

impl Default for Injuries {
    fn default() -> Self {
        Self { accumulated: [0.0; BONES], peak: [0.0; BONES], level: [-1; BONES] }
    }
}

/// The injury counts 0x82510990 makes from the levels (one per bone, at its highest level).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InjuryCounts {
    pub broken: u32,
    pub dislocations: u32,
    pub sprains: u32,
    pub bruises: u32,
    pub scrapes: u32,
}

impl Injuries {
    /// One tick: `part_impacts[i]` is bone i's contact impact float (before scaling); each over the floor adds to the
    /// bone and spreads along its links, then the levels are taken from the peaks and the accumulators decay.
    /// `ticks` is the step in console ticks (1 at 60 Hz) for the decay.
    pub fn tick(&mut self, tuning: &Tuning, part_impacts: &[f32; BONES], ticks: f32) {
        for (i, raw) in part_impacts.iter().enumerate() {
            let impact = raw * 2.0 * tuning.impact_scale;
            if impact > tuning.floor {
                self.add(tuning, i, impact, i);
            }
        }
        for i in 0..BONES {
            self.level[i] = (0..LEVELS)
                .rev()
                .find(|&k| {
                    let threshold = tuning.bones[i].levels[k].1;
                    threshold > 0.0 && self.peak[i] > threshold
                })
                .map_or(-1, |k| k as i8);
            self.accumulated[i] *= tuning.decay.powf(ticks);
        }
    }

    /// 0x82DAE6F0: add to the bone, keep the peak, pass factor x impact to each link but the one it came from.
    fn add(&mut self, tuning: &Tuning, bone: usize, impact: f32, from: usize) {
        self.accumulated[bone] += impact;
        self.peak[bone] = self.peak[bone].max(self.accumulated[bone]);
        for (link, factor) in tuning.bones[bone].links.into_iter().flatten() {
            let share = factor * impact;
            if link != from && share > tuning.floor {
                self.add(tuning, link, share, bone);
            }
        }
    }

    pub fn counts(&self) -> InjuryCounts {
        let mut c = InjuryCounts::default();
        for level in self.level {
            match level {
                0 => c.scrapes += 1,
                1 => c.bruises += 1,
                2 => c.sprains += 1,
                3 => c.dislocations += 1,
                4 | 5 => c.broken += 1,
                _ => {}
            }
        }
        c
    }

    /// Row 4: a level on bone 0 or 23 (the head's two channels); row 5: on bone 24 (groin).
    pub fn head_hit(&self) -> bool {
        self.level[0] >= 0 || self.level[23] >= 0
    }

    pub fn groin_hit(&self) -> bool {
        self.level[24] >= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> Tuning {
        let mut words = vec![0u32; BONES * 21];
        for i in 0..BONES {
            let w = &mut words[i * 21..(i + 1) * 21];
            w[0] = i as u32;
            for l in 0..4 {
                w[1 + l * 2] = u32::MAX;
            }
        }
        // Bone 0: links to 1 (x0.5); thresholds 7 / 14 / off / off / 21.
        words[1] = 1;
        words[2] = 0.5f32.to_bits();
        for (k, t) in [(0, 7.0f32), (1, 14.0), (4, 21.0)] {
            words[10 + k * 2] = t.to_bits();
        }
        // Bone 1: links back to 0 (never followed back) and a threshold 2 at level 0.
        words[21 + 1] = 0;
        words[21 + 2] = 1.0f32.to_bits();
        words[21 + 10] = 2.0f32.to_bits();
        Tuning::from_words(&words, 0.75).unwrap()
    }

    #[test]
    fn impacts_spread_rise_to_the_peak_level_and_decay() {
        let t = tuning();
        let mut s = Injuries::default();
        let mut hit = [0.0; BONES];
        hit[0] = 6.0; // x2 x0.75 = 9
        s.tick(&t, &hit, 1.0);
        assert_eq!((s.level[0], s.level[1]), (0, 0)); // 9 > 7; bone 1 gets 4.5 > 2
        s.tick(&t, &hit, 1.0); // 9 x 0.8 + 9 = 16.2 > 14
        assert_eq!(s.level[0], 1);
        for _ in 0..50 {
            s.tick(&t, &[0.0; BONES], 1.0);
        }
        assert_eq!(s.level[0], 1, "the peak keeps the level");
        assert!(s.head_hit() && !s.groin_hit());
        assert_eq!(s.counts(), InjuryCounts { bruises: 1, scrapes: 1, ..Default::default() });
    }

    #[test]
    fn a_small_impact_counts_nothing() {
        let mut s = Injuries::default();
        let mut hit = [0.0; BONES];
        hit[0] = 0.05; // 0.075 under the floor
        s.tick(&tuning(), &hit, 1.0);
        assert_eq!(s, Injuries { accumulated: [0.0; BONES], peak: [0.0; BONES], level: [-1; BONES] });
    }
}
