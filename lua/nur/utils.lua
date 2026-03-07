-- nur.utils — small helpers for user configs.

local M = {}

-- Round a number to `digits` decimal places.
function M.round(n, digits)
    local factor = 10 ^ (digits or 0)
    return math.floor(n * factor + 0.5) / factor
end

-- Format bytes into a human-readable string (KB / MB / GB).
function M.fmt_bytes(bytes)
    if bytes < 1024 then
        return bytes .. " B"
    elseif bytes < 1024 * 1024 then
        return M.round(bytes / 1024, 1) .. " KB"
    elseif bytes < 1024 * 1024 * 1024 then
        return M.round(bytes / (1024 * 1024), 1) .. " MB"
    else
        return M.round(bytes / (1024 * 1024 * 1024), 2) .. " GB"
    end
end

-- Clamp `n` between `lo` and `hi`.
function M.clamp(n, lo, hi)
    return math.max(lo, math.min(hi, n))
end

-- Trim leading/trailing whitespace.
function M.trim(s)
    return s:match("^%s*(.-)%s*$")
end

return M
