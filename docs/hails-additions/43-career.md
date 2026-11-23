# 43. Career mode

Status: in progress on branch `world/challenges` (with doc 42), milestone M1 "career skeleton" step 1 of 13.

## Problem

Retail Skate 3's career (board sales, milestones, unlocks, team, the career start and every challenge family) is not
in the engine. The challenge work (doc 42) runs own the spot on its own, without paying anything into a career.

## How retail does it (research, `.local/research/`)

- Checklist and roadmap: b853. Career progression runs on the same state graph and handler system as the challenges,
  one level up, from the progression bank `db.big data/db/progressionbanks/main` (1325 rows).
- Counters, conditions and pay order (b855, checked in Ghidra): board sales = owned it + killed it + bonus + set sales;
  money separately. Progression actions hold an AND list of conditions (0 board sales, 1 challenges done in a group,
  2 one challenge done, 3 never, 4 money earned, 5 money); actions are checked on events (first owned / killed, a board
  sales change, money gain), not polled. A first owned result pays the challenge's OwnedItReward, a first kill pays
  KilledItReward too; nothing pays twice.
- The career start (b868, checked), the progression graphs and their census (b861: 180 logic graphs, almost all
  straight-line), the save layout (b862, checked), team and branding (b870), unlocks (b872), the crossbar and Skate
  Profile feeds (b873 + follow-ups), presentation (b874), achievements (b881), and specs for every challenge family
  (b858 tutorials, b875 films / photos / follow the leader, b876 races / Hall of Meat, b877 contests / turn-based).
- The M1 design (b887): career logic as an engine-independent module driven by serializable events, the progression
  graphs as a data table translated at setup (no Lua runtime), one `career.json` per player profile, unlocks recorded
  but everything left unlocked by default (an earlier decision of the user), and a `sdk.career` mod entry.

## Decisions (user, 2026-11-06)

- The crossbar keeps retail's welcome gate (the board sales bar only after `film_skpk_univ_01` and `photo_univ_05`) in
  data, plus a "skip start" switch (also for mods and as an environment flag) so the bar shows before those exist.
- Presentation the engine does not have yet (Skate 101 cards, yes / no popups, career photos) is answered at once and
  logged as NOT RETAIL YET, so progression graphs finish and pay out.
- Own the spot wins recorded before the career save existed do not pay when they are imported.

## How progression runs (research b888, checked in Ghidra)

- ArmAction (82C1BF50): an unknown or already armed action is left alone; otherwise its conditions are checked at once;
  an action that holds fires without being armed, one that does not goes to the front of the armed list.
- Only `starting_action` is armed by the engine (rows whose auto-start bool is set, not yet on the fired list); it has
  no conditions, so it fires at once. Every other action, the milestones included, is armed by progression graphs.
- Fired actions leave the armed list after each check walk.
- One graph runs at a time (a FIFO queue); a graph ends when it enters the state named `complete` (not on a state
  without handlers); the next queued graph starts on the next manager update.

## Change: the career core (`crates/skate-core/src/career/mod.rs`)

Counters (board sales = the four sales counters; money and money earned, spending clamps at 0), the six condition
types AND-ed, actions, and the tracker: `arm` as ArmAction above, `check` per trigger (a challenge's first result,
sales changed, money changed) checking only the actions that watch it, firing in list order. `Career::challenge_result`
pays in retail's order (827CDDD8): with no earlier result the owned it reward, a kill not killed before the killed it
reward too, then money checks, board sales checks, the status upgrade and on a first result the challenge checks; it
returns effects (paid, status changed, action fired) for the game to log, save and grant. Tests in the module.

## Change: the career save (`crates/skate-data/src/career_save.rs`)

`settings/profiles/<player>/career.json` (`skate3rust.career` v1): challenge statuses (completion, high score,
discovery), the counters, the armed / fired action lists and the career flags (absent = unset, so a flag never set is
not `false`: retail tests `== false`, research b889). Every level keeps keys it does not know. The first load imports
`settings/challenges.json` once as statuses only: wins from before the career save pay nothing. Tests in the module.

## Change: the graph runner (`crates/skate-core/src/career/graph.rs`)

Graphs as data (states by name, handlers by message, ops), run one at a time from a FIFO queue: a graph starts at
`initial` with `stateenter`; a handler's ops run in order and its state change (the last op) applies after them; a
graph ends when it enters `complete`, after that state's stateenter ops; the next queued graph starts on the next
update; a graph without states never ends (blocks the queue, as in retail). Career flags are three-state; the school
exit's `pick` / `$picked`; a career photo reply must carry the request id the graph stored. Everything the game must do
comes out as a `Request` (arm action, unlock reward, activate challenge, presentation, teleport, graph started /
ended); presentation is answered by the host with the reply message (M1 answers at once, logged NOT RETAIL YET).

## Change: the unlock store and the career loop (`career/unlocks.rs`, `career/mod.rs`, skate-data `progression.rs`)

- Unlocks: grants by retail kind (0..12) and name with a "new" flag, the two reward summary lists (big / others), and
  `unlock_all` on by default, so every query answers unlocked until the user turns gating on.
- `Career::start` arms the auto-start actions not yet fired (only `starting_action`); `drive` starts the graphs of
  fired actions and routes a graph's ArmAction back through the career (more fired actions queue their graphs);
  `update` starts the next queued graph; `deliver` passes a presentation reply. Everything else comes out as requests.
- `skate_data::progression::parse` loads `progression.json` into these types; an unknown op or condition is an error.
- Checked on the user's own export (ignored tests with `SKATE_PROGRESSION_JSON`): it loads completely (204 actions,
  53 groups, 181 graphs, all with `complete`), and a new career runs retail's `starting_stategraph`, which activates
  `tut_sksc_01` and `ftl_univ_02` (research b861); `starting_action` fires once per career.
- Reward screens (research b893): the reward summary (FE 64) opens when the challenge's graph has finished; A goes on
  to the milestone cards (65) then teammate cards (66); the summary exit and the card chain clear the lists; the
  deferred milestone scenes play after that.

## Change: the career in play (`crates/skate-game/src/career.rs`)

With the Challenges opt-in, the game loads `private/progression.json` and `settings/profiles/local/career.json` (a new
one imports `settings/challenges.json` once, statuses only), runs `Career::start`, and takes each finished challenge
run from `challenges.rs` (`ChallengeResult` message): pay order, fired actions, graphs. Presentation requests are
answered at once and logged NOT RETAIL YET; every request and effect is logged as `SKATE_CAREER ...`; the file saves on
a short-lived thread after each change. Compiles; its tests and a verify run are still to do.

## Change: the reward presentation (`career/presentation.rs`)

Retail order after an offline challenge pass (research b893, b899; constructor 827CC3B8 and 827CF570 / 827CD150 /
827CD298 read in the recomp code):
1. The pay step marks the summary (C+0x34). Nothing opens while the challenge runner still runs.
2. Once it is idle: with summary content (money, sales or other rows) the presentation start message is sent and the
   summary (FE 64) opens. Without content the deferred events play and the end messages go out; the milestone and
   teammate counts stay pending.
3. The summary's accept closes it and clears its other rows. With pending milestones or teammates the card chain
   runs: one milestone card (FE 65) per accept, then one teammate card (FE 66) per teammate; otherwise it releases.
4. Release: the counts clear, the deferred events play oldest first, the end messages go out.
5. Cards that could not run are kept for the next location message (827CCFA0 sets C+0x35 on the sub-object at C+4;
   the constructor stores vtable 0x823117B0 there and 827CCFA0 is its slot 3, checked 2026-11-07). Who calls
   827CCFA0 is not known yet, so `Presentation::left` has no caller.
Failures and online passes show nothing. Deferred events from grants outside a challenge wait for the next release,
as in retail.

