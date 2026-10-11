//! Front-end pad input (doc 31, "Pad input"): retail's `input.cfg` Apt actions (`AptUp = LStickU.repeat ||
//! DPadU.repeat;` ...) evaluated over the 24 native pad slots (`input::xbox::convert`), giving the key indices the
//! FE poll (sub_825DFDC8) passes to the screen stack's input handler, in the order of the Apt key table
//! (0x83027BF0, extracted at setup). Research `.local/research/fe-input.md`.
//!
//! NOT RETAIL YET: retail's repeat delay / period and the stick "held" threshold are not found
//! (`.local/research/input-repeat.md`); `RepeatRule::DEFAULT` keeps the values our current menus use. The edge of
//! each suffix (`pressed` / `released` / `repeat` / `held`) is the order-based reading in fe-input.md section 3.

/// The 24 native pad slots in `input.cfg` / `xbox::convert` order (Button0..Button23).
pub const SLOTS: [&str; 24] = [
    "DPadU", "DPadD", "DPadL", "DPadR", "Start", "Back", "LStick", "RStick", "LBumper", "RBumper", "LTrigger",
    "RTrigger", "A", "B", "X", "Y", "LStickR", "LStickL", "LStickU", "LStickD", "RStickR", "RStickL", "RStickU",
    "RStickD",
];

/// A slot suffix in an Apt expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Held,
    Pressed,
    Released,
    Repeat,
}

/// One Apt key: fires when any of its terms does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AptKey {
    pub name: String,
    pub terms: Vec<(usize, Edge)>,
}

/// Parse the Apt actions of `input.cfg` for the given key names (the Apt key table's order, so the index of a
/// key is its retail key index). A name without a definition gets no terms (never fires).
pub fn parse_apt_keys(cfg: &str, names: &[String]) -> Result<Vec<AptKey>, String> {
    let source: String = cfg.lines().map(|l| l.split("//").next().unwrap_or_default()).collect::<Vec<_>>().join("\n");
    let mut defined = std::collections::BTreeMap::new();
    for statement in source.split(';') {
        let Some((name, expression)) = statement.split_once('=') else { continue };
        let name = name.trim();
        if name.starts_with("Apt") {
            defined.insert(name.to_owned(), expression.trim().to_owned());
        }
    }
    names
        .iter()
        .map(|name| {
            let terms = match defined.get(name) {
                Some(expression) => expression.split("||").map(term).collect::<Result<Vec<_>, _>>()?,
                None => Vec::new(),
            };
            Ok(AptKey { name: name.clone(), terms })
        })
        .collect()
}

fn term(text: &str) -> Result<(usize, Edge), String> {
    let text = text.trim();
    let (slot, suffix) = text.split_once('.').ok_or_else(|| format!("Apt term {text:?} has no suffix"))?;
    let slot = SLOTS.iter().position(|s| *s == slot.trim()).ok_or_else(|| format!("unknown pad slot in {text:?}"))?;
    let edge = match suffix.trim() {
        "held" => Edge::Held,
        "pressed" => Edge::Pressed,
        "released" => Edge::Released,
        "repeat" => Edge::Repeat,
        other => return Err(format!("unsupported Apt suffix {other:?}")),
    };
    Ok((slot, edge))
}

/// Repeat and threshold (data; mods may override).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RepeatRule {
    /// Seconds held before the first repeat.
    pub delay: f32,
    /// Seconds between repeats.
    pub period: f32,
    /// Slot value that counts as held (sticks are 0..1 per direction).
    pub threshold: f32,
}

impl RepeatRule {
    /// NOT RETAIL YET: our current menus' values until retail's are found.
    pub const DEFAULT: Self = Self { delay: 0.35, period: 0.09, threshold: 0.5 };
}

impl Default for RepeatRule {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Per-controller slot edges.
#[derive(Clone, Debug, Default)]
pub struct FeInput {
    held: [bool; 24],
    held_for: [f32; 24],
    next_repeat: [f32; 24],
}

/// This frame's edges per slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub held: [bool; 24],
    pub pressed: [bool; 24],
    pub released: [bool; 24],
    /// The press itself, then after `delay` every `period` while held.
    pub repeat: [bool; 24],
}

