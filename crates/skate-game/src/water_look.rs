//! Water look layer on top of the retail `water_defaultPS` port (family 33:
//! DownTown fountains, the Aletown canal). Project choices, not retail; see
//! docs/hails-additions/09-water.md "Water look layer". With `enabled = false`
//! the shader output is the plain retail program.
//!
//! Engine-facing: [`WaterLookSettings::active`] feeds `retail_render`'s frame
//! state. Player-facing: `settings/water-look.json`. Mod-facing:
//! `sdk.graphics.water_look(patch)` (one mod at a time, released on disable).
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use skate_mods::presentation::WaterLookPatch;

/// Every value of the look layer. The default is off (retail water); the
/// other defaults are the look approved in play (doc 09), used when the
/// player turns on "Better Water" (pause menu, EXTRAS). Retail values are in
/// [`WaterLook::RETAIL`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct WaterLook {
    pub enabled: bool,
    /// Ripple texture scale multiplier. Retail footage (Aletown canal, RPCS3)
    /// shows ~10-20 cm streaks; the decoded scale gives ~1 m blobs.
    pub ripple_scale: f32,
    /// Extra environment cube mip levels: an even, blurrier reflection.
    pub reflection_blur: f32,
    /// Reflection colour multiplier: slightly blue, as in the footage.
    pub reflection_tint: [f32; 3],
    /// Third wave from a rotated 0.613x sample plus broad swells, so the fine
    /// layer does not tile visibly over large stretches.
    pub third_wave: bool,
    /// Smaller bodies move less (area per body, `water_bodies`).
    pub body_calming: bool,
    /// Wave (ocean PCA) speed relative to retail, blended between frames.
    pub wave_rate: f32,
    /// Scroll speed of the water texture layers relative to retail
    /// (`g_fAnimationTime`, 1.0 per second, measured in the recomp).
    pub scroll_rate: f32,
    /// Scroll clock over a period where every authored scroll speed completes
    /// whole texture repeats, instead of the ported routine's restart after 5
    /// (a visible jump every 10 s). On in both looks: retail's water shaders
    /// scroll with `g_fAnimationTime`, which sub_82791628 accumulates without
    /// a reset; the 5-unit loop never reaches a shader (doc 09).
    pub seamless_clock: bool,
    /// The look on flowing water too (family 30 `water.flowing`, `flowingwater_defaultPS`: University's
    /// fountains; user 2026-10-19: "the fountains near the pcu library spawn"): its ripple scale, the reflection
    /// blur and tint. It has no PCA waves, so the third wave, swells and calming stay family 33 only.
    pub flowing_water: bool,
    pub flowing_ripple_scale: f32,
    /// A small splash where the skater or the board drops into shallow water (fountains, channels), as a share of
    /// the deep-water plume; 0 = none. Retail shows none in shallow water (doc 09), so Better Water only (user
    /// 2026-10-19: "the fountain splash should only haopen in better water mode").
    pub shallow_splash: f32,
    /// Calm swells on the open sea (family 31 `ocean.default`, seen from Industrial's Tanker spawn) as a slope
    /// amplitude; 0 = none. Shading only, the ocean mesh is too coarse to move. Retail has none (user 2026-10-19:
    /// "just make it look nice and calm but moving").
    pub ocean_swell: f32,
    /// Wavelength of the longest swell (m); the other two are 0.61x and 1.37x.
    pub ocean_swell_length: f32,
    /// Speed of the longest swell (m/s); the others move at the deep-water speed for their length.
    pub ocean_swell_speed: f32,
    /// Fountains (family 30) look like the large water, scaled down (user 2026-10-19: "in better water mode, water
    /// fountains ... should use the same water physics/shader and material as the larger bodies of water, just
    /// scaled down so it looks good as a fountain"): flat basins take the map's largest family 33 body's waves
    /// and textures (`retail_render::bind_fountain_donor`) in world space; steep cascades keep their own flow.
    pub fountain_water: bool,
    /// Metres per basin wave tile (the fine wave layer).
    pub fountain_tile: f32,
    /// Least wave strength on small basins (0..1), so body calming does not flatten them.
    pub fountain_waves: f32,
    /// Cascade scroll speed relative to retail, at the retail surface speed whatever the ripple scale ("water
    /// flowing down the drop offs should look more like water flowing").
    pub cascade_rate: f32,
    /// White foam on cascades from the two flow layers; 0 = none.
    pub cascade_foam: f32,
}

