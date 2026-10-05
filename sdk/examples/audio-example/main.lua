-- Audio example: the content overlay is audio.json (data, applied while this mod runs); this
-- script shows the runtime side: audio events (observe only) and a layered sound of its own.
local pops, landings = 0, 0

local function hud()
    local info = sdk.audio.info()
    local overlays = type(info.overlays) == "table" and #info.overlays or 0
    local map = type(info.map) == "table" and info.map.stem or ""
    sdk.ui.text("audio-example", string.format("Audio example: %d pops, %d landings  (audio mods running: %d%s)",
        pops, landings, overlays, map ~= "" and (", map " .. map) or ""))
end

return {
    on_load = function()
        if (sdk.capabilities.audio_events or 0) < 1 then
            sdk.ui.text("audio-example", "Audio example: this engine has no audio events")
            return
        end
        sdk.audio.preload("audio/tick.wav")
        -- Only the rows tagged pop / land reach this mod.
        sdk.audio.subscribe{tags = {"pop", "land"}}
        hud()
    end,
    on_update = function()
        local changed = false
        for _, e in ipairs(sdk.audio.events()) do
            if e.tag == "pop" then
                pops = pops + 1
                changed = true
                if sdk.settings.click then
                    sdk.audio.play("tick", {path = "audio/tick.wav", spatial = false, volume = 0.5})
                end
            elseif e.tag == "land" then
                landings = landings + 1
                changed = true
            end
        end
        if changed then hud() end
    end,
    on_event = function(event)
        if event.name == "world_changed" then hud() end
    end,
    on_unload = function()
        sdk.ui.text("audio-example", "")
    end,
}
