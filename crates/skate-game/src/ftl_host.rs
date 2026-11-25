//! Follow the leader in play (research b1006; doc 42 "Follow the leader"): a row from `private/challenges.json`
//! `follow_the_leaders` runs through `skate_core::challenges::film_run` (Kind::Follow), its owned objectives from the
//! exported rows (`challenge_objectives.rs`), the leader (competitor 1) riding the challenge's lines (`ch_<id>`) on the
//! contest rider code with the follower band. Start: the sign-up prompt at the row's sign-up sphere (challenges.rs),
//! the career's first challenge, or `SKATE_FTL=<id>` on that row's map.
//! The killed it group runs next to the owned one (make_objectives builds both lists, b875); no retail FTL row has one.
use crate::challenge_objectives::{DataObjectives, ObjectiveData};
use crate::hud_challenge::{ChallengeHudState, Component};
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::film_run::{Command, Event, FilmRun, HudComponent, Kind, Message, Params, State};
use skate_core::challenges::objectives::Event as GroupEvent;
use skate_core::challenges::race_ai::{follower_band, shaped_speed, BandTuning, FollowerBand};
use skate_core::challenges::timers::TICKS_PER_SECOND;
use skate_core::challenges::{Completion, Flow};

/// The failure text when the row's own is not exported (b1006 §0.3).
const FAILURE_TEXT: &str = "ID_CHALLENGE_FTL_FAILURE";
const AUTOPILOT_DIALOG_SECONDS: f32 = 3.0;

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct FtlStart {
    pub locator: String,
    pub matrix: [[f32; 4]; 4],
    /// Child n of the start locator: the player 0, the leader 1, a second leader 2.
    #[serde(default)]
    pub slots: Vec<[[f32; 4]; 4]>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct FtlObjectives {
    #[serde(default)]
    pub owned: Vec<ObjectiveData>,
    #[serde(default)]
    pub killed: Vec<ObjectiveData>,
}

fn yes() -> bool {
    true
}

fn start_or_here<'de, D: serde::Deserializer<'de>>(d: D) -> Result<FtlStart, D::Error> {
    Ok(Option::<FtlStart>::deserialize(d)?.unwrap_or_default())
}


/// The autopilot's gap behind the leader (m): inside it the brain holds the leader's speed or less. OURS, a test aid.
const FOLLOW_GAP: f32 = 8.0;

/// The line the autopilot may take next: the one after `current` on the leader's `path`; the leader's latest when
/// the brain is on it or off the path; the first one before the brain has a line.
fn follow_line(path: &[[u8; 16]], current: Option<[u8; 16]>) -> Option<[u8; 16]> {
    match current.and_then(|c| path.iter().rposition(|l| *l == c)) {
        Some(i) => path.get(i + 1).or(path.last()).copied(),
        None => current.map_or(path.first(), |_| path.last()).copied(),
    }
}

/// The autopilot never brakes below this for the gap (m/s): a leader standing at its start does not stop it.
const FOLLOW_MIN_SPEED: f32 = 4.0;

/// The autopilot's speed cap for a gap of `behind` meters behind the leader (negative: ahead of it) at the leader's
/// `speed`: none past the gap, the leader's speed easing down to 75 % at no gap, 60 % once ahead.
fn follow_cap(behind: f32, speed: f32) -> Option<f32> {
    let cap = if behind >= FOLLOW_GAP {
        return None;
    } else if behind <= 0.0 {
        0.6 * speed
    } else {
        speed * (0.75 + 0.25 * behind / FOLLOW_GAP)
    };
    Some(cap.max(FOLLOW_MIN_SPEED))
}

/// One row of the objectives_shared graph as `challenges.py` exports it: `follow_the_leaders` and `films` (spot films
/// and anywhere films share the export, b999 / b1003).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct FtlData {
    pub id: String,
    /// `follow_the_leaders`, `films` or `anywhere_films` (absent in an export from before films: follow the leader).
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub title_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(flatten)]
    pub sign_up: crate::challenges::SignUpFields,
    /// The anywhere types have none (null): the run starts where the player stands (b1003 §2).
    #[serde(default, deserialize_with = "start_or_here")]
    pub start: FtlStart,
    #[serde(default)]
    pub end_volume: Option<crate::challenges::Volume>,
    #[serde(default)]
    pub boundary: Option<crate::challenges::Volume>,
    #[serde(default)]
    pub max_follow_distance: Option<f32>,
    #[serde(default)]
    pub countdown_delay: Option<f32>,
    /// Film row values (b999 §3): PrefilmTime, TotalFilmTime (simul play only), FilmTimerLowLimit, OutOfBoundsTime and
    /// the banner times; absent = the family default in `Params::retail`.
    #[serde(default)]
    pub prefilm_time: Option<f32>,
    #[serde(default)]
    pub total_film_time: Option<f32>,
    #[serde(default)]
    pub film_low_limit: Option<i32>,
    #[serde(default)]
    pub out_of_bounds_time: Option<f32>,
    #[serde(default)]
    pub owned_display_time: Option<f32>,
    #[serde(default)]
    pub killed_display_time: Option<f32>,
    #[serde(default = "yes")]
    pub allow_player_bail: bool,
    #[serde(default)]
    pub allow_objective_failure: bool,
    #[serde(default)]
    pub spawn_offboard: bool,
    #[serde(default)]
    pub ai_paths: crate::contest_host::RoundPathsJson,
    #[serde(default)]
    pub competitors: Vec<crate::race_host::CompetitorRow>,
    #[serde(default)]
    pub objectives: FtlObjectives,
    /// TrackUniqueTrickGroup (b1056): the challenge_trick_group whose unique tricks the objectives count (16 hex digits;
    /// three films on the disc). Exports before 2026-11-23 lack it.
    #[serde(default)]
    pub track_unique_trick_group: Option<String>,
    /// C.IntroPresentationEvents (nis_overview, b875): the film's or pro's intro NIS.
    #[serde(default)]
    pub intro: crate::challenge_objectives::EventList,
    /// The volumes the objectives test (IsInVolume / EnteredVolume), from the challenge's hull: a film's spot volumes
    /// are not among the map's trigger volumes, so the run arms them.
    #[serde(default)]
    pub objective_volumes: Vec<crate::challenges::Volume>,
    /// Hall of Meat rows (b1005): TimeLimit (s), DisableTimeLimitForSinglePlayer, DisableHOM.
    #[serde(default)]
    pub time_limit: Option<f32>,
    #[serde(default)]
    pub disable_time_limit_for_single_player: bool,
    #[serde(default)]
    pub disable_hom: bool,
    /// C.HighlightDefinition (the Hall of Meat's best bail clip, b1005); exports before 2026-11-21 lack it.
    #[serde(default)]
    pub highlight: Option<crate::challenge_objectives::HighlightJson>,
    /// Photos: the magazine layout the taken photo goes on (MagazinePromo), None when the row's layout reference does
    /// not resolve (research b1060 §4).
    #[serde(default)]
    pub photo_layout: Option<String>,
    /// Photos: the magazine layout kind (Mission_GetPhotoLayoutKind without the display flag: 0..2 single page, 3 two
    /// pages, 5 billboard), MagazinePromo's state.
    #[serde(default)]
    pub photo_layout_kind: Option<u8>,
    /// Photos: the layout is a two-page spread (the picker movie's layout label).
    #[serde(default)]
    pub photo_layout_two_page: bool,
    /// Photos: the career photo slot (bank field A4473FA9B19FAEC9); 0 = the photo is not saved.
    #[serde(default)]
    pub photo_slot: u32,
    /// The row's central texts (C.CentralMessageHALID* / ModifiedCentralMessageHALID*): return_to_area,
    /// challenge_failed, line_timer_expired, out_of_bounds, scored_low, time_up.
    #[serde(default)]
    pub central_messages: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    follow_the_leaders: Vec<serde_json::Value>,
    #[serde(default)]
    films: Vec<serde_json::Value>,
    #[serde(default)]
    hall_of_meats: Vec<serde_json::Value>,
    #[serde(default)]
    photos: Vec<serde_json::Value>,
}

/// Every Hall of Meat row (`hall_of_meats`, b1005), read like the films'; a row that does not parse is logged.
pub(crate) fn load_hall_of_meat(assets: &std::path::Path) -> Vec<FtlData> {
    let Some(file) = std::fs::read(assets.join("private/challenges.json")).ok().and_then(|b| serde_json::from_slice::<File>(&b).ok()) else {
        return Vec::new();
    };
    file.hall_of_meats.into_iter().filter_map(|v| {
        let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("?").to_owned();
        serde_json::from_value(v).map_err(|e| warn!("SKATE_HOM row {id} not readable: {e}")).ok()
    }).collect()
}

/// Every follow the leader, film and photo row. Rows are read one by one, so a row that does not parse is left out
/// with a log line instead of hiding the others.
pub(crate) fn load(assets: &std::path::Path) -> Vec<FtlData> {
    let Some(file) = std::fs::read(assets.join("private/challenges.json")).ok().and_then(|b| serde_json::from_slice::<File>(&b).ok()) else {
        return Vec::new();
    };
    let mut skipped = Vec::new();
    let rows: Vec<FtlData> = file.follow_the_leaders.into_iter().chain(file.films).chain(file.photos).filter_map(|v| {
        let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("?").to_owned();
        serde_json::from_value(v).map_err(|_| skipped.push(id)).ok()
    }).collect();
    if !skipped.is_empty() {
        info!("SKATE_FTL rows not readable: {skipped:?}");
    }
    rows
}

fn volume_id(v: &Option<crate::challenges::Volume>) -> Option<u64> {
    v.as_ref().and_then(|v| u64::from_str_radix(&v.id, 16).ok())
}

/// The graph's challenge type for a row's family.
/// The anywhere photos' photographer (b1057): armed while an anywhere photo runs; `place_requested` is the phone's
/// B row (`session_marker`) or a mod's `sdk.challenges.place_photographer()`, served at the player's root on the next update.
#[derive(Resource, Default)]
pub(crate) struct AnywherePhotographer {
    pub state: skate_core::challenges::photographer::Photographer,
    pub place_requested: bool,
}

/// The `ribbon_indicator` row the phone adds at the placed spot (826682B0, string 82235268).
const PHOTOGRAPHER_INDICATOR: &str = "anywhere_photo";

pub(crate) fn kind(d: &FtlData) -> Kind {
    match d.family.as_deref() {
        Some("films") => Kind::Film,
        Some("anywhere_films") => Kind::AnywhereFilm,
        Some("photos") => Kind::Photo,
        Some("anywhere_photos") => Kind::AnywherePhoto,
        _ => Kind::Follow,
    }
}

