-- Full desktop setup: top bar + bottom status bar.

local Clock      = require("nur.widgets.clock")
local Battery    = require("nur.widgets.battery")
local Workspaces = require("nur.widgets.workspaces")

local clock      = Clock.new({ format = "%H:%M:%S" })
local tick = shell.state(0)
shell.once(500, function() tick:set(1) end)
shell.interval(1000, function() tick:set(tick:get() + 1) end)
local battery    = Battery.new()
local workspaces = Workspaces.new()

-- ---------------------------------------------------------------------------
-- Top bar
-- ---------------------------------------------------------------------------
-- Services used:
--   shell.services.sysinfo:get()    → { cpu_percent, memory_percent, memory_used_gb,
--                                       memory_total_gb, temperature, gpu_percent }
--   shell.services.audio:get()      → { volume (0.0–1.0), muted }
--   shell.services.network:get()    → { connected, ssid, strength }
--   shell.services.compositor:get() → { active_workspace, active_window, workspaces[] }

local theme = require("nur.theme")

local bar = shell.window({
    position  = "top",
    height    = 36,
    exclusive = true,
    font_size = 26,
})

bar:render(function()
    local si   = shell.services.sysinfo:get()
    local aud  = shell.services.audio:get()
    local net  = shell.services.network:get()
    local comp = shell.services.compositor:get()

    -- Left: workspaces + active window title
    local left = { workspaces:render() }
    if comp.active_window then
        left[#left + 1] = ui.text(comp.active_window)
    end

    -- Center: clock + sysinfo
    local center = {
        clock:render(),
        ui.text("t=" .. tick:get()),
        ui.text("CPU " .. si.cpu_percent .. "%"),
        ui.text("RAM " .. si.memory_percent .. "%"),
    }
    if si.gpu_percent ~= nil then
        center[#center + 1] = ui.text("GPU " .. si.gpu_percent .. "%")
    end
    if si.temperature ~= nil then
        center[#center + 1] = ui.text(si.temperature .. "°C")
    end

    -- Right: audio + network + battery
    local vol_icon = aud.muted and "󰖁" or "󰕾"
    local right = { ui.text(vol_icon .. " " .. math.floor(aud.volume * 100) .. "%") }
    if net.ssid then
        right[#right + 1] = ui.text(net.ssid)
    end
    right[#right + 1] = battery:render()

    return ui.bar_layout(left, center, right)
end)

-- ---------------------------------------------------------------------------
-- Bottom bar — greeting + app count
-- ---------------------------------------------------------------------------
-- shell.services.applications exposes the full XDG desktop entry list.
--   :get().apps      — array of { name, exec, icon, comment, keywords, categories }
--   :search(query)   — filtered subset, sorted by name/comment/keyword relevance
--   :launch(exec)    — spawn a process, stripping %f/%u/etc. field codes
--
-- Launch examples:
--   shell.services.applications:launch("firefox")
--   local results = shell.services.applications:search("terminal")
--   if results[1] then shell.services.applications:launch(results[1].exec) end

local greeting = shell.state("Good morning")

local function update_greeting()
    local hour = tonumber(os.date("%H"))
    if hour >= 18 then
        greeting:set("Good evening")
    elseif hour >= 12 then
        greeting:set("Good afternoon")
    else
        greeting:set("Good morning")
    end
end

update_greeting()
shell.interval(60 * 60 * 1000, update_greeting)

-- Reactively track installed app count
local app_count = shell.state(0)
shell.services.applications:subscribe(function()
    app_count:set(#shell.services.applications:get().apps)
end)
app_count:set(#shell.services.applications:get().apps)

local bottom = shell.window({
    position  = "bottom",
    height    = 36,
    exclusive = true,
    font_size = 26,
})

bottom:render(function()
    local count     = app_count:get()
    local count_str = count > 0 and (count .. " apps") or "scanning…"
    return ui.bar_layout(
        { ui.text(count_str) },
        { ui.text(greeting:get()) },
        {}
    )
end)
