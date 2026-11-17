//! The movie player's rows (`fe_movie`, vault class 0x18B275357481FB52 in skatercollections, research b490 / b898):
//! every movie is played by its row key, never by file name, and each row carries the file, volume, skip rules and
//! subtitles. Read from the setup's collections export with parent inheritance (`default`), DLC rows included
//! (parent `default_dlc`). A mod can add or replace rows (docs/hails-additions/47-movie-player.md).
use crate::collections::Collections;

const CLASS: &str = "Hash_18B275357481FB52";

#[derive(Clone, Debug, PartialEq)]
pub struct MovieRow {
    pub key: String,
    /// File base name (`intro_movie` -> `data/movies/intro_movie_english_ntsc.vp6`).
    pub file: String,
    pub folder: String,
    pub volume: f32,
    /// `82D9`: A shows the skip prompt, A again skips (after `skip_after_ms`).
    pub skippable: bool,
    /// `0AC1`, ms (5000 on `default`, 10000 on intro / coach).
    pub skip_after_ms: f32,
    /// `9A07`: B leaves the movie (credits).
    pub back_exits: bool,
    /// `6FC6`: the screen stays after the movie ends (credits).
    pub stays_after_end: bool,
    /// `A3A0` / `2914`: subtitles allowed / forced.
    pub subtitles: bool,
    pub forced_subtitles: bool,
    /// `07B8` / `AA75`: shown in the Skate Reel list, with this title id.
    pub listed: bool,
    pub title: String,
}

fn text(c: &Collections, key: &str, field: &str) -> String {
    c.field(CLASS, key, field).map(|f| f.data.clone()).unwrap_or_default()
}

/// Every playable row (parents with no file of their own left out), in vault order.
pub fn movie_rows(c: &Collections) -> Vec<MovieRow> {
    let b = |key: &str, f: &str| c.boolean(CLASS, key, f).unwrap_or(false);
    c.entries().iter().filter(|e| crate::attrib_hash::numeric_name(&e.class_name) == crate::attrib_hash::numeric_name(CLASS))
        .filter_map(|e| {
            let key = e.key.as_str();
            let file = text(c, key, "Hash_1971FB20F9154669");
            if file.is_empty() || file == "Set me" {
                return None;
            }
            Some(MovieRow {
                key: key.to_owned(),
                folder: Some(text(c, key, "Hash_C4B18D4B2056CCF1")).filter(|f| !f.is_empty()).unwrap_or_else(|| "movies".into()),
                volume: c.float(CLASS, key, "volume").unwrap_or(1.0),
                skippable: b(key, "Hash_82D90FC57883ACCB"),
                skip_after_ms: c.float(CLASS, key, "Hash_0AC11EDA5B6E4E50").unwrap_or(5000.0),
                back_exits: b(key, "Hash_9A07A75E2C186B08"),
                stays_after_end: b(key, "Hash_6FC69D07C41457AF"),
                subtitles: b(key, "Hash_A3A061EC3931B4DF"),
                forced_subtitles: b(key, "Hash_2914BCA40BC7DE58"),
                listed: b(key, "Hash_07B87C1F8036DDDB"),
                title: text(c, key, "Hash_AA753A536859233D"),
                file,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_disc_rows_match_the_research_table() {
        let root = std::path::PathBuf::from(std::env::var("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let rows = movie_rows(&Collections::load(&root).unwrap());
        let row = |k: &str| rows.iter().find(|r| r.key == k).unwrap_or_else(|| panic!("{k}"));
        let ea = row("ea_intro");
        assert_eq!((ea.file.as_str(), ea.skippable, ea.folder.as_str()), ("EA_Blackbox", false, "movies"));
        assert_eq!(row("intro_movie").skip_after_ms, 10000.0);
        assert!(row("credits").back_exits && row("credits").stays_after_end);
        assert!((row("coach_frank_intro").volume - 0.6).abs() < 1e-6);
        assert!(rows.iter().all(|r| r.key != "default" && r.key != "default_dlc"));
    }
}