/// The row's values over the family defaults (b1006 §3, b999 §3).
pub(crate) fn params(d: &FtlData) -> Params {
    let kind = kind(d);
    let base = Params::retail(kind);
    Params {
        // follow the leader always has its leader; films ride alone
        competitors: if kind == Kind::Follow { d.competitors.len().max(1) } else { d.competitors.len() } as u32,
        boundary: volume_id(&d.boundary),
        ftl_end_volume: volume_id(&d.end_volume),
        countdown_delay: d.countdown_delay.unwrap_or(base.countdown_delay),
        prefilm_time: d.prefilm_time.unwrap_or(base.prefilm_time),
        total_film_time: d.total_film_time.unwrap_or(base.total_film_time),
        film_low_limit: d.film_low_limit.map_or(base.film_low_limit, |v| v as f32),
        out_of_bounds_time: d.out_of_bounds_time.unwrap_or(base.out_of_bounds_time),
        owned_display_time: d.owned_display_time.unwrap_or(base.owned_display_time),
        killed_display_time: d.killed_display_time.unwrap_or(base.killed_display_time),
        allow_player_bail: d.allow_player_bail,
        allow_objective_failure: d.allow_objective_failure,
        ..base
    }
}

/// One objective group of the run: the owned it list or the killed it list.
pub(crate) struct FtlGroup {
    pub killed: bool,
    pub objectives: DataObjectives,
}

/// The run's groups from the row: the owned list (required), then the killed list when the row has one. None when
/// the owned list does not load; a killed list that does not load is left out (logged by DataObjectives).
pub(crate) fn load_groups(data: &FtlData, volumes: Vec<(String, u64)>, trick_groups: crate::challenge_objectives::TrickGroups) -> Option<Vec<FtlGroup>> {
    let owned = DataObjectives::new(&data.id, &data.objectives.owned, volumes.clone(), trick_groups.clone())?;
    let mut groups = vec![FtlGroup { killed: false, objectives: owned }];
    if let Some(killed) = DataObjectives::new(&data.id, &data.objectives.killed, volumes, trick_groups) {
        groups.push(FtlGroup { killed: true, objectives: killed });
    }
    // MakeObjectivesFromGroup gives every group of the run the row's tracked trick group (+0x78, b1056).
    let tracked = data.track_unique_trick_group.as_deref().and_then(|k| u64::from_str_radix(k, 16).ok());
    for g in &mut groups {
        g.objectives.track_unique(tracked);
    }
    Some(groups)
}

pub(crate) struct FtlState {
    pub data: FtlData,
    run: FilmRun,
    groups: Vec<FtlGroup>,
    /// The intro presentation group nis_overview waits for.
    intro_group: Option<String>,
    evaluating: bool,
    leader: Option<crate::contest_ai_host::ContestAi>,
    awaiting_teleport: bool,
    /// The leader band (None = switched off).
    band: Option<FollowerBand>,
    carry: f32,
    /// Fractional 60 Hz ticks toward the next objective evaluation.
    eval_carry: f32,
    /// The first leader's position last frame and its smoothed speed (m/s), for the autopilot's follow gap.
    leader_last: Option<[f32; 3]>,
    leader_speed: f32,
    /// The lines the first leader has taken this run, in order: the autopilot takes them in the same order.
    leader_path: Vec<[u8; 16]>,
    last_state: u32,
    failure_text: Option<String>,
    dialog_open: bool,
    dialog_seconds: f32,
    /// Leaders already counted inside the end volume.
    leaders_in_end: Vec<u32>,
    /// The competitors with a follow indicator (Leader.AddFollowIndicator), drawn as a trail behind their body.
    follow_indicators: Vec<u32>,
    meter: Option<skate_core::challenges::distance_meter::DistanceMeter>,
    /// Commands for the next update (the start's, made in arm).
    queued: Vec<Command>,
    /// What the player does, for the objective groups (shared with the tutorials).
    feed: crate::challenge_objectives::ObjectiveFeed,
    /// The autopilot's area lines were looked up for this run (films).
    area_checked: bool,
    /// The highlights and the highlight reel (research b1050).
    reel: ReelState,
    /// The photo shot picker while it is open (FE 2 PhotoSelect, `photo_picker`).
    pub(crate) photo: Option<skate_core::challenges::photo_select::PhotoSelect>,
}

impl FtlState {
    /// The picker finished (82618428 posts cMsgNISComplete): the run's niscomplete runs on the next update; a finish
    /// mode from MagazinePromo answers the end options at once (cMsgHUDContinue / cMsgHUDRetry, research b1067).
    pub(crate) fn photo_finished(&mut self, mode: skate_core::challenges::photo_select::FinishMode) {
        use skate_core::challenges::photo_select::FinishMode;
        self.photo = None;
        let mut commands = self.run.handle(Event::NisComplete);
        match mode {
            FinishMode::Normal => {}
            FinishMode::Continue => commands.extend(self.run.handle(Event::Continue)),
            FinishMode::Retry => commands.extend(self.run.handle(Event::Retry)),
        }
        self.queued.extend(commands);
    }
}

#[derive(Resource, Default)]
pub(crate) struct FollowTheLeader {
    pub rows: Vec<FtlData>,
    loaded: bool,
    generation: Option<u64>,
    pub run: Option<FtlState>,
    /// The last finished run's result line (tests, the autopilot).
    pub result: Option<String>,
    /// Hall of Meat rows and the run (hom_host.rs).
    pub hom_rows: Vec<FtlData>,
    pub hom: Option<crate::hom_host::HomState>,
    /// 1-Up rows and the run (one_up_host.rs).
    pub one_up_rows: Vec<crate::one_up_host::OneUpData>,
    pub one_up: Option<crate::one_up_host::OneUpState>,
}

impl FollowTheLeader {
    /// The rows on `map` with a sign-up sphere, for the sign-up prompt (challenges.rs).
    pub(crate) fn offers<'a>(&'a self, map: Option<&'a std::path::Path>) -> impl Iterator<Item = crate::challenges::Offer> + 'a {
        self.rows.iter().chain(&self.hom_rows).filter_map(move |r| r.sign_up.offer(&r.id, r.family.as_deref().unwrap_or("follow_the_leaders"), r.map.as_deref(),
            r.title_id.as_ref(), r.title.as_ref(), map))
            .chain(self.one_up_rows.iter().filter_map(move |r| r.sign_up.offer(&r.id, "1ups", r.map.as_deref(), r.title_id.as_ref(), r.title.as_ref(), map)))
    }

    /// The player is held from the start until the countdown releases it (PinPlayer / UnpinAI(player)).
    pub(crate) fn player_pinned(&self) -> bool {
        self.hom.as_ref().is_some_and(|h| h.pinned) || self.one_up.as_ref().is_some_and(|o| o.player_pinned()) || self.run.as_ref().is_some_and(|r| matches!(r.run.state(), State::PinPlayer | State::MakeObjectives | State::PrefilmDisplay
            | State::NisOverview | State::Setup | State::WaitForAssetLoad | State::SpawnPlayer | State::CheckAi | State::Wait | State::Countdown))
    }
}

/// A new map drops the old map's run and starts `SKATE_FTL`; a sign-up (the prompt at the row's sign-up sphere, the
/// career's first challenge) starts its row when nothing else runs.
fn arm(mut ftl: ResMut<FollowTheLeader>, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>,
    mut volumes: ResMut<crate::trigger_volumes::TriggerVolumes>, mut population: Option<ResMut<crate::living_world::PopulationState>>,
    mut signed: MessageReader<crate::challenges::ChallengeSignUp>, challenges: Option<Res<crate::challenges::Challenges>>,
    mut skater: ResMut<crate::physics::SkaterRuntime>) {
    let mut wanted = Vec::new();
    if ftl.generation != Some(map.generation) {
        ftl.generation = Some(map.generation);
        if !ftl.loaded {
            ftl.loaded = true;
            ftl.rows = load(&config.asset_root);
            info!("SKATE_FTL rows={}", ftl.rows.len());
        }
        if let Some(mut run) = ftl.run.take() {
            info!("SKATE_FTL {} dropped by the map change", run.data.id);
            if let (Some(ai), Some(pop)) = (run.leader.as_mut(), population.as_deref_mut()) {
                ai.remove(pop);
            }
        }
        wanted.extend(std::env::var("SKATE_FTL").ok());
        wanted.extend(std::env::var("SKATE_FILM").ok());
        // `--verify` runs: SKATE_VERIFY_CHALLENGE places the skater at the row's sign-up, as for own the spot.
        let verify = config.verification_capture.as_ref().and_then(|_| std::env::var("SKATE_VERIFY_CHALLENGE").ok());
        let path = map.path.as_deref();
        if let Some(o) = verify.and_then(|id| ftl.offers(path).find(|o| o.id == id)) {
            let at = ftl.rows.iter().find(|r| r.id == o.id).and_then(|r| r.sign_up.location.as_ref()).map(|l| l.matrix);
            info!("SKATE_VERIFY_CHALLENGE {} start at its sign-up {:?}", o.id, &o.location[..3]);
            if let Err(e) = at.map_or(Ok(()), |m| skater.travel_to(m)) {
                warn!("SKATE_VERIFY_CHALLENGE {e}");
            }
        }
    }
    let busy = challenges.as_deref().is_some_and(|c| c.run.is_some() || c.tutorial.is_some()) || ftl.one_up.is_some();
    for s in signed.read().filter(|s| s.body == crate::trigger_volumes::PLAYER) {
        if !ftl.rows.iter().any(|r| r.id == s.challenge) {
            continue;
        }
        if busy || ftl.run.is_some() {
            info!("SKATE_FTL sign-up {} ignored: a challenge is running", s.challenge);
            continue;
        }
        info!("SKATE_FTL sign-up {}", s.challenge);
        wanted.push(s.challenge.clone());
    }
    if let Some(id) = wanted.first() {
        let here = skater.animated_skeleton.roots.animation_to_world;
        start(&mut ftl, id, &config, &map, &mut volumes, here);
    }
}

