-- Dev test of the audio modding surface (PR #36). Content: audio.json. Runtime: retail posts,
-- a global, a MixMap watch and audio events, shown on the HUD.
local last = {}
local counts = {}
local posted, global_set = false, false
-- audio/moddability-2: the mod's own WAVs through the native mixer (F8), see doc 16 H.
local native_on = false
local tuned = false
local placed = false
local quiet = false
-- audio/moddability-2, third pass (doc 16 L1-L7): Digit1..Digit6.
local ducked, seeded, own_cars, dev_class, nose, orbit = false, false, false, false, false, nil
local pressed = {}

local function key(name)
    local down = sdk.snapshot.keys and sdk.snapshot.keys[name] == true
    local edge = down and not pressed[name]
    pressed[name] = down
    return edge
end

local function dev_level()
    return sdk.audio.global("g_dev_level")
end

local function hud()
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
        -- g_dev_level is the global of this mod's own Csis project (audio.json add.projects, doc 16
        -- L4); without it (an older engine) the watch fails, so it goes through a request.
        sdk.commands.request("watch", {kind = "audio_watch", globals = (sdk.capabilities.audio_content or 0) >= 3 and {"g_dev_level"} or {},
            mixmap = {{slot = "emitter", object = 0, instance = 0, output = 4}}})
        hud()
    end,
    on_update = function(event)
        if (sdk.capabilities.audio or 0) < 2 then return end
        for _, e in ipairs(sdk.audio.events()) do
            local name = e.tag or (e.kind .. ":" .. (e.slot ~= "" and e.slot or e.class))
            if not counts[name] then
                -- First row of each kind in the log too (unattended log checks).
                sdk.log("audio event " .. name .. " (" .. tostring(e.source) .. " " .. tostring(e.class) .. ")")
            end
            counts[name] = (counts[name] or 0) + 1
            table.insert(last, 1, name)
            if #last > 5 then table.remove(last) end
        end
        if key("F6") then
            -- The retail c_emitter class with the dry / send / pan / pitch / filter words of an
            -- unpositioned emitter at full level and patch 0 (the replaced Baby_Cry_1 bank answers
            -- when it is loaded, e.g. near a DownTown baby emitter).
            sdk.commands.request("post", {kind = "audio_post", key = "emit", class = "c_emitter", words = {32767, 32767, 0, 0, 4096, 25000, 0, 0, 0}})
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
            -- L3 / M3: six idling taxis around the skater, each on its own MixMap instance (retail's
            -- 4 traffic instances stay for the map's cars, and all six are heard): the default since
            -- world audio 4 (no slots); on 3 they ask for it.
            own_cars = not own_cars
            local p = sdk.player.read().position
            for i = 1, 6 do
                if own_cars then
                    local a = i * math.pi / 3
                    sdk.commands.request("own" .. i, {kind = "world_audio_spawn", key = "own_car" .. i, object = "traffic", options = {
                        engine = "c04_taxi01", slots = (sdk.capabilities.world_audio or 0) < 4 and "own" or nil, speed = 0, position = {p[1] + 9 * math.cos(a), p[2], p[3] + 9 * math.sin(a)}}})
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
    end,
    on_unload = function()
        sdk.ui.text("audio-content-test", "")
        sdk.ui.text("audio-content-test-2", "")
    end,
}