`Presentation` (skate-core, engine free) is the state machine; `career.rs` `present` drives it each frame with the
runner idle test (no challenge or tutorial running) and names the open screen in `RewardScreens`. The menu stage
(`retail_menu_stage.rs`) opens that movie (templates RewardSummary 105, MilestoneUnlock 107, TeammateUnlock 108;
each new card opens its movie again), shows it after its intro, and turns A (AptNext) into `accepted`, which the
career passes to `Presentation::accept` (retail 82650738 / 826504E0). Logged `SKATE_CAREER presentation open <screen>
<index>` and `RETAIL_MENUS open <template> <index>`. On by default since 2026-11-14 (`SKATE_RETAIL_END_SCREENS=0` turns it off):
desktop verify (tut_sksc_01 forced pass + a dev mod's `grant_reward`): the summary draws its tab and row ("Gear", "You
got new adidas stuff!"); stage tests `retail_reward_summary_draws`, `retail_milestone_and_teammate_cards_draw`. The
summary opens only with retail's content (827C58B8: money, sales or other rows, research b899); big rewards alone no
longer open an empty summary. Without the movies (sets exported before them) each screen is accepted the frame it
opens. NOT RETAIL YET: gameplay is not suspended under the cards (FE 65 / 66 suspend it in
retail's screen table); the teammate card offers only Continue (create / preset choices are M6).

Summary data: each pass adds its owned / killed cash and board sales to `Career.summary` (retail summary +0x3C..+0x4C;
online sales stay 0), cleared when the summary closes. When a screen chain opens, `career.rs` builds a `SummaryView`
(skate-core) into the `RewardScreens` resource: the tabs that hold at least one of the result's other rewards with
their rows, the milestone cards (big list), the totals, total cash and board sales, and the previous / next board
sale milestone (the crossbar's start / goal). `reward_summary_host.rs` answers the movie's FERewardSummary natives from
it (b873 §4 table). GetUnlocksInCategory takes the tab index or id; its three columns are title, image and an online
flag (false offline) [inferred from the movie's UNLOCKDATA_DESC / ICON / ONLINE indices].

Reward tabs (progression_reward_tabs, read at runtime from the installed collections by
`skate-data/career_summary.rs` `reward_tabs`; mods change them through collection overrides): title, list flag
(Bool 0xC83BB50EC9A5A801), order (Int16 0x804F77ACECCF24CB) and unlock kinds (array 0x17E926A1AA38ACC4). Stock
[data]: characters 1, challenges 12, gear 6 / 5, locations 0, skate park 7 / 9. Reward rows now also export their
description text id (0xEC2ED691DF1C9F4C) in `progression.json` (`skate-data` `Loaded.reward_text`).

Movies: setup exports `unlock/unlockoverlay`, `unlock_milestone`, `unlock_teammate` and `hudcomponents/endchallengeoptions`,
`endchallenge_small`, `challengefailed_small` (all in fedata.big) with the other menu movies.

Verification: skate-core `career::presentation` tests (summary, cards, release order; no summary keeps the cards;
failures and online show nothing; cards on the next arrival; tabs with rewards only); `reward_summary_host` tests
(natives answer from the view; milestone info columns).

## Change: the progression export (`tools/asset_pipeline/progression.py`)

Setup writes `assets/private/progression.json` (`skate3rust.progression` v1) after the challenges export:
- `groups`: each progression group's challenge ids (the class-typed challenges RefSpec array).
- `actions`: per action its condition list (type, threshold, the referenced group or challenge id), the graph it
  starts and whether it stays on the fired list (only `starting_action`).
- `rewards`: per reward row its unlock kind (inherited from the category parent rows), unlock name, title and image.
- `challenge_rewards`: per challenge the owned it and killed it money, board sales and reward ids, plus the online bonus
  sales (never paid offline), family fields inherited.
Names stay strings (stable ids for the engine and mods); references into the challenge banks resolve through the
challenge keys. The graphs follow in step 7 (translated handlers).

DLC progression banks (2026-11-07, research b912): retail mounts one progression bank per pack (`progressionbanks/
dlc_<pack>` beside the disc's `main`); the export used to read only `main`. It now reads every bank, the disc's `main`
first, then each pack's, translates each bank's graphs against its own `.bin` and merges them (the first copy of a key
wins). `banks` names each bank's source and `from_bank` lists the keys a pack added (the engine ignores both). Install
now copies a pack's BIG when it holds a progression bank too, and no longer only when some pack has map jobs. With the
user's four packs: `main` 204 actions / 181 graphs, `dlc_mmcn` (Maloof) 16 / 13, `dlc_play` 22 / 19, `dlc_dwgh` (Danny
Way) 35 / 30, `dlc_player1` 7 / 4; 231 keys from packs. A pack's starting action (`mmcn_starting_action`, record
fired, no conditions) arms itself like the disc's `starting_action`, so the Maloof chain (street contest, then both
photos, then both lots) runs with no engine change. [data]
- New graph call: `Discovery.SetDiscoveryStatus(challenge, Discovery.eDiscoveryStatus.<status>)` (only in the Danny Way
  bank, `ots_dwgh_01`) translates to `set_discovery`; the career writes the challenge's discovery record. [data]
- Check tools: `.local/tools/progression-export-check.py` (export with the packs into a work folder) and
  `progression-call-scan.py` (lists every handler call the translator cannot translate).

## Change: the graph translator (`tools/asset_pipeline/progression_lua.py`)

Each progression handler is a compiled Lua 5.1 chunk (little-endian, 4-byte floats; research b889). The translator
walks its bytecode symbolically (constants, globals, field chains, call results) and turns it into ops: calls become
`arm_action`, `unlock_reward`, `activate_challenge`, `change_state` (with the Defer / Execute event mode) and the rest of
the b889 schema; writes into the shared flags table become `set_flag`; an `EQ` + `JMP` becomes a structured `if` with
`flag_false`, `reply_matches` or `completion_is`; the school exit's ordered flag chain becomes nested ifs with `pick`
ops, a `picked_none` test and `teleport_to_challenge` / `auto_sign_up` of `$picked`. Any other call or construct fails
the export (so a new bank or DLC never gives a silently wrong graph). The export writes `graphs`: per graph its states
(by name) with their presentation events (type name, two string args, mask; research b892) and handlers by
message. Tests on hand-built bytecode
(`test_progression_lua.py`, no retail bytes).

<a id="dlc-graph-names"></a>

## Fix: the DLC packs' graphs held every new career (2026-11-22)

**Problem.** With the DLC packs installed, a new career never activated skate.School: the save kept the five starting actions as fired, with nothing armed and nothing active, so no challenge could ever progress. The career runs one graph at a time (b888); the DLC starting actions (`dlc_sanitariumdlc_starting_action`, `dwmc_`, `mmcn_`, `play_`) sort before `starting_action`, and their graph never started, so it held the queue for good and `starting_stategraph` (tut_sksc_01, the school teleports, ftl_univ_02, the achievement arms) never ran.

**Root cause** [data]. The DLC progression vaults carry the state and handler `Name` field keyed by its hash (`Hash_942AB8AEE4B414ED` = hash64("Name")) instead of `Name`, so `progression.py` fell back to the rows' keys: all 66 DLC graphs had hashed state names, none had `initial`, and their handlers' message names were hashes too (no `stateenter`).

**Change.**
- `progression.py`: the state and handler names are read under either key; the export reports any graph without an `initial` state (none now: 247 graphs on the disc plus the four packs). Re-export check: only the 66 DLC graphs change in progression.json.
- `career/mod.rs` `Career::start`: ours, a save repair, not retail. A career saved in that state (auto-start actions fired, nothing armed, nothing active, no completion, no graph running or queued) runs its fired auto-start actions' graphs again; they only activate, arm and set flags. The queue itself is saved since 2026-11-14, so this only meets older saves.

**Verification.** Test `a_save_whose_start_graphs_never_ran_runs_them_again_and_a_working_one_does_not`; career tests 52 passed. Desktop run with the user's career (map2, 2026-11-22): the five starting graphs ran on load, the saved career then held 17 active challenges (tut_sksc_01, ftl_univ_02, the school teleports and the DLC packs' firsts) and 61 armed actions.

## Change: milestones, crossbar facts and the Skate Profile page (step 10; design `.local/research/b895-career-steps-10-12-design.md`)

- Export: each action's `parent`, `killable` (challenges with a non-empty killed it list or race type 0x13) and a
  `career` block (`milestone_parent`, `crossbar_gates`). The loader reads them into `Progression`.
- `career/milestones.rs`: the milestones are the actions whose parent chain reaches `milestone_actions`, sorted by
  their first board sales threshold. Crossbar index (827CD728): -2 / -1 while a gate challenge has no status, else
  count - pending - still armed. `facts()` gives GetProgressionData's six numbers, which `career.rs` writes into the
  crossbar's progression facts. `dots()` gives GetMilestoneDotData.
- `Career.pending` counts MilestoneEarned / UnlockTeammate requests (retail's reward summary +0x04 / +0x18) and saves
  in `career.json` `rewards`. They clear when the reward presentation's card chain ends (see
  [Reward presentation](#change-the-reward-presentation-careerpresentationrs)), so the index moves then.
  Between photo_univ_05 done and its graph arming the milestones, the index reads the full count (retail too).
- Skate Profile: the summary rows (class progression_summary_group) are read at runtime from the installed
  collections (`skate-data/career_summary.rs`; no setup change). `allmilestones` / `allchallenges` are the overall
  bars and the parents of the list rows. `career/profile.rs` builds the 8-slot milestone rows (locked while above the
  goal) and the 6-slot challenge rows (members counted per group). The menu stage layers mods (`sdk.menus.profile`)
  on top of this page. The challenge bar now goes to the movie as [title, icon, total, current] (the movie's order;
  before, a mod-set challenges bar showed "current / current").
- Unlocks: the career holds the unlock store. Challenge payout reward ids and the graphs' UnlockProgressionReward
  grant through the reward rows (kind, name from `progression.json` `rewards`) and fill the reward summary lists,
  which the end-of-frame stub clears. Saved in `career.json` `unlocks` ({kind, name, new}); `unlock_all` stays the
  default (on) until gating is switched on. Mods see them in the snapshot and the `career_unlock` event.
- Unlock-gated summary rows (Ghidra 82503AE0 / 82503D50 / 82503BD8): a tUnlock field stores {kind, FNV-1 32 of
  the unlock name} (multiply then xor; the disc's world rows are FNV-1 of `megapark`, `startpark`, `downtownpark`,
  `industrialpark`). The row shows once that pair was granted; retail's unlock-all bypass (+0x24) is off for players,
  so ours does not open these rows either. The disc's only gated row (ID_WORLD_BLACKBOXPARK, kind 0, hash
  0x00009AAD, no known name) stays hidden as in retail. The DLC packs bring their own summary rows (Danny Way,
  Maloof Money Cup, Art Gallery, Sanitarium; no unlock field), so they show exactly when the DLC is installed.
- Checked against a capture of the recomp's Skate Profile (laptop, finished career, 2026-11-06): the untitled rows are
  not shown; Career Progression / Challenge Progression are only the two overall bars, not list rows (so ours leaves
  the overall rows out of the lists); the list order is the rows' order field (DLC Maloof sits between Hall of Meat
  and Own The Lot, After Dark between Races and skate.School, as the field says); the milestone icons match the
  three flags. Not ported yet: DLC rows the player does not own show dimmed with 0 in retail.

## Change: the career loads off the main thread (`career.rs` `read` / `apply`)

`progression.json`, the career and achievement files and the ~10 MB stock collections are read and parsed on a
background thread when the career first runs; the main thread only applies the result (import, adopt, clean-up,
defaults, load replay, start). Challenge results that arrive meanwhile wait and are applied after the load.

## Change: team, brand tiers, teammates and achievements (step 11)

- `career/team.rs`: five members (0 = the player) and the brand tier set. UnlockTier is idempotent; a tier query
  compares the rows' bit (field 0x4632B6D0D476D3AB, the bit TeamBrand.UnlockTier sets, b870), so tiers that share a bit
  share an unlock, as in retail. UnlockTeammate: the first member 1..4 not in yet joins, its
  `040N_team_photo_teammate_N` tier is set and the pending teammate count goes up; nothing at five. Default-unlocked
  tiers are set on every load [inferred b870]. Saved in `career.json` `team` as tier ids.
- The team record (research b980, the team reset 827CACF8, save 827CAFE0 / load 827CB0E0): team name, logo id and
  teammates 1..4 with joined flag, name and character key. A new team is retail's reset: name `MyTeamName`, every
  teammate `Teammate`, keys `teammate_01`..`teammate_04` (the living world rows), only the player joined, no logo
  (None = the fallback logo). Retail keeps a teammate's joined flag one record after its name and key; ours sits on
  the teammate it describes, with the same outcome while teammates join in order. Names hold 16 characters (retail
  char[17]). The defaults are data (`TeamDefaults`), so a mod can replace them.
- `career.json` `team` gains `name`, `logo` and `teammates` [{joined, name}] (retail saves name and flag, not the key;
  the key is rebuilt from the defaults on load, as retail rebuilds it on every reset). `members` is still written for
  older builds and read when `teammates` is absent, so an older career keeps its joined teammates.
- Saved looks (research b983, b988): retail saves teammate looks in their own "CAC" record next to the career
  record, five slots (0 = the player), and a slot with a look is teammate n's spawn gate (b977). Ours:
  `settings/profiles/local/cac.json` (`skate3rust.cac` v1, `crates/skate-data/src/cac_save.rs`), five slots, each
  `{source: preset {id} | custom | mod {id}, profile}` where `profile` is a customizer profile (the
  `settings/character.json` form). The player's own look stays in `settings/character.json`. An unreadable slot loads
  empty (retail resets a slot whose read fails); an unreadable file is kept as `cac.json.bad`. Retail parts the
  customizer does not apply yet (the 13 tint records, stamps) ride along as extra profile keys.
- Mods: `sdk.career.set_teammate_look(n, profile)` (nil clears), event `career_teammate_look {n, set}`, the snapshot's
  teammates carry `look` (its source). A mod's looks go when the mod stops, so that teammate's gate closes.
- Teammate presets (research b986, b988, b990, b994; `crates/skate-data/src/teammate_presets.rs`): the leaf rows of
  vault class progression_preset_teammate (`preset00`..`preset09`, ids 0..9, names `ID_TEAMMATE_PRESET_01`..`10`;
  `default` is a parent, so not a preset, 828DA0D8), each naming `data/cacrecipes/SavedRecipe<id>.bin`. Read at career
  load from the installed collections and recipe files (no setup step), off the main thread. A recipe becomes a
  customizer profile: gender, first asset + material per slot, truck / wheel, the four gestures, the 19 morph floats by
  the morph table's `target_order` (read from the same collections, b994 / b996), and the 13 tint records and tattoo
  records kept raw as `retail_tints` / `retail_tattoos` (their customizer mapping is open, b990). The stamp tail is not
  read yet (units open, b996). Retail lists the presets sorted by localized name; `sorted_by` does that for the list
  screen. `CareerState::apply_preset(n, key)` writes the look into slot n with source `preset {id}` (retail 8257DC48 ->
  827D5288). Mods: `sdk.career.set_teammate_preset(n, key)`; the snapshot lists `presets` {key, id, name_text}. A
  preset look is engine-owned and stays when the mod stops.
- Teammates in the world (research b977): retail has no teammate spawner. The four characters_marquee rows
  `teammate_01`..`04` sit in the normal free-roam pool, and the pool predicate 82461550 keeps one only while its save
  skater slot is filled (the unlocked flag check there is dead code in TU3). Ours: `skater_characters` keeps those rows
  with their slot (`teammate_index`, data), `SkaterWorld::eligible` drops them from the pool pick, the pool and a call
  request while the slot is empty, and the population tick takes the filled slots from `SaveSlotLooks`, which the
  career publishes from `cac.json` whenever a look changes (no map reload). A teammate on a line wears its slot's
  look: `living_world/npc_outfits.rs` assembles the customizer profile from the customizer's parts under one
  container (materials, tattoos, morphs) and binds it to the stock skeleton like any NPC look. Lines, tricks and voice
  come from the rows' own data (AI profile `teammate_default`, voices 100..103).
- FE 62 (Spawn Skater) lists a teammates group (key 1, `ID_CIC_TEAMMATES`, 82559EB8) after You: each joined teammate
  whose slot has a look, labeled with its name, for Call Skater. Playing as a teammate (Free Play) is not offered
  yet: the player model does not wear a slot look.
- Test hooks for scripted checks: `SKATE_TEAMMATE_PRESET=<n>:<preset key>[,...]` gives teammate n a preset's look at
  career load (saved in `cac.json`); `SKATE_CALL_SKATER=<key>[,...]` calls those skaters once per loaded world as FE 62
  Call Skater would (log lines `SKATE_CAREER teammate preset`, `LIVING_WORLD call skater`, `LIVING_WORLD teammate look
  bound`).
- `{N:TEAMMATE}` (824FEAB8): the sign-up poster's description goes through the typed path (825C2018 -> 8290C9A0):
  the argument goes to the lowest placeholder index present (8290CBD8), and the TEAMMATE type shows teammate v's name
  while 0 < v < the member count, else `ID_COMMON_UNKNOWN` (`fe_language::format_typed`, `teammate_text`). Used by
  the anywhere photo / film challenges (argument 1..4).
- Not in yet: the random start logo and the logo rows (backlog 14), the keyboards that rename (13), mod-added preset
  rows, the leave fade on outfit looks (the fade drives the stock and ped materials, not the customizer's), and the
  PLAYER text type.
- `career/achievements.rs`: a grant needs an existing, enabled row (class 9CB9012080F727B9: platform id, enabled
  gate) and happens once. Sites: the GrantAchievement op, the GrantAchievement presentation event, and the load replay
  (827CE7B0): the actions with one board sales condition whose graph grants an achievement, granted at load once
  their threshold is reached. Kept in `settings/profiles/local/achievements.json` (`skate3rust.achievements` v1),
  separate from the career so a new career keeps them.
- Graph states now carry their presentation events (type, args, mask). Execute plays them at once; Defer queues them
  in the career and they play when the presentation releases (the end-of-frame stub). Only GrantAchievement does
  anything yet; the other event types are logged NOT RETAIL YET.
- Tier and achievement rows are read at runtime from the installed collections, keyed by the names the graphs use
  (the row keys are stored hashed), so no names.txt or setup change is needed.

## Change: the mod entry `sdk.career` (step 12)

- Snapshot `career` (counters, milestone index / count / start / goal / thresholds / pending, armed and fired
  actions, flags, team, achievements, running graph and queue) and commands `career_add_sales`, `career_add_money`,
  `career_arm_action`, `career_set_flag`, `career_grant_achievement` (`crates/skate-game/src/modding/career.rs`,
  capability `career` 1, `sdk/ENGINE_API.md` "Career").
- Team names: `sdk.career.set_team_name(name)` and `set_teammate_name(n, name)` (16 characters kept, empty allowed),
  events `career_team_name` / `career_teammate_name`; the snapshot's `team` carries name, logo and teammates
  {joined, name, character}. Names stay with the career when the mod stops.
- Mod board sales go into the bonus counter (online only in retail, 0 offline), so they count in board sales and
  run the same checks as a challenge payout; money runs the money checks.
- When a mod stops, the career changes that work without it stay (board sales, money, retail actions and
  achievements: the commands only accept ids from the game data); what depends on the mod goes: its flags
  (namespaced `<mod id>:<name>`) at once, and at load any save entry naming an action or tier the loaded data does
  not have (logged `SKATE_CAREER dropped unknown save entries`). User decision 2026-11-06.
- Events go to the mods through `on_event` (newest 64 kept).
- Also `career_grant_unlock` (retail kind as `category`, since `kind` is the command tag), `career_grant_reward`
  (a reward row: unlock plus summary list) and `career_skip_start`.
- Skip start (b887 U1, user ok 2026-11-06): the crossbar shows its welcome panel until `film_skpk_univ_01` and
  `photo_univ_05` have a status, and M1 has no way to play them. Skip start arms the actions that wait for each gate
  (the start chain's graphs normally arm them), then completes the gate as owned through the normal result path (pay, checks, graphs: photo_univ_05's graph arms the milestones), as if the
  player had done the start chain. `sdk.career.skip_start()` or `SKATE_CAREER_SKIP_START=1` at load.
- Dev mod `mods/career-test` (`dev-career-test`, off by default; `SKATE3_MODS_ENABLE=dev-career-test`): skips the
  start, adds 15,000 board sales and logs `CAREER_TEST` lines (milestone facts before / after, career events).
- Mod data: a mod ships `progression/*.json` in the export's schema with rows by id (add a challenge to a group,
  change an action's conditions or graph, add rewards or graphs, change reward amounts). Objects merge key by key,
  other values replace, `null` removes (`skate-data/progression.rs` `merge`). The running mods' overlays apply in mod
  id order over the setup export and follow the mod set (synced with the audio overlays in `modding/mod.rs`); an
  overlay that breaks the data is logged and left out. When a mod stops its overlay goes and the data rebuilds; save
  entries that only existed through it (armed / fired actions, queued or running graphs, tiers) are dropped.
- Challenge activation: the career keeps the activated set (graphs' ActivateChallenge / ActivateGroup minus
  DeactivateChallenge; `career.json` `active`) and, while gating is on (`unlock_all` off), only those challenges can
  be signed up for (`challenges.rs` `available`). Gating stays off by default, so every challenge is
  available as before. Mods: `career_activate_challenge` / `career_activate_group`.
- Activation follows retail Activate(key, force 0) (82C1D7D0, research b846 / b908): a key already active or already
  completed is skipped (`Career::activate`). [code]
- Discovery activates (research b910, verified by main: ctor 8255B290 stores the career object at +0x6180, handler
  8255BD38 calls its vt+0x14 = Activate): riding into a challenge's boundary marks it discovered and activates it
  (`ChallengeDiscovered`, `career.rs`), so discovery no longer waits for the gate; only the mod switch hides a
  challenge from discovery. [code]
- Completion and the active list (research b918, verified by main: 8283FD00 calls career vt+0x14 with r5 = 1): an
  owned / killed result always takes the key off the active list; CompleteChallenge's flag puts it straight back
  (forced). Own the spot passes it on every result, tutorials pass their row's Persistent (true only on the park
  tutorials), every other family passes nothing. Ours: `Career::settle_active` after the result, with
  `ChallengeResult::reactivate` (own the spot true; tutorials their exported `persistent`, local data field Persistent,
  true only on tut_skpk_01..05, research b919; skip start false). [code + data]

## Change: the first challenge of a new career (`career/boot.rs`)

Retail's new career flags live in the profile save (ProfileBackEnd::ProfileData v5: +0x68 "new career pending",
+0x6C boot progress, written by 825007E8, read by 82500108; research b1001). The profile reset (825009F0) sets
pending; the first world load then queues one challenge by the world it loaded and clears it (827CF960, research
b976 / b1000): the skate school (`dist_skateschool`) starts `tut_sksc_01`, any other world `ftl_univ_02`, through
the runner's deferred start. Ours: `career.json` gains `boot { pending, progress }` (absent in a career saved before
it = not pending, so existing careers are untouched); a new career sets pending; `career.rs`
`queue_first_challenge` signs up for the challenge `FirstChallenge::pick` gives on the first map load and clears the
flag (the sign-up starts it when it is on that map; follow the leader is not ported yet, so on other maps only the
flag clears, as retail clears it either way). The rule is data (`FirstChallenge`, retail defaults) a mod can replace.
The rest of the boot flow (welcome, intro movie, team name, forced logo pick, first teammate, Coach Frank) is mapped in
research b1000 / b1001; the forced logo pick is in "Change: the team logo (FE 63)" below. FE 49 "Marketing" owns the skip-intro path 825F2C98 (career wipe, teleport,
photo_univ_05); no player path opens it, so `SKATE_CAREER_SKIP_START` stays a developer / mod option. A new
profile's settings follow the reset 824FFAB0 (research b1008): difficulty Normal (+0xB4 = 1) and camera Low (+0xBC
= 0), so `Difficulty` and `CameraAngle` default to those (they were Easy and High); a saved setting is kept.
Vibration (ours off by the user's decision) and units (retail Imperial, Metric on PAL; ours Metric) stay as they are. Tests:
`career::boot` (the rule), `career_save` (old careers are not pending, the flag round-trips).

## Change: the first lesson starts on its card (2026-11-17)

- Retail (PS3 recording 11:10): after the skate.School load the first lesson's sign-up card shows (title, description,
  B Decline / A Accept) before the demo and the lesson; ours started the lesson at once.