impl WaterLook {
    pub const RETAIL: Self = Self {
        enabled: false,
        ripple_scale: 1.,
        reflection_blur: 0.,
        reflection_tint: [1.; 3],
        third_wave: false,
        body_calming: false,
        wave_rate: 1.,
        scroll_rate: 1.,
        seamless_clock: true,
        flowing_water: false,
        flowing_ripple_scale: 1.,
        shallow_splash: 0.,
        ocean_swell: 0.,
        ocean_swell_length: 40.,
        ocean_swell_speed: 7.9,
        fountain_water: false,
        fountain_tile: 4.,
        fountain_waves: 0.,
        cascade_rate: 1.,
        cascade_foam: 0.,
    };

    fn apply(&mut self, patch: &WaterLookPatch) {
        if let Some(v) = patch.enabled { self.enabled = v; }
        if let Some(v) = patch.ripple_scale { self.ripple_scale = v; }
        if let Some(v) = patch.reflection_blur { self.reflection_blur = v; }
        if let Some(v) = patch.reflection_tint { self.reflection_tint = v; }
        if let Some(v) = patch.third_wave { self.third_wave = v; }
        if let Some(v) = patch.body_calming { self.body_calming = v; }
        if let Some(v) = patch.wave_rate { self.wave_rate = v; }
        if let Some(v) = patch.scroll_rate { self.scroll_rate = v; }
        if let Some(v) = patch.seamless_clock { self.seamless_clock = v; }
        if let Some(v) = patch.flowing_water { self.flowing_water = v; }
        if let Some(v) = patch.flowing_ripple_scale { self.flowing_ripple_scale = v; }
        if let Some(v) = patch.shallow_splash { self.shallow_splash = v; }
        if let Some(v) = patch.ocean_swell { self.ocean_swell = v; }
        if let Some(v) = patch.ocean_swell_length { self.ocean_swell_length = v; }
        if let Some(v) = patch.ocean_swell_speed { self.ocean_swell_speed = v; }
        if let Some(v) = patch.fountain_water { self.fountain_water = v; }
        if let Some(v) = patch.fountain_tile { self.fountain_tile = v; }
        if let Some(v) = patch.fountain_waves { self.fountain_waves = v; }
        if let Some(v) = patch.cascade_rate { self.cascade_rate = v; }
        if let Some(v) = patch.cascade_foam { self.cascade_foam = v; }
    }

    /// `FrameState.fountain`: tile (m), calming floor, foam and the cascade clock `clock`; all zero = off.
    pub fn fountain_row(&self, clock: f32) -> [f32; 4] {
        if !self.fountain_water {
            return [0.; 4];
        }
        [self.fountain_tile.max(0.5), self.fountain_waves.clamp(0., 1.), self.cascade_foam.max(0.), clock]
    }

    /// `FrameState.water_tint.w`: the flowing water's ripple scale, 0 = the look stays off flowing water.
    pub fn flowing_lane(&self) -> f32 {
        if self.flowing_water { self.flowing_ripple_scale.max(0.1) } else { 0. }
    }

    /// `FrameState.ocean_swell`: amplitude, wavelength, speed of the longest swell; amplitude 0 = off.
    pub fn ocean_swell_row(&self) -> [f32; 4] {
        [self.ocean_swell.max(0.), self.ocean_swell_length.max(1.), self.ocean_swell_speed.max(0.), 0.]
    }

    /// What the shader and clock use: retail values for every piece when the
    /// layer is off.
    pub fn effective(&self) -> Self {
        if self.enabled { self.clone() } else { Self::RETAIL }
    }
}

