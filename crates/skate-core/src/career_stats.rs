//! Career stats (retail's Stats object, `[0x83067094]`): one running total per row of the career stats table, the
//! loading screen's Stats panel pick and the value formats. Details: docs/hails-additions/31-retail-menus.md
//! "Career stats", research b589.

/// Rows in the retail table (TU3: 65).
pub const ROWS: usize = 65;

/// Row indices the engine writes (retail's index order).
pub mod row {
    pub const GENDER_SWITCHES: usize = 2;
    pub const WIPEOUTS: usize = 6;
    pub const DISTANCE: usize = 7;
    pub const TIME_PLAYED: usize = 8;
    pub const DISTANCE_OFFBOARD: usize = 9;
    pub const INVERT_TIME: usize = 10;
    pub const SKITCH_DISTANCE: usize = 11;
    pub const SKITCH_TIME: usize = 12;
    pub const GRIND_DISTANCE: usize = 13;
    pub const FLIP_TRICKS: usize = 15;
    pub const BODY_FLIPS: usize = 16;
    pub const BODY_FLIP_ATTEMPTS: usize = 17;
    pub const LATE_FLIPS: usize = 18;
    pub const PUSHES: usize = 19;
    pub const MONGO_PUSHES: usize = 20;
    pub const MANUAL_DISTANCE: usize = 21;
    pub const POWERSLIDE_DISTANCE: usize = 22;
    pub const UNSUCCESSFUL_TRICKS: usize = 23;
    pub const SUCCESSFUL_TRICKS: usize = 24;
    pub const PEDS_HIT: usize = 26;
    pub const VEHICLES_HIT: usize = 28;
    pub const BAIL_DISTANCE: usize = 29;
    pub const PUSH_OFFS: usize = 40;
    pub const TIME_IN_BAIL: usize = 45;
    pub const MOST_COLLIDED: usize = 46;
    /// Hits per ped type, in this order: bum, jock, teen girl, teen guy, man, woman, business man, business woman,
    /// male skater, female skater, male tourist, female tourist, security guard, granny.
    pub const PED_HITS: usize = 47;
    pub const PED_TYPES: usize = 14;
    pub const SPOT_TUTORIALS: usize = 61;
    pub const BOARDS_OWNED: usize = 64;
}

/// How a row's value shows (the formatter vt+0x14 = 0x8250D6D0 switches on the index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Seconds as `hh:mm:ss.ff` with leading zero fields left out, then " s".
    Time,
    /// Metres, cut to whole metres, through the 1-decimal distance template.
    DistanceWhole,
    /// Metres through the 1-decimal distance template.
    Distance,
    /// Row 17: the successes (row 16) over the attempts (this row), whole percent.
    Percent,
    /// Row 46: the ped type hit most (rows 47..60).
    PedType,
    Money,
    Integer,
}

pub fn kind(index: usize) -> Kind {
    match index {
        0 | 8 | 10 | 12 | 14 | 36 | 37 | 43 | 45 => Kind::Time,
        7 | 9 => Kind::DistanceWhole,
        11 | 13 | 21 | 22 | 29 | 38 | 39 => Kind::Distance,
        17 => Kind::Percent,
        46 => Kind::PedType,
        62 | 63 => Kind::Money,
        _ => Kind::Integer,
    }
}

/// The running totals (career, per player). Every write adds, except the rows retail sets (61, 64).
#[derive(Clone, Debug, PartialEq)]
pub struct CareerStats {
    pub values: Vec<f32>,
}

impl Default for CareerStats {
    fn default() -> Self {
        Self { values: vec![0.0; ROWS] }
    }
}

impl CareerStats {
    /// The setter vt+0x18 (0x8250DFF0): adds `value` to the row, sets rows 61 and 64.
    pub fn record(&mut self, index: usize, value: f32) {
        if !value.is_finite() {
            return;
        }
        if self.values.len() < ROWS {
            self.values.resize(ROWS, 0.0);
        }
        if let Some(v) = self.values.get_mut(index) {
            if matches!(index, row::SPOT_TUTORIALS | row::BOARDS_OWNED) {
                *v = value;
            } else {
                *v += value;
            }
        }
    }

