-- nur standard library
-- Loaded automatically before the user's init.lua.
-- Populates the `ui` table (created by Rust) with pure-Lua element
-- constructors.  Rust-backed native components are added separately.

-- ---------------------------------------------------------------------------
-- Layout
-- ---------------------------------------------------------------------------

-- ui.hbox(props) / ui.hstack(props)
-- Horizontal flex row.  `props.children` is a sequential table of elements.
function ui.hbox(props)
    props = props or {}
    props.type = "hbox"
    return props
end
ui.hstack = ui.hbox

-- ui.vbox(props) / ui.vstack(props)
-- Vertical flex column.
function ui.vbox(props)
    props = props or {}
    props.type = "vbox"
    return props
end
ui.vstack = ui.vbox

-- ui.spacer()
-- A flexible gap that expands to fill available space.
function ui.spacer()
    return { type = "spacer" }
end

-- ---------------------------------------------------------------------------
-- Text & icons
-- ---------------------------------------------------------------------------

-- ui.text(content_or_props)
-- `content` may be a plain string, a LuaState (auto-read), or a props table
-- with a `content` / `text` key.
function ui.text(content_or_props)
    if type(content_or_props) == "string" then
        return { type = "text", content = content_or_props }
    elseif type(content_or_props) == "userdata" then
        -- LuaState: unwrap current value
        return { type = "text", content = tostring(content_or_props:get()) }
    else
        content_or_props.type = "text"
        return content_or_props
    end
end

-- Alias
ui.label = ui.text

-- ui.icon(name_or_props)
-- Render a named SVG icon from the bundled icon set.
function ui.icon(name_or_props)
    if type(name_or_props) == "string" then
        return { type = "icon", name = name_or_props }
    else
        name_or_props.type = "icon"
        return name_or_props
    end
end

-- ---------------------------------------------------------------------------
-- Convenience helpers
-- ---------------------------------------------------------------------------

-- Build a horizontal bar section with left / center / right regions.
-- Returns a single hbox element.
function ui.bar_layout(left, center, right)
    return ui.hbox({ children = {
        ui.hbox({ children = left  or {} }),
        ui.spacer(),
        ui.hbox({ children = center or {} }),
        ui.spacer(),
        ui.hbox({ children = right  or {} }),
    }})
end
