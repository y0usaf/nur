-- Full desktop setup: top bar + notification overlay.

local Clock      = require("nur.widgets.clock")
local Battery    = require("nur.widgets.battery")
local Workspaces = require("nur.widgets.workspaces")
local utils      = require("nur.utils")

local clock      = Clock.new({ format = "%a %d %b  %H:%M" })
local battery    = Battery.new()
local workspaces = Workspaces.new()

-- ---------------------------------------------------------------------------
-- Top bar
-- ---------------------------------------------------------------------------

local bar = shell.window({
    position  = "top",
    height    = 32,
    exclusive = true,
})

bar:render(function()
    return ui.bar_layout(
        { workspaces:render() },
        { clock:render()      },
        {
            ui.text(shell.services.network.ssid or ""),
            ui.text("  "),
            battery:render(),
        }
    )
end)

-- ---------------------------------------------------------------------------
-- Example: dynamic greeting based on time of day
-- ---------------------------------------------------------------------------

local greeting = shell.state("Good morning")

shell.interval(60 * 60 * 1000, function()
    local hour = tonumber(os.date("%H"))
    if hour >= 18 then
        greeting:set("Good evening")
    elseif hour >= 12 then
        greeting:set("Good afternoon")
    else
        greeting:set("Good morning")
    end
end)

-- ---------------------------------------------------------------------------
-- Example: custom bottom overlay (no exclusive zone — floats above content)
-- ---------------------------------------------------------------------------

local bottom = shell.window({
    position  = "bottom",
    height    = 24,
    exclusive = false,
    layer     = "overlay",
})

bottom:render(function()
    local mem = shell.exec("free -m | awk '/^Mem/ { print $3 \"/\" $2 \" MB\" }'")
    return ui.hbox({ children = { ui.text(mem) } })
end)