    pub fn value(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// The ped type hit most (row 46): ties keep the earlier type, the scan runs jock .. security guard, then bum,
    /// then granny (0x8250D6D0 case 0x2E). None when no ped was hit.
    pub fn most_collided(&self) -> Option<usize> {
        let order = (1..row::PED_TYPES - 1).chain([0, row::PED_TYPES - 1]);
        let mut best: Option<(usize, f32)> = None;
        for t in order {
            let v = self.value(row::PED_HITS + t);
            if v > 0.0 && best.is_none_or(|(_, b)| b < v) {
                best = Some((t, v));
            }
        }
        best.map(|(t, _)| t)
    }

    /// The offline fetch (sub_82607840): enabled rows with a value other than 0, shuffled, at most 16.
    pub fn fetch(&self, enabled: &[bool], rng: &mut dyn FnMut() -> u32) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..ROWS.min(enabled.len())).filter(|&i| enabled[i] && self.value(i) != 0.0).collect();
        shuffle(&mut rows, rng);
        rows.truncate(16);
        rows
    }
}

/// One bail's Hall of Meat record (the tracker 0x82DAF1D8, copied to P+0x3030 and added to the rows at bail end by
/// 0x8250F820). Retail counts 1/60 s ticks; ours adds game time, so the totals match at any fixed rate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BailRecord {
    /// Ticks with the body not yet at rest (wipeout over_599 clear): row 45, time in bail.
    pub active_seconds: f32,
    /// Ticks with the wipeout response strength (+580) above 0, at rest or not: row 40, push offs.
    pub push_off_ticks: f32,
    /// Distinct peds / vehicles the body touched (the scorers 0x82DAF948 / 0x82DAFBC0 refuse a 9th).
    pub peds: Vec<u64>,
    pub vehicles: Vec<u64>,
}

impl BailRecord {
    pub const ENTITY_CAP: usize = 8;
    pub const TICK_RATE: f32 = 60.0;

    pub fn tick(&mut self, dt: f32, over: bool, response_strength: f32, peds: impl IntoIterator<Item = u64>,
        vehicles: impl IntoIterator<Item = u64>) {
        if !over {
            self.active_seconds += dt;
        }
        if response_strength > 0.0 {
            self.push_off_ticks += dt * Self::TICK_RATE;
        }
        for (list, ids) in [(&mut self.peds, peds.into_iter().collect::<Vec<_>>()), (&mut self.vehicles, vehicles.into_iter().collect())] {
            for id in ids {
                if list.len() < Self::ENTITY_CAP && !list.contains(&id) {
                    list.push(id);
                }
            }
        }
    }

    /// The rows 0x8250F820 adds at bail end.
    pub fn rows(&self) -> [(usize, f32); 4] {
        [
            (row::TIME_IN_BAIL, self.active_seconds),
            (row::PUSH_OFFS, self.push_off_ticks.round()),
            (row::PEDS_HIT, self.peds.len() as f32),
            (row::VEHICLES_HIT, self.vehicles.len() as f32),
        ]
    }
}

/// Push starts (0x82DB6EC0 pulses Q+0xA4 / Q+0xA3, 0x82775328 adds rows 19 / 20). While the state word +2468 has the
/// pushing bit 25, the push is a mongo push when bit 26 equals (regular stance == riding switch), else a regular one;
/// each kind counts on its rising edge, a mongo start first. The last tick's kinds are retail's flags_1296 bits 22 / 23.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushLatch {
    mongo: bool,
    regular: bool,
}

impl PushLatch {
    /// The row to add 1 to this tick, if a push started.
    pub fn update(&mut self, flags_2468: u32, regular_stance: bool, riding_switch: bool) -> Option<usize> {
        let (mut mongo, mut regular) = (false, false);
        if flags_2468 & (1 << 25) != 0 {
            if (flags_2468 & (1 << 26) != 0) == (regular_stance == riding_switch) {
                mongo = true;
            } else {
                regular = true;
            }
        }
        let started = if !self.mongo && mongo {
            Some(row::MONGO_PUSHES)
        } else if !self.regular && regular {
            Some(row::PUSHES)
        } else {
            None
        };
        *self = Self { mongo, regular };
        started
    }
}

