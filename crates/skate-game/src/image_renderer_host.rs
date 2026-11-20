//! The front end's image slot manager (b258): 40 slots, `Game.ImageRenderer_*` natives, `slot/<n>` textures.
//! Details: docs/hails-additions/31-retail-menus.md#image-slots

use crate::apt_vm::{to_integer, Value};
use crate::retail_menu_movies::{image_key, NAMED_IMAGE_PREFIX};

/// Retail's slot count (b258 section 0: 40 slot pointers, 40 reserved flags).
pub const SLOTS: usize = 40;
/// Texture key prefix the draw gives an `ImageRenderComponent` quad: `slot/<n>`.
pub const SLOT_TEXTURE_PREFIX: &str = "slot/";
/// Texture key prefix of `LoadAssetByName` images: a texture another system keeps resident under that name (the
/// player's park screenshot); nothing supplies them yet, so they draw nothing.
pub const ASSET_TEXTURE_PREFIX: &str = "asset/";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub reserved: bool,
    /// The loaded texture key (`image/<key>` or `asset/<name>`), None when unloaded.
    pub image: Option<String>,
    /// LockAspectRatio (loader +0x1C); reset by every load.
    pub lock: bool,
}

/// Retail's swatch slot count (825A5F88: reserved flags at +0x640, 100 of them).
pub const SWATCH_SLOTS: usize = 100;
/// Texture key prefix the draw gives a `SwatchRenderComponent` quad: `swatch/<n>`, filled with the slot's color.
pub const SWATCH_TEXTURE_PREFIX: &str = "swatch/";

#[derive(Clone, Debug, PartialEq)]
pub struct ImageSlots {
    pub slots: Vec<Slot>,
    /// Swatch slots: reserved, rgba.
    pub swatches: Vec<(bool, [f32; 4])>,
}

impl Default for ImageSlots {
    fn default() -> Self {
        Self { slots: vec![Slot::default(); SLOTS], swatches: vec![(false, [0.0, 0.0, 0.0, 1.0]); SWATCH_SLOTS] }
    }
}

impl ImageSlots {
    /// One `Game.ImageRenderer_*` native; None = not ours.
    pub fn call(&mut self, object: &str, method: &str, args: &[Value]) -> Option<Value> {
        if let Some(m) = method.strip_prefix("SwatchRenderer_").filter(|_| object == "Game") {
            return Some(self.swatch_call(m, args));
        }
        let method = method.strip_prefix("ImageRenderer_").filter(|_| object == "Game")?;
        if std::env::var_os("SKATE3_RETAIL_MENUS_TRACE").is_some() {
            bevy::log::info!("RETAIL_MENUS image renderer {method}({:?})", args);
        }
        let index = |i: usize| args.get(i).map(to_integer).filter(|n| (0..SLOTS as i64).contains(&(*n as i64))).map(|n| n as usize);
        let text = |i: usize| match args.get(i) {
            Some(Value::Text(t)) => Some(t.clone()),
            _ => None,
        };
        match method {
            // 825A5E28: the first slot not reserved, else -1.
            "ReserveNextFreeIndex" => {
                let free = self.slots.iter().position(|s| !s.reserved);
                if let Some(i) = free {
                    self.slots[i].reserved = true;
                }
                return Some(Value::Number(free.map_or(-1.0, |i| i as f64)));
            }
            // 825A5E88: clears the reserved flag only; the image stays until the next load or unload.
            "FreeImageIndex" => {
                if let Some(i) = index(0) {
                    self.slots[i].reserved = false;
                }
            }
            // 825A5D08: only with a string name; replaces the slot's image, the aspect lock resets.
            "Load" => {
                if let (Some(name), Some(i)) = (text(0), index(1)) {
                    self.slots[i].image = Some(format!("{NAMED_IMAGE_PREFIX}{}", image_key(&name)));
                    self.slots[i].lock = false;
                }
            }
            // 825A59B8: a resident texture by name; no file is requested. LoadTeamLogo / LoadAsset with a name (the team
            // logos' asset names, research b1011 section 3) resolve the same way.
            "LoadAssetByName" | "LoadTeamLogo" | "LoadAsset" => {
                if let (Some(name), Some(i)) = (text(0), index(1)) {
                    self.slots[i].image = Some(format!("{ASSET_TEXTURE_PREFIX}{name}"));
                    self.slots[i].lock = false;
                }
            }
            // 825A5D80.
            "Unload" => {
                if let Some(i) = index(0) {
                    self.slots[i].image = None;
                }
            }
            // 825A5ED8: number + bool, only on a loaded slot.
            "LockAspectRatio" => {
                if let (Some(i), Some(Value::Bool(lock))) = (index(0), args.get(1)) {
                    if self.slots[i].image.is_some() {
                        self.slots[i].lock = *lock;
                    }
                }
            }
            // MemcardLoad / OnlineLoad: memory card and online sources, not ported; the slot keeps what it has.
            _ => {}
        }
        Some(Value::Undefined)
    }

