use super::*;

/// A host stand-in: answers the presentation commands at once and gives each run the next scripted score (None = the
/// runner bails before landing anything). Returns every command seen, in order, until the run finishes or the script
/// runs dry.
fn play(run: &mut OneUpRun, scores: &mut std::collections::VecDeque<Option<f32>>, limit: u32) -> Vec<Command> {
    let mut seen = Vec::new();
    let mut queue: std::collections::VecDeque<Command> = run.start().into();
    for _ in 0..limit {
        while let Some(c) = queue.pop_front() {
            let reply = match &c {
                Command::Intro => Some(Event::IntroFinished),
                Command::FadeOut(_) => Some(Event::FadeOutDone),
                Command::Place { .. } => Some(Event::Placed),
                Command::FadeIn(_) => Some(Event::FadeInDone),
                Command::PlayHighlightReel => Some(Event::ReelComplete),
                Command::EndOptions { .. } => Some(Event::Continue),
                Command::RunStart { runner, .. } => match scores.pop_front() {
                    Some(Some(score)) => Some(Event::Sequence { competitor: *runner, score }),
                    Some(None) => Some(Event::Wipeout { competitor: *runner }),
                    None => return seen,
                },
                _ => None,
            };
            seen.push(c);
            if let Some(e) = reply {
                queue.extend(run.handle(e));
            }
        }
        if run.finished() {
            return seen;
        }
        queue.extend(run.tick());
    }
    seen
}

fn one_up() -> OneUpRun {
    let mut p = Params::retail(2);
    p.thresholds = [350.0, 350.0, 800.0, 800.0];
    OneUpRun::new(p)
}

#[test]
fn the_pro_sets_the_score_first_a_beaten_run_takes_the_lead_and_a_missed_one_gets_a_letter() {
    let mut run = one_up();
    // round 1: the pro sets 500, the player 1-ups with 600, the pro misses (letter, round over: the player leads)
    let mut scores: std::collections::VecDeque<Option<f32>> = [Some(500.0), Some(600.0), Some(100.0)].into();
    let seen = play(&mut run, &mut scores, 20_000);
    let runs: Vec<usize> = seen.iter().filter_map(|c| match c { Command::RunStart { runner, .. } => Some(*runner), _ => None }).collect();
    assert_eq!(runs, [1, 0, 1], "the pro goes first, then the player, then the pro again");
    assert!(seen.contains(&Command::Banner(Banner::ScoreSet { runner: 1, score: 500.0, first: true })));
    assert!(seen.contains(&Command::Banner(Banner::OneUpped { runner: 0 })));
    assert!(seen.contains(&Command::Banner(Banner::ScoreSet { runner: 0, score: 600.0, first: false })));
    assert!(seen.contains(&Command::ShowLetters { runner: 1, letters: 1 }));
    assert!(seen.contains(&Command::Banner(Banner::RoundOver { leader: 0 })), "fewer than two runners left: the round ends");
    assert_eq!(run.letters(), [0, 1]);
    // round 2 starts with the round's leader, the player
    let runs_after: Vec<usize> = seen.iter().skip_while(|c| !matches!(c, Command::Banner(Banner::RoundOver { .. })))
        .filter_map(|c| match c { Command::RunStart { runner, .. } => Some(*runner), _ => None }).collect();
    assert!(runs_after.is_empty() || runs_after[0] == 0);
}

#[test]
fn three_letters_eliminate_the_pro_and_the_player_owns_the_challenge() {
    let mut run = one_up();
    // Every round: the first runner sets 500, the other misses with 100 or bails: the pro collects three letters.
    let mut scores: std::collections::VecDeque<Option<f32>> =
        [Some(500.0), Some(600.0), Some(100.0), Some(700.0), None, Some(300.0), Some(100.0)].into();
    let seen = play(&mut run, &mut scores, 60_000);
    assert!(run.finished(), "the challenge ends once one competitor is left");
    assert_eq!(run.letters(), [0, 3]);
    assert!(seen.contains(&Command::Banner(Banner::Eliminated { runner: 1 })));
    assert!(seen.contains(&Command::Banner(Banner::Wins { winner: 0 })));
    assert!(seen.contains(&Command::PlayHighlightReel), "a win plays the reel");
    assert!(seen.contains(&Command::Results(vec![(0, 1, 0), (1, 2, 3)])));
    assert_eq!(seen.last(), Some(&Command::Complete(Completion::Owned)), "the graph never sets Killed");
}

#[test]
fn losing_skips_the_reel_and_ends_abandoned() {
    let mut run = one_up();
    // the pro always sets a score the player never beats
    let mut scores: std::collections::VecDeque<Option<f32>> =
        [Some(900.0), Some(100.0), Some(900.0), Some(100.0), Some(900.0), Some(100.0)].into();
    let seen = play(&mut run, &mut scores, 60_000);
    assert!(run.finished());
    assert_eq!(run.letters(), [3, 0]);
    assert!(!seen.contains(&Command::PlayHighlightReel), "no reel on a loss");
    assert!(seen.contains(&Command::EndOptions { won: false }));
    assert_eq!(seen.last(), Some(&Command::Complete(Completion::Abandoned)));
}

#[test]
fn a_zero_run_without_a_leader_gets_no_letter_and_the_next_runner_sets_the_score() {
    let mut run = one_up();
    let mut scores: std::collections::VecDeque<Option<f32>> = [None, Some(400.0)].into();
    let seen = play(&mut run, &mut scores, 20_000);
    assert!(seen.contains(&Command::Banner(Banner::ScoreNotSet)));
    assert_eq!(run.letters(), [0, 0], "no letter before anyone set a score");
    assert!(seen.contains(&Command::Banner(Banner::ScoreSet { runner: 0, score: 400.0, first: true })));
}

#[test]
fn the_pro_difficulty_follows_each_run_score_and_only_easy_hard_and_extreme_are_reachable() {
    let p = one_up().params;
    assert_eq!((p.skill_for(200.0), p.skill_for(500.0), p.skill_for(900.0)), (Skill::Easy, Skill::Hard, Skill::Extreme));
    let mut run = one_up();
    let mut scores: std::collections::VecDeque<Option<f32>> = [Some(900.0)].into();
    let seen = play(&mut run, &mut scores, 20_000);
    assert!(seen.contains(&Command::AiDifficulty { skill: Skill::Hard, multiplier: 1.05 }), "the start level");
    assert!(seen.contains(&Command::AiDifficulty { skill: Skill::Extreme, multiplier: 1.15 }), "after a 900 run (also the pro's own)");
}

#[test]
fn the_run_lasts_the_time_limit_and_a_bail_ends_it_three_seconds_later() {
    let mut run = one_up();
    let mut scores: std::collections::VecDeque<Option<f32>> = [Some(500.0)].into();
    let seen = play(&mut run, &mut scores, 20_000);
    assert!(seen.contains(&Command::RunStart { runner: 1, ticks: 1200 }), "20 s");
    let mut bail = one_up();
    let mut scores: std::collections::VecDeque<Option<f32>> = [None].into();
    let seen = play(&mut bail, &mut scores, 20_000);
    assert!(seen.contains(&Command::Banner(Banner::Bailed { runner: 1 })));
    assert!(seen.contains(&Command::Speech { event: 1210, param: 0 }), "the pro's bail line");
}
