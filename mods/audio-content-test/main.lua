-- Dev test of the audio modding surface (PR #36). Content: audio.json. Runtime: retail posts,
-- a global, a MixMap watch and audio events, shown on the HUD.
local last = {}
local counts = {}
local by_class = {}
local posted, global_set = false, false
-- audio/moddability-2: the mod's own WAVs through the native mixer (F8), see doc 16 H.
local native_on = false
local tuned = false
local placed = false
local quiet = false
-- audio/moddability-2, third pass (doc 16 L1-L7): Digit1..Digit6.
local ducked, seeded, own_cars, dev_class, nose, orbit = false, false, false, false, false, nil
local pressed = {}
local keep_t = 0
local own_center, own_t = nil, 0
-- Self-test (setting `autotest`, dev only): presses the keys below on a timer instead of a person.
local virtual = {}

local function key(name)
    if virtual[name] then
        virtual[name] = nil
        pressed[name] = sdk.snapshot.keys and sdk.snapshot.keys[name] == true
        return true
    end
    local down = sdk.snapshot.keys and sdk.snapshot.keys[name] == true
    local edge = down and not pressed[name]
    pressed[name] = down
    return edge
end

-- The watch names globals, which only resolve while the native audio runs (at on_load it does
-- not yet: "native audio is not running"), so it is (re)sent once the audio runs and after every
-- content change. g_dev_level is the global of this mod's own Csis project (audio.json
-- add.projects, doc 16 L4); without it (an older engine) the watch fails, so it goes through a request.
local watch_gen = nil
local function request_watch()
    local info = sdk.audio.info()
    if not info.native then return end
    watch_gen = info.generation or 0
    sdk.commands.request("watch", {kind = "audio_watch", globals = (sdk.capabilities.audio_content or 0) >= 3 and {"g_dev_level", "babycry_1_sel_snd"} or {"babycry_1_sel_snd"},
        mixmap = {{slot = "emitter", object = 0, instance = 0, output = 4}}})
end

local function dev_level()
    return sdk.audio.global("g_dev_level")
end

