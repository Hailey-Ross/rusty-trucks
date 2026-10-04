-- World Audio Test (dev only, PR #32; spec audio-specs/world-audio-hookin-spec.md §3.10 / §8.5).
-- Publishes traffic, pedestrians and a ghost NPC skater through sdk.world_audio around the spot
-- where the mod starts (F9 re-centres on the skater). The game decides who is audible with
-- retail's limits (4 nearest cars within 40 m, 15 nearest peds within 50 m with footsteps for
-- the nearest 3, one NPC skater within 30 m); the boxes show it: green = audible, grey = not.
-- A mod's cars and peds take their own MixMap instances by default (world audio 4); this test asks
-- for retail's pools (slots = 'shared') unless the "own_slots" setting is on.
-- Not retail behaviour: the motion, the gait clock, the honk timing and the reactions are this
-- script's; only the sounds and their rules are the game's port of retail.

local ENGINES = { "c00_heavy01", "c01_family01", "c03_sports01", "c04_taxi01", "c05_truck01",
                  "c06_sports02", "c07_family02", "c08_family03", "c01_family01" }
local LANES = 4
local CARS_PER_LANE = 4 -- the last lane's last car is parked (the alarm car)
local PEDS = 20
-- The living-world ped models (= speech voices) of retail's aud_characteristics: each ped takes one,
-- and the game gives it the model's shoe class, kind and speech words.
local VOICES = { 41, 42, 43, 46, 47, 48, 49, 51, 52, 53, 54, 55, 56, 59, 60, 61, 64, 65, 66, 69, 70, 71,
                 72, 73, 74, 75, 76, 77, 82, 83, 84, 85, 86, 87, 88 }
-- The ghost skater's voice (an AI skater's: its bail grunt), and the pro F2 switches it to
-- (Ryan Smith's model: the main-cast channel).
local GHOST_VOICE = 91
local GHOST_PRO = 24
local ghost_pro = false

local anchor = nil        -- {x, y, z}
local started = false
local cars, peds = {}, {}
local held = {}
local honk_timer = 12
local seed = 12345
local shown = {}          -- key -> last box colour state
local ghost_status = ""
local last_landing, last_bail = nil, nil
local warned = {}         -- ped key -> seconds until it may warn again
local chatter_timer = 6   -- seconds to the next ambient shout (dev only, not retail)
local falls = {}          -- pending BodyFallType keys: {ped key, seconds until sent, kind}
local photographer = nil  -- the ped holding PictureTaking (29) with the photo flag
-- A retail knock-down's BodyFallType keys (recomp sessions 163809 / 164620: 9, then 0.10 s later a
-- type-other key, 0.48 s later 9, 0.16 s later 9).
local KNOCK_DOWN = { { 0, 9 }, { 0.10, 1 }, { 0.58, 9 }, { 0.74, 9 } }

local function rand()
    seed = (seed * 1103515245 + 12345) % 2147483648
    return seed / 2147483648
end

local function setting(name, default)
    local v = sdk.settings and sdk.settings[name]
    if v == nil then return default end
    return v
end

-- Retail's pools unless the setting asks for the default own instances; world audio 3 and older
-- default to the pools anyway.
local function slots()
    if setting("own_slots", false) or (sdk.capabilities.world_audio or 0) < 4 then return nil end
    return "shared"
end

local function dist(a, b)
    local dx, dy, dz = a[1] - b[1], a[2] - b[2], a[3] - b[3]
    return math.sqrt(dx * dx + dy * dy + dz * dz)
end

-- A lane is a rectangle around the anchor; s = metres along its perimeter.
local function lane_size(l) return 26 + 7 * l, 16 + 6 * l end
local function lane_point(l, s)
    local hx, hz = lane_size(l)
    local w, h = 2 * hx, 2 * hz
    local p = 2 * (w + h)
    s = s % p
    local x, z, heading
    if s < w then x, z, heading = -hx + s, -hz, math.pi / 2
    elseif s < w + h then x, z, heading = hx, -hz + (s - w), 0
    elseif s < 2 * w + h then x, z, heading = hx - (s - w - h), hz, -math.pi / 2
    else x, z, heading = -hx, hz - (s - 2 * w - h), math.pi end
    if l % 2 == 1 then heading = heading end
    return { anchor[1] + x, anchor[2] + 0.5, anchor[3] + z }, heading, p
end