fn start(ftl: &mut FollowTheLeader, wanted: &str, config: &crate::config::Config, map: &crate::map_transition::CurrentMap,
    volumes: &mut crate::trigger_volumes::TriggerVolumes, here: [[f32; 4]; 4]) {
    let Some(mut data) = ftl.rows.iter().find(|r| r.id == wanted).cloned() else {
        info!("SKATE_FTL {wanted} not found");
        return;
    };
    // The anywhere types start in place: SetSessionMarker() at the spot; a restart teleports back to it (b1003 §2).
    if matches!(kind(&data), Kind::AnywhereFilm | Kind::AnywherePhoto) {
        data.start = FtlStart { locator: "session_marker".into(), matrix: here, slots: Vec::new() };
    }
    if data.map.as_deref().is_some_and(|m| map.path.as_deref().is_some_and(|p| !crate::teleport_menu::same_map(p, m))) {
        info!("SKATE_FTL {wanted} is not on {}", map.name);
        return;
    }
    for v in [&data.end_volume, &data.boundary].into_iter().flatten().chain(&data.objective_volumes) {
        if volumes.get(&v.id).is_none() {
            volumes.map.push(crate::trigger_volumes::TriggerVolume::from_record(&v.volume));
        }
    }
    info!("SKATE_FTL {wanted}: {} objective volumes armed", data.objective_volumes.len());
    let map_volumes: Vec<(String, u64)> =
        volumes.map.iter().filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
    let Some(groups) = load_groups(&data, map_volumes, crate::challenge_objectives::trick_groups(&config.asset_root)) else {
        warn!("SKATE_FTL {wanted}: its owned objectives do not load; not started");
        return;
    };
    let rows: Vec<String> = data.competitors.iter().map(|c| c.character.clone()).collect();
    // films ride alone: only a row with competitors (follow the leader) loads its riders
    let leader = if rows.is_empty() { None } else {
        crate::contest_ai_host::ContestAi::load(&config.asset_root, &data.id, &rows, crate::race_ai_host::start_seed(map.generation))
    };
    let tracking = groups.iter().any(|g| g.objectives.tracks_unique());
    let mut run = FilmRun::new(Params { track_unique: tracking, ..params(&data) });
    info!("SKATE_FTL start {} at {} end={:?} max={:?} leaders={rows:?}", data.id, data.start.locator, data.end_volume.as_ref().map(|v| &v.name), data.max_follow_distance);
    let commands = run.start();
    info!("SKATE_FTL {wanted}: {} objective groups (killed it: {})", groups.len(), groups.iter().any(|g| g.killed));
    let mut state = FtlState { data, run, groups, evaluating: false, leader, awaiting_teleport: false, band: Some(FollowerBand::default()),
        carry: 0.0, eval_carry: 0.0, leader_last: None, leader_speed: 0.0, leader_path: Vec::new(), last_state: 0, failure_text: None, dialog_open: false, dialog_seconds: 0.0, leaders_in_end: Vec::new(), follow_indicators: Vec::new(), meter: None, queued: Vec::new(), feed: Default::default(), area_checked: false, reel: Default::default(), intro_group: None, photo: None };
    // The first commands run on the next update with the frame's resources.
    state.pending(commands);
    ftl.run = Some(state);
}

impl FtlState {
    fn pending(&mut self, commands: Vec<Command>) {
        self.queued.extend(commands);
    }
}

/// The highlight reel played out: a film goes on to its end options; a photo's NIS (the objective's highlight)
/// reaches its pick point (nischallengenotify).
fn reel_done(run: &mut FilmRun) -> Vec<Command> {
    if run.state() == State::DisplayPhoto {
        run.handle(Event::NisChallengeNotify)
    } else {
        run.handle(Event::HighlightReelComplete)
    }
}

/// A challenge's highlights (cHighlightGroup) and its reel (research b1050): films, own the spot, Hall of Meat.
#[derive(Default)]
pub(crate) struct ReelState {
    pub(crate) highlights: skate_core::challenges::highlights::Highlights,
    /// The replay time the running sequence began at (SequenceBegin: the highlight's start).
    pub(crate) sequence_begin: f64,
    pub(crate) in_sequence: bool,
    /// The objectives that completed (tier, index) in order, one per AddHighlight that follows.
    completed: std::collections::VecDeque<(bool, usize)>,
    clips: std::collections::VecDeque<skate_core::challenges::highlights::Highlight>,
    playing: bool,
    /// The clips played so far this reel (the take a scene with several and no cut list uses).
    played: usize,
    /// The NIS scenes' camera takes and their freeze points by scene name (scene_000.bin + ice_000.bin), loaded on
    /// the first reel.
    scenes: Option<std::collections::HashMap<String, Vec<SceneTake>>>,
    /// The scene and take of the last clip started (the current NIS NIS.GetPhotoNISCount reads).
    last_scene: Option<(String, usize)>,
    /// The placed anywhere photographer (position, facing): while one is placed every clip plays
    /// `photo_anywhere_located` from that spot (82BCE0D8 asks the photographer's locator interface, 822FFAD4).
    pub(crate) photographer: Option<(Vec3, Vec3)>,
}

/// The scene a clip plays while a photographer is placed (string 820D4860, 82BCE0D8).
const PHOTOGRAPHER_SCENE: &str = "photo_anywhere_located";

/// One take of an NIS scene as the reel uses it: the camera, its FREEZE_CMD points and its screen color lanes.
#[derive(Clone)]
pub(crate) struct SceneTake {
    track: skate_core::camera_track::CameraTrack,
    freezes: Vec<skate_data::ice_camera::FreezePoint>,
    colour: skate_data::ice_camera::TakeColour,
}

/// Every NIS scene's ICE camera takes by name (lower case), from the install's scene table.
fn scene_cameras(asset_root: &std::path::Path) -> std::collections::HashMap<String, Vec<SceneTake>> {
    let dir = asset_root.join("private/hud/scene");
    let (Ok(table), Ok(ice)) = (std::fs::read(dir.join("scene_000.bin")), std::fs::read(dir.join("ice_000.bin"))) else { return Default::default() };
    let Ok(scenes) = skate_data::scene_table::parse(&table) else { return Default::default() };
    scenes.iter().filter_map(|s| {
        let tracks = skate_data::ice_camera::scene_tracks(&ice, s.hash).ok().filter(|t| !t.is_empty())?;
        let mut freezes = skate_data::ice_camera::freeze_points(&ice, s.hash).unwrap_or_default().into_iter();
        let mut colours = skate_data::ice_camera::take_colours(&ice, s.hash).unwrap_or_default().into_iter();
        Some((s.name.to_ascii_lowercase(), tracks.into_iter().map(|track| SceneTake {
            track, freezes: freezes.next().unwrap_or_default(), colour: colours.next().unwrap_or_default(),
        }).collect()))
    }).collect()
}

impl ReelState {
    /// NIS.GetPhotoNISCount (8285F220): the photo shots (FREEZE_CMD 4 points) of the last clip's take (research b1061).
    pub(crate) fn photo_shots(&self) -> usize {
        let Some((scene, take)) = &self.last_scene else { return 0 };
        self.scenes.as_ref().and_then(|s| s.get(&scene.to_ascii_lowercase())).and_then(|takes| takes.get(*take))
            .map_or(0, |t| t.freezes.iter().filter(|p| p.command == skate_data::ice_camera::PHOTO_FREEZE).count())
    }

    /// SequenceBegin (b933): the replay time a line began.
    pub(crate) fn track_sequence(&mut self, in_sequence: bool, replay: Option<&crate::replay::Replay>) {
        if in_sequence && !self.in_sequence {
            self.sequence_begin = replay.map_or(0.0, |r| r.last_time());
        }
        self.in_sequence = in_sequence;
    }

    /// AddHighlightUsingDefinition (82BCCEB0) with replay times in seconds: `second_half` picks the definition's
    /// +0x50 scene (Lua arg 3), the window runs from `begin` minus pre to `end` plus post (an override above 0
    /// replaces the definition's), kept in tier `killed`. True when added.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add(&mut self, def: &skate_core::challenges::highlights::HighlightDefinition, killed: bool, second_half: bool,
        begin: f64, end: f64, drop_oldest: bool, pre: f32, post: f32, tuning: Option<&ChallengeTuning>) -> bool {
        let fps = f64::from(self.highlights.rules.frames_per_second);
        let frames = |t: f64| (t * fps).max(0.0) as u32;
        if let Some(t) = tuning {
            self.highlights.rules.slots = t.reel_slots;
        }
        self.highlights.add(def, killed, second_half, frames(begin), frames(end), drop_oldest, pre, post)
    }

    /// PlayHighlightReel (82BCDDE8) over every highlight in the group: false when there is none (the reel is
    /// complete at once). A mod may switch the reel off or change its transition (`challenges` tuning, ours).
    pub(crate) fn play(&mut self, mut replay: Option<&mut crate::replay::Replay>, pause: Option<&mut crate::pause_man::ThePauseMan>,
        asset_root: &std::path::Path, tuning: Option<&ChallengeTuning>) -> bool {
        self.clips = self.highlights.list().to_vec().into();
        self.play_prepared(replay.as_deref_mut(), pause, asset_root, tuning)
    }

    /// PlayHighlightReel with the clips already listed (a film's PrepareReel).
    pub(crate) fn play_prepared(&mut self, mut replay: Option<&mut crate::replay::Replay>, pause: Option<&mut crate::pause_man::ThePauseMan>,
        asset_root: &std::path::Path, tuning: Option<&ChallengeTuning>) -> bool {
        if tuning.is_some_and(|t| !t.reel_enabled) {
            self.clips.clear();
        }
        if let (Some(r), Some(t)) = (replay.as_deref_mut(), tuning) {
            r.reel_transition_seconds = Some(t.reel_transition_seconds);
        }
        if self.next_clip(replay, asset_root) {
            enter_replay_nis(pause);
            true
        } else {
            false
        }
    }

    /// A playing reel's frame: a clip that played out (cMsgNISComplete -> 82BCDFC0) pops the replay NIS's pause
    /// modes (8285F720) and the next one starts (a skip ends the whole reel). True when the reel completed.
    pub(crate) fn step(&mut self, mut replay: Option<&mut crate::replay::Replay>, mut pause: Option<&mut crate::pause_man::ThePauseMan>,
        asset_root: &std::path::Path) -> bool {
        if !self.playing || !replay.as_deref().is_none_or(|r| r.reel_done) {
            return false;
        }
        if let Some(p) = pause.as_deref_mut() {
            p.pop(skate_core::pause_man::mode::REPLAY_NIS);
            p.pop(skate_core::pause_man::mode::NIS);
        }
        if let Some(r) = replay.as_deref_mut() {
            r.reel_done = false;
            if std::mem::take(&mut r.reel_skipped) {
                self.clips.clear();
            }
        }
        if self.next_clip(replay, asset_root) {
            enter_replay_nis(pause);
            false
        } else {
            true
        }
    }

    /// The reel removed (a retry, an abandon): a playing clip stops and its pause modes go.
    pub(crate) fn stop(&mut self, replay: Option<&mut crate::replay::Replay>, pause: Option<&mut crate::pause_man::ThePauseMan>) {
        if self.playing {
            if let Some(r) = replay {
                r.stop_reel();
            }
            if let Some(p) = pause {
                p.pop(skate_core::pause_man::mode::REPLAY_NIS);
                p.pop(skate_core::pause_man::mode::NIS);
            }
        }
        self.clips.clear();
        self.playing = false;
        self.played = 0;
    }

    /// 82BCE0D8 / 82BCDFC0: starts the next clip on the replay as its NIS scene (the scene's camera take fitted to
    /// the window, `ReelShot`); false when none is left (the reel is complete). A scene with several takes and no cut
    /// list uses them in turn, clip by clip [inferred]; a scene without camera data keeps the recorded game camera.
    fn next_clip(&mut self, replay: Option<&mut crate::replay::Replay>, asset_root: &std::path::Path) -> bool {
        let (Some(clip), Some(replay)) = (self.clips.pop_front(), replay) else {
            self.playing = false;
            self.played = 0;
            return false;
        };
        let fps = f64::from(self.highlights.rules.frames_per_second);
        let (start, end) = (f64::from(clip.start) / fps, f64::from(clip.end) / fps);
        let scenes = self.scenes.get_or_insert_with(|| scene_cameras(asset_root));
        // A placed photographer replaces the definition's scene and place (82BCE0D8): photo_anywhere_located at its
        // spot and facing.
        let (scene, located) = match self.photographer {
            Some(at) => (PHOTOGRAPHER_SCENE.to_owned(), Some(at)),
            None => (clip.scene.scene.clone(), clip.scene.located.then(|| (Vec3::from_array(clip.scene.position), Vec3::from_array(clip.scene.direction)))),
        };
        let take = scenes.get(&scene.to_ascii_lowercase()).map_or(0, |takes| self.played % takes.len().max(1));
        let track = scenes.get(&scene.to_ascii_lowercase()).and_then(|takes| takes.get(take)).cloned();
        self.last_scene = Some((scene.clone(), take));
        info!("SKATE_FTL highlight clip {scene} frames {}..{} camera {}{}", clip.start, clip.end,
            if track.is_some() { "scene take" } else { "game camera (no scene take)" },
            if self.photographer.is_some() { " from the placed photographer" } else { "" });
        replay.play_reel_clip(start, end, track.map(|t| crate::replay::ReelTake { track: t.track, located, freezes: t.freezes, colour: t.colour }));
        self.played += 1;
        self.playing = true;
        true
    }
}

