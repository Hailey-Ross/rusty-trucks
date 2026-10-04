# Pedestrian audio — retail spec, port state, hook points (2026-10-03)

Port: `crates/skate-audio/src/world/peds.rs`. Host: `game_audio/world_sources.rs`. Harness: `tests/world_sources.rs`
(`a_ped_walks_past_and_steps_on_each_plant`). Speech is in `world-speech.md`.
The MixMap Pedestrian slot (5) has 15 instances. Objects: PedestrianSpeech (5.0), PedestrianSFX (5.1), PedBodyFall (5.2), Tazer (5.3).
Every B lookup reads Ctl 5.1, the ped's 3DObjPos.

## Ped record the objects read
- **Audio state `[object+32]`:**
  - +52 = active.
  - +68 = footsteps on. Who sets it is not traced; probably distance or visibility.
  - +73 = foot B down, +74 = foot A down.
  - +96 = kind (64 → the close-range outputs).
  - +116 = speech target.
  - +132 = shoe class 1..5 → footstep w14.
  - +136 = the speech value last sent by the state graph (`SendSpeechEvent`).
  - +140 / +144 = materials A / B (143 = none → 3).
  - +148 / +156 = the speech flag pair.
- **Owner `[object+28]`:** +128 = speed, +144 = 1..5 → footstep w16 (weight?).

## PedestrianSFX (vtable `0x822FCCC8`, factory `sub_824D7E00`)
- **Process `sub_824D8078`:**
  - +416 = trunc(curve(speed)), using the OffBoard-tuning curve `90B47430C4ED2CCC` (not the player's walk curve).
  - Copy the feet into +52/+236 and zero +53/+237.
  - If footsteps are on:
    1. Poster `sub_824D81F8`. Post two `livingword_footstep` packets (B first) through `sub_824B77F0`(foot 0 / 1000, +416, eEQChain
       `A9023782094771B5` + 10): `[32767,0,4096,foot,25000,0×8,speed,1,1,1,0,0,0,eq]`.
    2. Steps `sub_824D8320`. On a foot's rising edge: stop that foot's last Splice sound and start `sk8_foley` (bank 7)
       id = run 64 if speed > 7.5, jog 63 if > 2.5, else walk 62. The Clothing thresholds and OffBoard ids are the
       same fields as the player's walking voices. Block = [0,1,0,dt,0,1].
    3. `sub_824D8E60`: a one-shot +412 flag by camera distance. Not ported, because nothing the footsteps read uses it.
  - Finally copy this frame's foot flags into +54/+238.
- **Update `sub_824D81A8`:**
  - `sub_824D8658` (if footsteps on) rewrites both packets:

    | word | value |
    |---|---|
    | w1 | raw 0 |
    | w2 | pitch 5 (≤ 8192) |
    | w4 / w5 | filters 7 / 8 (≤ 25001) |
    | w6 | level 9 |
    | w7 | level 1, or 2 when kind = 64 |
    | w8 | foot down |
    | w9 | +53 (always 0 from process) |
    | w10 | speech value ∈ {4,5} (jump) |
    | w12 | speech value ∈ {6,7} (collision) |
    | w13 | +416 |
    | w14 | class |
    | w15 | footstep surface(material), 1..7 (AudioSurfaceMap word 6) |
    | w16 | weight |
    | w17..19 | OffBoard `62A2E642…` = 32767 / 7000 / 25000 |

  - `sub_824D84D0`: the Splice steps follow block = [level(4 if kind 64 else 3)/32767, pitch 5/4096, raw 0·360/65535,
    dt, 0, 1], and a step that has ended is freed.
- **Bank behaviour** (harness and `program_trace` sweeps):
  - The program plays a step on w8's rising edge.
  - **w14 = 1 is silent.** Classes 2..5 each have their own shoe sample set: 2 → 0–4 / 19–24 / 112–116, 3 → 5–9 / 25–30 /
    43–50 / 118–133, 4 → 10–14 / 31–36 / 66–69, 5 → 15–18 / 37–42 / 70–74. Surface layers are 51–60.
  - **4,798 of retail's 4,949 `fstep_livingworld` starts (97 %, session 164620) fall in these groups.** Retail used
    classes 2, 4 and 5 that session. The other 151 starts (slots 78–100, 117) need word states we have not driven:
    w9 / w11 / jump / collision combinations.
- In the harness, a ped at 4 m walking 1.3 m/s gives a step per plant, AEMS voice gains 0.02–0.17 and sk8_foley 0.22–0.28.
  Retail fstep_livingworld GAIN×SEND is 0.000 p50 / 0.024 p90 over all distances.

## PedestrianSpeech (vtable `0x822FBF40`): `world-speech.md`

## Not ported yet
- **PedBodyFall:** `Bodyslide`-like falls when a ped is knocked down. Not decoded.
- **Tazer** (`Tazer.abk`, `c_tazer` + `c_tazer_grn_play`): uses AEMS op 38 ControlClass, which the evaluator has not ported.
  Retail had 49 starts in 164620.
- **Ped hand props, ATM / vending machine sounds:** the plugins' own objects. Not looked at.

## Waiting on the engine
1. **A ped system** (census spawns, state graphs; `npc-livingworld-re.md`). Per ped it fills
   `WorldOwners.peds[id]` with a `PedState`:
   - position, velocity and speed;
   - **foot plants** from the walk animation;
   - materials under the feet;
   - shoe class (from the model; 1 = silent);
   - weight;
   - speech value (the state graph's `SendSpeechEvent` on each state entry);
   - footsteps on (near the camera).
2. Speech voice id per ped model (for the speech index).

## Open
- **2026-10-03, gap run G2 (`world-audio-hookin-spec.md` §7.3): settled** +68 (the 3 nearest of a nearest-first
  50 m list, manager `sub_824F2890`), +148 / +156 (listener distance / the model's 20 m far threshold), and the
  per-model class / kind / voice (`aud_characteristics`, model = voice id). `[obj+28]+144` is dynamic, not a weight.
- Which model → class / weight / kind / voice. These are probably `livingworld_models` fields; find their readers.
- What sets +68 (footsteps on) and +148 / +156.
- The `sub_824D8E60` flag.
- The 3 % of retail slots we do not produce (above).
- Instance assignment: nearest 15, provisional.
