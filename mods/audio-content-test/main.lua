-- Dev test of the audio modding surface (PR #36). Content: audio.json. Runtime: retail posts,
-- a global, a MixMap watch and audio events, shown on the HUD.
local last = {}
local counts = {}
local posted, global_set = false, false
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
        "Audio content test: restarts %s, generation %s, conflicts %s | post %s | global %s | emitter 0 out4 %s | %s | last: %s  [F5 global, F6 post, F7 release]",
        tostring(info.restarts), tostring(info.generation), tostring(info.conflicts),
        h and (h.live and "live" or "dead") or "-", global_set and "set" or "-",
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
