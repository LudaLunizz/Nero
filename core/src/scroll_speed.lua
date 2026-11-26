-- Nero: two-finger scroll speed for touchpads (a libinput plugin).
-- Written by Nero Workspaces from its "2-finger scroll speed" setting; Mutter
-- loads it from ~/.config/libinput/plugins at login. Edit the setting, not this
-- file (it is rewritten, and removed again at 100 %).
--
-- How: libinput turns the movement of two fingers into scroll. While exactly
-- two fingers touch, every finger's position is moved FACTOR times as far as the
-- finger really moved, so libinput sees (and scrolls) FACTOR times the distance.
-- With one or three or more fingers the positions move 1:1 again, keeping the
-- offset they have, so nothing jumps when a finger is lifted or added.

local FACTOR = 1.0

-- evdev usages: (type << 16) | code, EV_ABS = 3.
local EV_ABS = 3 * 65536
local SLOT = EV_ABS + 0x2f -- ABS_MT_SLOT
local POS_X = EV_ABS + 0x35 -- ABS_MT_POSITION_X
local POS_Y = EV_ABS + 0x36 -- ABS_MT_POSITION_Y
local TRACKING_ID = EV_ABS + 0x39 -- ABS_MT_TRACKING_ID

local function watch(device)
    local usages = device:usages()
    if not (usages[SLOT] and usages[POS_X] and usages[POS_Y] and usages[TRACKING_ID]) then
        return
    end
    if device:udev_properties().ID_INPUT_TOUCHPAD ~= "1" then
        return
    end
    local absinfo = device:absinfos()
    local limits = { [POS_X] = absinfo[POS_X], [POS_Y] = absinfo[POS_Y] }
    local slots = {} -- slot number -> { active, real = {}, out = {} }
    local current = 0

    local function slot(n)
        local s = slots[n]
        if not s then
            s = { active = false, real = {}, out = {} }
            slots[n] = s
        end
        return s
    end

    device:connect("evdev-frame", function(_, frame, _)
        -- First the touches that start or end in this frame: they decide
        -- how many fingers are down for the movement in it.
        local n = current
        for _, e in ipairs(frame) do
            if e.usage == SLOT then
                n = e.value
            elseif e.usage == TRACKING_ID then
                local s = slot(n)
                s.active = e.value >= 0
                s.real, s.out = {}, {} -- a new touch starts where the finger is
            end
        end
        local fingers = 0
        for _, s in pairs(slots) do
            if s.active then
                fingers = fingers + 1
            end
        end
        local factor = fingers == 2 and FACTOR or 1

        local changed = false
        local result = {}
        for i, e in ipairs(frame) do
            local value = e.value
            if e.usage == SLOT then
                current = value
            elseif e.usage == POS_X or e.usage == POS_Y then
                local s = slot(current)
                local real, out = s.real[e.usage], s.out[e.usage]
                local new
                if real == nil then
                    new = value
                else
                    new = out + (value - real) * factor
                    local l = limits[e.usage]
                    if l then
                        new = math.max(l.minimum, math.min(l.maximum, new))
                    end
                end
                s.real[e.usage], s.out[e.usage] = value, new
                local rounded = math.floor(new + 0.5)
                if rounded ~= value then
                    value = rounded
                    changed = true
                end
            end
            result[i] = { usage = e.usage, value = value }
        end
        if changed then
            return result
        end
        return nil
    end)
    device:connect("device-removed", function(_)
        slots = {}
    end)
end

if libinput:register({ 1 }) == 1 then
    libinput:connect("new-evdev-device", function(device)
        watch(device)
    end)
end
