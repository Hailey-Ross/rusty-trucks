//! A ped fleeing a prop that is moving at it (`82E2EDC0` -> `82E2F018` -> `82E2EF40`, ask 374).
//! Retail rules: docs/hails-additions/26g-living-world-ped-interactions.md#object-flee

use super::super::Vec3;

/// Retail values (data-driven).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectFleeParams {
    pub enabled: bool,
    /// Flat distance from the prop's centre (8 m: 64.0 squared at 0x820ED958).
    pub radius: f32,
    /// The prop's speed floor (0.75 m/s: 0.5625 squared at 0x822F915C).
    pub min_speed: f32,
    /// Room on either side of the prop's swept lane (0x821814A0).
    pub lane_margin: f32,
}

impl Default for ObjectFleeParams {
    fn default() -> Self {
        Self { enabled: true, radius: 8.0, min_speed: 0.75, lane_margin: 0.75 }
    }
}

/// A moving prop: box centre, linear velocity and its four footprint corners (world x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovingObject {
    pub centre: Vec3,
    pub velocity: Vec3,
    pub corners: [[f32; 2]; 4],
}

/// Whether a ped at `ped` should flee `object`: near it, the prop fast enough, the ped inside the lane the prop's
/// footprint sweeps and ahead of the prop's corner on its side.
pub fn in_path(ped: Vec3, object: &MovingObject, p: &ObjectFleeParams) -> bool {
    if !p.enabled {
        return false;
    }
    let to_ped = [ped[0] - object.centre[0], ped[2] - object.centre[2]];
    if to_ped[0] * to_ped[0] + to_ped[1] * to_ped[1] > p.radius * p.radius {
        return false;
    }
    let v = object.velocity;
    if v[0] * v[0] + v[1] * v[1] + v[2] * v[2] < p.min_speed * p.min_speed {
        return false;
    }
    let flat = (v[0] * v[0] + v[2] * v[2]).sqrt();
    if flat < 1e-5 {
        return false;
    }
    let side = [v[2] / flat, -v[0] / flat];
    let (mut low, mut high) = ((0.0f32, [object.centre[0], object.centre[2]]), (0.0f32, [object.centre[0], object.centre[2]]));
    for c in object.corners {
        let s = (c[0] - object.centre[0]) * side[0] + (c[1] - object.centre[2]) * side[1];
        if s > high.0 {
            high = (s, c);
        } else if s < low.0 {
            low = (s, c);
        }
    }
    let lateral = to_ped[0] * side[0] + to_ped[1] * side[1];
    if lateral - p.lane_margin >= high.0 || lateral + p.lane_margin <= low.0 {
        return false;
    }
    let corner = if lateral <= 0.0 { low.1 } else { high.1 };
    (ped[0] - corner[0]) * v[0] + (ped[2] - corner[1]) * v[2] > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bin(centre: Vec3, velocity: Vec3) -> MovingObject {
        let (x, z) = (centre[0], centre[2]);
        MovingObject { centre, velocity, corners: [[x - 0.6, z - 0.6], [x + 0.6, z - 0.6], [x + 0.6, z + 0.6], [x - 0.6, z + 0.6]] }
    }

    #[test]
    fn a_bin_pushed_at_a_ped_within_eight_metres_makes_it_flee() {
        let p = ObjectFleeParams::default();
        assert!(in_path([0.0, 0.0, 5.0], &bin([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]), &p));
        assert!(!in_path([0.0, 0.0, 9.0], &bin([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]), &p), "beyond 8 m");
        assert!(!in_path([0.0, 0.0, 5.0], &bin([0.0, 0.0, 0.0], [0.0, 0.0, 0.7]), &p), "under 0.75 m/s");
        assert!(!in_path([0.0, 0.0, -3.0], &bin([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]), &p), "behind the bin");
    }

    #[test]
    fn the_lane_is_the_footprint_plus_the_margin() {
        let p = ObjectFleeParams::default();
        let b = bin([0.0, 0.0, 0.0], [0.0, 0.0, 2.0]);
        assert!(in_path([1.3, 0.0, 4.0], &b, &p), "0.7 m outside the 0.6 m half width");
        assert!(!in_path([1.4, 0.0, 4.0], &b, &p), "0.8 m outside");
        assert!(!in_path([0.0, 0.0, 5.0], &b, &ObjectFleeParams { enabled: false, ..p }), "a mod turned it off");
    }
}