- Change: the career's queued first challenge sends `ChallengeOffer` (`challenges.rs`): the sign-up card opens for any kind
  (tutorial rows now keep `description_id`, `tutorial_poster_view`), Accept signs up as a sphere's press does; without the
  card it signs up at once. Checked: career-boot-run -From 7, `tut_sksc_01` card as in the recording.
- Open: Decline on the first lesson (what retail offers next is not read).

## Change: the skater photo (2026-11-17)

- Retail (research b1013): FE 67 builds the player's photo before its menu shows (82650BA0 states 1..6, request
  0x577F33D8): a live render of the member's skater, 128 x 256, FOV 45, near 0.01, far 200, over the slot's fixed backdrop
  `fe/source/images/photo/team_profile_background_N` (class 569B02CE038D028A `team_profile_00..04`), in the slot's pose
  clip (`CAS_Single_Pose_5/2/3/7/8`). `GetTeamData()[3+i]` names the photo; the card loads it with `LoadAsset`.
- Change: `skater_photo.rs` renders the player's meshes (on layer 25 for the photo, with the character light's 28) and a
  backdrop quad into a 128 x 256 target, after the start's loading (before, the pipelines were still compiling and the
  picture stayed empty); FE 67 waits for it; `GetTeamData()[3]` = `skater_photo_0`, which the stage maps to the picture.
  The backdrops export with the menus (`prepare_menu_movies.py`).
- 2026-11-22: a preset picked on FE 68 keeps its look and the photo is taken again once the rebuilt outfit is on
  (82653960 states 3..8); every photo waits for the worn outfit first. FE 68's preview photos use the same setup (doc 47
  [Preset skaters](47-movie-player.md#preset-skaters)). The slot's pose clip is on the skater while the photo renders
  (`CAS_Single_Pose_5` from `cac_edit_<gender>`, held at 0.1 s [inferred], turned to the camera): the board is in hand,
  and the photo no longer depends on the world animating (it is held under the boot flow).
- NOT RETAIL YET: the setup's camera vectors are not decoded (b1013 open 1): a fixed full-body framing, and the pose
  row's yaw (260) and placement are not used. Teammate slots 1..4 and the re-render on a logo change come with
  the team view.
- Checked: career-boot-run -From 5, the skater on the teal backdrop in the first card.

## Change: the team logo (FE 63)

- **Retail** (research b1011, b974, b970): the new career's boot flow opens FE 63 TeamManagement (template 108) after
  the team name keyboard in mode 2, the forced logo pick: SetState 0x1E, every key goes to the movie (82651EE0 returns
  0 in mode 2, no Back). The movie's 3 x 3 grid lists `GetNumTeamLogos` thumbs through `GetLogoAssetID(i)`; moving
  calls `SelectLogo(i, true)` (header preview, SetState 0x15), A calls `SelectLogo(i, false)` (826524B8: write the
  logo, commit, close, post 0x8E). The list is the 42 `predefined` children of the logo class `challenge_team_logos`;
  the `usergen` profile-picture rows and the `brands` rows are not in it.
- **Order:** the picker shows the rows in ascending hash64 of the row key: all 42 checked against the PS3 recording of
  a new profile (2:06 to 2:40: mongo, team_blue_1, checker_bones ... team_blue_3, shoe, turtle). The vault's storage
  order is different. The header follows the hovered logo in the recording (b1011 open 4 settled).
- **Setup:** `prepare_menu_movies.py` exports the movie (`TeamSkater_Selection/team_management`), the 45 team logo
  textures (`data/fe/cas/logos/logo_object_team_logo_*`, named images) and the manifest's `team_logos`
  ([{key, text, image}] in picker order). The vault keys are hashed; a row `logo_<x>` is named from its texture
  `logo_object_team_logo_<x>_mat` (all 42 match). Needs a Refresh Game Files.
- **Engine:** `team_management_host.rs` answers `GetTeamData` / `GetNumTeamLogos` / `GetLogoAssetID` / `SelectLogo`
  while FE 63 is open; the asset name given to the movie is the texture name, which the menu images resolve by name
  (`ImageRenderer_Load`, `LoadAssetByName`, `LoadTeamLogo` / `LoadAsset` with a name). `career.rs` runs the step
  (`boot_flow::TEAM_LOGO`), saves the pick in `career.json` `team.logo` (the stable row key) and goes on to FE 67,
  whose header now shows the logo. A saved key that no longer resolves shows no picture (retail falls back to the
  `default` row, which has none).
- **Mods:** the rows are data (the manifest list); a mod-facing way to add logos (`sdk.career` team logos with their
  own bitmaps) is not there yet.
- **Not hosted yet:** FE 63's team view and the pause menu's logo picker (modes 0 / 1: teammate cards, rename, B / X /
  Y), member photos. FE 67's logo sits on a dark square that retail does not show (the logo clip's empty backing,
  not checked yet).
- **Checked** (desktop, `.local/poc-tests/career-boot-run.ps1 -From 4`, keys D-right, D-right, D-down, A): the grid
  in retail order, the cursor on the sixth logo with the header previewing it, the pick saved
  (`team.logo = logo_sv_crest_alan`) and shown on FE 67. Tests: `team_management_host` (list, preview, pick, closed
  screen, unknown key), `welcome_host` (FE 67 logo), `tools/test_prepare_menu_movies.py` (names from textures, hash
  order, brands left out).

<a id="nis-scenes"></a>

## Change: NIS scenes (lesson demos), step 1: the data (2026-11-18)

- **Problem.** Accepting a Skate School lesson plays a demo in retail: a fisheye scene of Benny Fairfax doing the
  lesson's move (PS3 recording 2026-11-16, 10:40, about 22 s). Ours skipped it (the presentation's NIS event is a
  logged stub, doc 42 "Tutorial presentation"), so the lesson looked like it had not started (ask 698).
- **Retail data** [data]. Every lesson's intro and restart lists play NIS `tut_sksc_NN` (challenges.json). The scene
  table `data/scene/scene_000.bin` (miscboot.big, big-endian): header {tag, version, 503 records, 1282 actors};
  104-byte records {name hash, kind, 7 floats (the last is the length in seconds), actor count, first actor,
  name[64]}; 180-byte actors {character[32], type[16] (`marquee`, `livingworld`), clip[64], scene[64], u32}. The
  name hash is h = 0xFFFFFFFF, h = h * 33 + byte, and it is also the scene's ICE camera block id in
  `ice_000.bin` (laptop job 3, 2026-11-06). tut_sksc_01: one actor, `benny_fairfax` (marquee), clip
  `scene/anim/tut_sksc_011.abin` (133 bones, 22.33 s at 30 fps), length 22.3 s, five camera tracks of 22.3 s;
  tut_sksc_04 has no actor. The actors name 751 clips; 712 are in scene.big (417 MB), the other 39 are cut or test
  scenes (`crowdtest_*`, `arrivalofmikey_*`, `corex_spotfilm*`).
- **Change.** Setup (`scene_clips.py` `export_nis`, hud group) writes `private/hud/scene/scene_000.bin` and every
  named clip into `private/hud/scene/anim/` (1.1 s on the desktop). `skate_data::scene_table` (`parse`,
  `scene_hash`, `Scene::length`). Scenes are looked up by name from this data, so a mod can replace a scene's clip
  or table.
