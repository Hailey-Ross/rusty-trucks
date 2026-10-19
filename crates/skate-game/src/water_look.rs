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
    }

    /// `FrameState.water_tint.w`: the flowing water's ripple scale, 0 = the look stays off flowing water.
    pub fn flowing_lane(&self) -> f32 {
        if self.flowing_water { self.flowing_ripple_scale.max(0.1) } else { 0. }
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