impl Default for WaterLook {
    fn default() -> Self {
        Self {
            enabled: false,
            ripple_scale: 4.,
            reflection_blur: 3.,
            reflection_tint: [0.82, 1., 1.04],
            third_wave: true,
            body_calming: true,
            wave_rate: 0.75,
            scroll_rate: 0.5,
            seamless_clock: true,
            flowing_water: true,
            flowing_ripple_scale: 2.,
            shallow_splash: 0.5,
            ocean_swell: 0.05,
            ocean_swell_length: 40.,
            ocean_swell_speed: 7.9,
            fountain_water: true,
            fountain_tile: 4.,
            fountain_waves: 0.5,
            cascade_rate: 1.5,
            cascade_foam: 0.6,
        }
    }
}

/// The player's water look plus a mod's patch on top of it.
#[derive(Resource, Debug)]
pub(crate) struct WaterLookSettings {
    /// The player's look (`settings/water-look.json`).
    pub selected: WaterLook,
    patched: Option<(String, WaterLookPatch)>,
    path: PathBuf,
}

impl WaterLookSettings {
    pub fn path(asset_root: &Path) -> PathBuf {
        asset_root.parent().unwrap_or(asset_root).join("settings/water-look.json")
    }

    pub fn new(selected: WaterLook, path: PathBuf) -> Self {
        Self { selected, patched: None, path }
    }

    /// A missing file is the default; an unreadable one is reported and ignored.
    pub fn load(asset_root: &Path) -> Self {
        let path = Self::path(asset_root);
        let selected = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<WaterLook>(&bytes).unwrap_or_else(|e| {
                warn!("Water look settings {}: {e}", path.display());
                WaterLook::default()
            }),
            Err(_) => WaterLook::default(),
        };
        Self::new(selected, path)
    }

    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(self.path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, serde_json::to_vec_pretty(&self.selected).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    /// The player's look with a mod's patch applied.
    pub fn active(&self) -> WaterLook {
        let mut look = self.selected.clone();
        if let Some((_, patch)) = &self.patched {
            look.apply(patch);
        }
        look
    }

    pub fn patched_by(&self) -> Option<&str> {
        self.patched.as_ref().map(|(owner, _)| owner.as_str())
    }

    /// `patch == None` releases the owner's patch.
    pub fn patch(&mut self, owner: &str, patch: Option<WaterLookPatch>) -> Result<(), String> {
        if let Some(holder) = self.patched_by()
            && holder != owner
        {
            return Err(format!("water look is set by mod {holder}"));
        }
        self.patched = patch.map(|patch| (owner.to_owned(), patch));
        Ok(())
    }

    /// Mod disable / failure (`Some(id)`) or a full mod runtime reset (`None`).
    pub fn clear_owner(&mut self, owner: Option<&str>) {
        if owner.is_none_or(|o| self.patched_by() == Some(o)) {
            self.patched = None;
        }
    }

    /// `sdk.snapshot.water_look`.
    pub fn snapshot(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self.active()).unwrap_or_default();
        value["owner"] = self.patched_by().into();
        value
    }
}

