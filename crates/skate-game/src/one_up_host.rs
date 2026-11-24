//! 1-Ups in play (research b1055; doc 42 "1-Ups"): a row from `private/challenges.json` `one_ups` runs through
//! `skate_core::challenges::one_up_run`. The pro rides its challenge lines (`ch_<id>`, the contest rider code) on its
//! turns; between turns everyone stands at the round's wait spot and the runner at its start spot. Start: the sign-up
//! prompt at the row's sphere, the Challenge Map's Sign Up, the career, or `SKATE_ONEUP=<id>` on its map.
//! NOT RETAIL YET (logged): the turn fades, the camera on the pro's run (Online.SwitchCamera), the ThrowDown
//! leaderboard menu type's mapping to the 1up layout [inferred], the pro's speech lines and the highlight reel; the score bias multiplies
//! the pro's landed lines [inferred: its retail consumer is not read].
use crate::hud_challenge::{ChallengeHudState, Component, DpoSlot, LeaderboardHud, LeaderboardRow, LEADERBOARD_NO_VARIANT,
    LEADERBOARD_ONE_UP};
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::one_up_run::{Banner, Command, Event, OneUpRun, Params, State};
use skate_core::challenges::timers::TICKS_PER_SECOND;
use skate_core::challenges::Completion;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Spot {
    pub locator: String,
    pub matrix: [[f32; 4]; 4],
}

/// One 1-Up row as `challenges.py` exports it.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OneUpData {
    pub id: String,
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub title_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(flatten)]
    pub sign_up: crate::challenges::SignUpFields,
    pub starts: Vec<Spot>,
    pub waits: Vec<Spot>,
    #[serde(default)]
    pub competitors: Vec<crate::race_host::CompetitorRow>,
    pub time_limit: f32,
    pub cumulative: bool,
    pub pro_goes_first: bool,
    pub allow_player_bail: bool,
    pub thresholds: [f32; 4],
    pub multipliers: [f32; 5],
    #[serde(default)]
    pub central_messages: std::collections::BTreeMap<String, String>,
    /// C.IntroPresentationEvents: the pro's intro NIS (nis_overview, b1055 §3).
    #[serde(default)]
    pub intro: crate::challenge_objectives::EventList,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    one_ups: Vec<serde_json::Value>,
}

/// Every 1-Up row with its pro (rows without competitors are not run); a row that does not parse is logged.
pub(crate) fn load(assets: &std::path::Path) -> Vec<OneUpData> {
    let Some(file) = std::fs::read(assets.join("private/challenges.json")).ok().and_then(|b| serde_json::from_slice::<File>(&b).ok()) else {
        return Vec::new();
    };
    file.one_ups.into_iter().filter_map(|v| {
        let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("?").to_owned();
        serde_json::from_value::<OneUpData>(v).map_err(|e| warn!("SKATE_ONEUP row {id} not readable: {e}")).ok()
    }).filter(|r| !r.competitors.is_empty()).collect()
}

pub(crate) fn params(d: &OneUpData) -> Params {
    Params {
        time_limit: d.time_limit, cumulative: d.cumulative, pro_goes_first: d.pro_goes_first, allow_bail: d.allow_player_bail,
        thresholds: d.thresholds, multipliers: d.multipliers, ..Params::retail(d.competitors.len() + 1)
    }
}

pub(crate) struct OneUpState {
    pub data: OneUpData,
    pub run: OneUpRun,
    ai: Option<crate::contest_ai_host::ContestAi>,
    carry: f32,
    names: Vec<String>,
    queued: Vec<Command>,
    dialog_open: bool,
    was_bailing: bool,
    /// The run's objective is armed from a sequence begin after the run started (mode 5 after ResetObjectives): the
    /// player's line in progress at the start, and the pro's, do not count. Per competitor: a line began in this run.
    began: Vec<bool>,
    player_active: bool,
    /// The competitors each turn holds (pinned) or lets ride.
    pinned: Vec<bool>,
    /// The intro presentation group nis_overview waits for.
    intro_group: Option<String>,
    /// Online.SwitchCamera's competitor (0 = the player), resolved to its living world id every frame (a pro placed in
    /// the same handler gets its id once spawned).
    watching: usize,
}