    /// `Game.SwatchRenderer_*` (825A5F88 / 825A5FE8 / 825A6038): 100 flat color slots the Create-a-Skater color rows
    /// paint their squares with. SetColour takes floats as is and integers / 255 with alpha 1; our VM has one number
    /// type, so every value is taken as is (NOT RETAIL YET for integer callers; the TU3 movies pass CAC_GetColours'
    /// floats).
    fn swatch_call(&mut self, method: &str, args: &[Value]) -> Value {
        let index = |i: usize| args.get(i).map(to_integer).filter(|n| (0..SWATCH_SLOTS as i64).contains(&(*n as i64))).map(|n| n as usize);
        match method {
            "ReserveIndex" => {
                let free = self.swatches.iter().position(|s| !s.0);
                if let Some(i) = free {
                    self.swatches[i].0 = true;
                }
                return Value::Number(free.map_or(-1.0, |i| i as f64));
            }
            "FreeIndex" => {
                if let Some(i) = index(0) {
                    self.swatches[i].0 = false;
                }
            }
            "SetColour" => {
                if let Some(i) = index(0) {
                    let c = |k: usize| args.get(k).map_or(0.0, |v| v.number() as f32);
                    self.swatches[i].1 = [c(1), c(2), c(3), 1.0];
                }
            }
            _ => {}
        }
        Value::Undefined
    }

    /// The color a `swatch/<n>` draw fills with.
    pub fn swatch(&self, key: &str) -> Option<[f32; 4]> {
        let slot: usize = key.strip_prefix(SWATCH_TEXTURE_PREFIX)?.parse().ok()?;
        self.swatches.get(slot).map(|s| s.1)
    }

    /// The texture key a `slot/<n>` draw shows, None when the slot is empty.
    pub fn texture(&self, key: &str) -> Option<&str> {
        let slot: usize = key.strip_prefix(SLOT_TEXTURE_PREFIX)?.parse().ok()?;
        self.slots.get(slot)?.image.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: f64) -> Value {
        Value::Number(v)
    }

    #[test]
    fn slots_reserve_load_free_and_unload_like_retail() {
        let mut s = ImageSlots::default();
        for i in 0..SLOTS {
            assert_eq!(s.call("Game", "ImageRenderer_ReserveNextFreeIndex", &[]), Some(n(i as f64)));
        }
        assert_eq!(s.call("Game", "ImageRenderer_ReserveNextFreeIndex", &[]), Some(n(-1.0)), "all 40 reserved");
        s.call("Game", "ImageRenderer_Load", &[Value::Text("fe\\source\\images\\map\\map1024".into()), n(3.0)]);
        assert_eq!(s.texture("slot/3"), Some("image/fe/source/images/map/map1024"));
        s.call("Game", "ImageRenderer_LockAspectRatio", &[n(3.0), Value::Bool(true)]);
        assert!(s.slots[3].lock);
        s.call("Game", "ImageRenderer_Load", &[Value::Text("fe/source/images/map/map512".into()), n(3.0)]);
        assert!(!s.slots[3].lock, "a load resets the lock");
        s.call("Game", "ImageRenderer_FreeImageIndex", &[n(3.0)]);
        assert!(!s.slots[3].reserved);
        assert_eq!(s.texture("slot/3"), Some("image/fe/source/images/map/map512"), "free keeps the image");
        assert_eq!(s.call("Game", "ImageRenderer_ReserveNextFreeIndex", &[]), Some(n(3.0)));
        s.call("Game", "ImageRenderer_Unload", &[n(3.0)]);
        assert_eq!(s.texture("slot/3"), None);
        s.call("Game", "ImageRenderer_Load", &[n(5.0), n(4.0)]);
        assert_eq!(s.texture("slot/4"), None, "a number name loads nothing");
        assert_eq!(s.call("Game", "OnMenuSelect", &[]), None);
        assert_eq!(s.call("FEChallengeMap", "ImageRenderer_Load", &[]), None);
    }

    #[test]
    fn swatch_slots_reserve_color_and_free_like_retail() {
        let mut s = ImageSlots::default();
        assert_eq!(s.call("Game", "SwatchRenderer_ReserveIndex", &[]), Some(n(0.0)));
        assert_eq!(s.call("Game", "SwatchRenderer_ReserveIndex", &[]), Some(n(1.0)));
        s.call("Game", "SwatchRenderer_SetColour", &[n(1.0), n(0.7), n(0.2), n(0.5)]);
        assert_eq!(s.swatch("swatch/1"), Some([0.7, 0.2, 0.5, 1.0]));
        assert_eq!(s.swatch("swatch/0"), Some([0.0, 0.0, 0.0, 1.0]), "a reserved slot starts black");
        s.call("Game", "SwatchRenderer_FreeIndex", &[n(0.0)]);
        assert_eq!(s.call("Game", "SwatchRenderer_ReserveIndex", &[]), Some(n(0.0)));
        for _ in 2..SWATCH_SLOTS {
            s.call("Game", "SwatchRenderer_ReserveIndex", &[]);
        }
        assert_eq!(s.call("Game", "SwatchRenderer_ReserveIndex", &[]), Some(n(-1.0)), "all 100 reserved");
    }
}