pub(crate) fn load(mut commands: Commands, config: Res<crate::config::Config>) {
    let settings = WaterLookSettings::load(&config.asset_root);
    info!("WATER_LOOK {:?} owner=None", settings.selected);
    commands.insert_resource(settings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_partial_file_keeps_the_defaults() {
        let root = std::env::temp_dir().join(format!("skate-water-look-{}", std::process::id()));
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        assert_eq!(WaterLookSettings::load(&assets).selected, WaterLook::default());
        std::fs::create_dir_all(root.join("settings")).unwrap();
        std::fs::write(WaterLookSettings::path(&assets), br#"{"enabled":false,"ripple_scale":2}"#).unwrap();
        let s = WaterLookSettings::load(&assets);
        assert!(!WaterLook::default().enabled, "retail water by default");
        assert!(!s.selected.enabled);
        assert_eq!(s.selected.ripple_scale, 2.);
        assert_eq!(s.selected.wave_rate, WaterLook::default().wave_rate);
        // Off means every piece at its retail value.
        assert_eq!(s.active().effective(), WaterLook::RETAIL);
        assert_eq!(WaterLook::RETAIL.flowing_lane(), 0., "off: flowing water keeps the retail program");
        assert_eq!(WaterLook { enabled: true, ..WaterLook::default() }.effective().flowing_lane(), 2.);
        std::fs::write(WaterLookSettings::path(&assets), br#"{"ripples":2}"#).unwrap();
        assert_eq!(WaterLookSettings::load(&assets).selected, WaterLook::default());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ocean_swells_only_with_better_water_and_mods_can_set_them() {
        assert_eq!(WaterLook::RETAIL.ocean_swell_row()[0], 0., "off: the sea keeps the retail shading");
        assert_eq!(WaterLook::default().effective().ocean_swell_row()[0], 0., "Better Water off by default");
        let on = WaterLook { enabled: true, ..WaterLook::default() }.effective().ocean_swell_row();
        assert!(on[0] > 0. && on[0] <= 0.1, "calm: {on:?}");
        let mut s = WaterLookSettings::new(WaterLook { enabled: true, ..WaterLook::default() }, PathBuf::from("unused/water-look.json"));
        let patch = WaterLookPatch { ocean_swell: Some(0.), ocean_swell_length: Some(80.), ..Default::default() };
        assert!(patch.validate());
        assert!(!WaterLookPatch { ocean_swell: Some(1.), ..Default::default() }.validate());
        s.patch("calm-sea", Some(patch)).unwrap();
        assert_eq!(s.active().effective().ocean_swell_row()[..2], [0., 80.]);
        s.clear_owner(Some("calm-sea"));
        assert!(s.active().effective().ocean_swell_row()[0] > 0.);
    }

    #[test]
    fn fountains_only_with_better_water_and_mods_can_set_them() {
        assert_eq!(WaterLook::RETAIL.fountain_row(5.), [0.; 4], "off: retail flowing water, retail clock");
        assert_eq!(WaterLook::RETAIL.cascade_rate, 1.);
        assert_eq!(WaterLook::default().effective().fountain_row(5.), [0.; 4], "Better Water off by default");
        let on = WaterLook { enabled: true, ..WaterLook::default() }.effective();
        assert_eq!(on.fountain_row(5.), [4., 0.5, 0.6, 5.]);
        assert!(on.cascade_rate > 1.);
        let mut s = WaterLookSettings::new(WaterLook { enabled: true, ..WaterLook::default() }, PathBuf::from("unused/water-look.json"));
        let patch = WaterLookPatch { fountain_tile: Some(8.), cascade_foam: Some(0.), ..Default::default() };
        assert!(patch.validate());
        assert!(!WaterLookPatch { fountain_tile: Some(0.), ..Default::default() }.validate());
        assert!(!WaterLookPatch { fountain_waves: Some(2.), ..Default::default() }.validate());
        assert!(!WaterLookPatch { cascade_rate: Some(-1.), ..Default::default() }.validate());
        s.patch("fountains", Some(patch)).unwrap();
        assert_eq!(s.active().effective().fountain_row(1.), [8., 0.5, 0., 1.]);
        s.patch("fountains", Some(WaterLookPatch { fountain_water: Some(false), ..Default::default() })).unwrap();
        assert_eq!(s.active().effective().fountain_row(1.), [0.; 4]);
        s.clear_owner(Some("fountains"));
        assert_eq!(s.active().effective().fountain_row(1.)[0], 4.);
    }

    #[test]
    fn one_mod_patches_and_disable_releases_it() {
        let mut s = WaterLookSettings::new(WaterLook::default(), PathBuf::from("unused/water-look.json"));
        let patch = WaterLookPatch { seamless_clock: Some(false), ripple_scale: Some(2.), ..Default::default() };
        s.patch("a", Some(patch.clone())).unwrap();
        assert!(s.patch("b", Some(patch)).is_err());
        assert_eq!(s.active().ripple_scale, 2.);
        assert!(!s.active().seamless_clock);
        assert_eq!(s.active().reflection_blur, 3.);
        assert_eq!(s.snapshot()["owner"], "a");
        s.clear_owner(Some("b"));
        assert_eq!(s.patched_by(), Some("a"));
        s.clear_owner(Some("a"));
        assert_eq!(s.active(), WaterLook::default());
    }
}
