# 45. Ghidra function names for Skate 3 (TU3)

**Problem.** Every retail function in a fresh Ghidra project is `FUN_xxxxxxxx`. Each decode started from unnamed callers and callees, and anyone else reading the TU3 code had to rediscover what we had already worked out.

**Change.** `tools/ghidra/skate3-tu3/`: the names of 32,931 functions decoded during this port (13,546 confirmed; 19,385 guesses, named `guess_<Subsystem>_<What>`), as a list anyone can load into their own Ghidra project (README there, written for a first-time Ghidra user):
- `skate3-tu3-symbols.txt` for Ghidra's built-in `ImportSymbolsScript.py` (nothing extra to install);
- `skate3-tu3-functions.tsv` (address, name, one-line description);
- `ApplyNames.java`, an optional script that applies the TSV with each description as a plate comment, skips addresses inside other functions and never overwrites names the user set (run from the Script Manager, it asks for the file; headless takes `<tsv> <log>`).

Names are `Subsystem_What`; where the game's Lua bindings name a function, that name is used (e.g. `FrontEnd_SetEndChallengeOption`). Confirmed names are functions whose role our research established; guesses are labeled as such and lose the prefix once confirmed. Names, addresses and descriptions only, no game code.

**Verification.** Applied to the TU3 project (276 named, 4 created where no function started, 2 skipped as mid-function addresses); a decompile of `0x82DF18B0` reads `Score_UpdatePlayers` calling `Score_DispatchPlayerEvents` and `MsgBus_Post`.

**Files.** `tools/ghidra/skate3-tu3/README.md`, `skate3-tu3-symbols.txt`, `skate3-tu3-functions.tsv`, `ApplyNames.java`.

## Port review against the decoded names (2026-11-14)

**Problem.** Many engine comments and a few behaviors were written before the functions they cite were decoded, so an early reading could still be wrong.

**Method.** Every TU3 address cited in `crates/*/src` was matched with the decoded name list (`.local/tools/port-vs-decoded.py` in my checkout); 1,038 rows were kept where the name was read from the retail code and shares no word with our text (500 confirmed names, 538 guesses). Each row was classified from the text; every possible contradiction was then read in the retail code (Ghidra pseudocode or the recomp's generated code).

**Result.** Most rows describe the same function in other words. Changed:
- The menus' secondary-font rule matches the APT font name exactly, as retail's `sub_825D6B68` does through `sub_82AE89B0` (a plain signed-byte compare) [code]; it was case-insensitive before ([doc 31, Milestone 3e](31-retail-menus.md#milestone-3e-the-futura-shadow-secondary-font-in-every-movie-2026-10-08)). Test: `apt_text::font_pair_tests::data_pairs_replace_the_retail_table_by_exact_name`.
- Comments only: the score record start `82DA45C8` (it stores the carrier delay at +0xB8 among other fields; `scoring/carrier.rs`), the shared generator `82A8AF10` (identified: the title's six-word add-with-carry generator, not the C runtime's; `skate-audio world/mod.rs`, `speech_manager.rs`), the HUD flag writer `825DA2A0` vs the front-end getter `824AD2F0` (`menu_values.rs`), the allocator `828AAE90` (`living_world/controller_b.rs`).
- 12 guess names corrected in the list (for example `825DA2A0` `guess_FrontEnd_ApplyHudVisible`, `82C1EAD8` `guess_GrindWorld_QueryRails`, `82E36D70` `guess_Ped_UpdateRoot`).

**Open.** Retail draws every random number from one shared generator; our audio hosts keep separate streams (the mod seed API saves and restores each). The values they reach are the same, so outcomes match; one shared stream would be a refactor of the audio hosts.

**Open.** The list grows with each decode.

Credits: Ghidra, the Xenon360 extension, skate3recomp / rexglue / Xenia.
