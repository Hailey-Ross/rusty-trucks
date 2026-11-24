//! The anywhere photos' photographer (ChallengeType 0x25, research b1057): in single player a spot the player places
//! from the phone, not a character. Retail keeps one manager on the presentation manager (+0x204): active while an
//! anywhere photo runs (825680D8 / 825681F8), placed by the phone option (826682B0), read by the objective calls
//! AnywherePhoto.HasUserPlacedPhotographer / GetDistanceToPhotographer (82568040 / 82568300).
//! docs/hails-additions/42-challenges.md#anywhere-photographer

/// The challenge type whose start arms the photographer (825680D8).
pub const ANYWHERE_PHOTO_TYPE: u32 = 0x25;
/// GetDistanceToPhotographer without a placed photographer (DAT_8216DEE0).
pub const NOT_PLACED: f32 = -1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Photographer {
    /// M+0x10: an anywhere photo runs.
    pub active: bool,
    /// M+0x11: the player placed the photographer.
    pub placed: bool,
    /// M+0x20: where (the player's root translation when placed).
    pub position: [f32; 3],
    /// M+0x30: the player's transform row 2 when placed (82568418 hands it out).
    pub direction: [f32; 3],
}

impl Photographer {
    /// Slot 6 (825680D8): a challenge of `challenge_type` started; only an anywhere photo arms it, unplaced, facing +z.
    pub fn start(&mut self, challenge_type: u32) -> bool {
        if challenge_type != ANYWHERE_PHOTO_TYPE || self.active {
            return false;
        }
        *self = Self { active: true, placed: false, position: [0.0; 3], direction: [0.0, 0.0, 1.0] };
        true
    }

    /// Slot 10 (825681F8): the challenge ended; everything clears.
    pub fn end(&mut self) {
        *self = Self::default();
    }

    /// The phone option (826682B0, mask 0x40): placed at the player's root, again whenever the player asks while
    /// active. False when no anywhere photo runs.
    pub fn place(&mut self, transform: &[[f32; 4]; 4]) -> bool {
        if !self.active {
            return false;
        }
        self.placed = true;
        self.position = [transform[3][0], transform[3][1], transform[3][2]];
        self.direction = [transform[2][0], transform[2][1], transform[2][2]];
        true
    }

    /// GetDistanceToPhotographer (82C61CC0 -> 82568300): -1 unless active and placed, else the 3D distance from the
    /// player's root; 0 without a player.
    pub fn distance(&self, player: Option<[f32; 3]>) -> f32 {
        if !(self.active && self.placed) {
            return NOT_PLACED;
        }
        player.map_or(0.0, |p| {
            let d = [p[0] - self.position[0], p[1] - self.position[1], p[2] - self.position[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, z: f32) -> [[f32; 4]; 4] {
        [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, 0.0, z, 1.0]]
    }

    #[test]
    fn only_an_anywhere_photo_arms_it_and_placing_measures_from_the_spot() {
        let mut p = Photographer::default();
        assert!(!p.start(0x26) && !p.place(&at(1.0, 1.0)), "spot photos have no placed photographer");
        assert!(p.start(ANYWHERE_PHOTO_TYPE) && !p.placed);
        assert_eq!(p.distance(Some([0.0; 3])), NOT_PLACED);
        assert!(p.place(&at(3.0, 4.0)));
        assert_eq!(p.distance(Some([0.0; 3])), 5.0);
        assert_eq!(p.distance(None), 0.0);
        assert!(p.place(&at(0.0, 1.0)), "placed again while active");
        assert_eq!(p.distance(Some([0.0; 3])), 1.0);
        p.end();
        assert_eq!((p, p.distance(Some([0.0; 3]))), (Photographer::default(), NOT_PLACED));
    }
}