/// The objective HUD's rows for a group (b844 / b947): the objectives with DisplayInHUD, in row order, each with its
/// title text id (the movie localizes it; `#OBJECTIVE NOT VALID!` without one, as retail's 8264D130) and its state
/// as the movie reads it: 0 failed, 1 complete, 2 not active yet, 3 active.
pub(crate) fn objective_rows(rows: &[ObjectiveData], states: &[skate_core::challenges::objectives::State]) -> Vec<crate::hud_challenge::ObjectiveRow> {
    use skate_core::challenges::objectives::State as S;
    rows.iter().zip(states).filter(|(r, _)| r.display_in_hud).map(|(r, s)| crate::hud_challenge::ObjectiveRow {
        state: match s {
            S::Failed => 0,
            S::Complete => 1,
            S::Inactive | S::Armed => 2,
            S::Active => 3,
        },
        kind: 0,
        title: r.title_id.clone().unwrap_or_else(|| "#OBJECTIVE NOT VALID!".into()),
        value: String::new(),
        title_arg: r.title_arg.clone(),
    }).collect()
}

fn hud_component(c: HudComponent) -> Option<Component> {
    match c {
        HudComponent::Timer => Some(Component::Timer),
        HudComponent::Objective => Some(Component::Objective),
        HudComponent::OwnedIt => Some(Component::OwnedIt),
        HudComponent::KilledIt => Some(Component::KilledIt),
        HudComponent::Countdown => Some(Component::Countdown),
        HudComponent::DistanceMeter => Some(Component::DistanceMeter),
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut ftl: ResMut<FollowTheLeader>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    (mut entered, mut exited, mut central, mut career): (
        MessageReader<crate::trigger_volumes::TriggerEntered>,
        MessageReader<crate::trigger_volumes::TriggerExited>,
        MessageWriter<crate::hud_central::CentralMessage>,
        MessageWriter<crate::career::ChallengeResult>,
    ),
    mut hud: ResMut<ChallengeHudState>,
    mut end_dialog: Option<ResMut<crate::end_dialog_host::EndDialogScreen>>,
    mut autopilot: Option<ResMut<crate::autopilot::Autopilot>>,
    (time, menus, mut follow, mut presenter, mut replay): (Res<Time>, Option<Res<crate::retail_menus::RetailMenus>>, ResMut<crate::visual_indicators::FollowIndicators>, Option<ResMut<crate::presentation_run::Presenter>>, Option<ResMut<crate::replay::Replay>>),
    (mut photographer, mut challenge_indicators): (ResMut<AnywherePhotographer>, ResMut<crate::visual_indicators::ChallengeIndicators>),
    (challenges, pad, physics, config, mut pause, tuning): (Option<Res<crate::challenges::Challenges>>, Option<Res<crate::input::ControllerInput>>, Option<Res<crate::physics::GamePhysics>>, Option<Res<crate::config::Config>>, Option<ResMut<crate::pause_man::ThePauseMan>>, Option<Res<ChallengeTuning>>),
    (mut population, index, npc_transforms, mut clocks, mut commands_ecs, replays, mut brains): (
        Option<ResMut<crate::living_world::PopulationState>>,
        Option<Res<crate::living_world::npc_skaters::NpcSkaterIndex>>,
        Query<&Transform, With<crate::living_world::npc_skaters::NpcSkater>>,
        Query<&mut crate::living_world::npc_skaters::NpcRaceClock>,
        Commands,
        Query<&crate::living_world::npc_skaters::NpcReplay>,
        Query<&mut crate::living_world::npc_sim::NpcSim, With<crate::autopilot::Possessed>>,
    ),
) {
    let entered: Vec<u64> = entered.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let exited: Vec<u64> = exited.read().filter(|e| e.body == crate::trigger_volumes::PLAYER)
        .filter_map(|e| u64::from_str_radix(&e.volume, 16).ok()).collect();
    let Some(state) = ftl.run.as_mut() else {
        // 825681F8: the challenge ended, the photographer and its indicator go.
        if photographer.state.active {
            info!("SKATE_FTL photographer cleared");
            photographer.state.end();
            photographer.place_requested = false;
            challenge_indicators.0.clear();
        }
        if !follow.0.is_empty() {
            follow.0.clear();
        }
        if let Some(a) = autopilot.as_mut().filter(|a| a.follow.is_some() || a.area.is_some()) {
            a.follow = None;
            a.area = None;
        }
        for mut b in &mut brains {
            b.speed_cap = None;
        }
        return;
    };
    let id = state.data.id.clone();
    let asset_root = config.as_ref().map(|c| c.asset_root.clone()).unwrap_or_default();
    let mut commands = std::mem::take(&mut state.queued);
    // 825680D8: an anywhere photo arms the photographer (unplaced); the phone or a mod places it at the player's root.
    if kind(&state.data) == Kind::AnywherePhoto && photographer.state.start(skate_core::challenges::photographer::ANYWHERE_PHOTO_TYPE) {
        info!("SKATE_FTL {id} photographer armed");
        photographer.place_requested = std::env::var("SKATE_PHOTOGRAPHER").is_ok_and(|v| v == "place");
    }
    if std::mem::take(&mut photographer.place_requested) {
        let at = skater.animated_skeleton.roots.animation_to_world;
        if photographer.state.place(&at) {
            info!("SKATE_FTL {id} photographer placed at {:?}", photographer.state.position);
            challenge_indicators.0 = vec![crate::visual_indicators::Placement { key: PHOTOGRAPHER_INDICATOR.into(), locator: String::new(),
                matrix: at, style: None, colour: None, always: false, fit: None }];
        }
    }
    // nis_overview: presentationeventgroupfinished for the intro group -> setup.
    if let (Some(group), Some(p)) = (state.intro_group.as_deref(), presenter.as_deref_mut()) {
        if p.take_finished(group) {
            state.intro_group = None;
            commands.extend(state.run.handle(Event::PresentationFinished));
        }
    }
    // The end options dialog: its pick feeds Continue / Retry; the autopilot continues after a short look.
    if state.dialog_open {
        use skate_core::challenges::end_dialog::Action;
        state.dialog_seconds += time.delta_secs();
        if autopilot.as_ref().is_some_and(|a| a.requested) && state.dialog_seconds > AUTOPILOT_DIALOG_SECONDS {
            if let Some(d) = end_dialog.as_deref_mut().filter(|d| d.choice.is_none() && d.open) {
                info!("SKATE_FTL {id} end options: the autopilot continues");
                d.choice = Some(Action::Continue);
                d.open = false;
            }
        }
        match end_dialog.as_deref_mut() {
            Some(d) => {
                let event = match d.choice.take() {
                    Some(Action::Continue) => Some(Event::Continue),
                    Some(Action::Retry { .. }) => Some(Event::Retry),
                    _ => None,
                };
                if let Some(event) = event {
                    d.open = false;
                    state.dialog_open = false;
                    commands.extend(state.run.handle(event));
                }
            }
            None => {
                state.dialog_open = false;
                commands.extend(state.run.handle(Event::Continue));
            }
        }
    }
    // Positions: the player's hips (as the contests), each leader's body.
    let player = crate::trigger_volumes::player_points(&skater)[2];
    let leaders: Vec<(u32, Option<[f32; 3]>, Option<[f32; 3]>)> = match (state.leader.as_ref(), index.as_deref()) {
        (Some(ai), Some(index)) => ai.riders.iter().map(|r| {
            let t = r.npc.and_then(|n| index.0.get(&n)).and_then(|e| npc_transforms.get(*e).ok());
            (r.id, t.map(|t| [t.translation.x, t.translation.y + 1.0, t.translation.z]), t.map(|t| { let f = t.forward(); [f.x, f.y, f.z] }))
        }).collect(),
        _ => Vec::new(),
    };
    // A film with a boundary: the autopilot (a test aid) keeps to the lines that run through it, so a test run stays
    // where the objectives are instead of riding off and being sent back to the start.
    if let (Some(a), Some(b), Some(pop)) = (autopilot.as_mut(), state.data.boundary.as_ref(), population.as_deref()) {
        if state.leader.is_none() && a.area.is_none() && !state.area_checked {
            state.area_checked = true;
            // the lines through the objectives' own volumes (the spots that complete them), else through the boundary
            let lines = crate::living_world::npc_skaters::npc_lines(pop);
            let through = |boxes: &[skate_core::triggers::OrientedBox]| -> Vec<[u8; 16]> {
                lines.values().filter(|l| l.nodes.iter().any(|n| boxes.iter().any(|b| b.contains_point(n.position)))).map(|l| l.id).collect()
            };
            let spots: Vec<_> = state.data.objective_volumes.iter().map(|v| v.volume.shape.oriented_box()).collect();
            // first the lines that do a trick (a grind, a flip) inside one: the objectives ask for a trick at the spot
            let tricking: Vec<[u8; 16]> = lines.values().filter(|l| {
                let mut open = false;
                l.nodes.iter().any(|n| {
                    use skate_core::living_world::replay::node_events::{END_TRICK, START_TRICK};
                    open = match n.event { START_TRICK => true, END_TRICK => false, _ => open };
                    open && spots.iter().any(|b| b.contains_point(n.position))
                })
            }).map(|l| l.id).collect();
            let mut inside = if tricking.is_empty() { through(&spots) } else { tricking.clone() };
            if !tricking.is_empty() {
                info!("SKATE_FTL {id} autopilot area: {} lines trick inside the objective volumes", tricking.len());
            }
            let what = if inside.is_empty() {
                inside = through(&[b.volume.shape.oriented_box()]);
                b.name.clone()
            } else {
                format!("{} objective volumes", spots.len())
            };
            info!("SKATE_FTL {id} autopilot area: {} lines through {what}", inside.len());
            if !inside.is_empty() {
                a.area = Some(std::sync::Arc::new(inside));
            }
        }
    }
    // The autopilot (a test aid) takes the first leader's lines in the leader's order: the line the leader took after
    // the one the brain is on (after a bail it keeps to the path instead of walking to where the leader is now).
    if let Some(a) = autopilot.as_mut() {
        let line_of = |id: Option<skate_core::living_world::LivingWorldId>| id.and_then(|n| index.as_deref().and_then(|i| i.0.get(&n).copied()))
            .and_then(|e| replays.get(e).ok()).map(|r| r.cursor.line);
        if let Some(l) = line_of(state.leader.as_ref().and_then(|ai| ai.riders.first()).and_then(|r| r.npc)) {
            if state.leader_path.last() != Some(&l) {
                state.leader_path.push(l);
            }
        }
        let line = follow_line(&state.leader_path, line_of(a.npc_id()));
        if a.follow != line {
            a.follow = line;
        }
    }
    // ...and keeps a gap: it may not pass the leader (OURS, a test aid; FOLLOW_GAP).
    if let Some((_, Some(at), Some(fwd))) = leaders.first() {
        let dt = time.delta_secs().max(1e-3);
        if let Some(last) = state.leader_last {
            let v = ((at[0] - last[0]).powi(2) + (at[2] - last[2]).powi(2)).sqrt() / dt;
            if v < 30.0 {
                state.leader_speed += (v - state.leader_speed) * (dt * 2.0).min(1.0);
            }
        }
        state.leader_last = Some(*at);
        let behind = (at[0] - player[0]) * fwd[0] + (at[2] - player[2]) * fwd[2];
        let cap = follow_cap(behind, state.leader_speed);
        for mut b in &mut brains {
            if b.speed_cap != cap {
                b.speed_cap = cap;
            }
        }
    }
    let mut distances = [None; 4];
    for (n, at, _) in &leaders {
        if let (Some(at), Some(slot)) = (at, distances.get_mut(*n as usize - 1)) {
            *slot = Some(((at[0] - player[0]).powi(2) + (at[1] - player[1]).powi(2) + (at[2] - player[2]).powi(2)).sqrt());
        }
    }
    // The leader's body entering the end volume (triggerentercollision with its competitor index, b1006 §0.6).
    let running = state.run.state() == State::ChallengeRunning;
    if let Some(end) = state.data.end_volume.as_ref().filter(|_| running) {
        let shape = end.volume.shape.oriented_box();
        for (n, at, _) in &leaders {
            let inside = at.is_some_and(|p| shape.contains_point(p));
            if inside && !state.leaders_in_end.contains(n) {
                state.leaders_in_end.push(*n);
                info!("SKATE_FTL {id} leader {n} reached the end volume");
                if let Some(v) = volume_id(&state.data.end_volume) {
                    commands.extend(state.run.handle(Event::TriggerEnter { volume: v, competitor: *n }));
                }
            }
        }
    }
    // The player's volumes: the run (end volume, boundary) and the objectives.
    for &v in &exited {
        commands.extend(state.run.handle(Event::TriggerExit { volume: v }));
    }
    for &v in &entered {
        commands.extend(state.run.handle(Event::TriggerEnter { volume: v, competitor: 0 }));
    }
    let state_id = skater.physical_state_id();
    if state_id == 300 && state.last_state != 300 {
        commands.extend(state.run.handle(Event::Wipeout));
    }
    state.last_state = state_id;
    // The groups get what the tutorial's get (the shared feed): skater facts, the frame's volumes, air, pushes,
    // tricks, markers and banked sequences, and one evaluation per 60 Hz challenge tick, so a row's own counters (the
    // speech rows' `Delay` of 60 evaluations) keep their time. [inferred: retail's evaluation rate is open, b1006]
    state.eval_carry += time.delta_secs() * TICKS_PER_SECOND;
    let ticks = state.eval_carry as u32;
    state.eval_carry -= state.eval_carry.floor();
    let shared = challenges.as_deref();
    let banked: Vec<(i32, bool)> = shared.map(|c| c.banked_this_frame.iter().map(|s| (s.score, s.wiped_out)).collect()).unwrap_or_default();
    let raw = pad.as_ref().map(|p| p.raw_input()).unwrap_or_default();
    let feed_input = crate::challenge_objectives::FeedInput {
        entered: &entered, exited: &exited, banked: &banked, ticks, state_id, in_sequence: skater.scoring.sequence_active() || crate::challenge_objectives::force_line(),
        markers: shared.map_or(&[][..], |c| &c.markers_this_frame), tricks: shared.map_or(&[][..], |c| &c.tricks_done_this_frame),
        began: shared.map_or(&[][..], |c| &c.tricks_began_this_frame), right_stick_y: raw.right[1], triggers: raw.triggers,
        manual: skater.scoring.manual(), pushes: shared.map_or(0, |c| c.pushes_this_frame),
        pump_acceleration: skater.ground.pumping.pump_acceleration, pump_user: skater.ground.pumping.intentional_pumping != 0,
        grind_rail: (400..=405).contains(&state_id).then_some(skater.player_input.grind.rail_owner).flatten()
            .and_then(|owner| physics.as_ref().and_then(|p| p.rail_guids(owner))),
        time_to_grind: {
            let air = &skater.player_input.physical.air;
            if air.flag_443 != 0 { air.collision_time_180 - air.time_in_state_176 } else { skate_core::challenges::api::NO_GRIND_TIME }
        },
        board_adjust: skater.animation_input.fields.board_adjust,
        wipeout_over: skater.player_input.physical.skeleton.over_599 != 0,
        bail_pose: shared.and_then(|c| c.last_bail_pose),
        bail_score: shared.and_then(|c| c.last_bail_score).map_or(0, |s| s.total),
        bail: shared.map(|c| c.last_bail).unwrap_or_default(),
        competitor_distances: distances,
        photographer: photographer.state.placed.then(|| photographer.state.distance(Some({
            let t = skater.animated_skeleton.roots.animation_to_world;
            [t[3][0], t[3][1], t[3][2]]
        }))),
        live: crate::challenge_objectives::live_facts(&skater, &raw),
    };
    let killed_of: Vec<bool> = state.groups.iter().map(|g| g.killed).collect();
    let mut groups: Vec<&mut DataObjectives> = state.groups.iter_mut().map(|g| &mut g.objectives).collect();
    let fed = state.feed.step(&mut groups, &feed_input, state.evaluating);
    // SequenceBegin (b933): the replay time a line began, the start of the highlight its objective adds.
    state.reel.track_sequence(skater.scoring.sequence_active(), replay.as_deref());
    // The highlight manager reads the photographer's locator interface for each clip (82BCE0D8).
    state.reel.photographer = photographer.state.placed
        .then(|| (Vec3::from_array(photographer.state.position), Vec3::from_array(photographer.state.direction)));
    // A reel clip played out: the next one, after the last the reel is complete.
    if state.reel.step(replay.as_deref_mut(), pause.as_deref_mut(), &asset_root) {
        info!("SKATE_FTL {id} highlight reel complete");
        commands.extend(reel_done(&mut state.run));
    }
    if state.evaluating {
        let events: Vec<(bool, GroupEvent)> = fed.into_iter().map(|(i, e)| (killed_of[i], e)).collect();
        let finished: Vec<bool> = events.iter().filter(|(_, e)| *e == GroupEvent::AllComplete).map(|(k, _)| *k).collect();
        for g in &mut state.groups {
            let o = &mut g.objectives;
            for (competitor, on) in o.follow_indicator.drain(..) {
                // retail removes any entry for the id first, so adding twice keeps one (82D4F470)
                state.follow_indicators.retain(|&c| c != competitor);
                if on {
                    state.follow_indicators.push(competitor);
                }
                info!("SKATE_FTL {id} follow indicator {competitor} {on}");
            }
        }
        // the followed competitors' bodies get the trail (by their living-world serial)
        let serials: Vec<u32> = state.leader.as_ref().map(|ai| ai.riders.iter()
            .filter(|r| state.follow_indicators.contains(&r.id)).filter_map(|r| r.npc.map(|n| n.serial)).collect()).unwrap_or_default();
        if follow.0 != serials {
            follow.0 = serials;
        }
        for (killed, e) in events {
            let rows = if killed { &state.data.objectives.killed } else { &state.data.objectives.owned };
            match e {
                GroupEvent::Completed(i) => {
                    info!("SKATE_FTL {id} objective {i} complete (killed it group: {killed}) {}", rows.get(i).map_or("", |o| o.key.as_str()));
                    // ObjectiveComplete to the machine (b933): the partial-objective speech and its highlight.
                    state.reel.completed.push_back((killed, i));
                    commands.extend(state.run.handle(Event::ObjectiveComplete { killed, last: finished.contains(&killed) }));
                }
                GroupEvent::AllComplete => {
                    info!("SKATE_FTL {id} all objectives complete (killed it group: {killed})");
                    commands.extend(state.run.handle(Event::AllObjectivesComplete { killed }));
                }
                GroupEvent::FailureCondition { objective, row } => {
                    let text = rows.get(objective).and_then(|o| o.inline_failure(row)).and_then(|f| f.text.clone());
                    info!("SKATE_FTL {id} objective {objective} (killed it group: {killed}) failure row {row} ({text:?}) distances {distances:?}");
                    state.failure_text = Some(text.unwrap_or_else(|| FAILURE_TEXT.into()));
                    commands.extend(state.run.handle(Event::ObjectiveFailed));
                }
                // Photos: leaving the PhotoChallengeBoundary (a mode 3 objective volume) fails the shot (b875 §1).
                GroupEvent::LeftScoringBoundary(i) => {
                    info!("SKATE_FTL {id} left the scoring boundary of objective {i}");
                    commands.extend(state.run.handle(Event::LeftScoringBoundary));
                }
                // A hint row plays its presentation only (82C755B8, b942): the leader's speech lines and the start tip.
                GroupEvent::Hint { objective, row } => {
                    let events = rows.get(objective).and_then(|o| o.hints.get(row)).map(|h| h.presentation.events()).unwrap_or_default();
                    if let (Some(p), false) = (presenter.as_deref_mut(), events.is_empty()) {
                        let group = format!("{id}/{}obj{objective}/hint{row}", if killed { "killed/" } else { "" });
                        let started = p.manager.start(skate_core::presentation::Group { id: group.clone(), owner: id.clone(), events });
                        info!("SKATE_FTL {id} hint row {row} of objective {objective}: presentation {group} {started:?}");
                    }
                }
                _ => {}
            }
        }
    }
    if state.awaiting_teleport && !skater.teleport_pending() {
        state.awaiting_teleport = false;
        commands.extend(state.run.handle(Event::TeleportFinished));
    }
    state.carry += time.delta_secs() * TICKS_PER_SECOND;
    while state.carry >= 1.0 {
        state.carry -= 1.0;
        commands.extend(state.run.tick());
    }
    let mut finished = None;
    let mut guard = 0;
    loop {
        let mut queue: std::collections::VecDeque<Command> = std::mem::take(&mut commands).into();
        while let Some(c) = queue.pop_front() {
            guard += 1;
            match c {
                Command::CreateHud { .. } => queue.extend(state.run.handle(Event::HudLoaded)),
                Command::StartPresentation => {
                    state.intro_group = crate::challenge_objectives::start_presentation(presenter.as_deref_mut(), &id, "intro", &state.data.intro);
                    if state.intro_group.is_none() {
                        queue.extend(state.run.handle(Event::PresentationFinished));
                    }
                }
                Command::SpawnAi | Command::RespawnAiAtStart => {
                    if let (Some(ai), Some(pop)) = (state.leader.as_mut(), population.as_deref_mut()) {
                        ai.place(&state.data.start.matrix, &state.data.start.slots, &state.data.ai_paths.paths(), pop);
                    }
                }
                Command::PinAi(true) => state.leader.iter_mut().for_each(|a| a.stop()),
                Command::PinAi(false) | Command::StartAiPaths => state.leader.iter_mut().for_each(|a| a.go()),
                // PinAI(leader): the leader that reached the end (competitor 1, b1006 §0.6).
                Command::PinLeader => {
                    if let Some(r) = state.leader.as_mut().and_then(|a| a.riders.first_mut()) {
                        r.moving = false;
                    }
                }
                // retail repositions twice in one frame after a second out-of-bounds (the stale flag, b999 §2); the
                // player is already on the way to the start, so the second one is that same move
                Command::RepositionPlayer if state.awaiting_teleport && skater.teleport_pending() => {
                    info!("SKATE_FTL {id} already on the way to the start");
                }
                Command::RepositionPlayer => {
                    let at = state.data.start.slots.first().copied().unwrap_or(state.data.start.matrix);
                    info!("SKATE_FTL {id} player to the start {:?}", &at[3][..3]);
                    if let Err(e) = skater.travel_ex(at, None, !state.data.spawn_offboard) {
                        warn!("SKATE_FTL teleport {e}");
                    }
                    if let Some(a) = autopilot.as_mut() {
                        a.host_teleported = true;
                    }
                    state.awaiting_teleport = true;
                }
                Command::Evaluation(on) => state.evaluating = on,
                // every objective active (the films' flow, b934): not only the first one, which left a film's other
                // objectives inactive however often their spots were skated (desktop run 2026-11-13)
                Command::ActivateStartObjectives => {
                    for g in &mut state.groups {
                        g.objectives.activate_flow();
                        info!("SKATE_FTL {id} objectives start (killed it group: {}): {:?}", g.killed, g.objectives.states());
                    }
                }
                Command::ResetObjectives => {
                    // a restart resets and the countdown activates later; a re-arm while running (a film's wipeout,
                    // objective failure or session marker, b999 §2) starts the objectives over at once
                    let running = state.run.state() == State::ChallengeRunning;
                    if !running {
                        state.leaders_in_end.clear();
                        state.leader_path.clear();
                        state.failure_text = None;
                    }
                    for g in &mut state.groups {
                        g.objectives.reset_only();
                        if running {
                            g.objectives.activate_flow();
                        }
                    }
                }
                Command::ShowHud(c, shown) => {
                    // Countdown entry: SetupDistanceHUDData(max = the row's FTLMaxFollowDistance, current) with the show.
                    // OURS: a row without a max gets -1 (never red).
                    if matches!(c, HudComponent::DistanceMeter) && shown {
                        let max = state.data.max_follow_distance.unwrap_or(-1.0);
                        state.meter = Some(skate_core::challenges::distance_meter::DistanceMeter::setup(max, distances[0].unwrap_or(0.0)));
                    }
                    match hud_component(c) {
                        Some(c) => hud.show(c, shown),
                        None => info!("SKATE_FTL {id} ShowHud({c:?}, {shown}) NOT RETAIL YET"),
                    }
                }
                Command::StartTimerHud { .. } => hud.show(Component::Timer, true),
                Command::CentralMessage(m) => {
                    // The row's text ids (C.CentralMessageHALID* in the first slot, ModifiedCentralMessageHALID* in the
                    // second, objectives_shared).
                    let row = |k: &str| state.data.central_messages.get(k).cloned();
                    let (text, modified) = match m {
                        Message::ObjectiveFailure => (Some(state.failure_text.clone().unwrap_or_else(|| FAILURE_TEXT.into())), false),
                        Message::ReturnToArea => (Some(row("return_to_area").unwrap_or_else(|| crate::challenges::RETURN_TO_AREA.into())), false),
                        Message::ChallengeFailed => (row("challenge_failed"), false),
                        Message::LineTimerExpired => (row("line_timer_expired"), true),
                        Message::OutOfBounds => (row("out_of_bounds"), true),
                        Message::ScoredLow => (row("scored_low"), true),
                        Message::TimeUp => (row("time_up"), true),
                    };
                    let Some(text) = text else {
                        info!("SKATE_FTL {id} central message {m:?}: no text id in the export (Refresh Game Files)");
                        continue;
                    };
                    central.write(if modified { crate::hud_central::CentralMessage::modified(text) } else { crate::hud_central::CentralMessage::new(text) });
                }
                // The highlights (research b1050): a window per completed objective with a HighlightDefinition, in
                // replay frames (1/60 s); the reel plays the result's tier, each window as a replay clip.
                Command::AddHighlight { killed } => {
                    let def = state.reel.completed.pop_front().filter(|(k, _)| *k == killed).and_then(|(k, i)| {
                        let rows = if k { &state.data.objectives.killed } else { &state.data.objectives.owned };
                        rows.get(i).and_then(|o| Some((o.highlight.as_ref()?.definition(), o.auto_highlight_timing)))
                    });
                    if let (Some((def, auto)), Some(r)) = (def, replay.as_deref()) {
                        let begin = if state.reel.in_sequence { state.reel.sequence_begin } else { r.last_time() };
                        // objectives_shared objectivecomplete@345-521: arg 3 is C.KilledIt, which the film graphs only
                        // set false (setup@53), so the +0x10 half; no drop-oldest. AllowAutoHighlightTiming (not a
                        // photo, no kill-it highlight as KilledIt stays false): pre / post 0.5 (@466-481).
                        let (pre, post) = if auto && !kind(&state.data).photo() { (0.5, 0.5) } else { (0.0, 0.0) };
                        let added = state.reel.add(&def, killed, false, begin, r.last_time(), false, pre, post, tuning.as_deref());
                        info!("SKATE_FTL {id} highlight added={added} {:?}", state.reel.highlights.list().last());
                    }
                }
                Command::AddLastUniqueTrick { killed } => {
                    if let Some(g) = state.groups.iter_mut().find(|g| g.killed == killed) {
                        let n = g.objectives.add_last_unique_trick();
                        info!("SKATE_FTL {id} unique tricks {n} (killed it group: {killed})");
                    }
                }
                Command::RemoveAllHighlights => state.reel.highlights.clear(),
                Command::PrepareReel { killed } => {
                    state.reel.clips = state.reel.highlights.reel(killed).into();
                    info!("SKATE_FTL {id} highlight reel: {} clips", state.reel.clips.len());
                }
                Command::PlayHighlightReel => {
                    // photo_skate2: the photo NIS stays up at its end until the shot picker finishes.
                    if let Some(r) = replay.as_deref_mut() {
                        r.reel_keep_still = state.run.state() == State::DisplayPhoto;
                    }
                    if !state.reel.play_prepared(replay.as_deref_mut(), pause.as_deref_mut(), &asset_root, tuning.as_deref()) {
                        queue.extend(reel_done(&mut state.run));
                    }
                }
                // FrontEnd.OpenPhotoSelection(NIS.GetPhotoNISCount) (photo_skate2 display_photo): the shot picker
                // (`photo_picker`) over the held photo NIS; without a held NIS the shot is taken as it is.
                Command::OpenPhotoSelection => {
                    let shots = state.reel.photo_shots() as u32;
                    let held = replay.as_deref().is_some_and(|r| r.reel_still);
                    info!("SKATE_FTL {id} photo selection over {shots} shots ({:?}), NIS held {held}", state.reel.last_scene);
                    if held {
                        let tables = crate::photo_picker::edit_tables(&asset_root);
                        let (picker, out) = skate_core::challenges::photo_select::PhotoSelect::open(shots, tables, Default::default(),
                            false, state.data.photo_layout.is_some());
                        state.photo = Some(picker);
                        crate::photo_picker::carry_out(&id, out, replay.as_deref_mut(), None, None);
                    } else {
                        queue.extend(state.run.handle(Event::NisComplete));
                    }
                }
                Command::SetSessionMarkerHere => info!("SKATE_FTL {id} session marker at the start spot {:?}", &state.data.start.matrix[3][..3]),
                Command::TeleportToSessionMarker => {
                    info!("SKATE_FTL {id} back to the start spot {:?}", &state.data.start.matrix[3][..3]);
                    if let Err(e) = skater.travel_ex(state.data.start.matrix, None, !state.data.spawn_offboard) {
                        warn!("SKATE_FTL teleport {e}");
                    }
                }
                Command::OpenContinueRetry => {
                    let killed = state.run.result() == Flow::Kill;
                    let won = killed || state.run.result() == Flow::Win;
                    info!("SKATE_FTL {id} end options won={won} killed={killed}");
                    match end_dialog.as_deref_mut().filter(|d| d.available) {
                        Some(d) => {
                            d.dialog.passed = won;
                            d.dialog.killed = killed;
                            d.title = state.data.title_id.clone();
                            d.open = true;
                            state.dialog_open = true;
                            state.dialog_seconds = 0.0;
                        }
                        None => queue.extend(state.run.handle(Event::Continue)),
                    }
                }
                Command::Complete(c) => finished = Some(c),
                // LuaBind_PauseSimulation (828409C8): pause mode 5, whose mask has bit 0, so GameSession_StepSimulation
                // stops stepping the worlds; audio is not a pause manager client and plays on.
                Command::PauseSimulation(on) => {
                    if let Some(p) = pause.as_deref_mut() {
                        if on {
                            p.push(skate_core::pause_man::mode::SIMULATION);
                        } else {
                            p.pop(skate_core::pause_man::mode::SIMULATION);
                        }
                    }
                    info!("SKATE_FTL {id} simulation {}", if on { "paused" } else { "resumed" });
                }
                other => info!("SKATE_FTL {id} {other:?}"),
            }
        }
        // A completion the machine ignored (locked out) adds no highlight.
        state.reel.completed.clear();
        // The waits the machine polls: assets (none to load), the leader's existence (spawned at once or never).
        match state.run.state() {
            State::WaitForAssetLoad => commands.extend(state.run.handle(Event::AssetsLoaded)),
            State::CheckAi => {
                let exists = state.leader.as_ref().is_some_and(|a| a.riders.iter().any(|r| r.npc.is_some()));
                if !exists {
                    warn!("SKATE_FTL {id}: no leader could spawn (lines or look missing); abandoned");
                }
                commands.extend(state.run.handle(if exists { Event::AiExist(true) } else { Event::Abandon }));
            }
            _ => {}
        }
        if commands.is_empty() || guard > 256 {
            break;
        }
    }
    // The follower band on each leader while it rides (b969).
    if let Some(ai) = state.leader.as_mut() {
        for r in &mut ai.riders {
            let band = leaders.iter().find(|(n, ..)| *n == r.id).and_then(|(_, at, fwd)| Some((at.as_ref()?, fwd.as_ref()?)));
            r.scale = match (state.band.as_ref(), band) {
                (Some(b), Some((at, fwd))) => shaped_speed(1.0, 0.0, follower_band(*at, *fwd, player, b), &BandTuning::default()),
                _ => 1.0,
            };
        }
        if let (Some(index), Some(pop)) = (index.as_deref(), population.as_deref()) {
            ai.step(pop.world.tick(), index, &mut clocks, &mut commands_ecs);
        }
    }
    if hud.is_shown(Component::Timer) {
        let (text, flag) = skate_core::challenges::timers::hud_text(state.run.ftl_ticks().unwrap_or(0));
        if hud.timer_text != text || hud.timer_flag != flag {
            hud.timer_text = text;
            hud.timer_flag = flag;
        }
    }
    // The objective HUD (SetupObjectiveHUDData on the first owned group, b875): one row per objective shown on the
    // HUD, its title text id and its state as the objective movie reads it. NOT RETAIL YET: a replay of an owned
    // challenge shows the first killed group instead.
    if hud.is_shown(Component::Objective) {
        let states = state.groups.iter().find(|g| !g.killed).map(|g| g.objectives.states()).unwrap_or_default();
        let rows = objective_rows(&state.data.objectives.owned, &states);
        if hud.objectives != rows {
            hud.objectives = rows;
        }
    }
    if let Some(meter) = state.meter.as_mut().filter(|_| hud.is_shown(Component::DistanceMeter)) {
        let default = skate_core::challenges::distance_meter::MeterTuning::default();
        let meter_tuning = tuning.as_deref().map_or(&default, |t| &t.meter);
        meter.advance(time.delta_secs(), distances[0].unwrap_or(meter.shown), meter_tuning);
        let now = crate::hud_challenge::DistanceHud {
            metres: meter.shown,
            imperial: menus.as_deref().is_some_and(crate::retail_menus::imperial),
            flag: meter.flag(meter_tuning),
        };
        if hud.distance != now {
            hud.distance = now;
        }
    }
    if let Some(c) = finished {
        let mut done = ftl.run.take().expect("the run");
        if let (Some(ai), Some(pop)) = (done.leader.as_mut(), population.as_deref_mut()) {
            ai.remove(pop);
        }
        info!("SKATE_FTL end {id} completion={c:?}");
        ftl.result = Some(format!("ftl={id} completion={c:?}"));
        // OURS: a run that ends while paused (an abandon) does not leave its pause modes behind.
        if let Some(p) = pause.as_deref_mut() {
            for m in [skate_core::pause_man::mode::SIMULATION, skate_core::pause_man::mode::REPLAY_NIS, skate_core::pause_man::mode::NIS] {
                p.pop(m);
            }
        }
        for h in [Component::Timer, Component::Countdown, Component::OwnedIt, Component::KilledIt, Component::DistanceMeter] {
            hud.show(h, false);
        }
        if c != Completion::Abandoned || done.run.result() == Flow::Win {
            career.write(crate::career::ChallengeResult { challenge: id, completion: c, reactivate: false });
        }
    }
}

/// The `challenges` world tuning (ours, doc 42; retail values by default): the distance meter and the highlight reel.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct ChallengeTuning {
    pub meter: skate_core::challenges::distance_meter::MeterTuning,
    pub reel_enabled: bool,
    pub reel_slots: usize,
    pub reel_transition_seconds: f32,
    /// A 1-Up's waiting pro steps off after this many seconds at rest (SPEC: the user's account; retail code not found).
    pub wait_step_off_seconds: f32,
}