impl OneUpState {
    pub(crate) fn player_pinned(&self) -> bool {
        self.pinned.first().copied().unwrap_or(false)
    }
}

/// The row's text for a banner (b1055 §3: the row fields, else the graph's constant ids).
/// The banner's language id, its ShowCustomCentralMessage argument list (comma-separated, as the 1up graph builds
/// it: `"<name>,<score>"`) and whether it goes to the modified slot.
fn banner_text(d: &OneUpData, b: &Banner, names: &[String]) -> (String, Option<String>, bool) {
    let row = |k: &str, default: &str| d.central_messages.get(k).cloned().unwrap_or_else(|| default.to_owned());
    // The names are `#` literals for the HUD; as an argument the name itself goes in.
    let name = |c: usize| names.get(c).map(|n| n.strip_prefix('#').unwrap_or(n).to_owned()).unwrap_or_default();
    let id = |s: &str| s.to_owned();
    match b {
        Banner::IsLeading { leader, score } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_ISLEADING"), Some(format!("{},{score}", name(*leader))), false),
        Banner::Turn { runner: 0, setting: true } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_SETYOURSCORE"), None, true),
        Banner::Turn { runner: 0, setting: false } => (row("CentralMessageHALIDYourTurn", "ID_CHALLENGE_CENTRAL_MESSAGE_YOURTURN"), None, true),
        Banner::Turn { runner, .. } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_OTHERSTURN"), Some(name(*runner)), true),
        Banner::OneUpped { .. } => (row("ModifiedCentralMessageHALIDOneUpped", "ID_CHALLENGE_CENTRAL_MESSAGE_MODIFIED_1UP_ONEUPPED"), None, true),
        Banner::Bailed { runner: 0 } => (row("ModifiedCentralMessageHALIDBailed", "ID_CHALLENGE_CENTRAL_MESSAGE_1UP_BAILED"), None, true),
        Banner::Bailed { runner } => (row("CentralMessageHALIDNameBailed", "ID_CHALLENGE_CENTRALMESSAGE_FREESKATEACTIVITY_BAIL"), Some(name(*runner)), false),
        Banner::ScoreSet { score, first: true, .. } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_SCORESET"), Some(score.to_string()), false),
        Banner::ScoreSet { score, .. } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_NEWSCORESET"), Some(score.to_string()), false),
        Banner::ScoreNotSet => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_SCORENOTSET"), None, false),
        Banner::Eliminated { runner } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_ELIMINATED"), Some(name(*runner)), false),
        Banner::OutForRound { runner: 0 } => (row("CentralMessageHALIDDidNotBeatHighScore", "ID_CHALLENGE_CENTRAL_MESSAGE_1UP_SCOREDLOW"), None, false),
        Banner::OutForRound { runner } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_OTHER_OUTFORROUND"), Some(name(*runner)), false),
        Banner::RoundOver { leader } => (row("CentralMessageHALIDRoundOver", "ID_CHALLENGE_CENTRAL_MESSAGE_1UP_ROUNDOVER"), Some(name(*leader)), false),
        Banner::Wins { winner } => (id("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_WINS"), Some(name(*winner)), false),
    }
}

