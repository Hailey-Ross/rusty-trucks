# World speech (peds) — data, mechanism, port state (2026-10-03)

Port:
- `crates/skate-audio/src/world/speech.rs`: the index;
- `speech_manager.rs`: value → event, the gate, the request words;
- `speech_rules.rs`: the `.evt` parser and the line and take choice;
- `peds.rs`: `PedSpeech`.

Setup: `tools/asset_pipeline/world_audio.py`:
- `speech_index`, with `rules` and each clip's `id` / `history`;
- `world_tuning` → `speech_tuning`;
- opt-in `decode_speech`.

Tests: the skate-audio `world::speech_*` unit tests, and `tests/world_speech.rs` on the dev install export.

Trace check: the local tool `speech_takes.py <sessions…>` (the take of each READ, the history rule, the sequence
records).
Dev install staging: `stage_world_audio.py [--decode 501,104,205,101|all]` (local tool).
Harness: `tests/world_sources.rs` `a_bumped_ped_warns_with_a_line_of_its_voice`.

## Data (`data/audio/english/livingworldspeech.big`, EB v3)
- **Clips.** 3,011 `.dat` clips named `<event>_<voice>[_<voice name>]_<line>.dat`. Examples:
  - `501_59_busm1_Warn_n`
  - `1901_53_Shout`
  - `806_47_adtf2_Int_c14_tour`
  - Voice ids run 41–96 (`adtm1`, `adtf1/2/4`, `grn1/2`, `joc2/3`, `busm1–3`, `busw1–3`, `tenm1–3`, `stdf1`,
    `tenf2/3`, `secg1/2`, `torm1–3`, `torf1`, `bum1–3`, `sktm1/4/5`, `sktf1–3`). Clips without a name in the file name use
    a generic voice (53, 91, …).
- **Archives inside** (EB v3, read at boot):
  - `livingworldhdr.big`: one `<clip>.hdr` per clip. Layout: +2 u16 take count, +14 u16 offsets of each take's `.sth` row.
  - `livingworldsth.big`: one `<clip>.sth` per clip. 12-byte rows = u32 byte offset of the take in the `.dat` + the
    take's 8-byte EA SNR header.
- **Takes.** All are codec 3 (EA-XMA2), mono, 36 kHz. A clip holds 1–48 takes, 20,096 in all and 14.3 h. Free-roam events come to
  about 9.3 h, about 2.4 GB as PCM16; events 101/104/205/501 alone are 518 MB.
- **Decoding.** vgmstream decodes `<take>.snr` (the 8 header bytes) plus `<take>.sns` (the `.dat` slice) pairs.
- **Recomp reads.** READ lines point at a take's offset inside the clip, not only the clip start (fixed in
  `speech_nearfar.py`). `speech_takes.py` maps each READ to its clip and take.
