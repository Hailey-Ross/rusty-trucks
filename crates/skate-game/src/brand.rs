//! The name the build shows the player (window titles, the splash, the crash and setup windows, messages). A build sets
//! it with the `SKATE_BRAND` environment variable at compile time (the fork's hails-additions branch sets "Rusty Trucks"
//! in `.cargo/config.toml`); without it the engine's own name. File, folder and release asset names do not change with
//! it (the updater finds releases by asset name; settings and reports stay in `%LOCALAPPDATA%\Skate3RustEngine`).

/// The display name, e.g. "Rusty Trucks".
pub(crate) const NAME: &str = match option_env!("SKATE_BRAND") {
    Some(name) => name,
    None => "Skate 3 Rust Engine",
};

/// [`NAME`] for the splash heading.
pub(crate) fn upper() -> String {
    NAME.to_uppercase()
}
