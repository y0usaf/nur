-- nur.widgets.battery
-- Displays battery percentage and charging status.
--
-- Usage:
--   local Battery = require("nur.widgets.battery")
--   local bat = Battery.new()
--   -- In a render function:
--   bat:render()

local M = {}

function M.new(opts)
    opts = opts or {}

    local self = {}
    self._state = shell.state(shell.services.battery)

    -- Refresh every 30 s (battery doesn't change that fast)
    shell.interval(30000, function()
        self._state:set(shell.services.battery)
    end)

    function self:render()
        local bat = self._state:get()
        local pct = bat.percent or 0
        local icon = bat.charging and "battery-charging" or (
            pct > 80 and "battery-full"    or
            pct > 40 and "battery-medium"  or
            pct > 15 and "battery-low"     or
            "battery-warning"
        )
        return ui.hbox({ gap = 4, children = {
            ui.icon(icon),
            ui.text(pct .. "%"),
        }})
    end

    return self
end

return M
