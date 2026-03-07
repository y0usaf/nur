-- Simple bar: clock center, battery right.
-- Catppuccin Mocha palette.

local Clock   = require("nur.widgets.clock")
local Battery = require("nur.widgets.battery")

local clock   = Clock.new({ format = "%H:%M" })
local battery = Battery.new()

local bar = shell.window({
    position  = "top",
    height    = 32,
    exclusive = true,
    bg        = "#1e1e2e",  -- Catppuccin Mocha base
    fg        = "#cdd6f4",  -- Catppuccin Mocha text
    font_size = 13,
})

bar:render(function()
    return ui.bar_layout(
        { ui.text("  ") },          -- left padding
        { clock:render() },          -- center: clock
        { battery:render(), ui.text("  ") }  -- right: battery
    )
end)