/// A new map drops the old run and starts `SKATE_ONEUP`; a sign-up starts its row when nothing else runs.
fn arm(mut ftl: ResMut<crate::ftl_host::FollowTheLeader>, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>,
    mut population: Option<ResMut<crate::living_world::PopulationState>>, mut signed: MessageReader<crate::challenges::ChallengeSignUp>,
    challenges: Option<Res<crate::challenges::Challenges>>, mut generation: Local<Option<u64>>) {
    let mut wanted = None;
    if *generation != Some(map.generation) {
        *generation = Some(map.generation);
        if ftl.one_up_rows.is_empty() {
            ftl.one_up_rows = load(&config.asset_root);
            info!("SKATE_ONEUP rows={}", ftl.one_up_rows.len());
        }
        if let Some(mut run) = ftl.one_up.take() {
            info!("SKATE_ONEUP {} dropped by the map change", run.data.id);
            if let (Some(ai), Some(pop)) = (run.ai.as_mut(), population.as_deref_mut()) {
                ai.remove(pop);
            }
        }
        wanted = std::env::var("SKATE_ONEUP").ok();
    }
    let busy = ftl.run.is_some() || ftl.hom.is_some() || ftl.one_up.is_some()
        || challenges.as_deref().is_some_and(|c| c.run.is_some() || c.tutorial.is_some());
    for s in signed.read().filter(|s| s.body == crate::trigger_volumes::PLAYER) {
        if !ftl.one_up_rows.iter().any(|r| r.id == s.challenge) {
            continue;
        }
        if busy {
            info!("SKATE_ONEUP sign-up {} ignored: a challenge is running", s.challenge);
        } else {
            info!("SKATE_ONEUP sign-up {}", s.challenge);
            wanted.get_or_insert_with(|| s.challenge.clone());
        }
    }
    let Some(wanted) = wanted else { return };
    let Some(data) = ftl.one_up_rows.iter().find(|r| r.id == wanted).cloned() else {
        info!("SKATE_ONEUP {wanted} not found");
        return;
    };
    if data.map.as_deref().is_some_and(|m| map.path.as_deref().is_some_and(|p| !crate::teleport_menu::same_map(p, m))) {
        info!("SKATE_ONEUP {wanted} is not on {}", map.name);
        return;
    }
    let rows: Vec<String> = data.competitors.iter().map(|c| c.character.clone()).collect();
    let ai = crate::contest_ai_host::ContestAi::load(&config.asset_root, &data.id, &rows, crate::race_ai_host::start_seed(map.generation));
    let looks = crate::race_ai_host::looks(&config.asset_root);
    let mut names = vec![format!("#{}", crate::multiplayer::local_player_name(&config.asset_root))];
    names.extend(rows.iter().map(|c| looks.get(c).map_or_else(|| format!("#{c}"), |l| l.1.clone())));
    let mut run = OneUpRun::new(params(&data));
    info!("SKATE_ONEUP start {} pro={rows:?} time={} thresholds={:?}", data.id, data.time_limit, data.thresholds);
    let queued = run.start();
    let n = rows.len() + 1;
    ftl.one_up = Some(OneUpState { data, run, ai, carry: 0.0, names, queued, dialog_open: false, was_bailing: false, began: vec![false; n],
        player_active: false, pinned: vec![true; n], intro_group: None, watching: 0 });
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut ftl: ResMut<crate::ftl_host::FollowTheLeader>,
    challenges: Option<Res<crate::challenges::Challenges>>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    (mut npc_events, mut central, mut career): (
        MessageReader<crate::living_world::npc_skaters::NpcSkaterEvent>,
        MessageWriter<crate::hud_central::CentralMessage>,
        MessageWriter<crate::career::ChallengeResult>,
    ),
    mut hud: ResMut<ChallengeHudState>,
    mut end_dialog: Option<ResMut<crate::end_dialog_host::EndDialogScreen>>,
    time: Res<Time>,
    (mut population, index, mut clocks, mut commands_ecs): (
        Option<ResMut<crate::living_world::PopulationState>>,
        Option<Res<crate::living_world::npc_skaters::NpcSkaterIndex>>,
        Query<&mut crate::living_world::npc_skaters::NpcRaceClock>,
        Commands,
    ),
    mut presenter: Option<ResMut<crate::presentation_run::Presenter>>,
    mut watch: ResMut<crate::camera::CameraWatch>,
) {
    let npc_list: Vec<crate::living_world::npc_skaters::NpcSkaterEvent> = npc_events.read().cloned().collect();
    let Some(state) = ftl.one_up.as_mut() else {
        // cleanup_challenge: SwitchCamera back to the local player.
        if watch.0.is_some() {
            watch.0 = None;
        }
        return;
    };
    let id = state.data.id.clone();
    let watched = (state.watching != 0).then(|| state.ai.as_ref()
        .and_then(|ai| ai.riders.iter().find(|r| r.id as usize == state.watching)).and_then(|r| r.npc)).flatten();
    if watch.0 != watched {
        info!("SKATE_ONEUP {id} camera follows {watched:?}");
        watch.0 = watched;
    }
    let mut commands = std::mem::take(&mut state.queued);
    // nis_overview: presentationeventgroupfinished for the intro group -> setup_challenge.
    if let (Some(group), Some(p)) = (state.intro_group.as_deref(), presenter.as_deref_mut()) {
        if p.take_finished(group) {
            state.intro_group = None;
            commands.extend(state.run.handle(Event::IntroFinished));
        }
    }
    if state.dialog_open {
        use skate_core::challenges::end_dialog::Action;
        if let Some(d) = end_dialog.as_deref_mut() {
            let event = match d.choice.take() {
                Some(Action::Continue) => Some(Event::Continue),
                Some(Action::Retry { .. }) => Some(Event::Retry),
                _ => None,
            };
            if let Some(event) = event {
                (d.results, d.other) = (None, None);
                state.dialog_open = false;
                commands.extend(state.run.handle(event));
            }
        } else {
            state.dialog_open = false;
        }
    }
    // The run's score: the runner's clean landed lines (its 1up_sequence_anywhere objective, trigger byte 1, mode 5).
    let running = state.run.state() == State::PerformRun;
    let active = skater.scoring.sequence_active();
    if running && active && !state.player_active {
        state.began[0] = true;
    }
    state.player_active = active;
    if running {
        for s in challenges.as_deref().map_or(&[][..], |c| &c.banked_this_frame[..]).iter().filter(|s| !s.wiped_out) {
            if !std::mem::take(&mut state.began[0]) {
                info!("SKATE_ONEUP {id} player line {} began before the run: not counted", s.score);
                continue;
            }
            commands.extend(state.run.handle(Event::Sequence { competitor: 0, score: s.score as f32 }));
        }
        let bailing = skater.physical_state_id() == 300;
        if bailing && !state.was_bailing {
            commands.extend(state.run.handle(Event::Wipeout { competitor: 0 }));
        }
        state.was_bailing = bailing;
        use crate::living_world::npc_skaters::NpcSkaterEvent;
        let rider = |state: &OneUpState, npc| state.ai.as_ref().and_then(|ai| ai.riders.iter().find(|r| r.npc == Some(npc))).map(|r| r.id as usize);
        for e in &npc_list {
            if let NpcSkaterEvent::SequenceBegin { id: npc } = e {
                if let Some(b) = rider(state, *npc).and_then(|c| state.began.get_mut(c)) {
                    *b = true;
                }
            }
            if let NpcSkaterEvent::SequenceComplete { id: npc, score, wiped_out, .. } = e {
                let Some(c) = rider(state, *npc) else { continue };
                let began = state.began.get_mut(c).map(std::mem::take).unwrap_or(false);
                if *wiped_out {
                    commands.extend(state.run.handle(Event::Wipeout { competitor: c }));
                } else if !began {
                    info!("SKATE_ONEUP {id} pro line {score} began before its run: not counted");
                } else {
                    let biased = *score as f32 * state.run.multiplier();
                    info!("SKATE_ONEUP {id} pro line {score} x {} = {biased}", state.run.multiplier());
                    commands.extend(state.run.handle(Event::Sequence { competitor: c, score: biased }));
                }
            }
        }
    } else {
        state.was_bailing = false;
    }
    state.carry += time.delta_secs() * TICKS_PER_SECOND;
    while state.carry >= 1.0 {
        state.carry -= 1.0;
        commands.extend(state.run.tick());
    }
    let mut finished = None;
    let mut queue: std::collections::VecDeque<Command> = commands.into();
    let mut guard = 0;
    while let Some(c) = queue.pop_front() {
        guard += 1;
        if guard > 512 {
            break;
        }
        match c {
            // NOT RETAIL YET: the turn fades are not played.
            Command::Intro => {
                state.intro_group = crate::challenge_objectives::start_presentation(presenter.as_deref_mut(), &id, "intro", &state.data.intro);
                if state.intro_group.is_none() {
                    queue.extend(state.run.handle(Event::IntroFinished));
                }
            }
            Command::FadeOut(_) => queue.extend(state.run.handle(Event::FadeOutDone)),
            Command::FadeIn(_) => queue.extend(state.run.handle(Event::FadeInDone)),
            Command::Pin { competitor, on } => {
                if let Some(p) = state.pinned.get_mut(competitor) {
                    *p = on;
                }
                if competitor > 0 {
                    if let Some(ai) = state.ai.as_mut() {
                        if on { ai.stop() } else { ai.go() }
                    }
                }
            }
            // setup_run FadeOutDelay: the runner to the round's start spot, everyone else to its wait spot.
            Command::Place { runner, rotation } => {
                // Online.SwitchCamera(runner) in the same handler (b1055 setup_run FadeOutDelay): the view follows the
                // runner, the pro's own camera on its turn.
                state.watching = runner;
                info!("SKATE_ONEUP {id} camera on competitor {runner}");
                let pick = |spots: &[Spot]| spots.get((rotation.max(1) - 1) % spots.len().max(1)).map(|s| (s.locator.clone(), s.matrix));
                let (start, wait) = (pick(&state.data.starts), pick(&state.data.waits));
                info!("SKATE_ONEUP {id} turn: runner {runner} to {:?}, the others to {:?}", start.as_ref().map(|s| &s.0), wait.as_ref().map(|s| &s.0));
                for c in 0..state.pinned.len() {
                    let Some((_, at)) = (if c == runner { start.clone() } else { wait.clone() }) else { continue };
                    if c == 0 {
                        if let Err(e) = skater.travel_ex(at, None, true) {
                            warn!("SKATE_ONEUP teleport {e}");
                        }
                    } else if let (Some(ai), Some(pop)) = (state.ai.as_mut(), population.as_deref_mut()) {
                        ai.place(&at, &[], &Default::default(), pop);
                        ai.stop();
                    }
                }
                queue.extend(state.run.handle(Event::Placed));
            }
            Command::Banner(b) => {
                let (text, args, modified) = banner_text(&state.data, &b, &state.names);
                info!("SKATE_ONEUP {id} banner {b:?}");
                let mut message = if modified { crate::hud_central::CentralMessage::modified(text) } else { crate::hud_central::CentralMessage::new(text) };
                message.args = args;
                central.write(message);
            }
            Command::ShowLeaderboard | Command::Letters(_) => {
                let letters = state.run.letters();
                let rows = state.names.iter().enumerate().map(|(i, n)| {
                    let l = letters.get(i).copied().unwrap_or(0);
                    LeaderboardRow { id: i as u32, rank: i as i32 + 1, score: l as f32, score_text: l.to_string(), name: n.clone(), highlight: i == 0 }
                }).collect();
                // LeaderboardHUDInit(Name, ThrowDown, ...): the 1up layout, each competitor's letters as its int.
                hud.leaderboard = LeaderboardHud { layout: LEADERBOARD_ONE_UP, title: String::new(), variant: LEADERBOARD_NO_VARIANT.into(), rows };
                hud.show(Component::Leaderboard, true);
            }
            Command::Dpo { score, target, title } => {
                let title = title.and_then(|c| state.names.get(c).cloned())
                    .unwrap_or_else(|| state.data.central_messages.get("CentralMessageHALIDSetScore").cloned().unwrap_or_else(|| "ID_1UP_SET_A_SCORE".into()));
                hud.dpo[0] = DpoSlot { numerator: (score as i64).to_string(), denominator: target.map_or_else(String::new, |t| (t as i64).to_string()),
                    title, highlight: hud.dpo[0].highlight, ..Default::default() };
                hud.show(Component::Dpo, target.is_some() || score > 0.0 || state.run.state() == State::SetupRun);
            }
            Command::StartCountdownHud { .. } => {}
            Command::RunStart { runner, ticks } => {
                info!("SKATE_ONEUP {id} run: competitor {runner}, {} s", ticks as f32 / TICKS_PER_SECOND);
                state.began.iter_mut().for_each(|b| *b = false);
                hud.show(Component::Timer, true);
            }
            Command::RunStop { runner } => {
                info!("SKATE_ONEUP {id} run over: competitor {runner} scored {}", state.run.run_score());
                hud.show(Component::Timer, false);
            }
            // HUD2.ShowSkateMessage(Name, letters, false, local): the letter on hudthrowmes with its sound (b1058).
            Command::ShowLetters { runner, letters } => {
                info!("SKATE_ONEUP {id} competitor {runner} letters {letters}");
                hud.throwdown.one_up(letters as u32, runner == 0);
            }
            Command::PlayHighlightReel => {
                info!("SKATE_ONEUP {id} highlight reel NOT RETAIL YET");
                queue.extend(state.run.handle(Event::ReelComplete));
            }
            Command::Results(rows) => info!("SKATE_ONEUP {id} results {rows:?}"),
            Command::EndOptions { won } => {
                info!("SKATE_ONEUP {id} end won={won}");
                match end_dialog.as_deref_mut().filter(|d| d.available) {
                    Some(d) => {
                        open_end_options(state, d, won);
                        state.dialog_open = true;
                    }
                    None => queue.extend(state.run.handle(Event::Continue)),
                }
            }
            Command::Complete(c) => finished = Some(c),
            other => info!("SKATE_ONEUP {id} {other:?}"),
        }
    }
    if hud.is_shown(Component::Timer) {
        let (text, flag) = skate_core::challenges::timers::hud_text(state.run.run_ticks().unwrap_or(0));
        if hud.timer_text != text || hud.timer_flag != flag {
            hud.timer_text = text;
            hud.timer_flag = flag;
        }
    }
    if let (Some(ai), Some(index), Some(pop)) = (state.ai.as_mut(), index.as_deref(), population.as_deref()) {
        ai.step(pop.world.tick(), index, &mut clocks, &mut commands_ecs);
    }
    if let Some(c) = finished {
        if let (Some(ai), Some(pop)) = (state.ai.as_mut(), population.as_deref_mut()) {
            ai.remove(pop);
        }
        info!("SKATE_ONEUP end {id} completion={c:?} letters={:?}", state.run.letters());
        ftl.result = Some(format!("oneup={id} completion={c:?}"));
        ftl.one_up = None;
        for comp in [Component::Countdown, Component::Timer, Component::Leaderboard, Component::Dpo] {
            hud.show(comp, false);
        }
        if c != Completion::Abandoned {
            career.write(crate::career::ChallengeResult { challenge: id, completion: c, reactivate: false });
        }
    }
}