-- ---------------------------------------------------------------------------------------------
-- Autotest (mod setting `autotest`, off by default; the runner sets it through SKATE3_MOD_SETTINGS).
-- Once the map's native audio runs it presses F5..F11 and Digit1..Digit6 in a fixed order, one step
-- every `autotest_step` seconds, reads back what the engine reports (sdk.audio.info / handle /
-- global / tuned / mixmap_inputs, sdk.world_audio.read, sdk.commands.result, the audio events) and
-- logs ONE line per check: `AUTOTEST <check> ok|fail <details>`. The hot swap step asks the runner
-- to edit audio.json (`AUTOTEST_REQ edit_audio_json` / `revert_audio_json`) and waits for the swap.
-- The pops come from a synthetic flick (gameplay action 68, the right stick's Y: down, then up).
local at = {on = false, t = 0, ready_t = nil, step = 0, step_t = 0, done_marks = {}, pass = 0, fail = 0,
            base = nil, loads = 0, ollie_t = nil, ollie_phase = nil}

local function fmt(v)
    if type(v) == "table" then
        local parts = {}
        for k, x in pairs(v) do parts[#parts + 1] = tostring(k) .. "=" .. fmt(x) end
        table.sort(parts)
        return "{" .. table.concat(parts, ",") .. "}"
    end
    return tostring(v)
end

local function report(check, ok, details)
    if ok then at.pass = at.pass + 1 else at.fail = at.fail + 1 end
    sdk.log(string.format("AUTOTEST %s %s %s", check, ok and "ok" or "fail", details))
end

local function result(k)
    local r = sdk.commands.result(k)
    if not r then return false, "no result" end
    return r.ok == true, r.ok and "ok" or tostring(r.error)
end

local function skater()
    local p = sdk.player.read() or {}
    return string.format("on_board=%s mode=%s speed=%.2f trick=%s", tostring(p.on_board), tostring(p.mode), p.speed or -1, tostring(p.trick))
end

-- A push (A, action 80) first, so the flick 0.8 s later is a rolling ollie.
local function ollie()
    at.ollie_t, at.ollie_phase, at.air_seen = -0.8, nil, false
    at.coll_log, at.coll_t0 = {}, at.t
    at.press = {action = 80, left = 0.25}
    sdk.log("AUTOTEST_INFO ollie from " .. skater())
end

local function drive_ollie(dt)
    if not at.ollie_t then return end
    at.ollie_t = at.ollie_t + dt
    if ((sdk.player.read() or {}).mode or ""):find("air") then at.air_seen = true end
    if at.ollie_t < 0 then return end
    -- right stick down 0.35 s, up 0.25 s, released; the air state is watched until 1.6 s
    local phase = at.ollie_t < 0.35 and "down" or (at.ollie_t < 0.6 and "up" or "off")
    if phase ~= at.ollie_phase then
        at.ollie_phase = phase
        sdk.commands.request("ollie", {kind = "input_override", action = 68, value = phase == "down" and -1 or (phase == "up" and 1 or nil)})
    end
    if at.ollie_t >= 1.6 then at.ollie_t = nil end
end

-- A short press of a mapped gameplay action (79 = Y: on / off the board).
local function drive_press(dt)
    if not at.press then return end
    if not at.press.down then
        at.press.down = true
        sdk.input.override_action(at.press.action, 1)
    end
    at.press.left = at.press.left - dt
    if at.press.left <= 0 then
        sdk.input.override_action(at.press.action, nil)
        at.press = nil
    end
end

local function has(list, x)
    for _, v in ipairs(list or {}) do if v == x then return true end end
    return false
end

local function count_own()
    local n = 0
    for i = 1, 6 do
        local r = sdk.world_audio.read("own_car" .. i)
        if r and r.own then n = n + 1 end
    end
    return n
end

-- Each step: name, what to listen for (HUD), and timed marks `{t, fn}`; `t` < 0 counts from the
-- step's end. A mark returning false holds the step (polling) until `wait` seconds have passed.
local steps
steps = {
    {name = "babycry", title = "A crying baby, now a beep", listen = "The camera flies to DownTown's Baby_Cry_1 emitter: its replaced samples (a beep)", marks = {
        {0, function()
            -- DownTown's Baby_Cry_1 record (sfx_downtown, an 18 m sphere, 13 m above the street on a
            -- building): the listener is the camera, so the camera is put 5 m from it (moving the
            -- skater there drops it to the street, out of reach). The bank is audio.json's beep.
            local rec = {-119.7, 53.48, -234.14}
            at.cry_rec, at.cry0 = rec, by_class["emitter_start:Baby_Cry_1"] or 0
            sdk.camera.set({rec[1] + 5, rec[2] + 1, rec[3]}, rec)
            sdk.log("AUTOTEST_INFO camera to Baby_Cry_1 record " .. fmt(rec))
        end},
        {1.5, function() sdk.engine.inspect("cry_catalog", "audio_catalog") end},
        {2.5, function()
            local starts = (by_class["emitter_start:Baby_Cry_1"] or 0) - at.cry0
            local r = sdk.commands.result("cry_catalog")
            local loaded = r and r.value and has(r.value.banks, "Baby_Cry_1")
            report("babycry_emitter", starts >= 1 and loaded, string.format("emitter_start Baby_Cry_1 rows=%d, camera set 5.1 m from the record (reach 18 m), bank Baby_Cry_1 loaded=%s (audio.json replaces its samples with the beep; the log's AUDIO_EMITTER line names the record)", starts, tostring(loaded)))
        end},
}},
    {name = "global", title = "Retail global set (F5)", listen = "No sound of its own: babycry_1_sel_snd = 1, then restored", note = "F5: babycry_1_sel_snd = 1 (no sound of its own)", marks = {
        {0, function() at.g0 = sdk.audio.global("babycry_1_sel_snd"); virtual.F5 = true end},
        {1.5, function()
            local ok, e = result("global")
            local v = sdk.audio.global("babycry_1_sel_snd")
            report("F5_global_set", ok and v == 1, "result=" .. e .. " value=" .. tostring(v) .. " before=" .. tostring(at.g0))
        end},
        {-1.0, function() virtual.F5 = true end},
        {-0.1, function()
            local v = sdk.audio.global("babycry_1_sel_snd")
            report("F5_global_restore", v == at.g0, "value=" .. tostring(v) .. " want=" .. tostring(at.g0))
        end}}},
    {name = "post", title = "Post a retail emitter (F6)", listen = "A second, centred beep on top of the baby emitter's (camera still there)", note = "F6: c_emitter post (Baby_Cry_1 = beep where its bank is loaded)", marks = {
        {0, function() virtual.F6 = true end},
        {1.5, function()
            local ok, e = result("post")
            local h = sdk.audio.handle("emit")
            report("F6_post", ok and h ~= nil and h.live == true and h.class == "c_emitter", "result=" .. e .. " handle=" .. fmt(h))
        end}}},
    {name = "release", title = "Release the post (F7)", listen = "The centred beep stops (the emitter's own beep goes on); then the camera comes back", note = "F7: release the c_emitter post", marks = {
        {0, function() virtual.F7 = true end},
        {1.0, function()
            local h = sdk.audio.handle("emit")
            report("F7_release", h == nil or h.live == false, "handle=" .. fmt(h))
        end},
        {-0.3, function() sdk.camera.clear() end}}},
    {name = "native", title = "Mod siren + beep (F8)", listen = "A looping siren 8 m to one side, a short beep; siren stops at the end", note = "F8: looping siren 8 m east + a beep (native mixer)", marks = {
        {0, function() at.v0 = sdk.audio.info().native_voices or 0; virtual.F8 = true end},
        {1.5, function()
            local ok1, e1 = result("native")
            local ok2, e2 = result("beep")
            local v = sdk.audio.info().native_voices
            report("F8_native_siren", ok1 and ok2 and (v or 0) >= 1, "siren=" .. e1 .. " beep=" .. e2 .. " native_voices=" .. tostring(v) .. " before=" .. tostring(at.v0))
        end},
        {-1.0, function() virtual.F8 = true end},
        {-0.1, function()
            local v = sdk.audio.info().native_voices or 0
            report("F8_native_stop", v <= at.v0, "native_voices=" .. tostring(v) .. " before=" .. tostring(at.v0))
        end}}},
    {name = "tuning", title = "Tuning writes (F9)", listen = "Very quiet: a taxi idling 3 m away, idle rpm 1200 -> 1800 and back (retail idle level)", note = "F9: taxi idle rpm 1800, reverb01 time 3 (the game spawns no traffic: a mod taxi idles 6 m away, retail's pool)", wait = 0, marks = {
        {0, function()
            local p = sdk.player.read().position
            sdk.commands.request("f9_taxi", {kind = "world_audio_spawn", key = "f9_taxi", object = "traffic", options = {
                engine = "c04_taxi01", slots = "shared", speed = 0, position = {p[1] + 3, p[2], p[3]}}})
        end},
        {2.0, function() virtual.F9 = true end},
        -- A fresh read once the patch is applied (writes land between audio passes).
        {3.0, function() sdk.audio.tuning("taxi_check", "world", "traffic_engine/c04_taxi01") end},
        {3.5, function()
            local ok1, e1 = result("tune_world")
            local ok2, e2 = result("tune_reverb")
            local r = sdk.commands.result("taxi_check")
            local rpm = r and r.value and r.value.idle_rpm
            local n = #sdk.audio.tuned()
            local t = sdk.world_audio.read("f9_taxi")
            report("F9_tuning_write", ok1 and ok2 and n >= 2 and rpm == 1800 and t and t.audible, "world=" .. e1 .. " reverb=" .. e2 .. " tuned=" .. n .. " taxi_idle_rpm=" .. tostring(rpm) .. " f9_taxi=" .. fmt(t))
        end},
        {-1.0, function() virtual.F9 = true end},
        {-0.6, function() sdk.audio.tuning("taxi_check", "world", "traffic_engine/c04_taxi01") end},
        {-0.1, function()
            local r = sdk.commands.result("taxi_check")
            local rpm = r and r.value and r.value.idle_rpm
            local n = #sdk.audio.tuned()
            report("F9_tuning_restore", n == 0 and rpm == 1200, "tuned=" .. n .. " taxi_idle_rpm=" .. tostring(rpm) .. " (audio.json's 1200)")
            sdk.world_audio.remove("f9_taxi")
        end}}},
    {name = "emitter", title = "Mod emitter + reverb zone (F10)", listen = "A beeping emitter 6 m away, a bigger reverb around you", note = "F10: beep emitter 6 m east (own instance) + reverb11 zone", marks = {
        {0, function() virtual.F10 = true end},
        {2.0, function()
            local ok1, e1 = result("emitter")
            local ok2, e2 = result("zone")
            local em, zn = sdk.world_audio.read("emitter"), sdk.world_audio.read("zone")
            report("F10_emitter_zone", ok1 and ok2 and em and em.audible and zn and zn.audible,
                "emitter=" .. e1 .. " zone=" .. e2 .. " read_emitter=" .. fmt(em) .. " read_zone=" .. fmt(zn) .. " slots=" .. tostring(sdk.audio.info().mod_emitter_slots))
        end},
        {-1.0, function() virtual.F10 = true end},
        {-0.1, function()
            local em, zn = sdk.world_audio.read("emitter"), sdk.world_audio.read("zone")
            report("F10_removed", em == nil and zn == nil, "read_emitter=" .. fmt(em) .. " read_zone=" .. fmt(zn))
        end}}},
    {name = "rules", title = "Rules: mute grinds, landing beacon (F11)", listen = "Ollie: the pop beeps at the skater (audio.json pop_click), the landing beeps 10 m away", note = "F11: grind starts muted, landing replaced by a beep 10 m east (an ollie follows)", marks = {
        {0, function() at.r0 = sdk.audio.info().rules or 0; at.land0 = counts.land or 0; at.sc0 = by_class["splice:Skate_Collisions"] or 0; virtual.F11 = true end},
        {1.0, function()
            local n = sdk.audio.info().rules or 0
            report("F11_rules_set", n == at.r0 + 2, "rules=" .. n .. " before=" .. at.r0)
            ollie()
        end},
        {-1.2, function()
            local l = (counts.land or 0) - at.land0
            report("F11_landing_seen", l >= 1, "land_events=" .. l .. " untagged Skate_Collisions splice rows=" .. ((by_class["splice:Skate_Collisions"] or 0) - at.sc0) .. " (synthetic ollie; the beacon beep replaces it) " .. skater() .. " ollie_cmd=" .. select(2, result("ollie")) .. " air_seen=" .. tostring(at.air_seen) .. " collisions=[" .. table.concat(at.coll_log or {}, " ") .. "]")
        end},
        {-1.0, function() virtual.F11 = true end},
        {-0.1, function()
            local n = sdk.audio.info().rules or 0
            report("F11_rules_removed", n == at.r0, "rules=" .. n .. " want=" .. at.r0)
        end}}},
    {name = "duck", title = "Duck the world (Digit1)", listen = "The whole world gets quieter (-12 dB), then comes back", note = "Digit1: the world ducked -12 dB through the Master MixMap inputs", marks = {
        {0, function() virtual.Digit1 = true end},
        {1.5, function()
            local rows = sdk.audio.mixmap_inputs()
            local held = 0
            for _, r in ipairs(rows) do if r.slot == "global" and r.object == 2 and r.value == 8192 then held = held + 1 end end
            local okc = true
            for i = 1, 4 do if not result("duck" .. i) then okc = false end end
            report("D1_duck", okc and held == 4 and (sdk.audio.info().mixmap_inputs or 0) >= 4,
                "inputs_held=" .. held .. " info.mixmap_inputs=" .. tostring(sdk.audio.info().mixmap_inputs) .. " out4=" .. fmt((sdk.audio.mixmap("emitter", 0, 0, 4) or {}).level))
        end},
        {-1.0, function() virtual.Digit1 = true end},
        {-0.1, function()
            report("D1_duck_release", #sdk.audio.mixmap_inputs() == 0, "inputs=" .. #sdk.audio.mixmap_inputs() .. " info.mixmap_inputs=" .. tostring(sdk.audio.info().mixmap_inputs))
        end}}},
    {name = "seed", title = "Seed the audio random state (Digit2)", listen = "Nothing to hear", note = "Digit2: audio random state seeded 1234 (no sound of its own)", marks = {
        {0, function() virtual.Digit2 = true end},
        {1.5, function()
            local s = sdk.audio.info().seed
            report("D2_seed", type(s) == "table" and s.seed == 1234 and s.owner == sdk.mod_id, "seed=" .. fmt(s))
        end},
        {-1.0, function() virtual.Digit2 = true end},
        {-0.1, function()
            local s = sdk.audio.info().seed
            report("D2_seed_release", type(s) ~= "table", "seed=" .. fmt(s))
        end}}},
    {name = "taxis", title = "Six taxis on their own instances (Digit3)", listen = "Six taxis driving a 6 m circle around you at 6 m/s", note = "Digit3: six idling taxis around the skater, each on its own MixMap instance", marks = {
        {0, function() virtual.Digit3 = true end},
        {2.5, function()
            local okc = true
            for i = 1, 6 do if not result("own" .. i) then okc = false end end
            local own = count_own()
            local r1 = sdk.world_audio.read("own_car1")
            local wi = sdk.world_audio.info()
            report("D3_own_taxis", okc and own == 6, "own=" .. own .. "/6 read1=" .. fmt(r1) .. " info.own=" .. fmt(wi and wi.own) .. " (held + posted; output level not readable from Lua)")
        end},
        {-1.0, function() virtual.Digit3 = true end},
        {-0.1, function()
            local left = 0
            for i = 1, 6 do if sdk.world_audio.read("own_car" .. i) then left = left + 1 end end
            report("D3_taxis_removed", left == 0, "left=" .. left)
        end}}},
    {name = "csis", title = "Mod's own Csis class + global (Digit4)", listen = "Nothing to hear (no bank uses the class)", note = "Digit4: the mod's own Csis class c_dev_mod posted, g_dev_level 9 (no sound: no bank binds it)", marks = {
        {0, function() at.lv0 = dev_level(); virtual.Digit4 = true end},
        {1.5, function()
            local ok1, e1 = result("dev_post")
            local ok2, e2 = result("dev_global")
            local h = sdk.audio.handle("dev_class")
            local v = dev_level()
            report("D4_csis", ok1 and ok2 and h and h.live and v == 9, "post=" .. e1 .. " global=" .. e2 .. " handle=" .. fmt(h) .. " g_dev_level=" .. tostring(v) .. " before=" .. tostring(at.lv0))
        end},
        {-1.0, function() virtual.Digit4 = true end},
        {-0.1, function()
            local h = sdk.audio.handle("dev_class")
            local v = dev_level()
            report("D4_csis_release", (h == nil or not h.live) and v == 7, "handle=" .. fmt(h) .. " g_dev_level=" .. tostring(v) .. " (default 7)")
        end}}},
    {name = "nose", title = "Beep ahead of the board on pops (Digit5)", listen = "The skater ollies: a beep in front of the board", note = "Digit5: a beep 2 m ahead of the board's nose on every pop (an ollie follows)", marks = {
        {0, function() at.r0 = sdk.audio.info().rules or 0; at.pop0 = counts.pop or 0; at.sc0 = by_class["splice:Skate_Collisions"] or 0; virtual.Digit5 = true end},
        {1.0, function()
            local n = sdk.audio.info().rules or 0
            report("D5_nose_rule", n == at.r0 + 1, "rules=" .. n .. " before=" .. at.r0)
            ollie()
        end},
        {-1.2, function()
            local p = (counts.pop or 0) - at.pop0
            report("D5_pop_seen", p >= 1, "pop_events=" .. p .. " Skate_Collisions splice rows=" .. ((by_class["splice:Skate_Collisions"] or 0) - at.sc0) .. " (synthetic ollie: action 68 down, then up) " .. skater() .. " ollie_cmd=" .. select(2, result("ollie")) .. " air_seen=" .. tostring(at.air_seen) .. " collisions=[" .. table.concat(at.coll_log or {}, " ") .. "]")
        end},
        {-1.0, function() virtual.Digit5 = true end},
        {-0.1, function()
            local n = sdk.audio.info().rules or 0
            report("D5_nose_removed", n == at.r0, "rules=" .. n .. " want=" .. at.r0)
        end}}},
    {name = "orbit", title = "Orbiting emitter (Digit6)", listen = "A beep circling around you every 6 s", note = "Digit6: an emitter orbiting the skater at 8 m every 6 s (beep follows it)", marks = {
        {0, function() virtual.Digit6 = true end},
        {2.0, function()
            local ok, e = result("orbit")
            local r = sdk.world_audio.read("orbit")
            at.orbit_a = orbit
            report("D6_orbit", ok and r and r.audible, "result=" .. e .. " read=" .. fmt(r) .. " phase_s=" .. string.format("%.2f", orbit or -1))
        end},
        {-1.2, function()
            local r = sdk.world_audio.read("orbit")
            report("D6_orbit_moving", r and r.audible and orbit and at.orbit_a and orbit > at.orbit_a, "read=" .. fmt(r) .. string.format(" phase %.2f -> %.2f s", at.orbit_a or -1, orbit or -1))
        end},
        {-1.0, function() virtual.Digit6 = true end},
        {-0.1, function()
            report("D6_orbit_removed", sdk.world_audio.read("orbit") == nil and orbit == nil, "read=" .. fmt(sdk.world_audio.read("orbit")))
        end}}},
    {name = "tags", title = "Event tags after subscribing again", listen = "The skater ollies once more (pop click from audio.json's rule)", marks = {
        -- The subscription made in on_load (before the native audio ran) is checked by the two
        -- ollie steps above; subscribing again now (the audio runs) must tag pops and landings.
        {0, function() at.pop0, at.land0 = counts.pop or 0, counts.land or 0; sdk.audio.subscribe{tags = {}} end},
        {0.5, function() ollie() end},
        {-0.5, function()
            local p, l = (counts.pop or 0) - at.pop0, (counts.land or 0) - at.land0
            report("tags_after_resubscribe", p >= 1 and l >= 1, "pop_events=" .. p .. " land_events=" .. l .. " " .. skater() .. " air_seen=" .. tostring(at.air_seen) .. " collisions=[" .. table.concat(at.coll_log or {}, " ") .. "]")
        end}}},
    {name = "hotswap", title = "Edit audio.json while running (hot swap)", listen = "No cut in the sound: the tunnel zone volume is swapped in and back", note = "L7: audio.json edited while running (tunnel zone volume), swapped in, then reverted", wait = 20, marks = {
        {0, function()
            local i = sdk.audio.info()
            at.s0, at.rs0 = i.swaps or 0, i.restarts or 0
            sdk.log("AUTOTEST_REQ edit_audio_json")
        end},
        {0.5, function()
            local i = sdk.audio.info()
            if (i.swaps or 0) <= at.s0 then return false end
            report("L7_hot_swap_edit", i.last_change == "swap" and (i.restarts or 0) == at.rs0 and at.loads == 1,
                "swaps=" .. tostring(i.swaps) .. " before=" .. at.s0 .. " last_change=" .. tostring(i.last_change) .. " restarts=" .. tostring(i.restarts) .. " script_loads=" .. at.loads)
            sdk.log("AUTOTEST_REQ revert_audio_json")
        end},
        {1.0, function()
            local i = sdk.audio.info()
            if (i.swaps or 0) <= at.s0 + 1 then return false end
            report("L7_hot_swap_revert", i.last_change == "swap" and (i.restarts or 0) == at.rs0 and at.loads == 1,
                "swaps=" .. tostring(i.swaps) .. " last_change=" .. tostring(i.last_change) .. " restarts=" .. tostring(i.restarts) .. " script_loads=" .. at.loads)
        end}}},
}

local function autotest(dt)
    at.t = at.t + dt
    drive_ollie(dt)
    drive_press(dt)
    local info = sdk.audio.info()
    local player = sdk.player.read()
    if not at.ready_t then
        if info.native and info.map and info.map.stem and player and player.position then
            at.ready_t = at.t
            sdk.log("AUTOTEST_INFO map ready: " .. tostring(info.map.stem) .. ", settling 4 s")
        end
        return
    end
    if at.step == 0 then
        -- The skater can start on foot: get on the board (Y) so the ollie steps pop.
        if at.t - at.ready_t >= 1 and not at.mounted then
            at.mounted = true
            if player.on_board == false then
                sdk.log("AUTOTEST_INFO on foot (" .. skater() .. "): pressing Y to get on the board")
                at.press = {action = 79, left = 0.2}
            end
        end
        if at.t - at.ready_t < 4 then return end
        at.base = {restarts = info.restarts or 0, generation = info.generation}
        report("start", info.native and info.map.stem == "DownTown" and has(info.overlays, sdk.mod_id),
            "map=" .. tostring(info.map.stem) .. " district=" .. tostring(info.map.district) .. " sources=" .. fmt(info.map.sources)
            .. " overlays=" .. fmt(info.overlays) .. " conflicts=" .. tostring(info.conflicts) .. " restarts=" .. tostring(info.restarts)
            .. " generation=" .. tostring(info.generation) .. " rules=" .. tostring(info.rules) .. " g_dev_level=" .. tostring(dev_level())
            .. " watch=" .. select(2, result("watch")) .. " " .. skater())
        at.step, at.step_t, at.mark, at.gap_t = 1, 0, 1, at.gap
        return
    end
    local s = steps[at.step]
    if not s then return end
    -- A silent gap before each step (setting autotest_gap; the HUD shows "Next: ...").
    if at.gap_t and at.gap_t > 0 then
        at.gap_t = at.gap_t - dt
        return
    end
    if at.step_t == 0 then sdk.log("AUTOTEST_STEP " .. at.step .. " " .. s.name .. ": " .. s.title .. " | listen: " .. s.listen) end
    at.step_t = at.step_t + dt
    local len = at.len
    -- Every step ends: one that is still running its marks past its length + wait + 5 s fails.
    local limit = len + (s.wait or 0) + 5
    local m = s.marks[at.mark]
    if m and at.step_t > limit then
        report(s.name .. "_timeout", false, string.format("step still at mark %d after %.1f s", at.mark, at.step_t))
        at.mark, m = #s.marks + 1, nil
    end
    if m then
        local when = m[1] >= 0 and m[1] or len + m[1]
        if at.step_t >= when then
            local held = m[2]() == false
            if held and at.step_t < when + (s.wait or 0) then return end
            if held then report(s.name .. "_timeout", false, "no change within " .. tostring(s.wait) .. " s") end
            at.mark = at.mark + 1
        end
        return
    end
    if at.step_t < len then return end
    at.step, at.step_t, at.mark, at.gap_t = at.step + 1, 0, 1, at.gap
    local n = steps[at.step]
    if not n then
        local i = sdk.audio.info()
        report("no_restart", (i.restarts or 0) == at.base.restarts, "restarts=" .. tostring(i.restarts) .. " at_start=" .. at.base.restarts .. " swaps=" .. tostring(i.swaps) .. " generation=" .. tostring(i.generation))
        sdk.log(string.format("AUTOTEST done pass=%d fail=%d", at.pass, at.fail))
    end
end

local function autotest_hud()
    if not at.on then return end
    local s = steps[at.step]
    local big, small
    if s and at.gap_t and at.gap_t > 0 then
        big = string.format("Next: %d/%d  %s", at.step, #steps, s.title)
        small = "(quiet for a moment)"
    elseif s then
        big = string.format("%d/%d  %s", at.step, #steps, s.title)
        small = "Listen: " .. s.listen
    elseif at.step == 0 then
        big, small = "Audio autotest", "Waiting for the map's audio..."
    else
        big, small = "Audio autotest done", string.format("%d checks ok, %d failed", at.pass, at.fail)
    end
    local status = string.format("checks: %d ok, %d failed", at.pass, at.fail)
    local muted = sdk.settings.autotest_muted == true
    if big == at.hud_big and status == at.hud_status then return end
    at.hud_big, at.hud_status = big, status
    -- A muted run (the runner's default) says so in red above the step.
    local top = muted and 56 or 0
    local items = {
        {key = "bg", type = "rect", position = {0, 0}, size = {1200, 150 + top}, color = {0, 0, 0, 0.8}},
        {key = "big", type = "text", position = {24, 14 + top}, size = {1160, 56}, text = big, font_size = 44, color = {1, 1, 1, 1}},
        {key = "small", type = "text", position = {24, 76 + top}, size = {1160, 36}, text = small, font_size = 28, color = {1, 0.85, 0.3, 1}},
        {key = "status", type = "text", position = {24, 116 + top}, size = {1160, 28}, text = status, font_size = 20, color = {0.7, 0.9, 0.7, 1}},
    }
    if muted then
        items[#items + 1] = {key = "muted", type = "text", position = {24, 8}, size = {1160, 50}, text = "MUTED TEST", font_size = 44, color = {1, 0.15, 0.15, 1}}
    end
    sdk.ui.canvas("autotest", {anchor = "bottom_left", offset = {40, 60}, size = {1200, 150 + top}, scale = 1, items = items})
end

local function hud()
    if at.on then
        if not at.hud_cleared then
            at.hud_cleared = true
            sdk.ui.text("audio-content-test", "")
            sdk.ui.text("audio-content-test-2", "")
        end
        return
    end
    local info = sdk.audio.info()
    local h = sdk.audio.handle("emit")
    local m = sdk.audio.mixmap("emitter", 0, 0, 4)
    local tags = {}
    for t, n in pairs(counts) do tags[#tags + 1] = t .. " " .. n end
    table.sort(tags)
    sdk.ui.text("audio-content-test", string.format(
        "Audio content test: restarts %s, generation %s, conflicts %s | post %s | global %s | native %s | tuning %s | emitter+zone %s | rules %s | emitter 0 out4 %s | %s | last: %s  [F5 global, F6 post, F7 release, F8 native siren, F9 tuning, F10 emitter + reverb zone, F11 mute grinds + landing beacon]",
        tostring(info.restarts), tostring(info.generation), tostring(info.conflicts),
        h and (h.live and "live" or "dead") or "-", global_set and "set" or "-", native_on and "on" or "-", tuned and (#sdk.audio.tuned() .. " fields") or "-",
        placed and ((sdk.world_audio.read("emitter") or {}).audible and "playing" or "placed") or "-",
        tostring(info.rules or 0) .. (quiet and " (grinds muted, landing beacon)" or ""),
        m and tostring(m.level) or "-", table.concat(tags, ", "), table.concat(last, " / ")))
    local own = 0
    for i = 1, 6 do
        local r = sdk.world_audio.read("own_car" .. i)
        if r and r.own then own = own + 1 end
    end
    sdk.ui.text("audio-content-test-2", string.format(
        "Content changes: swaps %s, restarts %s, last %s | duck %s | seed %s | own-instance cars %s (%d own) | c_dev_mod %s, g_dev_level %s | nose beep %s | orbiting emitter %s  [1 duck (Master inputs), 2 seed, 3 own-instance taxis, 4 mod Csis class + global, 5 pop beep 2 m ahead of the board, 6 orbiting emitter; edit audio.json while running: swapped, no restart]",
        tostring(info.swaps), tostring(info.restarts), tostring(info.last_change),
        ducked and "on" or "-", seeded and "1234" or "-", own_cars and "on" or "-", own,
        dev_class and "posted" or "-", tostring(dev_level()), nose and "on" or "-",
        orbit and ((sdk.world_audio.read("orbit") or {}).audible and "playing" or "placed") or "-"))
end

return {
    on_load = function()
        if (sdk.capabilities.audio or 0) < 2 then
            sdk.ui.text("audio-content-test", "Audio content test: this engine has no audio API 2")
            return
        end
        if sdk.settings.events then sdk.audio.subscribe{tags = {}} end
        request_watch()
        if sdk.settings.autotest then
            at.on, at.len, at.gap, at.loads = true, sdk.settings.autotest_step or 5, sdk.settings.autotest_gap or 0, at.loads + 1
            sdk.log("AUTOTEST_INFO on: step " .. tostring(at.len) .. " s, gap " .. tostring(at.gap) .. " s, script load " .. at.loads .. ", events " .. tostring(sdk.settings.events))
        end
        hud()
    end,
    on_update = function(event)
        if (sdk.capabilities.audio or 0) < 2 then return end
        if (sdk.audio.info().generation or 0) ~= watch_gen then request_watch() end
        if at.on then autotest(event and event.dt or 0.016) end
        for _, e in ipairs(sdk.audio.events()) do
            local name = e.tag or (e.kind .. ":" .. (e.slot ~= "" and e.slot or e.class))
            if not counts[name] then
                -- First row of each kind in the log too (unattended log checks).
                sdk.log("audio event " .. name .. " (" .. tostring(e.source) .. " " .. tostring(e.class) .. ")")
            end
            counts[name] = (counts[name] or 0) + 1
            if at.coll_log and #at.coll_log < 24 and e.kind == "splice" and e.class == "Skate_Collisions" then
                -- The board contacts' Splice ids after a synthetic ollie (Contacts tuning: pops 1097-1099 /
                -- hollow 1103-1105, landing 1095), with seconds since the push.
                at.coll_log[#at.coll_log + 1] = string.format("%d%s@%.2f", e.id, e.tag and ("/" .. e.tag) or "", at.t - at.coll_t0)
            end
            local kc = e.kind .. ":" .. tostring(e.class)
            by_class[kc] = (by_class[kc] or 0) + 1
            table.insert(last, 1, name)
            if #last > 5 then table.remove(last) end
        end
        if key("F6") then
            -- The retail c_emitter class with the dry / send / pan / pitch / filter words of an
            -- unpositioned emitter at full level and patch 22, the selector of DownTown's Baby_Cry_1
            -- records (word 8; patch 0 selected another emitter sound): the replaced Baby_Cry_1 bank
            -- (a beep) answers while it is loaded, e.g. near a DownTown baby emitter.
            sdk.commands.request("post", {kind = "audio_post", key = "emit", class = "c_emitter", words = {32767, 32767, 0, 0, 4096, 25000, 0, 0, 22}})
            posted = true
        end
        if key("F7") and posted then
            sdk.audio.release("emit")
            posted = false
        end
        if key("F8") and (sdk.capabilities.audio or 0) >= 3 then
            -- A looping siren 8 m from where the skater is, through the native mixer (the default:
            -- no `native` field): the retail emitter distance law, reverb send and panner, the
            -- default reach (40 m, squared); a beep (no position: the non-positional branch, also
            -- native by default) marks the toggle.
            native_on = not native_on
            if native_on then
                local p = sdk.player.read().position
                sdk.commands.request("native", {kind = "audio_play", key = "native_siren", options = {
                    path = "audio/siren.wav", position = {p[1] + 8, p[2], p[3]}, loop = true}})
            else
                sdk.audio.stop("native_siren", 0.3)
            end
            sdk.commands.request("beep", {kind = "audio_play", key = "native_beep", options = {path = "audio/beep.wav", spatial = false, volume = 0.6}})
        end
        if key("F9") and (sdk.capabilities.audio_tuning or 0) >= 1 then
            -- Tuning writes: the taxi engine idles higher and the default reverb preset (reverb01,
            -- applied at its next selection) gets a reverb time of 3 (retail 1.5; value 5 = offset 20); F9
            -- again restores both (as the mod stopping would).
            tuned = not tuned
            sdk.commands.request("tune_world", {kind = "audio_set_tuning", domain = "world",
                patch = tuned and {traffic_engine = {c04_taxi01 = {idle_rpm = 1800}}} or nil})
            sdk.commands.request("tune_reverb", {kind = "audio_set_tuning", domain = "reverb",
                patch = tuned and {["A2782D75A971CC8C"] = {["5"] = 3.0}} or nil})
            sdk.audio.tuning("taxi", "world", "traffic_engine/c04_taxi01")
        end
        if key("F10") and (sdk.capabilities.world_audio or 0) >= 2 then
            -- A mod emitter (the Baby_Cry_1 bank this mod's audio.json replaces with a beep; patch
            -- 22) 6 m from the skater, reached within 15 m (retail's reach test and squared
            -- falloff, a c_emitter post on its own emitter instance: the default "extra" slots, so
            -- the map's emitters keep retail's 5 states), and a reverb zone (reverb11) 30 m around
            -- the skater. F10 again removes both.
            placed = not placed
            if placed then
                local p = sdk.player.read().position
                sdk.commands.request("emitter", {kind = "world_audio_spawn", key = "emitter", object = "emitter", options = {
                    bank = "Baby_Cry_1", patch = 22, position = {p[1] + 6, p[2], p[3]}, extent = {15, 15, 15}, core = 0.2, volume = 0.9}})
                sdk.commands.request("zone", {kind = "world_audio_spawn", key = "zone", object = "reverb_zone", options = {
                    preset = "BEEFC8E3DE04FBAE", position = p, extent = {30, 12, 30}}})
            else
                sdk.world_audio.remove("emitter")
                sdk.world_audio.remove("zone")
            end
        end
        if key("F11") and (sdk.capabilities.audio_events or 0) >= 2 then
            -- Runtime rules: mute the grind start (the Class_grind post is not made; the event row
            -- still arrives), and replace the landing with a beep at a fixed world position 10 m
            -- east of where the skater is now (`at = 'world'`: land anywhere and it comes from that
            -- spot, panned and rolling off with distance, silent beyond 30 m). audio.json's rule
            -- "pop_click" layers a quiet beep on every pop at the skater (the default `at = 'owner'`),
            -- "honk_beep" a beep 1.5 m above every honking car.
            quiet = not quiet
            sdk.audio.rule("quiet_grind", quiet and {match = {tag = "grind_start"}, action = "mute"} or nil)
            local p = sdk.player.read().position
            sdk.audio.rule("land_beacon", quiet and {match = {tag = "land"}, action = "replace",
                play = {path = "audio/beep.wav", volume = 0.8, at = "world", position = {p[1] + 10, p[2], p[3]},
                        falloff = {radius = 30}}} or nil)
        end
        if (sdk.capabilities.audio or 0) >= 4 then
            if key("Digit1") then
                -- L2: duck the world through retail's own controllers: the Master category gains
                -- (Global object 2, inputs 1..4; the host writes 32767) held at 8192 (-12 dB).
                ducked = not ducked
                for i = 1, 4 do
                    sdk.commands.request("duck" .. i, {kind = "audio_set_mixmap_input", slot = "global", object = 2, instance = 0, input = i, value = ducked and 8192 or nil})
                end
            end
            if key("Digit2") then
                -- L5: seed every audio generator (the same draws from here on each time); again
                -- releases it.
                seeded = not seeded
                sdk.audio.seed(seeded and 1234 or nil)
            end
        end
        if key("Digit3") and (sdk.capabilities.world_audio or 0) >= 3 then
            -- L3 / M3: six taxis driving a 6 m circle around where the skater is, at 6 m/s (real
            -- positions and velocities every frame, as retail's moving cars; an idle taxi at 9 m is
            -- about -44 dBFS before the volume settings: too quiet to judge), each on its own MixMap
            -- instance (retail's 4 traffic instances stay for the map's cars, and all six are heard):
            -- the default since world audio 4 (no slots); on 3 they ask for it.
            own_cars = not own_cars
            local p = sdk.player.read().position
            own_center, own_t = {p[1], p[2], p[3]}, 0
            for i = 1, 6 do
                if own_cars then
                    local a = i * math.pi / 3
                    sdk.commands.request("own" .. i, {kind = "world_audio_spawn", key = "own_car" .. i, object = "traffic", options = {
                        engine = "c04_taxi01", slots = (sdk.capabilities.world_audio or 0) < 4 and "own" or nil, position = {p[1] + 6 * math.cos(a), p[2], p[3] + 6 * math.sin(a)},
                        velocity = {-6 * math.sin(a), 0, 6 * math.cos(a)}}})
                else
                    sdk.world_audio.remove("own_car" .. i)
                end
            end
        end
        if key("Digit4") and (sdk.capabilities.audio_content or 0) >= 3 then
            -- L4: this mod's Csis project: post its class c_dev_mod (no bank binds it: a post that
            -- makes nothing, as retail's posts to an unbound class) and set its global g_dev_level
            -- (default 7) to 9; again releases both.
            dev_class = not dev_class
            if dev_class then
                sdk.commands.request("dev_post", {kind = "audio_post", key = "dev_class", class = "c_dev_mod", words = {1}})
            else
                sdk.audio.release("dev_class")
            end
            sdk.commands.request("dev_global", {kind = "audio_set_global", name = "g_dev_level", value = dev_class and 9 or nil})
        end
        if key("Digit5") and (sdk.capabilities.audio_events or 0) >= 3 then
            -- The offset in the owner's axes: a beep on every pop 2 m ahead of the board's nose
            -- (frame = 'owner'), wherever the skater faces.
            nose = not nose
            sdk.audio.rule("nose_beep", nose and {match = {tag = "pop"}, action = "layer",
                play = {path = "audio/beep.wav", volume = 0.6, offset = {0, 0, 2}, frame = "owner"}, min_interval = 0.2} or nil)
        end
        if key("Digit6") and (sdk.capabilities.world_audio or 0) >= 2 then
            -- A published emitter that keeps moving (orbiting the skater at 8 m every 6 s): its
            -- sound follows it.
            if orbit then
                sdk.world_audio.remove("orbit")
                orbit = nil
            else
                orbit = 0
                local p = sdk.player.read().position
                sdk.commands.request("orbit", {kind = "world_audio_spawn", key = "orbit", object = "emitter", options = {
                    bank = "Baby_Cry_1", patch = 22, position = {p[1] + 8, p[2], p[3]}, extent = {20, 20, 20}, volume = 0.9}})
            end
        end
        keep_t = keep_t + (event and event.dt or 0.016)
        if keep_t >= 0.25 then
            keep_t = 0
            if sdk.world_audio.read("f9_taxi") then sdk.world_audio.update("f9_taxi", {speed = 0}) end
        end
        if own_cars and own_center then
            own_t = own_t + (event and event.dt or 0.016)
            local c = own_center
            for i = 1, 6 do
                local a = i * math.pi / 3 + own_t -- 6 m/s on a 6 m circle: 1 rad/s
                sdk.world_audio.update("own_car" .. i, {position = {c[1] + 6 * math.cos(a), c[2], c[3] + 6 * math.sin(a)},
                    velocity = {-6 * math.sin(a), 0, 6 * math.cos(a)}, heading = math.atan(-math.sin(a), math.cos(a))})
            end
        end
        if orbit then
            orbit = orbit + (event and event.dt or 0.016)
            local p = sdk.player.read().position
            local a = orbit * 2 * math.pi / 6
            sdk.world_audio.update("orbit", {position = {p[1] + 8 * math.cos(a), p[2], p[3] + 8 * math.sin(a)}})
        end
        if key("F5") then
            global_set = not global_set
            sdk.commands.request("global", {kind = "audio_set_global", name = "babycry_1_sel_snd", value = global_set and 1 or nil})
        end
        hud()
        autotest_hud()
    end,
    on_unload = function()
        sdk.ui.text("audio-content-test", "")
        sdk.ui.text("audio-content-test-2", "")
        sdk.ui.remove("autotest")
        if at.ollie_t then sdk.input.override_action(68, nil) end
    end,
}