- **Tests.** `scene_table` (hash = the ICE ids, records and actors, a short file is an error); data-gated with
  `SKATE3_SCENE_DIR`: the disc table (503 scenes, tut_sksc_01's actor, clip and length), its 5 ICE tracks and the
  clip decode.

### Step 2: the NIS player (2026-11-18)

- **Retail** (research b1027, partial): the presentation's NIS event and Lua `NIS.EnterBasicNIS` (82849618) both call
  the NIS manager (`*(830CFD94)+200`) vt+0x10(name, mode, 0, 1, 0, 0) [code]. The scene ends in 8285F720, which posts
  cMsgNISChallengeNotify with +0x10 = 1 (that ends the event), cMsgNISComplete and pops input mode 8 [code]. Marquee
  actors are queued loads of their character (82824610); `current_player` / `dynamic` actors are the local skater
  (82823F88) [code, function heads only].
- **Camera cuts** [data; meaning inferred]: a scene's camera takes are its ICE tracks. One track without camera keys
  holds group 35 per segment: TAKE_START (16 bits, as a fraction of the scene length) and TAKE_NUMBER. For
  tut_sksc_01 the cuts are 0.56 s take 2, 3.92 s take 3, 9.21 s take 0, 12.97 s take 0, 18.79 s take 1 (the last
  segment is empty). The takes' own keys fit this: take 1 holds still until 17.06 s and moves from 18.15 s, take 2
  moves from 0.56 s. Takes are sampled on scene time. Every key of the lesson scene is SPACE 1, a point fixed in the
  scene frame (828F36B0 has no case for it). The clip's hips at t = 0 (-32.5, 0.76, -15.6) and take 0's first look
  point (-34.2, 0.93, -14.9) are in the same frame, and in game that frame is the map's: the demo plays on the
  Skate School road as in the PS3 recording. The scene has no FISHEYE keys; the lens is 24.4 mm (wide).
- **Change.** `presentation_run::NisHandler` (NIS is no longer a stub): start asks the player for the scene, the event
  holds its group until the player reports the end, stop ends the running scene. `nis_player.rs`: the scene table
  by name, the first actor with a clip wears its character on the player rig (`CustomModels::request_preview`, as
  FE 62's preview does; put back at the end), poses with the clip in map space, the gameplay camera shows the take
  the cut list names (`ice_camera::scene_cuts`, `take_at`) with the take's lens; the scene ends after its length;
  the gameplay HUD hides while it runs (`nis_player::playing`). A scene missing from the table, or without an
  actor clip on disk, ends at once (logged), so no group waits forever.
- **Mods.** Scenes, actors, clips, cuts and lengths are data (scene table + ICE file); a mod's presentation group
  plays any scene by name through the NIS event, and a mod can override the NIS handler by name (registry).
- **NOT RETAIL YET / open.** Skip (cMsgNISSkip / cMsgNISKill exist; the button and prompt are not read); whether
  the end is the scene length or the clip end (equal for the lessons); scenes with several actors (only the first
  plays; the others need their own rigs); the player is not moved; what
  makes the retail demo look fisheye; NIS speech and subtitles inside scenes.
- **Checked** (`.local/poc-tests/nis-run.ps1 -At 8 / -At 30`, SKATE_VERIFY_TUTORIAL=tut_sksc_01, muted): at 8 s
  Benny Fairfax pushes down the road seen from low behind (the recording's shot), no HUD; the scene ends at
  22.30 s, the player's own skater stands on the start pad and the Coach Frank tip "3 pushes = Max speed" shows.
  Tests: `presentation_run` (NIS holds its group until the end report, removal stops it), skate-data `scene_table`
  data-gated (cut list of tut_sksc_01, `take_at`).
- **Paused simulation (2026-11-20, research b1053).** A basic NIS start (8285E968) pushes pause mode 12 on
  ThePauseMan (830CFE24; formerly read as an input map parser) and NIS complete (8285F720) pops it; mode 12's mask
  has bit 0, so the world step stops while the scene plays [code]. Ours: the NIS player pushes / pops
  `pause_man::mode::NIS`; the skater's simulation and the living world freeze for the scene (doc 42 "Paused
  simulation").
- **Step 3, done 2026-11-18:** the objective HUD texts (doc 42 "Tutorial HUD").

### Step 4: skip (2026-11-18)

- **Retail** (research b1028; claims from the agent's Ghidra reads, not re-checked by main): PostMsg_NISSkip 8285F0E8
  posts cMsgNISSkip only while a scene runs (+0x46) and it is skippable (+0x4D) or the skip is forced; it marks the
  run skipped (+0x260). The basic NIS start 8285E968 stores +0x4D from its fifth argument, 1 from the presentation
  event and Lua EnterBasicNIS, so the lessons can be skipped. The skip handler 82822D88 also needs no online
  session, a current clip and the scene table's yes (vt+0x14); record kinds 0x0C / 0x0F / 0x12 also wait for a
  minimum clip time (float 82060C50), kind 0x13 (the lessons) does not. A kill (82822E58: Lua KillNIS, the post
  challenge quit, the next activity, an online join) has no gate. Both end through the normal completion
  (notify 1); cMsgNISComplete carries skipped for a skip only.
- **Change.** `NisHost.skip` is a skip request (the button or a mod); the player ends a skippable scene
  ("skipped"); kinds 0x0C / 0x0F / 0x12 wait for 2.0 s of clip time. The request comes from one A press (pad) or
  Enter while a scene plays; no prompt.
- **Retail button** (research b1029, agent-derived; our key numbers checked: `retail_menu_stage.rs` APT_NEXT 2,
  APT_PREVIOUS 3, APT_START 0): FE screen 1's key handler 825EFF98 (vtable 82302A10) posts the skip for APT key 2
  (AptNext = A) only, on the first press, with no prompt (ShowMovieSkip / HideMovieSkip are FE 24's). The minimum
  time float 82060C50 is 2.0 (read as seconds).
- **Checked** (`nis-run.ps1 -SkipAt 4 -At 10`): "NIS tut_sksc_01 skipped at 4.00 s", the lesson goes on (start pad,
  objective panel, Coach Frank tip). Test aid `SKATE_NIS_SKIP_AT=<seconds>`. Test: `nis_player` (skippable starts,
  timed kinds before / after 2 s).
- **Open.** Whether Start opens the pause menu during a scene (825EB998 not followed); the scene table's "can skip"
  field (vt+0x14); what input mode 12 does to the skater (not frozen in ours); the skip key as a mod setting.

<a id="create-a-skater"></a>

## Change: the retail Create-a-Skater (FE 15), step 1 (2026-11-18, ask 697)

- **Problem.** "Customize Skater" opened our own compact customizer, not retail's Create-a-Skater.
- **Retail** (research b1030 / b1031, agent-derived; main checked the template rows and the extracted tables):
  FE 15 is template `CreateASkater` (`cas/createskater.swf`), FE 16 `Receipt` (`cas/receipt.swf`); FE 67's
  Customize row asks for the player's name first, then opens FE 15 (PS3 recording 2026-11-16, 3:00). The movie
  calls about 22 `Game.CAC_*` natives (registered in 825A3070) over a static menu table (37 records of 48 bytes:
  name, title id, panel kind, a mode field, help A / B kinds, initial highlight and window) with option lists
  (12-word lists, -1 terminated); EditSkaterMain (menu 15) lists Body Mods 0, Merchandise 1, Edit Style 46, trucks
  47, wheels 48. The 115 option records (text, description, target menu, action, type) are written by the
  constructor 825F4B98 in code, not a table. B pops a menu; on the top menu it opens the exit popup.
- **Change.** Setup: `prepare_menu_movies.py` exports `cas/createskater` and `cas/receipt` (fedynamic.big, with
  their four control libraries); `skate_data::menu_tables` extracts `cas_menus` by content (the "EditSkaterMain"
  record, list 15's words), identical on the TU3 image and the disc build. Game: `cas_host.rs` answers the menu
  natives from that table (title, rows, indices, help texts); the stage opens FE 15 for the boot flow's Customize
  row (`CasScreen`) and the boot flow waits for it to close.
- **Option records** (b1031): written by the constructor in code; `menu_tables::cas_options` finds the constructor
  by the instructions that build the option lists' address and runs its stores through a register file (the last
  store per field wins): 115 records with text, description, target menu and type, identical on TU3 and the disc
  build (option 0 Back Alley Pete's -> menu 17, 1 Slappy's -> 1, 46 Skate Style -> 16, 47 / 48 sliders).
- **Panel** (b1032): retail builds the panel with one call, `ScreenManager.SetScreenState(id, M+8)`, on every menu
  entry; the movie's SetState runs InitMenuPicker / InitThumbBrowser / InitStyleMenu / InitMorphMenu itself. The
  stage sends it once FE 15 is shown and whenever the menu or panel changes.
- **Input** (b1033, agent-derived): the movie owns Up / Down and A (its A posts `Game.OnMenuSelect`, whose
  handler 825FE3C0 pushes a row's target menu; leaves with target -1 do nothing); the C++ handler 825FC7E8 owns B
  (pop one menu, the exit popup on the top menu) and Left / Right (trucks / wheels move 0.1 within 0..1, then
  SetScreenState 50). Entering a menu sets the shop mode S+0x177C (menu 15 = 2 "blurb": the info panel shows the
  highlighted row's description; 17 = 1 "petes"; 1 = 0 "slappys"). Slider values: CAC_GetIntegerValue =
  round(value * 10), CAC_GetStringValue LOOSE / SOFT below 0.35, TIGHT / HARD above 0.75, else MEDIUM (look record
  +0x2004 / +0x2008). Exit popup: ID_COMMON_WARNING_TITLE, ID_CAC_EXIT_CAC_CONFIRMATION, Yes / No.
- **Change (navigation).** `cas_host`: OnMenuSelect pushes, B pops (the stage closes FE 15 on the top menu), the
  shop mode per menu, the slider values and labels, Left / Right on the sliders with state 50.
- **Change (exit).** B on the top menu opens the confirmation (`cas_host::exit_popup`, an engine popup on Sk8Popup);
  Yes closes FE 15 and the boot flow goes on (coach_frank_intro, as in the PS3 recording), No re-enters the top
  menu. Checked: the popup text, Yes / No and "B Cancel / A Select" draw; behind it the sliders read "Medium" and
  the info panel shows "Body Mods has all your plastic surgery needs."
- **Part grid** (research b1034 / b1035, agent-derived; main checked the categories, slot names and thumbnail
  keys against the data): a merchandise category's filter object names one catalog slot (its vt+0x14 returns the
  slot number; the slot names are the enum at 0x82FCA0A0, the same names our catalog uses) and a record test
  (t-shirts: TopType tshirt / ls_tshirt; button shirt / hoody / jacket / sweater: TopType = the option's payload;
  socks: SockStyle not "tiny"; piercing slots: Location = payload); the gender filter keeps the skater's gender
  and unisex parts; the list is sorted with the "skate" / "no_brand" parts last; Remove comes first.
  Thumbnails: `fe\cas\<male|female>\<slot>\<model>_<material>` (no gender folder for boards, trucks, wheels).
- **Change (part grid).** `menu_tables` keeps each option's action object (vtable, payload) and its slot number,
  and extracts the 26 slot names (TU3 = disc build). Setup: `cas_grid.py` writes `cas-parts.json` (4595 model /
  material pairs with Gender, TopType, SockStyle, Location, Unlock, ManufacturerID, Cost, DlcID) in the customizer's
  menu stage; the menu set exports every `data/fe/cas/` texture as a named image (2274; the set is 0.2 GB).
  `cas_host`: A on a category builds its grid; menu 10 answers CAC_GetNumItems / GetThumbnailFileName /
  GetOptionType / OnThumbnailSelect (logged, not applied yet). Test `a_category_lists_its_slots_parts_for_the_skater`.
- **Picks, wear styles, info panel, save** (main's own Ghidra reads, 2026-11-18; pseudocode
  `.local/research/ghidra/cas-next*.txt`):
  - Hidden parts [data]: the hidden byte (rec+0x44, listed only when clear) matches `cas.IsUsedInGame`. Every part
    flagged with it is a dirty in-game material (`*_material_d`, 84 of the 88 male shoes without a retail thumbnail;
    none of the parts with a thumbnail has it).
  - Wear style groups [code + data]: 82539188 (the list query's sort for slot types 11 SkateBoard, 15 Pants, 18 Hat,
    23 OuterTorso) keeps one part per group key (rec+0xE0; hats fall back to rec+0xB0 when it is unset). The first
    part of a group with the default flag (rec+0x2C) stays in place; a group without one keeps its first part, moved
    to the end. By data the key is `cas.StyleSubcategory` (hats: the material name) and the flag is
    `cas.IsDefaultWearStyle`. Simulated over the whole catalog (male and female), every listed hat, pant and board
    has a retail thumbnail (male hats 625 -> 131); of the shirts only the 16 bare torsos (no TopType, dropped by the
    record test anyway) and `windbreaker_closed_male` have none.
  - Pick 825FD9F0 [code]: the variants of the part (82538D38: same slot, gender and group; 82603CB0 sorts them by
    model name). More than one opens menu 30 WearStyles (82600780(S, 0x1E)) and keeps the worn part when it is one
    of them. One goes on unless worn, then 82600820(S, 1) pops back to Merchandise (menu 1). Remove clears the slot
    (825377E0 with no part sets the slot key to none) and also returns to Merchandise.
  - Info panel [code]: CAC_GetInfoPanelType = "petes" / "blurb" by the shop mode, else on menu 10 Remove "blurb" and
    a part its type 825FF5E8: **"owned" when the part or one of its wear styles is worn** (the check mark), else
    "locked", "dlc" (the DlcID gate) or "none"; CAC_GetOptionType the same, Remove "none".
    CAC_GetDescriptionText (825FF160 -> 82605528): Remove ID_CAC_REMOVE_PART, a part the first
    `ID_CAC_PART_<MODEL>_<MATERIAL>` the language table has over its wear styles, else `ID_CAC_COMPONENT_TYPE_<SLOT>`
    [data: 3136 part ids in the table, e.g. ID_CAC_UNLOCK_BS_GROUP2_UNLOCK = "Sell 15,000 boards"].
    CAC_GetItemUnlockHALID 825FF3E0: `ID_<Unlock, else SponsorUnlock>_UNLOCK`, "#" without. CAC_GetSponsorLogoFileName
    825FFA38: the brand's logo record (82604070, vault class 0x548B58B0C27C9564), "no_image" for Remove or no logo.
  - Exit Yes 82604748 [code]: sets profile flag +0x6C bit 0, builds the apply record from the look (82539790),
    starts a 250 ms close countdown and the fade.
  - Camera [code]: the screen posts cMsgPresCASCameraTarget(hash): 0xAD454A0ECD0E2EDF (whole body) on the top and
    Merchandise menus, the option's +0x20 hash per category, plus cMsgPresCASBodyTarget (the option's +0x28 hash:
    the pose) and cMsgPresCASCharacterRotate(0, absolute: the turntable back to 0). Research b1036 (agent-derived;
    main checked the class, the row keys, the field values of the default / whole-body rows and the ctor's hash
    stores in the recomp text): the only C++ subscriber, PresCACMan (82539F38 -> 8253B7F0 -> 82531868), snaps the
    camera with no blend to the row of Attrib class 0x0F21DD698DF85813 with that key (47 rows in
    skatercollections.vlt, already in our stock export): eye (field 0xF323B5D55CA55A89) and look-at
    (0xF0FC2B540F11109E) in stage space plus a stage offset, up (0x513070D9CBBE801A, optional), FOV
    (0xEFB7F7EF35801C19; whole body 35, hats 45, tops 42). The offset is (0, 200, 0) by the b585 trace (eye y 200.48
    = the freeskate row's 0.48 + 200) [inferred; the .bss offset's init value 820D063C not dumped]. The ctor builds
    each hash with lis / ori / rldimi and stores it with std at the record's +0x20 / +0x28.
- **Change (picks).** Setup: `cas-parts.json` v2 adds IsUsedInGame, IsDefaultWearStyle, StyleSubcategory.
  `cas_host`: hidden parts left out, one part per wear style group, the pick / Remove / menu 30 flow above, Left /
  Right on menu 30's Style row cycles the wear styles, A there returns to Merchandise [inferred: help kind
  "Accept"; 825FE3C0's menu 30 branch not read], the info panel natives, thumbnail names lowercased (the disc's
  names). The look: `cas_host::sync_look` brings the customizer's draft in on open (the gender filter follows its
  gender), resolves every pick through the parts library into the draft the skater wears, and Yes saves it
  (`Customiser::save_look`, settings/character.json). A removed slot the look cannot do without gets the body's
  default part. The skater stands on the Create-a-Skater set (`marquee_preview` with `own`: the player's own
  skater on FE 62's stage and pose). Camera: `menu_tables` extracts each option's camera / body hash (`camera`,
  `body`; TU3 = disc), `cas_host::load_cameras` reads the 47 rows from the stock collections (inheritance
  resolved), the host posts the whole-body target on the top / Merchandise menus and the option's target on A, and
  the stage camera takes that row (eye, look-at, up, FOV over `MarqueeStage::origin`). Old tables without the
  targets force a Refresh Game Files (asset check "menu tables lack the Create-a-Skater camera targets").
  The grid's title is its category's text (CAC_GetTitleText on menu 10 = S+0x1788; the PS3 recording titles the
  hat grid "Hats"). The logo: the record of class 0x548B58B0C27C9564 named like the brand, its image path field
  0xFF4850A86FD9E4C9 (`fe\source\images\sponsor_logos\<brand>`; "no_brand" -> the skate logo) [inferred: which of
  the record's fields 825FFA38 returns]. Left / Right on the grid go to the movie (its cursor) [inferred].
- **Checked in game** (`career-boot-run.ps1 -From 5` with SKATE_RETAIL_CAS=1, the dev install's saved outfit is
  female): Merchandise -> Hats lists 128 entries (Remove + 127 female hats); Right then A puts the trucker cap on
  (the skater rebinds, the cap shows on the set) and opens Wear Styles (the cap has styles); A returns to
  Merchandise; B, B opens the exit confirmation, Up + A = Yes saves the look ("look saved"), FE 15 closes and the
  boot flow plays coach_frank_intro (runs cas11..cas15). The skater stands on the concrete-wall set with the
  board; entering Hats snaps the camera to the head close-up, which matches the PS3 recording's hat framing.
  The info box shows the part name ("Trademark Mesh").
- **Highlight preview** (2026-11-18; main's own Ghidra reads `.local/research/ghidra/cas-hilight*.txt`, recomp
  `skate3_recomp.21.cpp` 49120-49160 / 56088-56182 / 60061):
  - The CAS OnMenuHilight [code, high]: CAS vtable 0x82303940 slot 0x44 (word at 82303984) = 825FEDB8. On menu 10
    it stores the highlighted grid record (S+0x17A8 = list[row]), calls 82604A78 (formats the thumbnail name; nothing
    for us) unless it is the Remove entry, sets S+0x1C44 = 0 and S+0x1C40 = 1. Menu 16 has its own branch (not
    ported).
  - The tick 825FBAE8 [code, high]: on menu 10 with S+0x1C40 set it adds the frame time to S+0x1C44; past 500 ms
    the Remove entry calls 82537268 (the skater's pending request back to the committed look), a part 825FEEA0, and
    the flag clears. 825FEEA0 collects the part's wear styles and checks the slot's worn part: worn already ->
    82537268, else guess_HudCASEditor_NewAssetRequest(db, record) (the part on the skater as a request, the look record
    unchanged).
  - Leaving the grid [code, high]: the menu pop 826008C0 (from Back 825FC768 and OnMenuSelect 825FE3C0) calls
    82536778 (camera) and 82537268 when the menu it leaves is 10. A pick 825FD9F0 commits (CasCtrl_ApplyAsset).
  - Change: `cas_host` answers `Game.OnMenuHilight(menu, row, text)` on the grid (`hilight`: row kept, wait
    restarted), `tick` (from `cas_look`, per frame) puts the look with the highlighted part into `preview` after
    `preview_delay` (0.5 s, a field a mod can change) or ends the preview for Remove / a worn part, `enter` of any
    other menu and every committed change (`set_slot`) end it, and `sync_look` resolves `preview` into the draft the
    skater wears (the committed look again when it ends; a save always writes the committed look). Test
    `a_rested_highlight_previews_its_part_and_leaving_the_grid_takes_it_off`.
- **Brand logos** (2026-11-18): the images are in fedynamic.big under `data/fe/source/images/sponsor_logos/`
  (96 single-texture .rx2, one per brand plus `default`; an earlier note here said the disc had none, which was
  wrong). `prepare_menu_movies.py` exports that folder as named images with the CAS thumbnails
  (IMAGE_PREFIXES_DYNAMIC), so CAC_GetSponsorLogoFileName's `fe\source\images\sponsor_logos\<brand>` draws. An
  install exported before this keeps an empty box: the asset check offers Refresh Game Files ("Create-a-Skater brand
  logos", not forced; test `the_splash_names_what_stays_off_until_a_refresh`). Dev install re-exported with
  `.local/tools/menus-reexport.py` (with the DLC packages; diff: 96 images added, nothing removed or changed).
  Checked in game (run caslogo1): the VOX logo next to "Trademark Mesh".
- **Turntable** (2026-11-18; Ghidra `.local/research/ghidra/cas-rotate*.txt`, research b1036 section 4):
  - Input [code, high]: the CAS tick 825FBAE8 (only while no screen is pushed over FE 15 and no part load is
    pending) evaluates two input queries every frame; a value at or past 0.02 (821E9580) posts
    cMsgPresCASCharacterRotate(+value) for one and (-value) for the other, relative (msg+0x10 = 0). Which query is
    LT and which RT is [inferred]: ours turns positive with RT.
  - Handler 8253B618 [code, high]: angle (CAS animation manager +0x64) += rate (+0x60) * value; the angle is added to
    the character's yaw (825325D8), the camera stays. Every cMsgPresCASCameraTarget is followed by
    CharacterRotate(0, absolute): a new camera target resets the angle.
  - The rate [code + data]: 82532800 (CreateAnimator, run for each body target) copies the body target row's float
    at record +0x24 into +0x60 and the float at +0x28 times pi/180 (8206D110 = 0x3C8EFA35) into the base yaw +0x40.
    Class 0x95E142284549717F has two floats: 0.1 in all 69 rows (field 0x165367CDE7FF6727) and 260 in 61 rows (field
    0x4591287983616615; 84 / 189 / 335 / 149 / 348 in the others). The 0.1 is the rate and the 260 the base yaw
    [inferred from the pi/180 scale: a 0.1 degree yaw or a 260 rad-per-frame rate make no sense; the record offsets
    of the two fields are not read]. So a full trigger turns the skater 0.1 rad per FE frame, 3 rad/s at the
    console's 30 Hz.
  - Change: `CasHost::rotate` (rate a field, 0.1; frames counted at 30 Hz from the frame time, so the speed is the
    same at any frame rate), `post_camera` resets the angle, `cas_look` feeds the pad's triggers while FE 15 is up
    with no popup, `MarqueeView::turntable` turns the character on the set. Test aid: `SKATE3_RETAIL_MENUS_KEYS`
    takes `lt` / `rt` / `rtxN` (a held trigger for N frames). Test
    `the_triggers_turn_the_skater_frame_rate_independent_and_a_new_camera_resets_it`. NOT RETAIL YET: the base yaw
    and pose per body target (the next step).
- **Body targets** (2026-11-18): cMsgPresCASBodyTarget (8253B7B0) -> 82532800 CreateAnimator opens the row of class
  0x95E142284549717F named by the option's +0x28 key (the top and Merchandise menus post the whole body
  0xAD454A0ECD0E2EDF with their camera, 82600AF8 lines 190-239) and sets the character from it [code]: the clip name
  (field 0x249D0D4624998D68, record +0x20), the yaw (0x4591287983616615 in degrees, +0x28) and the place relative to
  the stage origin (0x6D2BEA1EFEA33425, +0x10). The options we have post four rows [data]: whole body (hats, shirts,
  pants, shoes ...: CAS_Stand, 260 degrees), wheels 335.13, rings 189.09, wrist (`wrist`, key 0x91098B2B2461D2DC)
  149.25; all at (-1, 0, -7.05) = our FE 62 stage place (and 260 = its -100 degree heading), so only the yaw and the
  clip change.
  The bank (2026-11-22) [code]: the CAS animator (82532408) takes its bank from the CAS record's gender entry, and
  PresCACRecord_LoadGenderAnimBank (82532E58) loads `cac_edit_male` or `cac_edit_female` into it. That bank holds
  CAS_STAND, CAS_STAND_BUFF / SLOUCH / STIFF and CAS_SINGLE_POSE_1..8 [data; its CAS_STAND is a different, longer
  clip than cac_viewer's]. The posture [code]: cMsgPresCASBodyTarget carries a `cac_posture` key (+0x18; 8253B7B0
  keeps it at +0x98), and 82532800 reads that row (class 0xDE50DFA8296749A7). A body row whose play-once Bool
  (0x9C04CB22EB98D1CC; record +0x2C [inferred: that branch queues the row the way a play-once gesture runs]) is clear plays the posture's stand clip (0xD32BD93ACFF3E542) when it has one, its
  own clip otherwise. Posture rows [data]: index field 0xC356DF187557B552 -1 normal CAS_Stand, 0 stiff
  CAS_Stand_Stiff, 1 slouch CAS_Stand_Slouch, 2 buff CAS_Stand_Buff. The index is the look's posture - 1 (Default,
  Straight, Hunched, Macho) [inferred from the label order].
  Change: `cas_host::load_vault` reads the posture rows (`CasHost::posture_stands`), `CasHost::pose` swaps in the
  posture's stand for a row that does not play once, and `marquee_preview::body_bank` plays CAS_Stand* and
  CAS_Single_Pose_* from `cac_edit_<gender>.abin`. Tests `a_stand_row_plays_the_postures_stand_and_a_gesture_row_its_own_clip`,
  `body_rows_play_from_the_gender_s_cas_or_gesture_bank`, the vault test (posture rows) on the installed data. Seen in
  game (desktop run casstand1, 2026-11-22, Merchandise): CAS_Stand from cac_edit_female (a 16.7 s cycle) stands
  holding the board as in the PS3 recording (cac_viewer's CAS_STAND, run casbody5, stood without it).
  Change: `cas_host::load_vault` reads the 69 rows (inheritance resolved), `CasHost::body` is posted with the camera
  (option +0x28; whole body on the top / Merchandise menus), `MarqueeView::body` gives the stage the clip, yaw and
  place, `marquee_preview` reloads the clip when its bank / clip change. Tests
  `an_options_body_target_poses_the_skater_and_merchandise_puts_the_whole_body_back`, the vault test (69 rows).
  NOT RETAIL YET: the rows' other fields (the vector at +0x00, the Float 0.1 (field 0x1653...) at +0x24, the Bool
  0x760AE32CF50D24CB). The Bool 0x9C04CB22EB98D1CC is set on the 46 gesture rows only: play once, own clip (see
  "Gesture preview").
- **Locks** (2026-11-18; Ghidra `.local/research/ghidra/cas-lock*.txt`) [code, high]: 825FD9F0's lock test is the
  predicate object with vtable 823046E0, test 825F46C8: a part with an Unlock name (record +0xF0) or a SponsorUnlock
  name (+0xDC) is locked unless the unlock manager has the Unlock under kind 5 (clothing unlock) or the SponsorUnlock
  under kind 6 (clothing sponsor); a part with neither is open. A locked pick plays sound 0x47F720222F2CAED4 and stays
  on the grid; only when entry mode S+0x1780 is not 1, the part's DLC numbers parse (825F4500 on record +0x64) and
  825F45B0 does not count it as owned does it open the purchase popup 0x11E. 825FF5E8 returns "owned" before
  "locked" (82202428) before "dlc" before "none". Change: `CasHost::locked` (the career's `Unlocks`, copied in by
  `cas_look` while the screen is open; with the career's gating off, the default, nothing is locked), "locked" in
  the option / info types, a locked pick stays on the grid. The highlight preview still shows a locked part
  (825FEEA0 has no lock test). Test `a_part_with_an_ungranted_unlock_is_locked_and_cannot_be_picked`. The error
  sound plays by its key (core_error, 0x47F720222F2CAED4). The DLC purchase popup 0x11E opens only for a part whose
  DlcID parses to numbers; the export (refresh 2026-11-21, with the DLC installed) has no part with a DlcID (all 4595
  empty), so it can never open, in retail either, on these parts; a mod part that sets one shows the "dlc"
  type and is picked normally (no popup, no store).
- **Fix: the world stops behind Create-a-Skater** (2026-11-19): the skater on the CAS set jumped around (seen while
  the boot flow's first Create-a-Skater ran; worse the deeper into the menus). Measured with `SKATE_TRACE_MARQUEE=1`
  (marquee_preview.rs: the rig root, one end bone and one mid bone per frame, and how many bound bones another system
  rewrote between two frames; summary tool `.local/poc-tests/marquee-trace.py`) and from a capture split into frames
  (`.local/poc-tests/track-dark-blob.py`): with the world running, gameplay animation and the fixed-step world posed the
  same player rig every frame (about 360 of 385 bones, at the player's spot in DownTown 300 m away), peds kept streaming
  in (skinned meshes 107 -> 207, each change also rebinds the stage), and the posed skater jumped 5 to 24 cm out and
  back 34 times in 41 s (320 px pops in the capture). Cause in our code: FE 15 never suspended gameplay, and the name
  entry (FE 46) closing in the frame FE 15 opens sent "resume". Change (`retail_menu_stage.rs`): opening FE 15 suspends
  gameplay (the PC overlay's `fe_suspended`, virtual time paused) and keeps it suspended while open (a resume from a
  screen under it only changes what FE 15's close restores); closing restores the state from before; the CAS look /
  preview / turntable system runs on real time. After: rig 0.00 m over 41 s, skinned mesh count constant, no jumps over
  5 cm on the end or mid bone, the idle sway plays (mid bone about 1 mm per frame), on Merchandise, Body Shape and
  Brow; the user watched the runs: smooth, sways in place like retail. Retail parity: retail's front end has no world
  running behind FE 15 [inferred from the boot flow; the world-tick gating code is not read]. Open: small frame-time
  hitches remain (the head material swap: 15 hitches, worst 46 ms on Left / Right in run casbrow2).
- **Fix: the list pickers' rows are enabled** (2026-11-18): the CAS screen's OnMenuGetItemEnabled (vtable slot 0x48,
  word at 82303988) is 8281DD70, which returns 1 [code]. Our host did not answer the call, so the Merchandise /
  Accessories / Skateboards lists (panel 0, the movie's menu picker) showed no highlight and ignored Up / Down; only
  their first row (Hats) could be opened (seen while scripting a run to Accessories > Wrist; the "no row highlighted"
  note below). Now answered `true`; test `every_picker_row_is_enabled`.
- **Facial hair and eyebrow style selectors** (2026-11-19; Ghidra `.local/research/ghidra/cas-input.txt`,
  `cas-stringvalue.txt`, `cas-style-*.txt`; research b1037 for the record layout, main verified its name table and
  the head match function):
  - Left / Right on Facial Hair Style (option 67, menu 21) and Eyebrow Style (80, menu 24) [code, high]: the CAS
    input 825FC7E8 steps the menu's part list (S+0x1798) with wraparound and applies the part. The list is built on
    entering the menu (82600AF8) with the screen's own filter objects (ctor 825F4B98: S+0xCDC "Facial Hair Style
    Filter" 82304560, S+0xF4C "Eyebrow Style Filter" 82304548), not the option's action object. Their tests (825F3F38,
    825F3DA8) keep the head parts that equal the worn head in Gender, SkinTone, HairlineShadowStyle,
    HairlineShadowColour and the other style; the eyebrow list is sorted, and drops "style7" (82302284) when the
    screen's gender check returns 0 [inferred: male].
  - Data [data]: the head looks are the head model's materials (male "Head" 470: skin tone, hairline shadow, facial
    hair and eyebrow combinations; female "Thin Head" 36, no facial hair), so the selectors step materials, read by
    flag NAME (a mod's materials join by their flags).
  - The row's value (825FFE08) [code]: `ID_CAC_FACIAL_HAIR_<style>` / `ID_CAC_EYEBROW_<style>` (the flag value upper
    case, spaces to underscores, e.g. "full beard" -> ID_CAC_FACIAL_HAIR_FULL_BEARD "Full Beard"), and the retail
    error texts `#FacialHairStyleNotTaggedError` / `#BrowStyleNotTaggedError` for an untagged look.
  - Change: `cas_host` fills `heads` from the parts library (`head_looks`), `build_style_list` on entering menu 21 /
    24, `step_style` on Left / Right (puts the material on; facial hair also sets the look's `beard`, which
    `customiser_parts::resolve` matches the head material to), `style_text` for CAC_GetStringValue. Test
    `the_facial_hair_and_eyebrow_selectors_step_head_looks_that_keep_the_other_fields`. NOT RETAIL YET: the Left /
    Right sounds (0xAD8BF9A0B4997B49 / 0xD8EE38A61C45EA28), the hair style (65, its filter's stored index is open),
    skin tone (63) and the Edit Style rows.
- **Color rows** (2026-11-19; research b991, b1038 (main verified the vault side), b1040 (TU3 shaders)):
  - Rows and zones [code, 82603E98]: Skin Color 62 -> skin zone (COLORIZED_SLOT 2), Hair Color 66 -> hair (1),
    Facial Hair Color 68 -> facial hair (8; in no TU3 menu), Eye Color 76 -> eye (6). Each `cac_colour_zone` (class
    0xAE1F950CA0D1ADF2) names its slot, a group (RefSpec) and a default color; the list is the group's `cac_colour`
    rows (class 0x6D934D0E38C95E19) by their order field, ascending [data: hair 10, skin 7, eye 5, clothing 10 red ..
    black; board 10]. Left / Right step the current index (the look's color key for the zone, else 0) clamped, no
    wrap; CAC_GetColours gives the swatches, CAC_GetIntegerValue on a color row the current index [code, b1038].
  - Applying [code b991 + shader b1040]: CAS_ColorPickerApply writes the color's recipe pair (A = `cac_colour`
    0x97462E6ABC78A423, HDR up to 2.69; B = 0xBC3B0D9B2DB64358) into the zone's tint channel (hair also into the
    eyebrow and facial hair channels; skin also into the arm, leg and torso channels). The skater's character
    shaders (cac_cloth, cacstamp_cloth / skin, cac_face, cac_hair) multiply the squared diffuse by one color
    (i_modulateColor1), no ped-style mask; only cac_hair has a second color (i_modulateColor2) on its highlight.
    Which recipe feeds which constant, and that it is uploaded unconverted, is [inferred]: the upload code was not
    found (the constant names live only in the shaders' constant tables), and the ped port (doc 26f "Ped colorize")
    uses its data tints raw in the same squared space.
  - Change: `cas_host` loads the zones with the vault (`load_colour_zones`), handles the color rows, keeps the
    picked key per zone (`colour_keys`) and writes recipe A to `colour_tints`; `customiser_parts::profile_material`
    uses `colour_tints` as a linear, unclamped base color for the zone's parts (skin by the SkinTone flag, hat 0,
    hair 1, top 9, sock 10, bottom 12), so the retail tint path takes it as is. Tests
    `a_color_row_starts_on_the_zones_default_steps_clamped_and_keeps_the_key`, the vault test (zone sizes, the eye
    order). The pick log names each color by its text field, which is the part match text (hair: blonde / medium
    brown / black), not a display name: pink hair logs as "medium brown" [data].
  - Swatch squares [code 825A5F88 / 825A5FE8 / 825A6038, movie `controls/swatch_renderer`]: the color row's
    `highlight_color` builds one `swatch_renderer` per color and passes CAC_GetColours' r, g, b to
    `Game.SwatchRenderer_SetColour(index, r, g, b)`; the host keeps 100 slots (reserve = first free, else -1;
    free clears the flag), floats as is, integers / 255, alpha 1. Ours: `image_renderer_host::ImageSlots`
    swatches, `apt_scene` draws a `SwatchRenderComponent` as `swatch/<n>`, the stage fills it with the slot's
    color. NOT RETAIL YET: integer arguments (our VM has one number type, all taken as is; the TU3 movies pass
    floats). Test `swatch_slots_reserve_color_and_free_like_retail`.
  - Hair highlight [shader b1040, cac_hair_defaultPS]: `lin x (diffuse light x MOD1 + highlight x MOD2)`. The pick's
    recipe B goes to `colour_highlights` and the hair material's `CharacterParams::highlight_tint`; `shade_character`'s hair
    path takes the untinted `lin` and uses `lin x highlight_tint` for its highlight term when set (unset: unchanged). That
    recipe B is MOD2 is [inferred] like recipe A's upload.
  - Skin color: the skin zone's colors carry match text light / dark, and their recipe A is a tone, not the
    darkness (the first dark color's A is 0.56, 0.53, 0.46, lighter than the fourth light one's 0.45, 0.36, 0.29)
    [data]. So a skin pick also sets the look's `skin` to the text, which the parts resolve turns into the light or
    dark SkinTone material of the head, arms and legs, and recipe A tints over it [inferred from that data; the
    retail code that swaps the texture is not read: 82538060 writes tints only, b1038]. Checked in game (the
    darkest color darkens face, arms and legs). Test `a_skin_color_pick_sets_the_skin_tone_by_its_text_and_tints`.
    The skin Tone row (63) is dead UI in retail (b1039), so skin tone is this row.
  - Eye color [code 82541788, b1038 section 3]: the eye zone tints nothing; the pick's match text selects the eye
    part's material with that `EyeColour` flag (our data: slot `Organ`, one eye model per gender with five
    materials). Ours: `cas_host` keeps the eye models (`eye_models`) and wears the worn (else the gender's) eye
    model with the matching material. Test `an_eye_color_pick_wears_the_eye_material_with_that_color_and_no_tint`.
    An eye or skin pick sets the face preset row to Custom (see "Face presets" below).
  - Hairline, stubble and brows follow the hair color (2026-11-19): every hair color's match text is one of the
    three `HairlineShadowColour` values the male head materials come in (blonde; black for black, gray and medium
    brown; medium brown for light brown, red and the dye colors) [data, vault + library]. A hair pick sets the look's
    `hairline` to that text and the parts resolve prefers the head material with that hairline color [inferred from
    that data, the same reading as skin; retail's apply copies the hair tint to the eyebrow / facial hair channels
    7 / 8, which no exported material reads]. Tests `a_hair_color_pick_sets_the_hairline_by_its_text`, the
    data-gated `owned_library_choices_resolve_without_conversion` (every hairline color resolves to a matching head).
  - Wear style color, row 98 (2026-11-19) [code 82603E98 / 825416A8, b1038]: on menu 30 the row's zone is the edited
    part's slot (hat 0, top 9, sock 10, bottom 12, board 11). Ours: the part whose wear styles the menu shows, and
    only while its material is IsColourizable (48 materials: plain tees, jackets, hoodies, socks, two pants, two
    hats, three hair models and the 20 plain decks); clothing tints go on colorizable materials only, so a printed
    shirt keeps its print [inferred: retail reads the part type only]. Not in the PS3 recording (it never opens a
    wear style page). Test `the_wear_style_color_row_follows_the_part_and_a_board_pick_swaps_the_deck`.
  - Board color [code 82DDC160 / 82DDEDB0, b1038 section 3]: the pick's match text selects the worn deck's material
    with that `DeckColour` (the plain deck has a clean and a worn material per color); ours keeps the same one of the
    pair and writes no tint. Whether retail also tints the deck through channel 11 is open.
  - NOT RETAIL YET: the eyebrow / facial hair channels a hair pick also writes (our heads bake brows and facial
    hair into the head material, whose `HairlineShadowColour` matches the hair color's text; that a hair pick
    swaps the head material by it is NOT proven, b1038), the board part swap by the color's text, the clothing
    color row (98 on the wear style menu), the sounds.
- **Tattoos** (2026-11-19; research b1042 (agent, from the recomp's asm; Ghidra not run), main checked the menu tree
  in the extracted table and the category data in the vault export, which corrected the agent's list field hash to
  0xFFDEC982A33E9230 and showed 0x49F0AF4276FBBAD3 is a Bool, not the design list):
  - Flow [code 825FE3C0, b1042]: Body Mods' Upper / Lower Body Tattoo (options 19 / 20) push the region menus 4
    (27 front, 28 back, 29 left arm, 30 right arm) / 5 (31 left leg, 32 right leg); a region pushes the categories
    (6), a category the design list (11, panel 6), a design the placement editor (7 / 8); "none" clears the
    location and pops to Body Mods. The region rows have no target: the pushes are C++.
  - Data [data]: `cac_tattoo_categories/default` (class 0xD38EA152CCFDEE0D) lists three `cac_tattoo_category`
    records (class 0x345C8861CFA6BCA5) whose text field 0xC283F7484D8204B6 is '1' / '2' / '3', the value of the
    designs' cas.TattooCategory (66 / 44 / 60 designs). Retail keeps one upper and one lower tattoo (stamp tail
    locations 1 / 2).
  - Change: the library export keeps each tattoo's `category` (customisation_library.py; installs set up before
    it get the field from `.local/tools/add-tattoo-category.py` or the next Refresh Game Files); `cas_host` loads
    the category list with the vault, fills menu 6's rows from it, lists a category's designs with "none" last,
    and a pick writes the look's `tattoos.Arm` / `tattoos.Leg` with our stamp `side` for the region.
    Test `a_tattoo_goes_region_category_design_and_none_removes_it`, the vault test (categories).
  - Placement [code + data, research b1043 (agent) and main's dumps]: a placement is (rotation, scale k, u, v).
    82C2E3A8 builds M = T(u, v) T(-0.5, -0.5) R(rot) S(k, 2k) T(0.5, 0.5) on row vectors, graphic_uv = body_uv * M:
    the graphic centered at body (0.5 - u, 0.5 - v), 1 / k wide and 1 / (2k) high, never stretched to the region;
    the stamp shaders take M's columns as i_customGraphic (c190 / c191). A design pick posts the region's default
    placement (table 0x830442B8 by the region's row: upper 1 / 5 / 7 / 10, lower 8 / 11 from 0x83044468 /
    0x83044498; e.g. arms k 7 at u +-0.25, v -0.15) and pushes the editor (7 / 8).
  - Editor [code 825FBAE8 / 82536C40 / 82C2E698 / 82C2EBD8]: per FE frame each input past 0.01 adjusts one value:
    rotate 0.01, scale -0.1 (capped at 15), u -0.005, v +0.005 (which pad input drives which is [inferred] from the
    legend: bumpers rotate, left stick scales, right stick moves). Then, when the design's bounds corners (its
    cas.StampBorderConstraint) leave the region's constraint rect (the worn part's StampUVConstraintQ1..Q4 by the
    row: x [left, right], y [1 - top, 1 - bottom]; missing: the row's UV quadrant), u and then v are pushed back by
    the first corner's overshoot + 0.001 (not when it sticks out on both sides), and k grows by 1.001 until it fits
    (ours caps that loop at 20,000 steps). A returns to Body Mods, key 4 resets to the default placement.
  - Change: `tattoos.<slot>.placement` in the look (validated finite, k > 0); `customiser_material::placement_rows`
    gives the shader the two rows and `customiser_stamp.wgsl` samples the design at M (uv_b) where that is inside
    0..1 [inferred: what retail draws outside the graphic is not read]; stamps without a placement keep the old
    fit. `cas_host::adjust_graphic` / `tick_tattoo` port the editor. Tests
    `the_placement_matrix_and_the_fit_follow_retail`, the tattoo flow test (default placement, reset, A).
  - Region cameras [data 0x83044548 / 0x83044568 camera rows, 0x830445E8 / 0x83044608 body rows]: a region pick
    posts the region's camera and pose and resets the turntable (CharacterRotate(1, 0)); upper front / back / left
    arm / right arm 94ADE31AD9BCCBAB / 99D608AFAD331C62 / BB4783E9DB0E4647 / 387F458990473B6B, legs C8946ECFD41E5776
    / B115E4069E2C5B1E (all in the vault); the front body row 4A663E264EABE260 is not in the TU3 vault, so that
    region keeps the whole-body pose. Checked in game (the right arm view shows the default placement).
  - Clothing [code 825FB718 / 825FB8C8, data, the PS3 recording 2026-11-16 11-13-28.mp4 ~4:25 / ~5:30]: a region
    pick keeps the worn shirt (upper) or pants (lower) and puts on the slot's lookup part 0; X (key 5) toggles,
    A and the "none" pick put the kept part back. Lookup part 0 in our data: upper, the gender's skin torso
    ("Arms Wholetorso", whose female texture is the sports bra the recording shows) in the look's skin tone;
    lower, the gray Retro Shorts ("Retroshorts Grey", as recorded) [inferred: matched by name from the
    recording, the lookup's own order is not read]. Checked in game (front torso, left leg).
  - Design grid [data, the PS3 recording ~4:10 / ~4:35]: each design's thumbnail is `fe\cas\logos\<model>_<variant>`
    (all 170 TU3 designs are among the disc's logos images; the library export keeps `thumbnail`); no check marks
    except on the location's current design ("owned", like the part grid); the remove entry comes first and only
    when the location already holds a tattoo (the first visit lists designs only; b1042 read "appended last" from
    the code, which the recording contradicts). Checked in game (the Tribal grid).
  - NOT RETAIL YET: the skin torso's body-shape material (thin / average / chubby: ours takes the first of the
    tone), the replace popup, the two designs the recording shows after the remove entry on a later visit (a skull
    and a red swirl; their source is not read), the design order (ours: id order; retail's
    8253E3D8 order not read), the shirt / board / hat graphics (34..36) that share the editor. Keys 4 / 5 are
    AptOption (Y) / AptMisc (X) in the menu tables, so Y resets and X toggles the clothing, as the retail legend
    reads (b1042's conflict came from a swapped key map).
- **Face and body sliders (morph rows)** (2026-11-19; research b1041 (agent), main verified the step field hash
  0x2D4497B70742FD97 and the 8253CD18 call in the recomp's 825FBAE8, the constants 122.0 (822F95E4), 0.5
  (8209975C) and 2.0 (82060C50) in Ghidra, and the option hashes through the menu table data test):
  - Rows: option kind "morph" (64 body shape, 77..94 eyes / brows / nose / jaw / mouth / chin); the option's +0x18
    is the vault key hash of its `cac_morph_params` row (class 0x7F47CA62F4100F41: target name, target_order,
    min / max / default; 19 rows, face 0..0.5 default 0.25, fat / thin 0..0.5 default 0) [code 8253C9F8, data].
  - Left / Right [code 825FBAE8, the CAS screen tick, not 825FC7E8 or the movie]: every FE frame the stick is
    past the threshold the shown value (the morph normalized over its range, 8253CD18) moves one step, clamped
    0..1, no repeat delay; threshold 0.1 and step 0.01 from `cac_fe_morph_settings/default` (class
    0xBB852B438BF578ED) [data]. The value is written back raw over the range into the look (82536B10), then the
    movie refreshes (SetState 50). CAC_GetIntegerValue = floor(n x 122 + 0.5).
  - Body shape (64) is one slider over fat and thin: shown (1 - thin) / 2 + fat / 2; at or above the middle
    fat = (v - 0.5) x 2, thin 0, below it thin = 1 - 2v, fat 0 [code 825FB490 / 825FB5B8].
  - Change: the menu table extractor keeps the option's +0x18 (`CasOptionRow::morph`; a menu table re-export puts
    it into the install); `cas_host` loads the morph rows and settings with the vault (`load_morphs`),
    `tick_morph` steps the highlighted morph row from the held left stick / d-pad (825FBAE8's two input queries:
    which pad inputs they are is [inferred]) at the 30 Hz FE cadence like the turntable (retail's FE tick rate is
    [inferred]), and the look's `morphs.<target>` drive the existing morph apply. Scripted runs hold the stick with
    `sl` / `sr` (`slxN` / `srxN`) keys. Tests `morph_rows_step_per_frame_clamped_and_the_body_row_splits_fat_and_thin`,
    the vault test (19 morphs, eye width, settings), the menu table data test (option 64 / 78 hashes).
  - A face row edit sets the face preset row to Custom (825FBAE8 after the morph post; the body row does not).
  - The move sound (2026-11-20) [code 825FBAE8, 825DFAF0, 82495828; data]: while the stick moves a morph row and
    the value is inside its range, 825FBAE8 starts the `fe` record cas_slider (0x119E9CE644D5A9E1, flag S+0x1A81)
    and stops it at either end or when the stick is let go. The disc's cas_slider (and cas_slider_end,
    hom_score_scroll) has level 0, and the front-end queue 82495828 drops a request whose level is not above 0, so
    the sound is silent in TU3 as read; ours does not play it. Undo: see "Undo Changes" below.
- **Face presets** (2026-11-19; Ghidra 825FC7E8 option 95, 82604208, 82539048, 825FBAE8; recomp asm of 82538E78's
  callers; the PS3 recording at 3:32 shows "< Custom >" on entering Facial Presets):
  - [data] Class 0x3269B4F0F5E38E09 (b1038): the Custom row (index 0, ID_CAC_PRESET_CUSTOM) and ten rows per gender
    under `male` / `female` (bool field `male`), each with a text id (ID_CAC_PRESET_01..10), an index 1..10, a color
    list of two (eye zone 0x1A5E3EA9A0A82591 = slot 6, skin zone 0x3AA5050BEDEFDDFA = slot 2, each with a
    `cac_colour` key) and a morph list of all 19 morph rows with a value 0..1.
  - [code] Left / Right (825FC7E8): the largest index among the skater's gender's rows is the last; the pick's index
    +/- 1; below 0 goes to the last, 0 or past the last is Custom. A preset (82604208) posts each morph's value through
    82536B10, which takes the 0..1 value (the morph rows' own edit posts its normalized value the same way), skipping
    fat and thin, then applies its colors in one CAS_ColorPickerApply. Custom restores the look from the copy at
    +0x5DF0 (82539048: CharacterSlot_Deserialize). 82538E78 writes that copy in CASScreen_EnterMenu (82600AF8) when
    the shop mode changes and on every menu entry while the mode is Body Mods (1), so Custom goes back to the look
    as it was on entering Facial Presets. The row shows the pick's text field (825FFE08).
  - Ours: `load_face_presets` (vault), `step_face_preset`, `enter` keeps the copy (`look_copy`) by the same rule; a
    preset sets `morphs.<target>` and picks its colors like the color rows (skin texture, eye material). Unset reads as Custom [video; the ctor leaves the pick empty]. Test
    `the_face_preset_row_steps_the_gender_s_presets_through_custom` and the vault test (21 rows, male last 10, eye /
    skin color keys in their zones, 19 morphs 0..1).
  - Sounds: core_nav_left / core_nav_right on each step (see "Sounds" below, 2026-11-20).
- **Base Outfit opt-in** (2026-11-20, user ask 713; ours, not retail): with the living world opt-in "Base Outfit"
  (`SKATE_BASE_OUTFIT=1`, or the opt-ins page), Remove on a top (OuterTorso) or pants in the Merchandise grids leaves
  the base part on instead of an empty slot: the bare torso in the look's skin tone (the sports bra on a female
  skater) or the short shorts, the same parts the tattoo editors wear (`cas_host` `base_part`). Off, Remove clears the
  slot as before. Test `with_the_base_outfit_opt_in_remove_leaves_the_sports_bra_and_shorts`.
- **Custom graphics** (2026-11-20; research b1051 from Ghidra 82600AF8 / 825FE3C0 / 825FD9F0 / 825FAAF0 / 8253E228,
  main checked the menu 30 rows in cas-enter.txt 1553..1655) [code, data]:
  - Menu 30 (Edit Clothing) builds its rows on entry: Color (98) when the worn style's material is IsColourizable,
    Style (99) unless the part is a board and only with more than one style, Add Graphic when the material is
    IsLogoable (100 for a shirt, 103 for a board or hat), then Remove Graphic (104), always.
  - 100 pushes the shirt's regions (menu 33: front, back, left arm, right arm, rows 111..114); a region keeps the
    region, posts its camera (0x83044528 per region) and body target, and pushes the categories (menu 12). 103 posts
    the option's camera and body targets and pushes the categories. Brand (101) lists the `logo_*` logos (182), Team
    (102) the `team_logo_*` ones (42; retail adds up to 4 downloaded user logos when online), both in the asset
    database's order, no "none" entry; `tattoo_*` and `team_brand_logo_*` are never listed.
  - A pick puts the logo on the part at its default placement (0x830442B8: the shirt's rows 0 / 4 / 6 / 9 by region,
    the board's 2, the hat's 3) and opens the part's Graphic Adjust (34 shirt, 35 board, 36 hat): the tattoo
    editor's tick, sticks and fit; A pops to Merchandise (core_a_button), key 4 resets, no clothing key. The board's
    and hat's editors post their cameras on entry. Remove (104) clears only that part's graphic.
  - Ours: `customisation_library.py` exports `logos` (name, decal texture, stamp border, kind, database order, thumbnail);
    `customiser_parts` loads their images with the tattoos' and stamps a profile's `graphics.<OuterTorso | SkateBoard |
    Hat> {id, region, placement}` onto the part's IsLogoable material through the tattoo stamp path (the shirt's
    region picks the constraint quadrant like the upper tattoo regions [inferred]); `cas_host` `build_wear_style_rows`,
    `graphic_select`, `pick_graphic`, the grids 13 / 14, the editors in `tattoo_editor_key` / `tick_tattoo`. Data
    a mod can replace: the logo lists, IsLogoable, the default rows and cameras. Needs a Refresh Game Files (the logos
    export). Test `custom_graphics_go_rows_category_logo_editor_and_remove`.
  - The online user logos: retail adds the downloaded ones only while online, so an offline game lists none, as
    ours does (the engine has no online service).
  - The hat's body target (0x83044648, dumped 2026-11-21) is key 0x4A663E264EABE260, the board's row; the hat
    editor now posts it with its camera.
  - NOT RETAIL YET / open: whether retail draws the graphic through
    the same shader path (not traced); not checked in game yet.
- **Sounds** (2026-11-20) [code 825FC7E8, 825FD9F0; data: the `fe` records by key]: every row the Left / Right
  handler takes (sliders, selectors, Edit Style rows, gesture slots, color rows, face presets, wear style) plays
  core_nav_left (0xAD8BF9A0B4997B49) or core_nav_right (0xD8EE38A61C45EA28) at its common exit, also when a color
  is already at its end, then refreshes the row (state 50). A on the graphic editors (menus 7 / 8; 34..36 with
  their port) plays core_a_button (0x373D38D7EA167A71); a thumbnail pick on menu 10 plays core_a_button, or
  core_error (0x47F720222F2CAED4) on a locked part. Ours: `cas_host` queues the keys in `sounds`, the stage sends
  them as `FrontendSound` (the install's levels; a mod can change a record). Test
  `a_color_row_starts_on_the_zones_default_steps_clamped_and_keeps_the_key` (the clamped step's sound).
- **Undo Changes** (2026-11-19; Ghidra 825FE3C0, 82538F60, 82542D38): option 96 (Skin, Hair, Facial Hair, Eyes,
  Brow, Nose, Jaw, Mouth, Chin) restores the whole look from the copy above (82539048, the skater reloads); option
  97 (Body Shape only) copies the copy's 19 morph values back into the look, posts each (82538F60 / 82542D38) and
  re-enters the menu, which takes a new copy [code]. Option 96 sets state 5 (S+0x1800); when the reload is done the
  load state handler 825FAAF0 (guess_CASScreen_UpdateLoadState) re-enters the current menu for state 5 (and for 6,
  after the save fade) [code, Ghidra 2026-11-20], so every row redraws from the restored look. Ours: `cas_host`
  `undo` from A on the row, then `reenter` (the face preset row's Custom too, also state 5): the stage sets the
  menu's panel state again (`menu_refresh`). Test `undo_rows_put_back_the_look_from_entering_the_menu`. Checked in
  game 2026-11-20: a dark skin pick, then Undo Changes, brings the light skin back.
- **Hair style selector** (2026-11-19; research b1039 from the recomp, main verified the filter's setter (vtable
  82304518 +0x10 = 825F3C70) and its string 8230224C = "full", and the enter remap's 82202A78 = "full"):
  - [code] The "Hair Style Filter" (S+0xC7C) keeps the parts whose kind-2 index (82DDC998: the HairModelType at record
    +0x88) equals the name index of "full", set by its vt+0x10 before RebuildBrandList (82603480) collects them, so
    the list is one entry per hair style, never the hat variants. Entering menu 20 maps the worn part to its "full"
    model (FindByNameIndex on HairStyle +0x8C, Gender +0x7C, "full"); with the flag at (cas+0x54)+0x2030 set the
    Remove entry (S+0x1A84) goes first. Left / Right (825FC7E8 option 0x41) wraps; Remove puts the bald default on
    (ThumbnailLookup(cas, 2, 1)), any other entry its part. The value text is ID_CAC_HAIRSTYLE_<HairStyle>, Remove
    ID_CAC_HAIRSTYLE_BALD.
  - [data] 56 hair models: male 18 full + 18 hat, female 10 + 10, one material each; the hair color is the separate
    color row 66 (b1038), so a style pick keeps the color.
  - Change: `cas_host` reads HairModelType / HairStyle from the parts library (`hair_models`), `build_hair_list` on
    entering menu 20 (the gender's full parts in `cas-parts.json` order; Remove first for a male skater [inferred:
    the 0x2030 flag read as "male", as for the eyebrow filter]), `step_hair`, `hair_text`. Test
    `the_hair_style_selector_steps_full_hair_of_the_gender_with_bald_first_for_a_male_skater`. NOT RETAIL YET: bald
    clears the hair slot instead of putting retail's bald default part on; RebuildBrandList's sort key is not read
    (database order kept).
  - Skin Tone (option 63) is in no TU3 menu (menu 18 Skin is [62, 96]): its input and text code are dead UI; retail
    skin is the color row 62. Not ported.
- **Edit Style rows and the board sliders** (2026-11-19) [code, 825FC7E8 / 825FFE08]: Style (54) steps 0..3 with
  wraparound, labels ID_CAC_LABEL_STYLE_NORMAL / LOOSE / GONZO / AGGRESSIVE (look +0x2014); Posture (53) 0..3,
  ID_CAC_LABEL_POSTURE_1..4 (+0x2018, also re-posts the body target); Stance (55) toggles +0x2010, 0 =
  ID_CAC_STANCE_GOOFY, else ID_CAC_STANCE_REGULAR. Gesture slots 49..52: entering menu 16 (82600AF8) loads the four
  slots from look +0x201C / +0x2020 / +0x2024 / +0x2028 into S+0x180C / +0x1810 / +0x1814 / +0x1818 (rows 49 / 52 /
  51 / 50) and lists the gesture table's indices 0..36 whose kind 4 unlock `gesture_<name>` is granted, in table
  order; Left / Right step that list with wraparound (an index not in it counts as position 0); the label is the
  ctor's table at S+0x1830 + 16 * index (`ID_GESTURE_AIRGUITAR` .. `ID_GESTURE_YARDSALE`, e.g. 18 =
  ID_GESTURE_PUMP_HI "High Pump", 22 = ID_GESTURE_FINGERPOINT). The 37 names match gameplay's clip table order
  (`graph_host/motion_character_gesture.rs` NAMES). Our profile already has these values and gameplay already reads them
  (`customiser.rs` `apply`: `stance` (default 1) and `style` to the skater animation, `posture` to its posture
  profile, `truck` / `wheel` (default 0.7) to the board physics, `gestures` four of 37 entries). Change: the Edit
  Style rows write `posture` / `style` / `stance` and the Trucks / Wheels sliders `truck` / `wheel` into the look
  (saved on exit Yes), the sliders start from the profile. Test
  `edit_style_rows_write_the_profiles_posture_style_stance_and_the_sliders_its_truck_and_wheel`; the gesture rows
  write the profile's `gestures` "0" / "3" / "2" / "1" (test `gesture_rows_step_the_unlocked_gestures_into_the_profiles_slots`).
  The Left / Right sounds are ported ("Sounds" below) and 0.7 is retail's slider default (0x3F333333 at
  82593988 / 94). Posture's re-post (2026-11-21, recomp asm of 825FC7E8 at 825FD190): after the store at look
  +0x2018 it calls 82539688 with no key, which posts the look's current body target again (+0x10), so the animator
  rebuilds the same row and restarts its clip. Ours: Posture bumps the body post (`body_posts`), so the preview
  restarts the stand (test `edit_style_rows_write_...`: Posture posts once, Style not).
- **Gesture preview** (2026-11-19) [code 82604AD0 recomp asm + ctor 825F4B98 Ghidra, data]: after a gesture slot
  steps, 82604AD0 takes the slot's gesture index, reads the 64-bit key at S+0x1838 + 16 * index (the ctor's table:
  label at +0, key at +8, e.g. AIRGUITAR 0xBCACFFAA652FAC9B .. YARDSALE 0xBD81B91A70FB59B1), stores it in the look
  (+0x10) and posts it (82539688). Every key is a body target row (class 0x95E142284549717F, parent
  0xAD454A0ECD0E2EDF = CAS_Stand) whose clip is `cac_male_<gesture>`; the clips are miscload's
  `data/anim/gesture/cac_<male|female>_<gesture>.abin` (one clip per bank, already in the install with the rest of
  miscload). Ours: `GESTURE_BODIES` (the ctor's 37 keys), the step posts the row as `CasHost::body`;
  `marquee_preview::body_bank` loads the gender's bank (the female bank for a female skater [inferred from the file
  names]) and plays the clip once, then the stand comes back [inferred: the rows' Bool 0x9C04CB22EB98D1CC, set on
  gesture rows only]. The gesture banks are layers [data]: their pose stores only some hierarchy parts (our full
  scene clip parse refuses them, "pose leaves hierarchy parts unset"), and a stored part leaves components out (the
  arm bones' translations), so `SceneClip::parse_layer` starts the layer's pose from the stand clip's first frame
  (bones matched by name) and returns the clip and the bones its stored parts drive, and
  `marquee_preview::layered` puts those bones over the stand clip
  (MARQUEE_STAND, matched by bone name, the stand looping under the gesture's frames). The row
  0xA774F79D33B6BE83 names `cas_gesturestand_cycle`, likely retail's base under gestures; that clip is in no
  installed bank (searched by its FastString name), so the stand is ours [inferred]. `cac_<gender>_axe` has no clip
  and no vault row. The PS3 recording shows the gesture on the skater as the slot changes (Bruce, Airplane).
  Tests `gesture_rows_step_the_unlocked_gestures_into_the_profiles_slots` (the posted row),
  `gesture_rows_play_from_the_gender_s_gesture_bank`, the data-gated `every_gesture_bank_parses_as_a_scene_clip`
  (every bank parses as a layer, shares driven bones with the stand and keeps every bone's length below the root's
  children, the board aside).
  - Fix (2026-11-20, found in the in-game check): the first gesture preview dropped the skater into a heap. The
    layer's pose started from identity, so the components its parts leave out were zero (RIGHTFOREARM lost its 0.3 m
    length, every bank); the asset test now checks bone lengths.
  - Highlight (2026-11-20) [code, Ghidra 825FEDB8 / 82604AD0]: OnMenuHilight on menu 16 calls 82604AD0 with the
    highlighted row's option: a gesture slot posts its gesture's body row, any other row CAS_Stand, then the
    turntable goes back to 0; it also stops cas_slider. So a gesture plays as soon as its slot is highlighted, and
    again on every post of the same row. Ours: `CasHost::hilight` -> `post_edit_style_body` (the step uses it too),
    `body_posts` counts posts so the marquee replays one. Test `highlighting_an_edit_style_row_posts_its_gesture_or_the_stand`.
  - The PS3 recording (9:05 to 9:23) [video]: on the gesture slots the skater stands without the board, faces the
    camera on standing legs and only the upper body moves (Airplane: both arms straight out to the sides); the board
    is back on Stance. The banks are recorded riding [data]: TRAJECTORY a 90 degree yaw from the stand's, the stored
    hips and legs a wide riding stance, the clip moving SPINE and below only. Ours: the marquee hides the board
    (SKATEBOARD_ROOT scaled to 0) while a gesture row is posted, and `layered` takes the gesture from SPINE down
    (`GESTURE_LAYER_ROOT`), the root, hips, legs and board from the stand [inferred from the recording]. Checked in
    game 2026-11-20 (Airplane highlighted, captures at 0.1 to 1.4 s): upright, facing the camera, no board.
  - The arms (fixed 2026-11-20, measured): a gesture clip adds onto the skeleton's bind pose (the stand bank's pose
    record, `SceneClip::parse_bind`), not onto the gesture bank's own stored pose. On the bank's pose the Airplane
    started and ended with both arms up (arm direction y +0.95) and only came out to about 45 degrees; on the bind
    pose it goes down (y -0.95 at 0 s), straight out to the sides (y 0.11 at 1.0 s), down (1.8 s), as in the
    recording. The data-gated `every_gesture_bank_parses_as_a_scene_clip` asserts those three moments. Checked in
    game (Airplane highlighted, captures at 1.23 s and 1.5 s): arms out level, then lowering. The base under the
    gesture is still our stand (`cas_gesturestand_cycle`, row 0xA774F79D33B6BE83, is in no installed bank) with the
    board hidden, and the gesture drives SPINE and below [inferred from the recording].
  After the post it calls PostMsg_PresCASCharacterRotate(1, [0x82165A10] = 0.0): the turntable goes back to 0
  (ours too). The rows that post 0xAD454A0ECD0E2EDF (CAS_Stand) through 82604AD0 are menu 16's other rows on
  highlight (see "Highlight" above).
- **Camera targets of the other menus** [code]: the tables 83044528 / 83044548 / 83044568 / 83044588 / 830445A8
  that 82600AF8 posts from belong to the tattoo / graphic adjust menus (7, 8, 34, 35, 36: indexed by the tattoo's
  location) and menu 11 (board graphics: the camera key comes from a vault record). They come with those features;
  every menu we have already posts its camera (whole body or the option's +0x20).
- **Seen, open:** back on
  Merchandise after a pick no row is highlighted (seen once, run cas11); on the exit popup an A right away answered
  No and Up + A answered Yes (which row retail highlights first is not checked). Tests `a_grid_keeps_one_part_per_wear_style_group_and_hides_in_game_materials`,
  `the_info_panel_names_the_part_its_brand_and_unlock_and_marks_the_worn_one`,
  `a_pick_wears_the_part_and_remove_clears_the_slot`, `a_part_with_wear_styles_opens_menu_30_and_left_right_cycles_them`,
  `a_rested_highlight_previews_its_part_and_leaving_the_grid_takes_it_off`.
- **On by default since 2026-11-20** (after the in-game checks; `SKATE_RETAIL_CAS=0` keeps our customizer). NOT
  RETAIL YET at the time of this section: the lock check and
  the purchase popup 0x11E, the pack / DLC helper filters, the rec+0x24 / +0x34 flags that also open menu 30, the
  board graphic special case, the two "#Bryant" parts, retail's OuterTorso fallback on Remove (825377E0), the logo
  record walk (the logo name follows the disc's file names), menu 30's color row, the camera targets of the menus
  other than top / Merchandise / part categories (tables 83044528 / 83044548: head, face, tattoos), the body target
  (pose) rows, the turntable and the camera row's exposure, the 250 ms close countdown and fade; piercings have no catalog
  data; X / Y, selectors / stance / colors / morphs on Left / Right, 825FE3C0's other menu and leaf branches, the
  slider values' profile storage and defaults (0.5 meanwhile), refresh states 52 / 53.
- **Checked** (`career-boot-run.ps1 -From 5 -Keys "_x150,2,_x90,0"` with SKATE_RETAIL_CAS=1): FE 67 -> name entry
  -> "RETAIL_MENUS open CreateASkater (FE 15)" -> "menu / panel (15, 3)": "Edit Skater" with Body Mods,
  Merchandise, Edit Style, Trucks and Wheels (sliders), "A Select / B Exit", "LT RT Rotate Skater", as in the PS3
  recording's top menu (values "undefined", placeholder info text). Tests: skate-data `menu_tables` (TU3 and disc: 37 menus, Shops, EditSkaterMain's list,
  Slappy's list, BackAlleyPete), `cas_host` (the top menu from the table, sliders without A help, closed host).

## Verification

- `python -m unittest tools.asset_pipeline.test_progression tools.asset_pipeline.test_progression_lua`: OK.
- Local run against the user's disc (counts only, nothing kept): 53 groups, 204 actions, 222 rewards; condition types
  108 one challenge done, 79 group done, 33 board sales, 2 money (matches b855); 179 actions start a graph; only
  `starting_action` keeps its fired flag; 495 challenges carry rewards (`ots_dwtn_01`: 10,000 sales owned, 2,500 killed,
  reward `skatepark_unlock_ots_dwtn_01`). One condition references a challenge outside the banks (`tut_univ_01`,
  cut content per b861). Graphs: all 248 handler chunks translate with no failure; 181 graphs, each starting at
  `initial` and each with `complete`, 386 states, 246 handlers; the two school exits pick from 6 and 4 challenges.

## Open questions

- What sets the career manager's auto-start flag (new career only or every load); harmless either way (b888).
- Whether any progression graph lacks a `complete` state (it would block the queue, retail too): a setup export check.
