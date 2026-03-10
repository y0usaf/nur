-- Full desktop setup: top bar + bottom sysmon bar.

local Clock      = require("nur.widgets.clock")
local Battery    = require("nur.widgets.battery")
local Workspaces = require("nur.widgets.workspaces")

local clock      = Clock.new({ format = "%a %d %b  %H:%M" })
local battery    = Battery.new()
local workspaces = Workspaces.new()

-- ---------------------------------------------------------------------------
-- System stats come from the sysinfo service (sysinfo crate, no shell-outs).
-- shell.services.sysinfo:get() returns:
--   { cpu_percent, memory_percent, memory_used_gb, memory_total_gb,
--     temperature (number|nil), gpu_percent (number|nil) }

-- ---------------------------------------------------------------------------
-- Top bar
-- ---------------------------------------------------------------------------

local bar = shell.window({
    position  = "top",
    height    = 36,
    exclusive = true,
    font_size = 26,
})

bar:render(function()
    local si = shell.services.sysinfo:get()
    local center = {
        clock:render(),
        ui.text("   "),
        ui.text("CPU " .. si.cpu_percent .. "%"),
        ui.text("  "),
        ui.text("RAM " .. si.memory_percent .. "%"),
    }
    if si.gpu_percent ~= nil then
        center[#center + 1] = ui.text("  ")
        center[#center + 1] = ui.text("GPU " .. si.gpu_percent .. "%")
    end
    if si.temperature ~= nil then
        center[#center + 1] = ui.text("  ")
        center[#center + 1] = ui.text(si.temperature .. "°C")
    end

    return ui.bar_layout(
        { workspaces:render() },
        center,
        {
            ui.text(shell.services.network:get().ssid or ""),
            ui.text("  "),
            battery:render(),
        }
    )
end)

-- ---------------------------------------------------------------------------
-- Bottom bar — greeting
-- ---------------------------------------------------------------------------

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

local bottom = shell.window({
    position  = "bottom",
    height    = 36,
    exclusive = true,
    font_size = 26,
})

bottom:render(function()
    return ui.bar_layout({}, { ui.text(greeting:get()) }, {})
end)
