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
local pressed = {}

local function key(name)
    local down = sdk.snapshot.keys and sdk.snapshot.keys[name] == true
    local edge = down and not pressed[name]
    pressed[name] = down
    return edge
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
end

return {
    on_load = function()
        if (sdk.capabilities.audio or 0) < 2 then
            sdk.ui.text("audio-content-test", "Audio content test: this engine has no audio API 2")
            return
        end
        if sdk.settings.events then sdk.audio.subscribe{tags = {}} end
        sdk.audio.watch{mixmap = {{slot = "emitter", object = 0, instance = 0, output = 4}}}
        hud()
    end,
    on_update = function()
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
        if key("F5") then
            global_set = not global_set
            sdk.commands.request("global", {kind = "audio_set_global", name = "babycry_1_sel_snd", value = global_set and 1 or nil})
        end
        hud()
    end,
    on_unload = function()
        sdk.ui.text("audio-content-test", "")
    end,
}
