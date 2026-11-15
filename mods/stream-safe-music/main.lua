-- Stream-safe Music: the launcher's stream-safe playlist in place of the soundtrack, title and artist always shown
-- (README.md). The launcher writes playlists/stream-safe.json before each start.
local started, checked, wait = false, false, 0

local function setting(name, default)
  local v = sdk.settings and sdk.settings[name]
  if v == nil then return default end
  return v
end

local function start()
  sdk.music.play_playlist("playlists/stream-safe.json", {soundtrack = false, shuffle = setting("shuffle", true)})
  local volume = setting("volume", 1)
  if volume ~= 1 then sdk.music.set_volume(volume) end
  sdk.music.caption{enabled = true}
  sdk.log("STREAM_SAFE_MUSIC playlist replacing the soundtrack")
end

return {
  on_update = function(event)
    if checked then return end
    if not started then
      start()
      started, wait = true, 3
      return
    end
    wait = wait - ((event and tonumber(event.dt)) or 0)
    if wait > 0 then return end
    checked = true
    local s = sdk.music.state()
    if s.owner == nil or not s.on then
      sdk.log("STREAM_SAFE_MUSIC the playlist did not load: turn Stream-safe mode off and on in the launcher (Tools)")
      sdk.ui.central_message("Stream-safe Music: no music yet (launcher > Tools > Stream-safe mode)", 6)
    end
  end,
  on_event = function(event)
    if type(event) == "table" and event.name == "music_track_started" then
      sdk.log(string.format("STREAM_SAFE_MUSIC playing %s - %s", tostring(event.artist), tostring(event.title)))
    end
  end,
}