impl FeInput {
    pub fn update(&mut self, values: &[f32; 24], seconds: f32, rule: &RepeatRule) -> Edges {
        let mut e = Edges::default();
        for i in 0..24 {
            let held = values[i] >= rule.threshold;
            e.held[i] = held;
            e.pressed[i] = held && !self.held[i];
            e.released[i] = !held && self.held[i];
            if e.pressed[i] {
                self.held_for[i] = 0.0;
                self.next_repeat[i] = rule.delay;
                e.repeat[i] = true;
            } else if held {
                self.held_for[i] += seconds.max(0.0);
                if self.held_for[i] >= self.next_repeat[i] {
                    e.repeat[i] = true;
                    self.next_repeat[i] = self.held_for[i] + rule.period.max(0.01);
                }
            }
            self.held[i] = held;
        }
        e
    }
}

/// The key indices that fire this frame, in key order (retail polls the 56 keys in table order).
pub fn fired(keys: &[AptKey], e: &Edges) -> Vec<u32> {
    keys.iter()
        .enumerate()
        .filter(|(_, k)| {
            k.terms.iter().any(|(slot, edge)| match edge {
                Edge::Held => e.held[*slot],
                Edge::Pressed => e.pressed[*slot],
                Edge::Released => e.released[*slot],
                Edge::Repeat => e.repeat[*slot],
            })
        })
        .map(|(i, _)| i as u32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "//Apt\nAptStart = Start.pressed;\nAptSelect = Back.pressed;\nAptNext = A.pressed;\n\
        AptUp = LStickU.repeat || DPadU.repeat; // menu up\nAptUpReleased = LStickU.released || DPadU.released;\n\
        GP_A = A;";

    fn names() -> Vec<String> {
        ["AptStart", "AptSelect", "AptNext", "AptUp", "AptUpReleased", "AptMissing"].map(String::from).to_vec()
    }

    fn slot(name: &str) -> usize {
        SLOTS.iter().position(|s| *s == name).unwrap()
    }

    #[test]
    fn parses_retail_expressions_in_key_order() {
        let keys = parse_apt_keys(CFG, &names()).unwrap();
        assert_eq!(keys[3].terms, [(slot("LStickU"), Edge::Repeat), (slot("DPadU"), Edge::Repeat)]);
        assert!(keys[5].terms.is_empty());
        assert!(parse_apt_keys("AptUp = Nose.repeat;", &["AptUp".into()]).is_err());
    }

    #[test]
    fn press_repeat_release() {
        let keys = parse_apt_keys(CFG, &names()).unwrap();
        let rule = RepeatRule { delay: 0.3, period: 0.1, threshold: 0.5 };
        let mut input = FeInput::default();
        let mut up = [0.0; 24];
        up[slot("DPadU")] = 1.0;
        assert_eq!(fired(&keys, &input.update(&up, 0.016, &rule)), [3], "press fires up once");
        assert!(fired(&keys, &input.update(&up, 0.2, &rule)).is_empty());
        assert_eq!(fired(&keys, &input.update(&up, 0.11, &rule)), [3], "first repeat after the delay");
        assert!(fired(&keys, &input.update(&up, 0.05, &rule)).is_empty());
        assert_eq!(fired(&keys, &input.update(&up, 0.06, &rule)), [3], "then every period");
        assert_eq!(fired(&keys, &input.update(&[0.0; 24], 0.016, &rule)), [4], "release");
        let mut stick = [0.0; 24];
        stick[slot("LStickU")] = 0.4;
        assert!(fired(&keys, &input.update(&stick, 0.016, &rule)).is_empty(), "below the threshold");
        let mut start = [0.0; 24];
        start[slot("Start")] = 1.0;
        assert_eq!(fired(&keys, &input.update(&start, 0.016, &rule)), [0]);
        assert!(fired(&keys, &input.update(&start, 1.0, &rule)).is_empty(), "pressed does not repeat");
    }
}