/// Fisher-Yates with the tips RNG (retail's own shuffle order is not decoded; the outcome, a uniform order, is).
pub fn shuffle(rows: &mut [usize], rng: &mut dyn FnMut() -> u32) {
    for i in (1..rows.len()).rev() {
        let j = rng() as usize % (i + 1);
        rows.swap(i, j);
    }
}

/// Seconds in the default time format `hh:mm:ss.ff` (0x8290EB38): a field shows when it or an earlier field is not
/// zero, padded to its width; the hundredths show only without hours.
pub fn clock(seconds: f32) -> String {
    let total = (seconds.max(0.0) * 100.0) as u64;
    let (h, m, s, f) = (total / 360_000, total / 6_000 % 60, total / 100 % 60, total % 100);
    let mut out = String::new();
    if h > 0 {
        out.push_str(&format!("{h:02}:"));
    }
    if m > 0 || !out.is_empty() {
        out.push_str(&format!("{m:02}:"));
    }
    if s > 0 || f > 0 || !out.is_empty() {
        out.push_str(&format!("{s:02}"));
        if h == 0 {
            out.push_str(&format!(".{f:02}"));
        }
    }
    out
}

/// What a row shows: the localised pieces the caller passes in (units setting, unit words, number grouping and the
/// ped type names).
pub struct Format<'a> {
    /// Game Settings > Units: imperial (the distance handler 0x824FE848 multiplies by 3.0 and shows feet).
    pub imperial: bool,
    /// `ID_COMMON_METRIC_DISTANCE`, `ID_COMMON_METRIC_NEW_FEET`, `ID_COMMON_METRIC_TIME`.
    pub metres: &'a str,
    pub feet: &'a str,
    pub seconds: &'a str,
    pub int: &'a dyn Fn(i64) -> String,
    pub float: &'a dyn Fn(f64, usize) -> String,
    pub money: &'a dyn Fn(f64) -> String,
    /// The ped type name by type (0 bum .. 13 granny).
    pub ped_type: &'a dyn Fn(usize) -> String,
}

