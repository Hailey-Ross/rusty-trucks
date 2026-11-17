//! Movie / scene subtitle cues (docs/hails-additions/47-movie-player.md "Subtitles"). Setup writes
//! `private/stock/subtitles.json` from the disc's `.sub` files with each cue's string hash resolved to its language
//! label (tools/asset_pipeline/subtitles.py); a mod can add files or replace cues by name.
use std::collections::BTreeMap;

/// One cue: FE time in tenths of a second, and the string label shown from then on (`None` clears the line).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct Cue(pub u16, pub Option<String>);

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Subtitles {
    /// Language folder name (`English`, from `SubTitles_English`) -> cue file name -> cues.
    pub languages: BTreeMap<String, BTreeMap<String, Vec<Cue>>>,
}

impl Subtitles {
    pub fn load(asset_root: &std::path::Path) -> Result<Self, String> {
        let path = asset_root.join("private/stock/subtitles.json");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The cues of `name` in `language`, else English (retail 8263E368 tries the language folder, then English);
    /// names match without case, as the archive lookup does.
    pub fn cues(&self, language: &str, name: &str) -> Option<&[Cue]> {
        let find = |l: &str| {
            self.languages.get(l)?.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, c)| c.as_slice())
        };
        find(language).or_else(|| find("English"))
    }
}

/// Plays one cue list against FE time (retail Subtitler 8263E6B0: a cue is due when `time x 100 ms <= elapsed`).
#[derive(Clone, Debug, Default)]
pub struct Subtitler {
    cues: Vec<Cue>,
    next: usize,
}

impl Subtitler {
    pub fn new(cues: &[Cue]) -> Self {
        Self { cues: cues.to_vec(), next: 0 }
    }

    /// The line from the last cue that became due by `elapsed_ms` (`Some(None)` clears), or `None` when no cue is due.
    pub fn due(&mut self, elapsed_ms: f32) -> Option<Option<String>> {
        let mut line = None;
        while let Some(cue) = self.cues.get(self.next).filter(|c| f32::from(c.0) * 100.0 <= elapsed_ms) {
            line = Some(cue.1.clone());
            self.next += 1;
        }
        line
    }
}

/// The part of a localized line retail shows (825E0F60): a leading `#` and a leading `[N]` (an auto-timed duration)
/// are skipped, and a later `[` starts the next page, which timeline cues never reach. `None` when nothing is left.
pub fn first_page(text: &str) -> Option<&str> {
    let mut text = text.strip_prefix('#').unwrap_or(text);
    if let Some(rest) = text.strip_prefix('[') {
        if let Some(end) = rest.find(']').filter(|e| rest[..*e].bytes().all(|b| b.is_ascii_digit())) {
            text = &rest[end + 1..];
        }
    }
    let page = text.split('[').next().unwrap_or_default();
    (!page.is_empty()).then_some(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(t: u16, l: Option<&str>) -> Cue {
        Cue(t, l.map(str::to_owned))
    }

    #[test]
    fn cues_fire_at_tenths_of_fe_time_and_the_last_due_one_wins() {
        let mut s = Subtitler::new(&[cue(43, Some("A")), cue(62, Some("B")), cue(167, None)]);
        assert_eq!(s.due(4299.0), None);
        assert_eq!(s.due(4300.0), Some(Some("A".into())));
        assert_eq!(s.due(4400.0), None);
        assert_eq!(s.due(20000.0), Some(None), "B and the clear both due: the clear wins");
        assert_eq!(s.due(30000.0), None);
    }

    #[test]
    fn a_language_without_the_file_falls_back_to_english_and_names_ignore_case() {
        let json = r#"{"languages": {"English": {"02_ProChallenge_Cardiel": [[43, "X"], [62, null]]}}}"#;
        let subs: Subtitles = serde_json::from_str(json).unwrap();
        assert_eq!(subs.cues("French", "02_prochallenge_cardiel").unwrap(), &[cue(43, Some("X")), cue(62, None)]);
        assert!(subs.cues("English", "intro_movie").is_none());
    }

    #[test]
    fn the_first_page_drops_markers_and_later_pages() {
        assert_eq!(first_page("Hey what up Navs?"), Some("Hey what up Navs?"));
        assert_eq!(first_page("#[30]One[Two"), Some("One"));
        assert_eq!(first_page("[x] gone"), None, "a non-number bracket is a page break at the start");
        assert_eq!(first_page(""), None);
    }
}