local function box(key, position, heading, scale, audible)
    if not setting("boxes", true) then return end
    local state = audible and 1 or 0
    local half = heading / 2
    local rotation = { 0, math.sin(half), 0, math.cos(half) }
    if shown[key] ~= state then
        shown[key] = state
        sdk.graphics.mesh("box_" .. key, { path = "", position = position, rotation = rotation, scale = scale,
            color = audible and { 0.2, 0.9, 0.3 } or { 0.5, 0.5, 0.5 }, opacity = 0.6 })
    else
        sdk.graphics.set_transform("box_" .. key, { position = position, rotation = rotation })
    end
end

local function spawn_all()
    local player = sdk.player.read()
    anchor = { player.position[1], player.position[2], player.position[3] }
    started = true
    cars, peds, shown, warned = {}, {}, {}, {}
    if setting("cars", true) then
        local n = 0
        for l = 0, LANES - 1 do
            local _, _, perimeter = lane_point(l, 0)
            for c = 0, CARS_PER_LANE - 1 do
                n = n + 1
                local key = "car" .. n
                local parked = (l == LANES - 1 and c == CARS_PER_LANE - 1)
                local car = { key = key, lane = l, s = perimeter * c / CARS_PER_LANE, speed = 0,
                    cruise = (8 + 7 * rand()) * setting("speed_scale", 1), phase = "accelerate", timer = 0,
                    engine = ENGINES[(n - 1) % #ENGINES + 1], parked = parked, perimeter = perimeter }
                local p, h = lane_point(l, car.s)
                if parked then
                    p = { anchor[1] + 8, anchor[2] + 0.5, anchor[3] + 6 }
                    h = 0
                    car.engine = "c04_taxi01"
                end
                car.position, car.heading = p, h
                sdk.world_audio.spawn(key, "traffic", { engine = car.engine, position = p, heading = h, speed = 0, slots = slots() })
                cars[#cars + 1] = car
            end
        end
    end
    if setting("peds", true) then
        for i = 1, PEDS do
            local kind = (i == PEDS) and "run" or ((i >= PEDS - 3) and "jog" or "walk")
            local speed = ({ walk = 1.3, jog = 4.0, run = 8.0 })[kind]
            local angle = i * 2.399
            local radius = 4 + (i % 7) * 3
            local start = { anchor[1] + math.cos(angle) * radius, anchor[2], anchor[3] + math.sin(angle) * radius }
            local key = "ped" .. i
            local ped = { key = key, start = start, dir = { math.cos(angle + 1.3), 0, math.sin(angle + 1.3) },
                length = 6 + (i % 4) * 3, t = rand() * 10, speed = speed,
                step = ({ walk = 0.55, jog = 0.36, run = 0.27 })[kind], voice = VOICES[1 + (i * 7) % #VOICES],
                x = 0, sign = 1 }
            sdk.world_audio.spawn(key, "ped", { voice = ped.voice, position = start, slots = slots() })
            peds[#peds + 1] = ped
        end
    end
    if setting("ghost", true) then
        local name = tostring(setting("ghost_log", "state_20261003_143434"))
        -- Through a request: a missing log is reported here instead of failing the mod.
        sdk.commands.request("ghost", { kind = "world_audio_spawn", key = "ghost", object = "skater",
            options = { source = "state_log:" .. name, from = setting("ghost_from", 30), seconds = 20,
                voice = GHOST_VOICE, position = { anchor[1] + 5, anchor[2], anchor[3] } } })
        ghost_status = "Ghost: loading " .. name
    end
end

local function remove_all()
    for _, c in ipairs(cars) do sdk.world_audio.remove(c.key); sdk.graphics.remove("box_" .. c.key) end
    for _, p in ipairs(peds) do sdk.world_audio.remove(p.key); sdk.graphics.remove("box_" .. p.key) end
    sdk.world_audio.remove("ghost")
    sdk.graphics.remove("box_ghost")
    cars, peds, shown = {}, {}, {}
end

local function drive(car, dt)
    if car.parked then
        sdk.world_audio.update(car.key, { position = car.position, heading = car.heading, speed = 0, load = 0, skidding = false })
        return
    end
    -- Accelerate to cruise, cruise 3-8 s, brake hard to 3 m/s (skids below -8 m/s²), repeat.
    local load = 0
    car.timer = car.timer - dt
    if car.phase == "accelerate" then
        load = 2.5
        if car.speed >= car.cruise then car.phase, car.timer = "cruise", 3 + 5 * rand() end
    elseif car.phase == "cruise" then
        if car.timer <= 0 then car.phase = "brake"; car.brake = (rand() < 0.4) and -12 or -4 end
    else
        load = car.brake
        if car.speed <= 3 then car.phase = "accelerate" end
    end
    car.speed = math.max(0, math.min(car.cruise, car.speed + load * dt))
    car.s = car.s + car.speed * dt
    local p, h = lane_point(car.lane, car.s)
    local v = { math.sin(h) * car.speed, 0, math.cos(h) * car.speed }
    car.position, car.heading = p, h
    sdk.world_audio.update(car.key, { position = p, heading = h, velocity = v, speed = car.speed, load = load,
        skidding = load < -8 and car.speed > 4 })
end

local function walk(ped, dt, player)
    ped.t = ped.t + dt
    ped.x = ped.x + ped.sign * ped.speed * dt
    if ped.x > ped.length then ped.x, ped.sign = ped.length, -1 elseif ped.x < 0 then ped.x, ped.sign = 0, 1 end
    local p = { ped.start[1] + ped.dir[1] * ped.x, ped.start[2], ped.start[3] + ped.dir[3] * ped.x }
    local v = { ped.dir[1] * ped.speed * ped.sign, 0, ped.dir[3] * ped.speed * ped.sign }
    -- Gait clock (dev only, not retail): alternate feet, each planted for 60 % of its step.
    local phase = (ped.t % (2 * ped.step)) / ped.step
    local feet = { phase < 0.6, phase >= 1 and phase < 1.6 }
    ped.position = p
    sdk.world_audio.update(ped.key, { position = p, velocity = v, heading = math.atan(v[1], v[3]), feet = feet })
    -- Warn (the code's value 53, `501_warn`) when the skater passes within 1.5 m above 3 m/s.
    warned[ped.key] = math.max(0, (warned[ped.key] or 0) - dt)
    if warned[ped.key] == 0 and (player.speed or 0) > 3 and dist(p, player.position) < 1.5 then
        sdk.world_audio.event(ped.key, "speech", { value = "warn" })
        warned[ped.key] = 5
    end
end

local function react(player)
    -- A landed trick within 10 m: cheer (23); a bail: slam (25).
    local landing, bail = player.landing_seq or 0, player.bail_seq or 0
    local event = nil
    if last_landing ~= nil and landing ~= last_landing then event = "cheer" end
    if last_bail ~= nil and bail ~= last_bail then event = "slam" end
    last_landing, last_bail = landing, bail
    if not event then return end
    for _, ped in ipairs(peds) do
        if ped.position and dist(ped.position, player.position) < 10 then
            sdk.world_audio.event(ped.key, "speech", { value = event })
        end
    end
end

local function nearest_ped(player)
    local best, best_d = nil, 1e9
    for _, ped in ipairs(peds) do
        if ped.position then
            local d = dist(ped.position, player.position)
            if d < best_d then best, best_d = ped, d end
        end
    end
    return best
end

local function pressed(key)
    local down = sdk.input.down(key) == true
    local edge = down and not held[key]
    held[key] = down
    return edge
end

return {
    on_update = function(event)
        local dt = (event and tonumber(event.dt)) or 0
        if not started then
            if not (sdk.capabilities.world_audio and sdk.capabilities.world_audio >= 1) then
                sdk.ui.text("world-audio-test", "World audio test: this game has no sdk.world_audio")
                started = true
                return
            end
            spawn_all()
            return
        end
        if not anchor then return end
        local player = sdk.player.read()
        if pressed("F9") then remove_all(); spawn_all(); return end
        if pressed("F8") then
            for _, c in ipairs(cars) do if c.parked then sdk.world_audio.event(c.key, "alarm") end end
        end
        -- F7: the nearest ped zaps its tazer (retail's c_tazer burst, the state graph's 2 s hold).
        if pressed("F7") then
            local ped = nearest_ped(player)
            if ped then sdk.world_audio.event(ped.key, "tazer") end
        end
        -- F6: the nearest ped is knocked down (a retail knock-down's BodyFallType keys).
        if pressed("F6") then
            local ped = nearest_ped(player)
            if ped then
                for _, k in ipairs(KNOCK_DOWN) do falls[#falls + 1] = { ped.key, k[1], k[2] } end
            end
        end
        -- F5: the nearest ped's phone rings (speech value 49); it answers when the ring ends.
        if pressed("F5") then
            local ped = nearest_ped(player)
            if ped then sdk.world_audio.event(ped.key, "speech", { value = 49 }) end
        end
        -- F3: the ghost saw your trick (its voice's `101_pos`); F2: it switches between its AI voice
        -- and a pro's (24, the main cast), then crashes (`131_collide_object` / `906_aislm`).
        if pressed("F3") then sdk.world_audio.event("ghost", "reaction", { value = "trick", by = 0 }) end
        if pressed("F2") then
            ghost_pro = not ghost_pro
            sdk.world_audio.update("ghost", { voice = ghost_pro and GHOST_PRO or GHOST_VOICE })
            sdk.world_audio.event("ghost", "reaction", { value = "crash" })
        end
        -- F4: the nearest ped becomes a photographer (PictureTaking, 29, with the game flag:
        -- the request repeats every second, gated by the event's timers); F4 again stops it.
        if pressed("F4") then
            if photographer then
                sdk.world_audio.update(photographer, { photo_flag = false })
                sdk.world_audio.event(photographer, "speech", { value = 0 })
                photographer = nil
            else
                local ped = nearest_ped(player)
                if ped then
                    photographer = ped.key
                    sdk.world_audio.update(ped.key, { photo_flag = true })
                    sdk.world_audio.event(ped.key, "speech", { value = 29 })
                end
            end
        end
        for i = #falls, 1, -1 do
            local f = falls[i]
            f[2] = f[2] - dt
            if f[2] <= 0 then
                sdk.world_audio.event(f[1], "body_fall", { kind = f[3] })
                table.remove(falls, i)
            end
        end
        for _, car in ipairs(cars) do drive(car, dt) end
        for _, ped in ipairs(peds) do walk(ped, dt, player) end
        react(player)
        -- Ambient chatter (dev only): every 6-10 s the nearest ped shouts (value 2, `1901_shout`).
        chatter_timer = chatter_timer - dt
        if chatter_timer <= 0 and #peds > 0 then
            chatter_timer = 6 + 4 * rand()
            local best, best_d = nil, 1e9
            for _, ped in ipairs(peds) do
                if ped.position then
                    local d = dist(ped.position, player.position)
                    if d < best_d then best, best_d = ped, d end
                end
            end
            if best then
                sdk.world_audio.event(best.key, "speech", { value = 2 })
            end
        end
        -- One moving car honks every 10-15 s (kind 1-5).
        honk_timer = honk_timer - dt
        if honk_timer <= 0 and #cars > 1 then
            honk_timer = 10 + 5 * rand()
            local car = cars[1 + math.floor(rand() * (#cars - 1))]
            sdk.world_audio.event(car.key, "horn", { kind = 1 + math.floor(rand() * 5), seconds = 0.4 + rand() })
        end
        -- Boxes and the readout.
        local audible = { traffic = 0, ped = 0, skater = 0 }
        for _, c in ipairs(cars) do
            local r = sdk.world_audio.read(c.key)
            local on = r and r.audible
            if on then audible.traffic = audible.traffic + 1 end
            box(c.key, c.position, c.heading, { 1.8, 1.4, 4.2 }, on)
        end
        for _, p in ipairs(peds) do
            local r = sdk.world_audio.read(p.key)
            local on = r and r.audible
            if on then audible.ped = audible.ped + 1 end
            if p.position then box(p.key, { p.position[1], p.position[2] + 0.9, p.position[3] }, 0, { 0.5, 1.8, 0.5 }, on) end
        end
        local result = sdk.commands.result("ghost")
        if result then
            if result.ok then
                local r = sdk.world_audio.read("ghost")
                if r and r.audible then audible.skater = 1 end
                ghost_status = "Ghost: " .. ((r and r.audible) and "audible" or "out of range (30 m)")
            else
                ghost_status = "Ghost: " .. tostring(result.error)
            end
        end
        local info = sdk.world_audio.info()
        sdk.ui.text("world-audio-test", string.format(
            "World audio test: cars %d/%d audible, peds %d/%d, skater %d, speech lines %d  (limits %d/%d/%d%s)  %s  [F4 photographer, F5 phone, F6 knock-down, F7 tazer, F8 alarm, F9 re-centre]",
            audible.traffic, #cars, audible.ped, #peds, audible.skater, info.speech_lines or 0,
            info.instances and info.instances.traffic or 4, info.instances and info.instances.peds or 15,
            info.instances and info.instances.skaters or 1, info.more_audible and ", more audible" or "", ghost_status))
    end,
    on_unload = function()
        sdk.ui.text("world-audio-test", "")
    end,
}