impl Default for ChallengeTuning {
    fn default() -> Self {
        let rules = skate_core::challenges::highlights::Rules::default();
        Self {
            meter: skate_core::challenges::distance_meter::MeterTuning::default(),
            reel_enabled: true,
            reel_slots: rules.slots,
            reel_transition_seconds: crate::replay::REEL_TRANSITION_SECONDS,
            wait_step_off_seconds: 1.0,
        }
    }
}

/// The replay NIS enter (8285EC08): pause modes 12, then 8.
fn enter_replay_nis(pause: Option<&mut crate::pause_man::ThePauseMan>) {
    if let Some(p) = pause {
        p.push(skate_core::pause_man::mode::NIS);
        p.push(skate_core::pause_man::mode::REPLAY_NIS);
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<FollowTheLeader>()
        .init_resource::<AnywherePhotographer>()
        .init_resource::<ChallengeTuning>()
        .add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet).run_if(crate::challenges::opted_in))
        .add_systems(Update, update.after(crate::challenges::CareerResultSet).run_if(crate::challenges::opted_in))
        .init_resource::<crate::photo_picker::PhotoScreen>()
        .init_resource::<crate::photo_picker::MagazineScreen>()
        .add_systems(Update, crate::photo_picker::photo_picker.after(update).run_if(crate::challenges::opted_in));
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::challenges::objectives::Message as GroupMessage;

    #[test]
    fn a_placed_photographer_plays_the_clip_as_photo_anywhere_located_from_its_spot() {
        let take = SceneTake { track: skate_core::camera_track::CameraTrack { length: 3.33, keys: Vec::new(), segments: Vec::new() },
            freezes: Vec::new(), colour: Default::default() };
        let mut reel = ReelState::default();
        reel.scenes = Some([("photo_anywhere".to_owned(), vec![take.clone()]), (PHOTOGRAPHER_SCENE.to_owned(), vec![take])].into_iter().collect());
        let clip = skate_core::challenges::highlights::Highlight { killed: false, start: 60, end: 240,
            scene: skate_core::challenges::highlights::SceneDef { scene: "photo_anywhere".into(), ..Default::default() } };
        let mut replay = crate::replay::Replay::default();
        for i in 0..300 {
            replay.record(crate::presentation::Snapshot { root: Transform::from_xyz(i as f32 * 0.1, 0.0, 0.0), bones: Vec::new(),
                camera: Transform::IDENTITY, fov: 1.0 }, 1.0 / 60.0, false);
        }
        reel.clips = [clip.clone()].into();
        reel.next_clip(Some(&mut replay), std::path::Path::new("."));
        assert_eq!(reel.last_scene.as_ref().map(|s| s.0.as_str()), Some("photo_anywhere"), "no photographer: the definition's scene");
        reel.photographer = Some((Vec3::new(1.0, 0.0, 2.0), Vec3::Z));
        reel.clips = [clip].into();
        reel.next_clip(Some(&mut replay), std::path::Path::new("."));
        assert_eq!(reel.last_scene.as_ref().map(|s| s.0.as_str()), Some(PHOTOGRAPHER_SCENE), "82BCE0D8: the placed photographer's scene");
    }

    #[test]
    fn the_autopilot_keeps_a_gap_and_never_passes_the_leader() {
        assert_eq!(follow_cap(12.0, 8.0), None);
        assert_eq!(follow_cap(FOLLOW_GAP, 8.0), None);
        assert_eq!(follow_cap(4.0, 8.0), Some(7.0));
        assert_eq!(follow_cap(0.0, 8.0), Some(4.8));
        assert_eq!(follow_cap(-3.0, 8.0), Some(4.8));
        assert_eq!(follow_cap(2.0, 0.0), Some(FOLLOW_MIN_SPEED), "a leader still at its start does not stop it");
    }

    #[test]
    fn the_autopilot_takes_the_leaders_lines_in_order() {
        let (a, b, c) = ([1u8; 16], [2u8; 16], [3u8; 16]);
        let path = [a, b, c];
        assert_eq!(follow_line(&path, Some(a)), Some(b), "after a bail on a: b, not where the leader is now");
        assert_eq!(follow_line(&path, Some(c)), Some(c));
        assert_eq!(follow_line(&path, None), Some(a));
        assert_eq!(follow_line(&path, Some([9u8; 16])), Some(c));
        assert_eq!(follow_line(&[], Some(a)), None);
    }

    fn volume_row(key: &str, volume: &str) -> ObjectiveData {
        let mut triggers = vec![false; 14];
        triggers[12] = true;
        ObjectiveData { key: key.into(), activation: 0, triggers, points_valid: None,
            condition: serde_json::json!({"locals": 0, "body": [{"return": {"api": "ObjectiveTrigger.IsInVolume",
                "args": [{"lit": "1"}, {"lit": format!("DIST_DownTown|{volume}")}]}}]}),
            failures: Vec::new(), failures_inline: Vec::new(), failure_first: false, hints: Vec::new(), title_id: None, title_arg: None, display_in_hud: true, highlight: None, auto_highlight_timing: false }
    }

    #[test]
    fn a_killed_it_list_runs_as_its_own_group_and_completes_on_its_own() {
        let mut row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "ftl_test", "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}
        })).unwrap();
        let volumes = vec![("ftl_end".to_string(), 7), ("ftl_kill".to_string(), 9)];
        row.objectives.owned = vec![volume_row("owned_0", "ftl_end")];
        assert_eq!(load_groups(&row, volumes.clone(), Default::default()).map(|g| g.len()), Some(1), "retail rows: the owned list only");
        row.objectives.killed = vec![volume_row("killed_0", "ftl_kill")];
        let mut groups = load_groups(&row, volumes, Default::default()).unwrap();
        assert_eq!(groups.iter().map(|g| g.killed).collect::<Vec<_>>(), [false, true]);
        let mut done = Vec::new();
        for g in &mut groups {
            g.objectives.activate_first();
            g.objectives.enter(9);
            let events = g.objectives.handle(GroupMessage::Tick { gated: false });
            if events.contains(&GroupEvent::AllComplete) {
                done.push(g.killed);
            }
        }
        assert_eq!(done, [true], "inside the killed volume only the killed it group completes");
        row.objectives.owned.clear();
        assert!(load_groups(&row, vec![], Default::default()).is_none(), "no owned list: not started");
    }

    /// b1056: the row's TrackUniqueTrickGroup reaches every group of the run when it names a loaded trick group; an
    /// unknown key or a row without one tracks nothing.
    #[test]
    fn the_tracked_unique_trick_group_reaches_every_group() {
        let mut row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "film_test", "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},
            "track_unique_trick_group": "0ae5c05d32117e6c"
        })).unwrap();
        row.objectives.owned = vec![volume_row("owned_0", "a")];
        row.objectives.killed = vec![volume_row("killed_0", "a")];
        let groups: crate::challenge_objectives::TrickGroups = std::sync::Arc::new([(0x0ae5c05d32117e6c, [5, 6].into())].into());
        let loaded = load_groups(&row, vec![("a".into(), 1)], groups.clone()).unwrap();
        assert!(loaded.len() == 2 && loaded.iter().all(|g| g.objectives.tracks_unique()));
        row.track_unique_trick_group = Some("0000000000000001".into());
        assert!(!load_groups(&row, vec![("a".into(), 1)], groups).unwrap().iter().any(|g| g.objectives.tracks_unique()));
    }

    #[test]
    fn rows_with_a_sign_up_sphere_on_this_map_are_offered_to_the_prompt() {
        let row = |id: &str, map: &str, sphere: bool| -> FtlData {
            let mut v = serde_json::json!({"id": id, "map": map, "title_id": "ID_T", "challenge_type": 23,
                "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}});
            if sphere {
                v["location"] = serde_json::json!({"locator": "l", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[5,6,7,1]]});
                v["sign_up_radius"] = serde_json::json!(3.0);
            }
            serde_json::from_value(v).unwrap()
        };
        let ftl = FollowTheLeader { rows: vec![row("a", "university", true), row("b", "university", false), row("c", "downtown", true)], ..Default::default() };
        let offers: Vec<_> = ftl.offers(Some(std::path::Path::new("maps/University.skate"))).collect();
        assert_eq!(offers.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["a"]);
        assert_eq!((offers[0].location, offers[0].sign_up_radius, offers[0].challenge_type), ([5., 6., 7., 1.], 3.0, Some(23)));
        assert_eq!(crate::challenges::sign_up_state(&offers[0]), 1, "the Challenge overlay state, not own the spot's");
        assert_eq!(ftl.offers(None).count(), 0);
    }

    #[test]
    fn a_films_objectives_all_start_active_not_only_the_first() {
        use skate_core::challenges::objectives::State as S;
        // film_univ_02's owned rows: activation 0, 5 (on a sequence), 0
        let mut rows = vec![volume_row("film_univ_02_0", "a"), volume_row("film_univ_02_1", "b"), volume_row("film_univ_02_2", "c")];
        rows[1].activation = 5;
        let mut d = DataObjectives::new("film_univ_02", &rows, vec![("a".into(), 1), ("b".into(), 2), ("c".into(), 3)], Default::default()).unwrap();
        d.activate_flow();
        assert_eq!(d.states(), [S::Active, S::Active, S::Active], "the films' flow mode 0 (b934)");
    }

    #[test]
    fn the_objective_hud_lists_the_shown_objectives_with_their_titles_and_states() {
        use skate_core::challenges::objectives::State as S;
        let mut a = volume_row("film_indu_01_0", "x");
        a.title_id = Some("ID_CHALLENGE_FILM_INDU_01_OWNED01".into());
        let mut hidden = volume_row("film_indu_01_6", "x");
        hidden.display_in_hud = false;
        let mut b = volume_row("film_indu_01_1", "x");
        b.title_id = Some("ID_CHALLENGE_FILM_INDU_01_OWNED02".into());
        let rows = objective_rows(&[a, hidden, b], &[S::Complete, S::Active, S::Active]);
        let got: Vec<(i32, &str)> = rows.iter().map(|r| (r.state, r.title.as_str())).collect();
        assert_eq!(got, [(1, "ID_CHALLENGE_FILM_INDU_01_OWNED01"), (3, "ID_CHALLENGE_FILM_INDU_01_OWNED02")],
            "DisplayInHUD false leaves the row out (film_indu_01_6, b947)");
    }

    /// The exported film_univ_02 (SKATE_TEST_ASSETS = an assets folder with private/challenges.json): a clean line
    /// that grinds inside an objective's volumes completes that objective and its HUD row, through the same feed and
    /// rows the host uses; a line that grinds outside them or bails completes nothing.
    #[test]
    fn film_univ_02_grind_objectives_complete_from_the_exported_rows() {
        use crate::challenge_objectives::{FeedInput, ObjectiveFeed};
        use skate_core::challenges::objectives::State as S;
        let Some(assets) = std::env::var_os("SKATE_TEST_ASSETS") else { test_skip!("set SKATE_TEST_ASSETS") };
        let Some(data) = load(std::path::Path::new(&assets)).into_iter().find(|r| r.id == "film_univ_02") else {
            test_skip!("no film_univ_02 in the export")
        };
        let volumes: Vec<(String, u64)> =
            data.objective_volumes.iter().filter_map(|v| Some((v.name.clone(), u64::from_str_radix(&v.id, 16).ok()?))).collect();
        let id = |name: &str| volumes.iter().find(|(n, _)| n == name).map(|(_, i)| *i).unwrap_or_else(|| panic!("no volume {name}"));
        // (volume to grind in, the owned objective it completes)
        let cases = [("film_univ_02_grind_volume_2", Some(2)), ("film_univ_02_gap_01_grd_volume_3", Some(0)), ("film_univ_02_gap_02_volume_1", None)];
        for (volume, expected) in cases {
            for bail in [false, true] {
                let mut groups = load_groups(&data, volumes.clone(), Default::default()).expect("the rows load");
                for g in &mut groups {
                    g.objectives.activate_flow();
                }
                let mut feed = ObjectiveFeed::default();
                let mut completed = Vec::new();
                let mut frame = |entered: &[u64], exited: &[u64], state_id: u32, in_sequence: bool, banked: &[(i32, bool)]| {
                    let input = FeedInput { entered, exited, banked, ticks: 1, state_id, in_sequence, ..Default::default() };
                    let mut refs: Vec<&mut DataObjectives> = groups.iter_mut().map(|g| &mut g.objectives).collect();
                    for (i, e) in feed.step(&mut refs, &input, true) {
                        if let GroupEvent::Completed(o) = e {
                            completed.push((i, o));
                        }
                    }
                };
                let v = id(volume);
                frame(&[], &[], 100, true, &[]);
                frame(&[v], &[], 200, true, &[]);
                for _ in 0..20 {
                    frame(&[], &[], 401, true, &[]);
                }
                frame(&[], &[v], 200, true, &[]);
                frame(&[], &[], if bail { 300 } else { 100 }, true, &[]);
                frame(&[], &[], 100, false, &[(800, bail)]);
                let owned: Vec<usize> = completed.iter().filter(|(g, _)| *g == 0).map(|(_, o)| *o).collect();
                let want: Vec<usize> = if bail { Vec::new() } else { expected.into_iter().collect() };
                assert_eq!(owned, want, "grinding in {volume}, bail {bail}");
                let rows = objective_rows(&data.objectives.owned, &groups[0].objectives.states());
                let done: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.state == 1).map(|(i, _)| i).collect();
                assert_eq!(done, want, "the HUD rows show it ({volume}, bail {bail})");
                assert!(groups[0].objectives.states().iter().all(|s| *s != S::Failed));
            }
        }
    }

    #[test]
    fn a_film_row_runs_on_the_same_host_without_a_leader_or_a_film_clock() {
        let row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "film_univ_02", "family": "films", "map": "university",
            "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},
            "total_film_time": 180.0, "film_low_limit": 5, "prefilm_time": 0.0, "allow_player_bail": false
        })).unwrap();
        let p = params(&row);
        assert_eq!((p.kind, p.competitors, p.allow_player_bail), (Kind::Film, 0, false), "films ride alone and re-arm on a bail");
        assert_eq!((p.total_film_time, p.film_low_limit, p.prefilm_time, p.simul_play), (180.0, 5.0, 0.0, false));
        let old: FtlData = serde_json::from_value(serde_json::json!({
            "id": "ftl_test", "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}})).unwrap();
        assert_eq!(kind(&old), Kind::Follow, "an export from before films is follow the leader");
        let anywhere: FtlData = serde_json::from_value(serde_json::json!({"id": "film_anyw_01", "family": "anywhere_films", "start": null})).unwrap();
        assert_eq!(kind(&anywhere), Kind::AnywhereFilm, "an anywhere film loads without a start (it starts in place)");
    }

    #[test]
    fn a_photo_row_runs_on_the_film_host_and_ends_after_its_photo_nis() {
        let row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "photo_univ_05", "family": "photos", "map": "university",
            "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},
            "allow_player_bail": false, "boundary": null
        })).unwrap();
        let p = params(&row);
        assert_eq!((p.kind, p.competitors, p.allow_player_bail), (Kind::Photo, 0, false));
        let mut run = FilmRun::new(p);
        run.start();
        for e in [Event::HudLoaded, Event::AssetsLoaded, Event::TeleportFinished] {
            run.handle(e);
        }
        while run.state() != State::ChallengeRunning {
            run.tick();
        }
        run.handle(Event::AllObjectivesComplete { killed: false });
        while run.state() != State::DisplayPhoto {
            run.tick();
        }
        let mut played = false;
        for _ in 0..400 {
            played |= run.tick().contains(&Command::PlayHighlightReel);
        }
        assert!(played, "the photo NIS plays after the banner");
        assert_eq!(reel_done(&mut run), vec![Command::OpenPhotoSelection], "the NIS's end reaches the shot pick");
        run.handle(Event::NisComplete);
        assert_eq!(run.state(), State::EndOptions);
    }

    #[test]
    fn rows_become_follow_params_with_the_end_volume_and_their_competitors() {
        let row: FtlData = serde_json::from_value(serde_json::json!({
            "id": "ftl_test", "start": {"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},
            "max_follow_distance": 100.0, "competitors": [{"character": "reda_ftl"}, {"character": "second"}]
        })).unwrap();
        let p = params(&row);
        assert_eq!(p.kind, Kind::Follow);
        assert_eq!(p.competitors, 2);
        assert!(p.allow_player_bail && !p.allow_objective_failure, "family defaults (b1006 §0.3 / §0.5)");
        assert_eq!(p.ftl_window, 10.0);
    }
}
