-- Dev only (doc 42, a mod's own challenges): an own the spot challenge around where the player starts, started
-- through sdk.challenges, with CHALLENGE_TEST lines for the list entry and the run.
local added = false
local started = false
local reported = 0
local id = "mod:" .. sdk.mod_id .. "/plaza"

return {
  on_update = function()
    local player = sdk.player.read()
    local p = player and player.position
    if not p then return end
    if not added then
      added = true
      local x, y, z = p[1], p[2], p[3]
      sdk.challenges.add("plaza", {
        title = "Mod Plaza",
        boundary = { center = { x, y, z }, size = { 120, 40, 120 } },
        spot = { center = { x + 10, y, z }, size = { 16, 8, 16 } },
        start = { position = { x, y, z } },
        owned_points = 200,
        killed_points = 400,
      })
      sdk.log(string.format("CHALLENGE_TEST added %s at %.1f %.1f %.1f", id, x, y, z))
      return
    end
    local c = sdk.challenges.get(id)
    if c and not started then
      started = true
      sdk.log("CHALLENGE_TEST listed " .. c.id .. " family " .. tostring(c.family) .. " owned " .. tostring(c.owned_points) .. " killed " .. tostring(c.killed_points))
      sdk.challenges.start(id)
      return
    end
    local run = sdk.challenges.run()
    if started and run and reported < 3 then
      reported = reported + 1
      sdk.log("CHALLENGE_TEST run " .. tostring(run.id) .. " points " .. tostring(run.points) .. " requirement " .. tostring(run.requirement))
    end
  end,
}
