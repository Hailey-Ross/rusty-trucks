//! A 1-Up (eChallengeTypes 0x20), re-implemented from the retail graph `1up` (< turn_based_leader < turn_based),
//! single player (research b1055, b877 B1). Competitors take turns; the first run of a round that scores sets the
//! round's best score, a later run that ends strictly above it takes the lead, a run that does not beat the leader
//! gets a letter and is out for the round. Three letters eliminate a competitor; the standings (fewer letters, then
//! the best single sequence) decide the result: Owned when the player is first, else Abandoned (the graph never sets
//! Killed). The host plays the presentation (intro, fades, teleports, banners, HUD, speech) and answers with events.
//! Details: docs/hails-additions/42-challenges.md#one-ups
use super::timers::{TimerId, Timers};
use super::Completion;

/// AI.eSkillLevel as the graph picks it (RefreshAIDifficulty).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skill {
    Easy,
    Medium,
    Hard,
    VeryHard,
    Extreme,
}

impl Skill {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// One 1-Up's values (row fields over the family defaults, b1055 §3); graph constants as defaults a mod can replace.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Competitors including the player (index 0); the pros follow.
    pub competitors: usize,
    pub time_limit: f32,
    /// CumulativeScoring: a run's score is the sum of its landed sequences; else its best one.
    pub cumulative: bool,
    /// ProGoesFirst: the first NPC starts round 1.
    pub pro_goes_first: bool,
    /// AllowPlayerBail: a bail does not end the run.
    pub allow_bail: bool,
    /// AIPathScoreThreshold Medium / Hard / VeryHard / Extreme (0 = unused) and AIScoreMultiplier per Skill.
    pub thresholds: [f32; 4],
    pub multipliers: [f32; 5],
    /// Graph constants (b1055 §1c).
    pub max_letters: u32,
    pub countdown: f32,
    pub end_run: f32,
    pub letters: f32,
    pub end_round: f32,
    pub winner: f32,
    pub bail: f32,
    pub fade: f32,
    pub fade_delay: f32,
    pub load_delay: f32,
    pub leading: f32,
    /// AudioDefine*: the last-chance and idle warnings (seconds before the end), the low score, the epic run count,
    /// the 1-upped margins and the big-sequence fraction.
    pub last_chance: f32,
    pub idle: f32,
    pub low_score: f32,
    pub epic_run: u32,
    pub margins: [f32; 2],
    pub big_fraction: f32,
}

impl Params {
    /// The 1-Up family defaults (b1055 §3).
    pub fn retail(competitors: usize) -> Self {
        Self {
            competitors, time_limit: 20.0, cumulative: true, pro_goes_first: true, allow_bail: false,
            thresholds: [0.0; 4], multipliers: [0.95, 1.0, 1.05, 1.1, 1.15],
            max_letters: 3, countdown: 3.0, end_run: 3.0, letters: 3.0, end_round: 3.0, winner: 3.0, bail: 3.0,
            fade: 0.5, fade_delay: 0.1, load_delay: 1.0, leading: 3.0,
            last_chance: 5.0, idle: 11.0, low_score: 400.0, epic_run: 4, margins: [400.0, 200.0], big_fraction: 0.35,
        }
    }

