-- Dev only (doc 26h, Routes for mods): a two-way square loop of four roads beside where the player starts, then
-- the live network's counts in ROADS_TEST lines.
local done = false
local reported = false

return {
  on_update = function()
    if not done then
      local player = sdk.player.read()
      local p = player and player.position
      if not p then return end
      done = true
      local x, y, z = p[1] + 30, p[2], p[3]
      local corners = { { x, y, z }, { x + 120, y, z }, { x + 120, y, z + 120 }, { x, y, z + 120 } }
      local roads = {}
      for i = 1, 4 do
        local j = i % 4 + 1
        roads["loop_" .. i] = { from = "corner_" .. i, to = "corner_" .. j, points = { corners[i], corners[j] }, lanes = 1, speed_limit = 9,
          traffic = { cars = 2 } }
      end
      sdk.world.set_tuning("roads", { roads = roads })
      sdk.world.tuning("roads_live", "roads")
      sdk.log(string.format("ROADS_TEST loop set at %.1f %.1f %.1f", x, y, z))
      return
    end
    if not reported then
      local r = sdk.commands.result("roads_live")
      if r then
        reported = true
        local v = r.value or {}
        sdk.log("ROADS_TEST inspect segments " .. tostring(v.segments) .. " junctions " .. tostring(v.junctions) .. " revision " .. tostring(v.revision))
      end
    end
  end,
}