impl CareerStats {
    pub fn format(&self, index: usize, f: &Format) -> String {
        let v = self.value(index);
        let distance = |metres: f64| {
            if f.imperial {
                format!("{} {}", (f.float)(metres * 3.0, 1), f.feet)
            } else {
                format!("{} {}", (f.float)(metres, 1), f.metres)
            }
        };
        match kind(index) {
            Kind::Time if v <= 0.0 => format!("#0 {}", f.seconds),
            Kind::Time => format!("{} {}", clock(v), f.seconds),
            Kind::DistanceWhole => distance(v.trunc() as f64),
            Kind::Distance => distance(v as f64),
            Kind::Percent => {
                let attempts = v;
                let pct = if attempts > 0.0 { (self.value(row::BODY_FLIPS) / attempts * 100.0).floor().clamp(0.0, 100.0) } else { 0.0 };
                format!("{}%", pct as i64)
            }
            Kind::PedType => self.most_collided().map(|t| (f.ped_type)(t)).unwrap_or_default(),
            Kind::Money => (f.money)(v as f64),
            Kind::Integer => (f.int)(v as i64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> Format<'static> {
        Format {
            imperial: false,
            metres: "m",
            feet: "ft",
            seconds: "s",
            int: &|n| n.to_string(),
            float: &|x, d| format!("{x:.d$}"),
            money: &|x| format!("${x:.0}"),
            ped_type: &|t| ["Bum", "Jock"].get(t).unwrap_or(&"?").to_string(),
        }
    }

    #[test]
    fn records_add_except_the_set_rows() {
        let mut s = CareerStats::default();
        s.record(row::PUSHES, 1.0);
        s.record(row::PUSHES, 1.0);
        s.record(row::BOARDS_OWNED, 3.0);
        s.record(row::BOARDS_OWNED, 2.0);
        s.record(row::DISTANCE, f32::NAN);
        assert_eq!((s.value(row::PUSHES), s.value(row::BOARDS_OWNED), s.value(row::DISTANCE)), (2.0, 2.0, 0.0));
    }

    #[test]
    fn the_clock_drops_leading_zero_fields_and_the_hundredths_with_hours() {
        assert_eq!(clock(83.45), "01:23.45");
        assert_eq!(clock(5.2), "05.20");
        assert_eq!(clock(3723.5), "01:02:03");
        assert_eq!(clock(0.0), "");
    }

    #[test]
    fn values_format_by_kind() {
        let mut s = CareerStats::default();
        s.record(row::DISTANCE, 1234.56);
        s.record(row::GRIND_DISTANCE, 12.34);
        s.record(row::TIME_PLAYED, 83.45);
        s.record(row::BODY_FLIPS, 2.0);
        s.record(row::BODY_FLIP_ATTEMPTS, 3.0);
        s.record(row::PED_HITS + 1, 2.0);
        s.record(row::PED_HITS, 2.0);
        let f = fmt();
        assert_eq!(s.format(row::DISTANCE, &f), "1234.0 m");
        assert_eq!(s.format(row::GRIND_DISTANCE, &f), "12.3 m");
        assert_eq!(s.format(row::TIME_PLAYED, &f), "01:23.45 s");
        assert_eq!(s.format(row::INVERT_TIME, &f), "#0 s");
        assert_eq!(s.format(row::BODY_FLIP_ATTEMPTS, &f), "66%");
        assert_eq!(s.format(row::MOST_COLLIDED, &f), "Jock", "a tie keeps the type scanned first (jock before bum)");
        assert_eq!(s.format(row::PUSHES, &f), "0");
        let imperial = Format { imperial: true, ..fmt() };
        assert_eq!(s.format(row::GRIND_DISTANCE, &imperial), "37.0 ft");
    }

    #[test]
    fn the_fetch_keeps_enabled_rows_with_values_at_most_16() {
        let mut s = CareerStats::default();
        let mut enabled = vec![true; ROWS];
        enabled[row::PUSHES] = false;
        s.record(row::PUSHES, 1.0);
        s.record(row::DISTANCE, 5.0);
        assert_eq!(s.fetch(&enabled, &mut || 0), [row::DISTANCE]);
        for i in 0..30 {
            s.record(i, 1.0);
        }
        assert_eq!(s.fetch(&enabled, &mut || 7).len(), 16);
    }

    #[test]
    fn pushes_count_on_the_rising_edge_by_kind() {
        let mut l = PushLatch::default();
        let push = 1 << 25;
        let right = 1 << 26;
        assert_eq!(l.update(0, true, true), None);
        // Bit 26 clear, stance == switch: not equal, a regular push.
        assert_eq!(l.update(push, true, true), Some(row::PUSHES));
        assert_eq!(l.update(push, true, true), None);
        assert_eq!(l.update(push | right, true, false), None);
        // The kind changes while pushing: the new kind starts.
        assert_eq!(l.update(push | right, true, true), Some(row::MONGO_PUSHES));
        assert_eq!(l.update(0, true, true), None);
        assert_eq!(l.update(push, true, false), Some(row::MONGO_PUSHES));
    }

    #[test]
    fn a_bail_record_counts_time_push_offs_and_distinct_entities() {
        let mut b = BailRecord::default();
        let dt = 1.0 / 60.0;
        for i in 0..20u64 {
            b.tick(dt, i >= 12, if i < 5 || i >= 18 { 0.5 } else { 0.0 }, [i % 3, 100 + i], []);
        }
        b.tick(dt, true, 0.0, [], [7, 7]);
        let rows = b.rows();
        assert!((rows[0].1 - 12.0 * dt).abs() < 1e-6);
        assert_eq!(rows[1], (row::PUSH_OFFS, 7.0));
        assert_eq!(rows[2], (row::PEDS_HIT, BailRecord::ENTITY_CAP as f32));
        assert_eq!(rows[3], (row::VEHICLES_HIT, 1.0));
    }
}