    /// RefreshAIDifficulty: the highest level whose threshold (when set) the score is above; Easy otherwise.
    pub fn skill_for(&self, score: f32) -> Skill {
        let [medium, hard, very_hard, extreme] = self.thresholds;
        if extreme > 0.0 && score > extreme {
            Skill::Extreme
        } else if very_hard > 0.0 && score > very_hard {
            Skill::VeryHard
        } else if hard > 0.0 && score > hard {
            Skill::Hard
        } else if medium > 0.0 && score > medium {
            Skill::Medium
        } else {
            Skill::Easy
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Intro,
    SetupChallenge,
    SetupRound,
    SetupRun,
    Countdown,
    PerformRun,
    EndRun,
    EndRound,
    HighlightReel,
    ShowResults,
    Complete,
}

/// The banners the graph shows (central messages; the host maps them to the row's text ids).
#[derive(Clone, Debug, PartialEq)]
pub enum Banner {
    /// `..._1UP_ISLEADING`: the leader and the score to beat.
    IsLeading { leader: usize, score: f32 },
    /// The turn: `..._1UP_SETYOURSCORE` / C.CentralMessageHALIDYourTurn for the player, `..._1UP_OTHERSTURN` else.
    Turn { runner: usize, setting: bool },
    /// C.ModifiedCentralMessageHALIDOneUpped.
    OneUpped { runner: usize },
    /// The runner bailed (the player: ModifiedCentralMessageHALIDBailed, red; another: CentralMessageHALIDNameBailed).
    Bailed { runner: usize },
    /// `..._1UP_SCORESET` (first score of the round) / `..._1UP_NEWSCORESET`.
    ScoreSet { runner: usize, score: f32, first: bool },
    /// `..._1UP_SCORENOTSET`.
    ScoreNotSet,
    /// `..._1UP_ELIMINATED`.
    Eliminated { runner: usize },
    /// The player: C.CentralMessageHALIDDidNotBeatHighScore; another: `..._1UP_OTHER_OUTFORROUND`.
    OutForRound { runner: usize },
    /// C.CentralMessageHALIDRoundOver with the round's leader.
    RoundOver { leader: usize },
    /// `..._1UP_WINS`.
    Wins { winner: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The intro presentation finished (nis_overview presentationeventgroupfinished).
    IntroFinished,
    FadeOutDone,
    /// Every competitor reached its turn spot (skaterteleportcomplete).
    Placed,
    FadeInDone,
    /// A clean landed sequence of a competitor (its `1up_sequence_anywhere` objective, trigger byte 1 / mode 5).
    Sequence { competitor: usize, score: f32 },
    Wipeout { competitor: usize },
    ReelComplete,
    Continue,
    Retry,
    Restart,
    Abandon,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Play the row's intro presentation, then answer IntroFinished.
    Intro,
    /// Pin (true) or release one competitor (AI.PinAI / UnpinAI).
    Pin { competitor: usize, on: bool },
    /// SetAIDifficulty: every NPC's preferred line skill and score bias.
    AiDifficulty { skill: Skill, multiplier: f32 },
    ShowLeaderboard,
    /// Each competitor's letters (UpdateLeaderboardHUD) and the runner (LeaderboardSetTurn).
    Letters(Vec<u32>),
    Turn { runner: usize },
    /// Effects.StartEffect(FadeOut / FadeIn, seconds); answer FadeOutDone / FadeInDone.
    FadeOut(f32),
    FadeIn(f32),
    /// The runner to the round's start spot, everyone else to the wait spot (rotation = the round, 1-based); answer
    /// Placed.
    Place { runner: usize, rotation: usize },
    /// The DPO: the runner's score and the score to beat (None = setting the score), titled by the competitor shown.
    Dpo { score: f32, target: Option<f32>, title: Option<usize> },
    Banner(Banner),
    /// The 3 s turn countdown on the HUD.
    StartCountdownHud { ticks: i32 },
    /// SessionMarker.SetSessionMarker on the player's turn (the run's reset point).
    SetSessionMarker,
    /// The run starts: unpin the runner (an NPC starts its line), the HUD timer, the runner's objectives on.
    RunStart { runner: usize, ticks: i32 },
    /// The run ends: the runner's objectives off, pinned again.
    RunStop { runner: usize },
    /// The letters banner (HUD2.ShowSkateMessage) for the runner.
    ShowLetters { runner: usize, letters: u32 },
    /// The pro's line (PlaySkaterSpeech event 1201..1231, with its parameter).
    Speech { event: u32, param: u32 },
    /// The player's best sequence as the reel, then ReelComplete.
    PlayHighlightReel,
    /// The final results: (competitor, place from 1, letters) in standing order.
    Results(Vec<(usize, u32, u32)>),
    EndOptions { won: bool },
    Complete(Completion),
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Competitor {
    letters: u32,
    eliminated: bool,
    /// The best single landed sequence of the challenge (the standings' tie-break).
    best: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Ids {
    fade_out_delay: Option<TimerId>,
    fade_in_delay: Option<TimerId>,
    load_delay: Option<TimerId>,
    leading: Option<TimerId>,
    countdown: Option<TimerId>,
    run: Option<TimerId>,
    bail: Option<TimerId>,
    almost_over: Option<TimerId>,
    idle: Option<TimerId>,
    show_letters: Option<TimerId>,
    end_run: Option<TimerId>,
    end_round: Option<TimerId>,
    winner: Option<TimerId>,
}

#[derive(Clone, Debug)]
pub struct OneUpRun {
    params: Params,
    state: State,
    pending: Vec<State>,
    timers: Timers,
    ids: Ids,
    competitors: Vec<Competitor>,
    non_eliminated: Vec<usize>,
    in_round: Vec<usize>,
    order: usize,
    round_start: usize,
    rotation: usize,
    round_best: f32,
    leader: Option<usize>,
    run_best: f32,
    runner: usize,
    bailed: bool,
    new_leader: bool,
    one_upped_run: bool,
    setting_score: bool,
    run_count: u32,
    first_round: bool,
    round_over: bool,
    challenge_over: bool,
    multiplier: f32,
    result: Completion,
}

impl OneUpRun {
    pub fn new(params: Params) -> Self {
        let n = params.competitors.max(1);
        Self {
            params, state: State::Intro, pending: Vec::new(), timers: Timers::default(), ids: Ids::default(),
            competitors: vec![Competitor::default(); n], non_eliminated: Vec::new(), in_round: Vec::new(), order: 0,
            round_start: 0, rotation: 0, round_best: 0.0, leader: None, run_best: 0.0, runner: 0, bailed: false,
            new_leader: false, one_upped_run: false, setting_score: true, run_count: 0, first_round: true, round_over: false,
            challenge_over: false, multiplier: 1.0, result: Completion::Abandoned,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }
    pub fn runner(&self) -> usize {
        self.runner
    }
    pub fn letters(&self) -> Vec<u32> {
        self.competitors.iter().map(|c| c.letters).collect()
    }
    pub fn run_score(&self) -> f32 {
        self.run_best
    }
    pub fn round_best(&self) -> f32 {
        self.round_best
    }
    /// The current NPC score bias (SetScoreBiasMultiplier).
    pub fn multiplier(&self) -> f32 {
        self.multiplier
    }
    pub fn run_ticks(&self) -> Option<i32> {
        self.ids.run.and_then(|id| self.timers.remaining(id))
    }
    pub fn finished(&self) -> bool {
        self.state == State::Complete
    }

    pub fn start(&mut self) -> Vec<Command> {
        let mut out = vec![Command::Intro];
        self.state = State::Intro;
        self.flush(&mut out);
        out
    }

    pub fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        if !self.finished() {
            self.on_event(event, &mut out);
            self.flush(&mut out);
        }
        out
    }

    /// One 60 Hz tick of the graph's timers.
    pub fn tick(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        for id in self.timers.tick() {
            if self.finished() {
                break;
            }
            self.on_timer(id, &mut out);
            self.flush(&mut out);
        }
        out
    }

    fn change(&mut self, state: State) {
        self.pending.push(state);
    }

    fn flush(&mut self, out: &mut Vec<Command>) {
        while !self.pending.is_empty() {
            let next = self.pending.remove(0);
            if next == self.state && next != State::SetupRun && next != State::SetupRound {
                continue;
            }
            self.state = next;
            self.enter(out);
        }
    }

    fn add(&mut self, seconds: f32) -> Option<TimerId> {
        Some(self.timers.add_countdown(seconds))
    }

    fn clear_timers(&mut self) {
        let ids = std::mem::take(&mut self.ids);
        for id in [ids.fade_out_delay, ids.fade_in_delay, ids.load_delay, ids.leading, ids.countdown, ids.run, ids.bail,
                   ids.almost_over, ids.idle, ids.show_letters, ids.end_run, ids.end_round, ids.winner].into_iter().flatten() {
            self.timers.remove(id);
        }
    }

    fn set_difficulty(&mut self, skill: Skill, out: &mut Vec<Command>) {
        self.multiplier = self.params.multipliers[skill.index()];
        out.push(Command::AiDifficulty { skill, multiplier: self.multiplier });
    }

    /// RefreshAIDifficulty (single player): the NPCs' level follows the run's score.
    fn refresh_ai(&mut self, score: f32, out: &mut Vec<Command>) {
        let skill = self.params.skill_for(score);
        self.set_difficulty(skill, out);
    }

    /// CalculateScores: fewer letters first, then the higher best sequence; equal keeps the competitor order.
    pub fn standings(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.competitors.len()).collect();
        order.sort_by(|&a, &b| {
            let (x, y) = (&self.competitors[a], &self.competitors[b]);
            x.letters.cmp(&y.letters).then(y.best.partial_cmp(&x.best).unwrap_or(std::cmp::Ordering::Equal))
        });
        order
    }

    fn speech_param(&self) -> u32 {
        match self.params.skill_for(self.run_best) {
            Skill::Easy | Skill::Medium => 1,
            Skill::Hard | Skill::VeryHard => 2,
            Skill::Extreme => 3,
        }
    }

    fn enter(&mut self, out: &mut Vec<Command>) {
        let p = self.params.clone();
        match self.state {
            State::Intro => {}
            State::SetupChallenge => {
                self.clear_timers();
                self.result = Completion::Abandoned;
                self.challenge_over = false;
                self.round_over = false;
                self.rotation = 0;
                self.competitors = vec![Competitor::default(); self.competitors.len()];
                self.non_eliminated = (0..self.competitors.len()).collect();
                self.round_start = if p.pro_goes_first && self.competitors.len() > 1 { 1 } else { 0 };
                for competitor in 0..self.competitors.len() {
                    out.push(Command::Pin { competitor, on: true });
                }
                self.set_difficulty(Skill::Hard, out);
                out.push(Command::Letters(self.letters()));
                out.push(Command::ShowLeaderboard);
                self.first_round = true;
                self.change(State::SetupRound);
            }
            State::SetupRound => {
                self.rotation += 1;
                self.round_best = 0.0;
                self.leader = None;
                self.round_over = false;
                self.in_round = self.non_eliminated.clone();
                self.order = self.in_round.iter().position(|&c| c == self.round_start).unwrap_or(0);
                self.challenge_over = self.in_round.len() < 2;
                self.run_count = 0;
                self.setting_score = true;
                self.change(if self.challenge_over { State::EndRound } else { State::SetupRun });
            }
            State::SetupRun => {
                out.push(Command::FadeOut(p.fade));
                self.run_best = 0.0;
                self.bailed = false;
                self.one_upped_run = false;
                self.run_count += 1;
                out.push(match self.leader {
                    Some(l) => Command::Dpo { score: 0.0, target: Some(self.round_best), title: Some(l) },
                    None => Command::Dpo { score: 0.0, target: None, title: None },
                });
            }
            State::Countdown => {
                out.push(Command::Banner(Banner::Turn { runner: self.runner, setting: self.leader.is_none() }));
                out.push(Command::Turn { runner: self.runner });
                if self.runner == 0 {
                    out.push(Command::SetSessionMarker);
                }
                self.ids.countdown = self.add(p.countdown);
                out.push(Command::StartCountdownHud { ticks: Timers::ticks(p.countdown) });
                let npc = self.runner != 0;
                if self.run_count == p.epic_run {
                    out.push(Command::Speech { event: 1225, param: 0 });
                } else if self.first_round {
                    self.first_round = false;
                    if npc {
                        out.push(Command::Speech { event: 1201, param: 0 });
                    }
                } else {
                    out.push(Command::Speech { event: if npc { 1227 } else { 1226 }, param: self.speech_param() });
                }
            }
            State::PerformRun => {
                out.push(Command::Pin { competitor: self.runner, on: false });
                self.ids.run = self.add(p.time_limit);
                out.push(Command::RunStart { runner: self.runner, ticks: Timers::ticks(p.time_limit) });
                if self.runner == 0 {
                    self.ids.almost_over = self.add((p.time_limit - p.last_chance).max(0.0));
                    self.ids.idle = self.add((p.time_limit - p.idle).max(0.0));
                }
            }
            State::EndRun => {
                out.push(Command::Pin { competitor: self.runner, on: true });
                let score = self.run_best;
                self.refresh_ai(score, out);
                self.new_leader = false;
                let leader_before = self.leader;
                if score > self.round_best {
                    out.push(Command::Banner(Banner::ScoreSet { runner: self.runner, score, first: self.round_best == 0.0 }));
                    self.new_leader = true;
                    self.round_best = score;
                    self.leader = Some(self.runner);
                    self.ids.end_run = self.add(p.end_run);
                } else if self.leader.is_some() {
                    let c = &mut self.competitors[self.runner];
                    c.letters = (c.letters + 1).min(p.max_letters);
                    let letters = c.letters;
                    out.push(Command::ShowLetters { runner: self.runner, letters });
                    out.push(Command::Letters(self.letters()));
                    self.ids.show_letters = self.add(p.letters);
                } else {
                    out.push(Command::Banner(Banner::ScoreNotSet));
                    self.ids.end_run = self.add(p.end_run);
                }
                if leader_before.is_some() && (self.setting_score || self.new_leader) {
                    out.push(Command::Speech { event: if self.runner != 0 { 1216 } else { 1215 }, param: self.speech_param() });
                }
                if self.leader.is_some() {
                    self.setting_score = false;
                }
            }
            State::EndRound => {
                if !self.challenge_over {
                    if let Some(l) = self.leader {
                        out.push(Command::Banner(Banner::RoundOver { leader: l }));
                        self.round_start = l;
                    }
                }
                self.leader = None;
                out.push(Command::Dpo { score: 0.0, target: None, title: None });
                self.ids.end_round = self.add(p.end_round);
                let npc = self.runner != 0;
                let tied = self.competitors.len() == 2 && self.competitors[0].letters == self.competitors[1].letters;
                let event = if self.challenge_over {
                    if npc { 1230 } else { 1231 }
                } else if tied {
                    if npc { 1222 } else { 1221 }
                } else if self.competitors[self.runner].letters + 1 == p.max_letters {
                    if npc { 1218 } else { 1217 }
                } else if npc {
                    1224
                } else {
                    1223
                };
                out.push(Command::Speech { event, param: u32::from(self.competitors[self.runner].letters == 1) });
            }
            State::HighlightReel => {
                let standings = self.standings();
                if standings.first() == Some(&0) {
                    self.result = Completion::Owned;
                    out.push(Command::PlayHighlightReel);
                } else {
                    self.change(State::ShowResults);
                }
            }
            State::ShowResults => {
                let rows = self.standings().into_iter().enumerate()
                    .map(|(i, c)| (c, i as u32 + 1, self.competitors[c].letters)).collect();
                out.push(Command::Results(rows));
                out.push(Command::EndOptions { won: self.result != Completion::Abandoned });
            }
            State::Complete => {
                self.clear_timers();
                out.push(Command::Complete(self.result));
            }
        }
    }

    fn on_event(&mut self, event: Event, out: &mut Vec<Command>) {
        let p = self.params.clone();
        match (self.state, event) {
            (State::Intro, Event::IntroFinished) => self.change(State::SetupChallenge),
            (State::SetupRun, Event::FadeOutDone) => self.ids.fade_out_delay = self.add(p.fade_delay),
            (State::SetupRun, Event::Placed) => self.ids.fade_in_delay = self.add(p.fade_delay),
            (State::SetupRun, Event::FadeInDone) => self.ids.load_delay = self.add(p.load_delay),
            (State::PerformRun, Event::Sequence { competitor, score }) if competitor == self.runner && !self.bailed => {
                let c = &mut self.competitors[competitor];
                c.best = c.best.max(score);
                let before = self.run_best;
                self.run_best = if p.cumulative { self.run_best + score } else { self.run_best.max(score) };
                let crossed = self.leader.is_some() && self.run_best > self.round_best && before <= self.round_best;
                if crossed {
                    self.one_upped_run = true;
                    out.push(Command::Banner(Banner::OneUpped { runner: competitor }));
                }
                let title = if self.run_best > self.round_best { Some(competitor) } else { self.leader };
                out.push(Command::Dpo { score: self.run_best, target: self.leader.map(|_| self.round_best), title });
                if competitor == 0 && !self.setting_score {
                    if crossed {
                        let margin = self.run_best - self.round_best;
                        let param = if margin > p.margins[0] { 3 } else if margin > p.margins[1] { 2 } else { 1 };
                        out.push(Command::Speech { event: 1205, param });
                    } else if !self.one_upped_run && score >= self.round_best * p.big_fraction {
                        out.push(Command::Speech { event: 1206, param: 0 });
                    }
                }
            }
            (State::PerformRun, Event::Wipeout { competitor }) if !p.allow_bail && competitor == self.runner && !self.bailed => {
                self.bailed = true;
                if let Some(id) = self.ids.run.take() {
                    self.timers.remove(id);
                }
                out.push(Command::Banner(Banner::Bailed { runner: competitor }));
                out.push(Command::RunStop { runner: competitor });
                self.ids.bail = self.add(p.bail);
                out.push(Command::Speech { event: if competitor != 0 { 1210 } else { 1209 }, param: 0 });
            }
            (State::HighlightReel, Event::ReelComplete) => self.change(State::ShowResults),
            (State::ShowResults, Event::Continue) => self.change(State::Complete),
            (State::ShowResults, Event::Retry) => self.change(State::SetupChallenge),
            (_, Event::Restart) if self.state != State::Intro => self.change(State::SetupChallenge),
            (_, Event::Abandon) => {
                self.result = Completion::Abandoned;
                self.change(State::Complete);
            }
            _ => {}
        }
    }

    fn on_timer(&mut self, id: TimerId, out: &mut Vec<Command>) {
        let p = self.params.clone();
        let is = |t: Option<TimerId>| t == Some(id);
        match self.state {
            State::SetupRun if is(self.ids.fade_out_delay) => {
                self.ids.fade_out_delay = None;
                self.runner = self.in_round[self.order.min(self.in_round.len().saturating_sub(1))];
                out.push(Command::Place { runner: self.runner, rotation: self.rotation });
            }
            State::SetupRun if is(self.ids.fade_in_delay) => {
                self.ids.fade_in_delay = None;
                out.push(Command::FadeIn(p.fade));
            }
            State::SetupRun if is(self.ids.load_delay) => {
                self.ids.load_delay = None;
                match self.leader {
                    Some(leader) => {
                        out.push(Command::Banner(Banner::IsLeading { leader, score: self.round_best }));
                        self.ids.leading = self.add(p.leading);
                    }
                    None => self.change(State::Countdown),
                }
            }
            State::SetupRun if is(self.ids.leading) => {
                self.ids.leading = None;
                self.change(State::Countdown);
            }
            State::Countdown if is(self.ids.countdown) => {
                self.ids.countdown = None;
                self.change(State::PerformRun);
            }
            State::PerformRun if is(self.ids.run) || is(self.ids.bail) => {
                for t in [self.ids.run.take(), self.ids.bail.take(), self.ids.almost_over.take(), self.ids.idle.take()].into_iter().flatten() {
                    self.timers.remove(t);
                }
                if !self.bailed {
                    out.push(Command::RunStop { runner: self.runner });
                }
                self.change(State::EndRun);
            }
            State::PerformRun if is(self.ids.idle) => {
                self.ids.idle = None;
                if !self.one_upped_run && self.run_best <= p.low_score {
                    out.push(Command::Speech { event: if self.setting_score { 1203 } else { 1204 }, param: 0 });
                }
            }
            State::PerformRun if is(self.ids.almost_over) => {
                self.ids.almost_over = None;
                if !self.one_upped_run && !self.bailed {
                    let score = self.run_best;
                    self.refresh_ai(score, out);
                    out.push(Command::Speech { event: if self.setting_score { 1207 } else { 1208 }, param: self.speech_param() });
                }
            }
            State::EndRun if is(self.ids.show_letters) => {
                self.ids.show_letters = None;
                let runner = self.runner;
                self.in_round.retain(|&c| c != runner);
                self.round_over = self.in_round.len() < 2;
                if self.competitors[runner].letters >= p.max_letters {
                    self.competitors[runner].eliminated = true;
                    self.non_eliminated.retain(|&c| c != runner);
                    out.push(Command::Banner(Banner::Eliminated { runner }));
                } else if !self.round_over {
                    out.push(Command::Banner(Banner::OutForRound { runner }));
                }
                self.challenge_over = self.non_eliminated.len() <= 1;
                self.ids.end_run = self.add(p.end_run);
            }
            State::EndRun if is(self.ids.end_run) => {
                self.ids.end_run = None;
                if self.round_over || self.challenge_over {
                    self.change(State::EndRound);
                } else {
                    // a letter already took the runner out of the list, so the next runner is at the same position
                    if self.new_leader || self.leader.is_none() {
                        self.order += 1;
                    }
                    if self.order >= self.in_round.len() {
                        self.order = 0;
                    }
                    self.change(State::SetupRun);
                }
            }
            State::EndRound if is(self.ids.end_round) => {
                self.ids.end_round = None;
                if self.challenge_over {
                    let winner = self.standings()[0];
                    out.push(Command::Banner(Banner::Wins { winner }));
                    self.ids.winner = self.add(p.winner);
                } else {
                    self.change(State::SetupRound);
                }
            }
            State::EndRound if is(self.ids.winner) => {
                self.ids.winner = None;
                self.change(State::HighlightReel);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "one_up_run_tests.rs"]
mod tests;
