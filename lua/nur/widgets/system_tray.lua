-- nur.widgets.system_tray
-- AGS-like StatusNotifierItem tray renderer.

local theme = require("nur.theme")

local M = {}
local icon_cache = {}

local function shquote(s)
    s = tostring(s or "")
    return "'" .. s:gsub("'", "'\\''") .. "'"
end

local function add_unique(list, seen, value)
    if value and value ~= "" and not seen[value] then
        seen[value] = true
        list[#list + 1] = value
    end
end

local function icon_dirs()
    local dirs, seen = {}, {}
    local home = os.getenv("HOME") or ""
    local xdg_data_home = os.getenv("XDG_DATA_HOME") or (home .. "/.local/share")

    add_unique(dirs, seen, xdg_data_home .. "/icons")
    add_unique(dirs, seen, home .. "/.local/share/icons")
    add_unique(dirs, seen, home .. "/.icons")

    local data_dirs = os.getenv("XDG_DATA_DIRS") or "/usr/local/share:/usr/share"
    for dir in data_dirs:gmatch("[^:]+") do
        add_unique(dirs, seen, dir .. "/icons")
    end

    add_unique(dirs, seen, "/run/current-system/sw/share/icons")
    add_unique(dirs, seen, "/usr/local/share/icons")
    add_unique(dirs, seen, "/usr/share/icons")
    add_unique(dirs, seen, "/usr/share/pixmaps")

    return dirs
end

local function file_exists(path)
    if not path or path == "" then return false end
    return shell.exec("test -f " .. shquote(path) .. " && printf yes || true") == "yes"
end

local function cache_absolute_icon(path)
    -- Some SNI providers expose temporary /run/user tray-icon PNGs and then
    -- replace/unlink them before GPUI's async image loader opens the file. Copy
    -- absolute icon paths to a Nur-owned stable cache first.
    local runtime = os.getenv("XDG_RUNTIME_DIR") or "/tmp"
    local cache_dir = runtime .. "/nur-tray-icon-cache"
    local key = path:gsub("[^%w%._%-]", "_")
    if #key > 180 then key = key:sub(#key - 179) end
    local dest = cache_dir .. "/" .. key
    local cmd = "mkdir -p " .. shquote(cache_dir)
        .. " && cp -f " .. shquote(path) .. " " .. shquote(dest) .. " 2>/dev/null"
        .. " && printf %s " .. shquote(dest)
        .. " || true"
    local copied = shell.exec(cmd)
    if copied ~= "" and file_exists(copied) then return copied end
    return nil
end

function M.resolve_icon_path(name)
    name = tostring(name or "")
    if name == "" then return nil end

    if name:sub(1, 1) == "/" then
        local cached = icon_cache[name]
        if cached and cached ~= false and file_exists(cached) then return cached end
        if file_exists(name) then
            local stable = cache_absolute_icon(name)
            if stable then
                icon_cache[name] = stable
                return stable
            end
        end
        -- Do not cache misses for volatile absolute paths; providers may create
        -- the file shortly after publishing the item.
        return nil
    end

    if icon_cache[name] ~= nil then
        return icon_cache[name] ~= false and icon_cache[name] or nil
    end

    local base = name:gsub("%.[%w]+$", "")
    local patterns = {
        name,
        base,
        base .. ".svg",
        base .. ".png",
        base .. ".xpm",
    }

    local clauses = {}
    local seen = {}
    for _, pattern in ipairs(patterns) do
        if pattern ~= "" and not seen[pattern] then
            seen[pattern] = true
            clauses[#clauses + 1] = "-name " .. shquote(pattern)
        end
    end

    local find_predicate = "\\( " .. table.concat(clauses, " -o ") .. " \\)"
    local commands = {}
    for _, dir in ipairs(icon_dirs()) do
        commands[#commands + 1] = "if [ -d " .. shquote(dir) .. " ]; then find " .. shquote(dir) .. " -type f " .. find_predicate .. " -print 2>/dev/null; fi"
    end

    -- Use the first matching themed icon. Cache results so the shell search
    -- only happens once per icon name.
    local cmd = "{ " .. table.concat(commands, "; ") .. "; } | head -n 1"

    local path = shell.exec(cmd)
    if path == "" then
        icon_cache[name] = false
        return nil
    end

    icon_cache[name] = path
    return path
end

local function fallback_label(item, opts)
    local text = item.title or item.tooltip or item.icon_name or "•"
    if text == "" then text = "•" end
    return ui.text({
        content = text:sub(1, 1),
        color = opts.fg or theme.text,
        size = opts.icon_size or 16,
        weight = "bold",
        font_family = opts.font_family or "monospace",
    })
end

function M.icon(item, opts)
    opts = opts or {}
    local size = opts.icon_size or 16
    local name = item.icon_name or ""

    if name == "" then
        return fallback_label(item, opts)
    end

    -- Resolve the icon name through the multi-format icon-theme search
    -- (SVG + PNG + XPM across all XDG icon directories). This catches PNG
    -- icons in fixed-size directories (e.g. hicolor/48x48/apps/) that the
    -- Rust-side NurAssets SVG-only loader misses.
    local path = M.resolve_icon_path(name)
    if path then
        -- SVG files go through ui.icon (GPUI svg element); raster images
        -- (PNG, XPM, etc.) go through ui.image (GPUI img element).
        if path:sub(-4):lower() == ".svg" then
            return ui.icon({ name = name, path = path, size = size })
        end
        return ui.image({ src = path, width = size, height = size })
    end

    -- Fall back to ui.icon for the rare case where a scalable SVG is
    -- available through the Rust asset system but not via shell find.
    if name:sub(1, 1) ~= "/" then
        return ui.icon({ name = name, size = size })
    end

    return fallback_label(item, opts)
end

function M.item(item, opts)
    opts = opts or {}
    local size = opts.icon_size or 16
    local pad = opts.item_padding or 2.1

    return ui.button({
        padding = pad,
        min_width = size,
        min_height = size,
        hover_bg = opts.hover_bg or theme.surface1,
        border_radius = opts.item_border_radius or 0,
        on_click = function()
            shell.services.systemtray:activate(item.id, opts.click_x or 0, opts.click_y or 0)
        end,
        on_right_click = function(x, y)
            shell.services.systemtray:context_menu(item.id, x, y)
        end,
        children = { M.icon(item, opts) },
    })
end

function M.new(opts)
    opts = opts or {}
    local self = {}

    function self:render()
        local tray = shell.services.systemtray:get()
        local children = {}
        local seen = {}

        for _, item in ipairs(tray.items or {}) do
            local key = ((item.icon_name or "") .. "|" .. (item.title or ""))
            if key == "|" then key = item.id end
            if not seen[key] and (opts.show_passive or item.status ~= "Passive") then
                seen[key] = true
                children[#children + 1] = M.item(item, opts)
            end
        end

        return ui.hbox({
            gap = opts.gap or 2,
            children = children,
        })
    end

    -- Periodically clear the icon cache so that icons that failed to
    -- resolve on first attempt (e.g. because the SNI provider hadn't
    -- published them yet) get retried.
    shell.interval(60000, function()
        icon_cache = {}
    end)

    return self
end

return M