- **`livingworld_Events.evt`** (114 KB): the speech library's event table, 81 events (`501_warn` = 0x2012, …).
  - **Decoded:** `world_audio.parse_evt` and `speech_rules::EventTable::parse`. All four banks parse: maincast bank 0,
    living world 1, announcer 3, cameraman 4.
  - **Header:**
    - +4: u32 offset of the name table (32-byte rows);
    - +8: bank; +9: sub-bank;
    - +0x10: u16 event count;
    - +0x18: u16 event offsets × 4.
  - **Event:**
    - u16 id, u16 queue timeout (60–180), u16 priority (500–550);
    - u8 record count;
    - u8 external conditions (0 in all banks);
    - u8 flags (high nibble = field count);
    - u8 probability % (100);
    - u8 flags2 (0), then one more byte;
    - u16 record offsets × 4;
    - 3-byte field descriptors `FF <request word> 04`.
  - **Record:**
    - weight code (0x39 = 4 × 25 everywhere in the living world);
    - probability (100);
    - clips << 2 | mode;
    - locals;
    - field count, then 3 pad bytes;
    - one byte per clip (offset × 4);
    - u32 field masks (0 = any);
    - 8-byte clip entries (u16 `.hdr` id; the rest is 0 in the living world).
  - **Fields:**
    - 1 = speaker type bit;
    - 2 = voice variant bit;
    - 3 depends on the event: the near/far flag, zombie (1901, 201, 606, 609), the conversation partner's type (806 / 807),
      or the conversation word (550–557);
    - 4 = zombie for 603.
  - **Type bits** (from the voices of their records):

    | bit | type | voices |
    |---|---|---|
    | 0x1 | adult m | 41 / 42 / 43 |
    | 0x2 | adult f | 46 / 47 / 49 |
    | 0x4 | granny | 51 / 52 |
    | 0x8 | jock | 55 / 56 |
    | 0x10 | teen f | 72 / 73 / 74 |
    | 0x20 | teen m | 69 / 70 / 71 |
    | 0x40 | security | 75 / 76 / 77 |
    | 0x80 | tourist m | 82 / 83 / 84 |
    | 0x100 | tourist f | 85, **53** |
    | 0x200 | skater m | 89, 91, 92, 90 |
    | 0x400 | skater f | 94 / 95 / 96, 93 |
    | 0x1000 | business m | 59–61 |
    | 0x2000 | business w | 64–66 |
    | 0x4000 | bum | 87, 88, 86 |

    Variant bits: 1, 2, 4, 8 and 16.
  - 133 of the 3,348 living-world clip references have no clip (for example 603's tazer lines for some voices). Those
    records never play.
- **`.hdr`:**
  - u16 id;
  - +2 flags (0 in all banks);
  - +3 take count;
  - +4 runtime;
  - +8 history length: the take count, or 0 for 685 clips (conversations 550–557, zombie lines, 497 Silence);
  - +9 / +10 size scale;
  - +16 u16 `.sth` row offsets;
  - then the history ring (cursor + entries, 0xFF = empty).

## Mechanism (TU3)
- **State graphs.** The ped state graphs (`data/state/livingworldentities/pedestrian/aigraph/*.xml`) send
  `SendSpeechEvent speechevent=… speechvalue=N` on state entry. The values are listed in `speech::SPEECH_VALUES`, for example
  10 CollisionNearbyReaction (wanttoobserve), 25 StopCheer (slamreaction), 23 LongCheer (nearbyskatertrick),
  20 Flee, 14 JoinChase, 17 AttemptTakeDown, 19 TakeDownSuccess, 12 ChaseResting, 66 EscapedEndChase.
  - The warn (11, `pedestrian_dowarning.xml`) and takedown success are commented out ("moved to the code").
  - The value lands in the ped audio state +136. Footstep packets read the same field (jump / collision).
- **SFXObj_PedestrianSpeech** process `sub_824D9908`:
  1. When +136 changes, it builds a request to the speech manager `*(0x830CFDDC)`: `sub_824AB6C8` (or `sub_824AC438` with a
     target).
  2. flag = 1 if +148 > +156 else 2.
  3. Values 7 / 8 are remapped to 30 after a 29, else 51.
  4. Value 49 goes to `sub_824D9C70`.
  5. Value 29 (photographer) repeats on a timer while a global flag is set.
  - Ported: `PedSpeech::process` (49 and the 29 timer are not).
- **Speech manager** (`Sk8::Audio::TheSpeechSystem`, global `*(0x830CFDDC)`). **Ported** (`speech_manager.rs`).
  - **Request block** (`sub_824D9908`):
    - w0 = `+96` type bit, w1 = `+88` variant bit, w2 = flag, w3 = `+92`, w4 = `+120`, w5 = `+124`;
    - speaker slot = `+84`.
    - The living-world path needs `+116 != 0`. Otherwise `sub_824AC438` sends main-cast events (pros; not ported).
  - **Value → event** (`sub_824AB6C8`): the table in `event_for_value`.
    - Coin flips use `rand()`; bums use `rand() % 3`; guards get radio lines.
    - Value 6 with `+71` also sends main-cast event 0x8017 (not ported).
  - **The request** (`sub_824ABA18`), in order:
    1. Game-state preconditions.
    2. The vault tuning of the event (class `Hash_9C1F48F5D637E275`, `SPCHType_1_EventID`).
    3. Timers (`sub_824A8C78`): one per speaker slot × 81 events, f32 seconds, starting at 0.
       - The same event needs ≥ `+24` s.
       - Any event needs ≥ `+8` s, when `+8` > 0.
       - Not-follow needs strictly more than its time. Ids 0 / 294 / 8318 / 33245 / 24752 are skipped.
    4. `sub_824A75F0`:
       - probability: `rand() % 1000 × 0.1f` < `+20`, or ≥ 99.9;
       - the player's speed × 3.6 within `+32` / `+36`;
       - the challenge list;
       - the `+40` / `+44` timers;
       - the `+49..51` game flags;
       - zombie mode needs `+60`.
    5. w9 = 2 in zombie mode (virtual `[-620]+212` vfunc 156; the `zomb_Shout` records prove it).
    6. `sub_824A73F0`: interrupt by priority (not ported).
    7. `sub_824ABD90`: the request words of each event.
  - The timers restart when the line starts (`sub_824A90B8` from `sub_824A84B8`).
- **Speech library** (generic, `sub_829717A0` …). **Ported** (`speech_rules.rs`).
  - `sub_82971480`: the event probability (fails when `(draw>>16)·100>>16 > p`), then the queue slot (not ported).
  - `sub_82973CB8`:
    - the weighted record order (`sub_82972980`): weight = 4^(b>>5)·(b&31), scale table at 0x82FDB3D8;
    - per record: a probability draw, the field match (`sub_82973BD8`) and the clips (`sub_82972D70`: at most 12; a
      missing clip fails the record).
  - Candidates per clip (`sub_82972660`): the takes not in the history ring; when every take is in it, the oldest one.
  - Pick (`sub_82974220`):
    - index = `(draw>>16)·n>>16`;
    - redrawn (at most 32 draws) while it is among the last min(n/2, 10) picks of the same header in a global 32-entry
      (index, header) ring;
    - if every draw hits, the one whose match is oldest.
  - `sub_82973408`: writes the history when the line starts.
  - Generator: add-with-carry at 0x82FDB3C0. Retail shares this state with the grain player's title generator.
  - **The queue is not ported** (`sub_82971340` / `sub_82971890` / `sub_82971DA8`):
    - 16 request slots and 8 streams;
    - highest priority wins, then the newest; older requests are dropped;
    - timeout = event +2, in a clock whose unit is not known.
    - The recomp plays several living-world lines at once (`overlap.py` (local script)), so the living-world
      channel is not exclusive. How is not resolved.

## Reaction → event (measured; `speech_reads.py`, sessions 161849 / 163809 / 164620; the clip starts 0.03 s after)
| reaction | events (evidence) |
|---|---|
| bump → warn | 501 Warn_n (M) |
| bump → warn + taze (female) | 501 Warn_f (M); 330 / 331 tazer lines (name) |
| bump → flee | 108 ChsFlee (M) |
| slam nearby | 104 Slam_n (M) |
| collision nearby | 205 SpecCol, 204 Gasp, 202 HImpRct (M) |
| trick nearby | 101 SpecPos_n (M) |
| chase start | 105 SpecChs bystanders (M), 603 chaser (name) |
| greet / returngreet / conversation | 1901 Shout, 806 Int_c<n>, 807 Rct_c<n> (M) |
| ambient (no reaction) | phone 4402 → 805 → 4405 with 497 Silence; bums 206 / 207; guard radio 102; pro lines from maincast (906/101/130) |

`speech::REACTION_CUES` holds this table, for logs and as evidence. The real choice is `SpeechManager::request`.
`speech::choose` (uniform) is only a fallback for hosts without the rules export.

- **`_n` / `_f` lines: solved from the data.** Request flag 1 (ped `+148 > +156`) means far.
  - The flag-1 records name `101_51_GenPos_Grn1_far` / `104_51_grn1_Slam_far`, and the flag-2 records `…_near`.
  - All 366 `_f` / `_n` records of 101 / 104 / 202 / 203 / 501 split the same way (`tests/world_speech.rs`).
  - **Settled 2026-10-03 (`world-audio-hookin-spec.md` §7.3 G2):** `+148` = the ped's distance to the listener,
    `+156` = `aud_characteristics` field `A27215A909135B62` (20 m for peds, 30 for pros), so `_f` lines play beyond 20 m.
  - (Was:) what `+148` / `+156` hold (a distance and a threshold?) is not traced. `speech_nearfar.py`'s distance join is still
    unreliable.
- **Take choice vs the recomp** (`speech_takes.py`, 10 clean sessions; reads inside > 500 ms audio stalls left out):
  - 57 of 57 clips with 2+ plays follow the history rule, including a full cycle. A uniform pick would have repeated a
    take in about 13.6 of them.
  - 15 of 15 multi-clip lines are one record, in order.
  - The first takes of 806 lines are often 0 (6 of 13 in 164620). This may be chance, or a pairing we cannot see. Keep an
    eye on it.
- **Coverage.** 40 voices have 501 and 104 lines, 39 have 205, 36 have 101 (harness print).

## Playback (port)
- `speech::SPEECH_BANK` (`1 << 22`) mixer bank. `SpeechSlots` maps (clip, take) → slot. Takes play as direct voices.
- The harness renders a busm1 warn take (5.49 s, peak −9 dBFS at unity gain).
- **Level / pan: open** (next step).
  - The stream request (`sub_82973408` → `sub_82C5F310`) is 9 words: offset, size, channel, and the context (ped `+64`).
    It goes to a SpeechBank stream. The voice's level is set where those requests are used; that code is not found yet.
  - A lead: in 163809 a speech READ comes with a `GAIN … 0.0 → 1.0` and a `PITCH` ratio of ~0.52–0.70 on one voice
    (0x40C43350).
  - The speech manager reads the PedestrianSpeech owner outputs itself, and which of its 24 outputs is
  not known (E0 out2 −1000 dB via B2 is the main volume candidate; out13/14 filters; out11/12 via B20 = 2–70 m
  camera distance).

## Waiting on the engine
- **The ped system** must provide:
  - speech values on state entry (`PedState.speech_value`);
  - the speaker fields (`Speaker`: slot, type bit, variant bit, partner type);
  - the near/far pair;
  - zombie mode;
  - the player's speed;
  - a manager clock.
- **The game host:** world_sources logs requests (`AUDIO_WORLD speech …`) but plays nothing. To play, it needs:
  1. the index and `rules` from `speech/livingworld.json` → `SpeechIndex::set_ids`, `EventTable`,
     `Library::new(ClipHeader…)`;
  2. `world_tuning.speech_tuning["1"]` → `EventTuning`;
  3. `SpeechManager::request` for each `SpeechRequest`;
  4. `SpeechIndex::picks_to_lines` → `SpeechSlots` with the decoded takes, played in sequence;
  5. the level mapping.

  The JSON → struct mapping is in `tests/world_speech.rs` `load()`.
- Maincast / cameraman / announcer speech (pro skater lines around the player, the cameraman's "own the spot"):
  same format, not indexed yet.

## Next steps
1. Level / pan: find the code that uses the SpeechBank stream requests (the 9-word requests `sub_82C5F338` queues) and the
   speech voice's GAIN / SEND. Or hook it (category `audio`) and join it with the ped distance.
2. What `+148` / `+156` are: find who writes the ped audio state, or add a first-pass hook on `sub_824D9908` that logs
   both.
3. The request queue and stream count for the living world (why lines overlap in the recomp).
4. Main cast / cameraman / announcer: they use the same library and already parse. Their managers differ (other vault
   types, `sub_824AC560`).