/// show_results (single player): FinalResults with place, skater and letters in the score column; Continue_Retry /
/// Retry_Quit; OpenContinueRetryDialog(won, false).
fn open_end_options(state: &OneUpState, d: &mut crate::end_dialog_host::EndDialogScreen, won: bool) {
    use skate_core::challenges::end_dialog::{CONTINUE_RETRY, NONE, RETRY_QUIT};
    use skate_core::challenges::results::{contest_results, ContestScore, MENU_FINAL_RESULTS};
    let letters = state.run.letters();
    let order = state.run.standings();
    let scores: Vec<ContestScore> = order.iter().map(|&i| ContestScore { id: i as u32, name: state.names.get(i).cloned().unwrap_or_default(),
        local: i == 0, score: letters.get(i).copied().unwrap_or(0) as f32 }).collect();
    let columns = vec!["ID_LEADERBOARD_HEADER_RANK".to_owned(), "ID_LEADERBOARD_HEADER_SKATER".to_owned(), "ID_COMMON_SCORE".to_owned()];
    d.dialog.passed = won;
    d.dialog.killed = false;
    d.dialog.slots = [CONTINUE_RETRY, RETRY_QUIT, NONE, NONE];
    d.other = None;
    d.results = Some(contest_results(&scores, true, "ID_CHALLENGE_COMMON_CONTEST_STANDINGS", &columns, MENU_FINAL_RESULTS));
    d.title = state.data.title.as_ref().map(|t| format!("#{t}")).or_else(|| state.data.title_id.clone());
    d.open = true;
    d.choice = None;
    d.refill = true;
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(PreUpdate, arm.after(crate::map_transition::MapTransitionSet).run_if(crate::challenges::opted_in))
        .add_systems(Update, update.after(crate::challenges::CareerResultSet).run_if(crate::challenges::opted_in));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_becomes_its_params_and_rows_without_a_pro_are_not_run() {
        let row = serde_json::json!({"id": "1up_x", "map": "downtown", "starts": [{"locator": "s", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}],
            "waits": [{"locator": "w", "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}], "competitors": [{"character": "chris_cole"}],
            "time_limit": 22.0, "cumulative": true, "pro_goes_first": true, "allow_player_bail": false,
            "thresholds": [350, 350, 800, 800], "multipliers": [0.95, 1.0, 1.05, 1.1, 1.15]});
        let d: OneUpData = serde_json::from_value(row).unwrap();
        let p = params(&d);
        assert_eq!((p.competitors, p.time_limit, p.thresholds, p.max_letters), (2, 22.0, [350.0, 350.0, 800.0, 800.0], 3));
        let names = ["#me".to_owned(), "#Chris Cole".to_owned()];
        let (text, args, modified) = banner_text(&d, &Banner::Turn { runner: 0, setting: true }, &names);
        assert_eq!((text.as_str(), args, modified), ("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_SETYOURSCORE", None, true));
        // The id and its arguments stay apart, so the id is looked up and the arguments fill its placeholders.
        let (text, args, _) = banner_text(&d, &Banner::Eliminated { runner: 1 }, &names);
        assert_eq!((text.as_str(), args.as_deref()), ("ID_CHALLENGE_CENTRAL_MESSAGE_1UP_ELIMINATED", Some("Chris Cole")));
        assert_eq!(crate::hud_central::format_message("{0:s} is eliminated", args.as_deref()), "Chris Cole is eliminated");
        let (_, args, _) = banner_text(&d, &Banner::IsLeading { leader: 1, score: 2839.55 }, &names);
        assert_eq!(crate::hud_central::format_message("{0:s} is Leading the Round: {1:points} pts", args.as_deref()),
            "Chris Cole is Leading the Round: 2,839 pts");
    }

    /// The installed export (SKATE_TEST_ASSETS): the four career 1-Ups load with their pro.
    #[test]
    fn the_career_one_ups_load_from_the_export() {
        let Some(root) = std::env::var_os("SKATE_TEST_ASSETS").map(std::path::PathBuf::from) else { return };
        let rows = load(&root);
        for id in ["1up_sksc_01", "1up_dwtn_01", "1up_dwtn_05", "1up_univ_01"] {
            let r = rows.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(!r.competitors.is_empty() && !r.starts.is_empty() && !r.waits.is_empty(), "{id}");
        }
    }
}
