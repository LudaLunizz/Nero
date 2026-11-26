-- Runs core/src/scroll_speed.lua (the two-finger scroll speed libinput plugin)
-- against a fake libinput and fake touchpad frames.
--   lua5.4 tests/test-scroll-plugin.lua        (from the repo root)

local here = (arg and arg[0] or "tests/test-scroll-plugin.lua"):match("^(.*)/[^/]*$") or "."
local f = assert(io.open(here .. "/../core/src/scroll_speed.lua"))
local template = f:read("a")
f:close()

local EV_ABS = 3 * 65536
local SLOT, X, Y, TID = EV_ABS + 0x2f, EV_ABS + 0x35, EV_ABS + 0x36, EV_ABS + 0x39
local failures = 0

local function check(cond, msg)
    if not cond then
        failures = failures + 1
        print("FAIL: " .. msg)
    end
end

-- Load the plugin with `factor` and give it one device; returns a function
-- that sends a frame and gives back what libinput would get.
local function plugin(factor, props, usages)
    local src = template:gsub("local FACTOR = 1%.0", "local FACTOR = " .. factor, 1)
    local cb = {}
    local libinput = {}
    function libinput:register(v) return v[1] end
    function libinput:connect(name, fn) cb[name] = fn end
    local frameCb
    local device = {}
    function device:usages() return usages or { [SLOT] = true, [X] = true, [Y] = true, [TID] = true } end
    function device:udev_properties() return props or { ID_INPUT_TOUCHPAD = "1" } end
    function device:absinfos()
        return { [X] = { minimum = -3678, maximum = 3934 }, [Y] = { minimum = -2478, maximum = 2587 } }
    end
    function device:connect(name, fn) if name == "evdev-frame" then frameCb = fn end end
    local env = { libinput = libinput, ipairs = ipairs, pairs = pairs, math = math }
    assert(load(src, "scroll_speed.lua", "t", env))()
    cb["new-evdev-device"](device)
    return function(frame)
        if not frameCb then return frame end
        return frameCb(device, frame, 0) or frame
    end
end

local function val(frame, usage, nth)
    local seen = 0
    for _, e in ipairs(frame) do
        if e.usage == usage then
            seen = seen + 1
            if seen == (nth or 1) then return e.value end
        end
    end
end

-- Two fingers down, both move 100 down: at 50 % they move 50.
do
    local send = plugin(0.5)
    send({ { usage = SLOT, value = 0 }, { usage = TID, value = 1 }, { usage = X, value = 0 }, { usage = Y, value = 0 },
        { usage = SLOT, value = 1 }, { usage = TID, value = 2 }, { usage = X, value = 500 }, { usage = Y, value = 0 } })
    local out = send({ { usage = SLOT, value = 0 }, { usage = Y, value = 100 }, { usage = SLOT, value = 1 }, { usage = Y, value = 100 } })
    check(val(out, Y, 1) == 50 and val(out, Y, 2) == 50, "two fingers at 50 %: " .. tostring(val(out, Y, 1)) .. "," .. tostring(val(out, Y, 2)))
    -- 10 small steps of 3: 15 in total, no rounding drift
    for i = 1, 10 do
        out = send({ { usage = SLOT, value = 0 }, { usage = Y, value = 100 + 3 * i }, { usage = SLOT, value = 1 }, { usage = Y, value = 100 + 3 * i } })
    end
    check(val(out, Y, 1) == 65, "small steps add up: " .. tostring(val(out, Y, 1)))
    -- lift finger 1: finger 0 keeps its offset (no jump) and moves 1:1
    out = send({ { usage = SLOT, value = 1 }, { usage = TID, value = -1 } })
    check(val(out, Y) == nil, "lifting sends no positions")
    out = send({ { usage = SLOT, value = 0 }, { usage = Y, value = 140 } })
    check(val(out, Y) == 75, "one finger after scroll moves 1:1 from its spot: " .. tostring(val(out, Y)))
    -- new touch starts at the real spot
    out = send({ { usage = SLOT, value = 0 }, { usage = TID, value = -1 } })
    out = send({ { usage = SLOT, value = 0 }, { usage = TID, value = 3 }, { usage = X, value = 10 }, { usage = Y, value = 20 } })
    check(val(out, X) == 10 and val(out, Y) == 20, "new touch at real position")
end

-- One finger and three fingers are not touched.
do
    local send = plugin(0.25)
    send({ { usage = SLOT, value = 0 }, { usage = TID, value = 1 }, { usage = X, value = 0 }, { usage = Y, value = 0 } })
    local out = send({ { usage = X, value = 300 } })
    check(val(out, X) == 300, "one finger 1:1")
    send({ { usage = SLOT, value = 1 }, { usage = TID, value = 2 }, { usage = X, value = 0 }, { usage = Y, value = 0 },
        { usage = SLOT, value = 2 }, { usage = TID, value = 3 }, { usage = X, value = 0 }, { usage = Y, value = 0 } })
    out = send({ { usage = SLOT, value = 1 }, { usage = X, value = 400 } })
    check(val(out, X) == 400, "three fingers 1:1: " .. tostring(val(out, X)))
end

-- Positions stay inside the device range.
do
    local send = plugin(0.5)
    send({ { usage = SLOT, value = 0 }, { usage = TID, value = 1 }, { usage = X, value = 3900 }, { usage = Y, value = 0 },
        { usage = SLOT, value = 1 }, { usage = TID, value = 2 }, { usage = X, value = 3000 }, { usage = Y, value = 0 } })
    send({ { usage = SLOT, value = 0 }, { usage = X, value = 3000 } }) -- out 3450, real 3000
    send({ { usage = SLOT, value = 1 }, { usage = TID, value = -1 } })
    local out = send({ { usage = SLOT, value = 0 }, { usage = X, value = 3930 } }) -- 3450 + 930 > max
    check(val(out, X) == 3934, "clamped to maximum: " .. tostring(val(out, X)))
end

-- 0 % stops two-finger movement completely.
do
    local send = plugin(0)
    send({ { usage = SLOT, value = 0 }, { usage = TID, value = 1 }, { usage = X, value = 0 }, { usage = Y, value = 0 },
        { usage = SLOT, value = 1 }, { usage = TID, value = 2 }, { usage = X, value = 500 }, { usage = Y, value = 0 } })
    local out = send({ { usage = SLOT, value = 0 }, { usage = Y, value = 100 }, { usage = SLOT, value = 1 }, { usage = Y, value = 100 } })
    check(val(out, Y, 1) == 0 and val(out, Y, 2) == 0, "0 % holds still")
end

-- Other devices are left alone.
do
    local send = plugin(0.5, { ID_INPUT_MOUSE = "1" })
    local frame = { { usage = SLOT, value = 0 }, { usage = Y, value = 100 } }
    check(send(frame) == frame, "a mouse with MT axes is not a touchpad")
    send = plugin(0.5, nil, { [EV_ABS] = true })
    check(send(frame) == frame, "a device without MT axes is skipped")
end

-- Unchanged frames pass through (nil = as they are).
do
    local send = plugin(0.5)
    local frame = { { usage = SLOT, value = 0 }, { usage = TID, value = 1 }, { usage = X, value = 7 }, { usage = Y, value = 8 } }
    check(send(frame) == frame, "a landing finger passes through unchanged")
end

if failures > 0 then
    error(failures .. " scroll plugin check(s) failed")
end
print("scroll plugin: all checks passed")
