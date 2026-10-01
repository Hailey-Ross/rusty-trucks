# Planned upstream pull requests

The fork's changes will be offered to SK8-ENGINE/skate-3-rust-engine as separate
pull requests, one per type of change, so each can be reviewed and merged on its
own. **None have been opened yet.** Each PR description is drawn from the linked
document(s), which hold the full context (problem, root cause, evidence,
verification, open questions).

| PR | Type | Title | Documents | Fork commits | Depends on | Status |
|---|---|---|---|---|---|---|
| A | Setup fix | Setup: ISO extraction argument order and Windows long paths | [01](01-iso-extraction.md), [02](02-long-paths.md) | `ea549ab`, `0a88f7a` | — | Ready to propose |
| B | Feature (input) | SDL3 gamepad input with XInput fallback | [03](03-sdl3-gamepad-input.md) | `3d485c8` | — | Ready to propose |
| C | Gameplay data | Authored map spawns and headings | [04](04-map-spawns.md) | `b495ed6`, `f00b329` | — | Ready to propose |
| D | Gameplay data | Skip collision volumes that have no surface IDs | [05](05-collision-volumes.md) | `dec7ade` | — | Ready to propose |
| E | Performance | Indexed stock collection lookups (game startup −5.5 s) | [07](07-collections-index.md) | `810defa` | — | Verified |
| F | Tooling | Streaming map validator (`--validate-maps`) wired into setup | [06](06-map-validator.md) | `18f41a3` | E (speed only) | Verified (full setup) |
| G | Performance (setup) | Skip decoding duplicate stream copies; run the customiser beside the maps; parallel clothing library and pro roster, plus a native RefPack DLL in `Build.ps1` | [08](08-setup-performance.md) | `8327e94` (+ overlap hunks of `install.py` in `18f41a3`) | — | Verified (full setup 466 → 331 s; customiser library 127 → 51 s, roster 90 → 16 s; outputs identical; one-time customiser rebuild) |

Notes for whoever opens them:

- Each PR needs its own branch cut from upstream `main`, containing only its
  commits (the fork's `hails-additions` branch carries all of them in sequence).
- PR A overlaps upstream PR #9 (Linux/macOS port), which also fixes the
  extract-xiso argument order. If #9 lands first, drop that half of PR A.
- PRs F and G both change the `maps` fingerprint and both add pairs to
  `tools/asset_pipeline/pipeline-equivalence.json`. The fork currently holds one
  combined pair per group (committed → F+G). When splitting into separate PRs,
  recompute each PR's pairs from its own tree (old = upstream base, new = that
  PR), and re-check the customiser fingerprint is unchanged.
- `install.py` changes for PRs F and G are both in commit `18f41a3` (the
  customiser overlap — `available_memory`, `overlap_customiser`, `Background` —
  is interleaved with the validator wiring in `_install`). Separate them when
  cutting the PR G branch. PR G now changes the customiser fingerprint (one
  rebuild of the customiser, with identical output).
- PR F works without PR E but validates ~10× slower (each skater/camera load
  then takes ~5.5 s instead of ~0.2 s).
- Pre-existing upstream test failures (present on untouched upstream `60efdef`,
  not caused by these PRs): skate-core
  `a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`,
  `predictive_contacts_and_retention_match_full_scan_for_every_primitive`;
  skate-game `production_factory_routes_three_handlers_and_all_five_conditions_to_grind_owner`,
  `embedded_static_rwcm_hits_distinct_actor_query_ids`,
  `pipelines_accept_valid_group_outputs_when_fingerprint_changes`,
  `sky_shader_validates`; and three skate-data examples that do not compile
  (`apt_data`, `hud_data`, `scoring_flow_data`).
- Fork-only commits not meant for upstream: `167605a` (ignore the local
  `.claude/` workspace) and `c2be42b` (docs index; the docs travel with each PR).
