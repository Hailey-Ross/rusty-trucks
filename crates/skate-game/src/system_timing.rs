//! Opt-in per-system cost readout (`SKATE_SYSTEM_TIMING=1`, also under trace-all): once per second one
//! `SYSTEM_TIMING` log line with the systems that took the most wall time, as mean ms per frame, the longest single run
//! and the run count. Bevy opens a `system` span (field `name`) around every system run (the `trace` feature is always
//! on, research b214); this layer names each span once when Bevy creates it and adds up the time between enter and exit
//! per name, with no allocation per run. Parallel systems overlap, so the per-system numbers add up to more than the
//! frame; they show where the CPU goes, the frame log shows the frame. Off: the layer is not installed at all.
use bevy::log::tracing::{Id, Subscriber, field::Field, field::Visit, span::Attributes};
use bevy::log::tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};
use bevy::prelude::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Whether the readout is on, read once.
pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE_SYSTEM_TIMING").is_ok_and(|v| v == "1"))
}

/// Systems named so far; a slot is never reused.
const SLOTS: usize = 2048;
/// Systems on the line.
const TOP: usize = 16;

struct Slot {
    total_ns: AtomicU64,
    runs: AtomicU64,
    max_ns: AtomicU64,
}

struct Table {
    names: Mutex<Vec<String>>,
    slots: Vec<Slot>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| Table {
        names: Mutex::new(Vec::new()),
        slots: (0..SLOTS).map(|_| Slot { total_ns: AtomicU64::new(0), runs: AtomicU64::new(0), max_ns: AtomicU64::new(0) }).collect(),
    })
}

/// The span's slot, kept in its extensions.
struct SlotId(usize);

#[derive(Default)]
struct NameField(Option<String>);
impl Visit for NameField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.to_owned());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" && self.0.is_none() {
            self.0 = Some(format!("{value:?}").trim_matches('"').to_owned());
        }
    }
}

thread_local! {
    static OPEN: RefCell<Vec<(u64, Instant)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct SystemTimingLayer;

/// The layer when the readout is on.
pub(crate) fn layer() -> Option<SystemTimingLayer> {
    on().then_some(SystemTimingLayer)
}

impl<S> Layer<S> for SystemTimingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if attrs.metadata().name() != "system" {
            return;
        }
        let mut field = NameField::default();
        attrs.record(&mut field);
        let Some(name) = field.0 else { return };
        let t = table();
        let mut names = t.names.lock().unwrap_or_else(|e| e.into_inner());
        let slot = match names.iter().position(|n| *n == name) {
            Some(i) => i,
            None if names.len() < SLOTS => {
                names.push(name);
                names.len() - 1
            }
            None => return,
        };
        drop(names);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SlotId(slot));
        }
    }
    fn on_enter(&self, id: &Id, _ctx: Context<'_, S>) {
        OPEN.with(|open| open.borrow_mut().push((id.into_u64(), Instant::now())));
    }
    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let key = id.into_u64();
        let Some(begin) = OPEN.with(|open| {
            let mut open = open.borrow_mut();
            let at = open.iter().rposition(|(k, _)| *k == key)?;
            Some(open.remove(at).1)
        }) else {
            return;
        };
        let Some(span) = ctx.span(id) else { return };
        let Some(slot) = span.extensions().get::<SlotId>().map(|s| s.0) else { return };
        let ns = begin.elapsed().as_nanos() as u64;
        let s = &table().slots[slot];
        s.total_ns.fetch_add(ns, Ordering::Relaxed);
        s.runs.fetch_add(1, Ordering::Relaxed);
        s.max_ns.fetch_max(ns, Ordering::Relaxed);
    }
}

pub(crate) struct SystemTimingPlugin;

impl Plugin for SystemTimingPlugin {
    fn build(&self, app: &mut App) {
        if on() {
            app.add_systems(Last, report);
        }
    }
}

#[derive(Default)]
struct ReportState {
    since: Option<Instant>,
    frames: u64,
}

/// Once per second: the `SYSTEM_TIMING` line, then the counters start again.
fn report(mut state: Local<ReportState>) {
    let now = Instant::now();
    let since = *state.since.get_or_insert(now);
    state.frames += 1;
    if now.duration_since(since).as_secs_f32() < 1.0 {
        return;
    }
    let frames = std::mem::take(&mut state.frames).max(1);
    state.since = Some(now);
    let t = table();
    let names = t.names.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut rows: Vec<(u64, u64, u64, &str)> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = &t.slots[i];
            (s.total_ns.swap(0, Ordering::Relaxed), s.runs.swap(0, Ordering::Relaxed), s.max_ns.swap(0, Ordering::Relaxed), n.as_str())
        })
        .filter(|r| r.1 > 0)
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let total: u64 = rows.iter().map(|r| r.0).sum();
    let top: Vec<String> = rows
        .iter()
        .take(TOP)
        .map(|(ns, runs, max, name)| format!("{}={:.3}/{:.2}x{}", short(name), *ns as f64 / 1e6 / frames as f64, *max as f64 / 1e6, runs))
        .collect();
    info!("SYSTEM_TIMING frames={frames} systems_ms_per_frame={:.3} | {}", total as f64 / 1e6 / frames as f64, top.join(" "));
}

/// The last two path segments of a system name (`living_world::peds::think_peds` -> `peds::think_peds`), generics kept.
fn short(name: &str) -> &str {
    let plain = name.split('<').next().unwrap_or(name);
    let mut cut = 0;
    let mut seen = 0;
    for (i, _) in plain.match_indices("::").collect::<Vec<_>>().into_iter().rev() {
        seen += 1;
        if seen == 2 {
            cut = i + 2;
            break;
        }
    }
    &name[cut..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_keep_the_last_two_segments() {
        assert_eq!(short("skate3rust::living_world::peds::think_peds"), "peds::think_peds");
        assert_eq!(short("think"), "think");
        assert_eq!(short("bevy_pbr::material::check<skate3rust::a::B>"), "material::check<skate3rust::a::B>");
    }
}
