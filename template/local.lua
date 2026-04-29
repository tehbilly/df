-- local.lua — machine-specific configuration
-- Do not commit this file (it is listed in .gitignore).
--
-- Activate modules for this machine and override variables as needed.
-- The dotfiles global provides helpers for conditional configuration:
--
--   dotfiles.os()                            -> "linux", "macos", "windows", …
--   dotfiles.hostname()                      -> machine hostname
--   dotfiles.which("nvim")                   -> path to binary, or nil
--   dotfiles.env("VAR")                      -> env var value, or nil
--   dotfiles.env("VAR", "default_if_unset")  -> env var value, or default_if_unset

local modules = {
    {%- for module in active_modules %}
    "{{ module }}",
    {%- endfor %}
}
local vars = {}

-- This is a work machine, override some global vars
if dotfiles.hostname():find("^work") then
    vars.email = "john.doe@workinghere.com"
end

-- Activate git module if git is available
if dotfiles.which("git") then
    if dotfiles.hostname():find("^work") then
        -- This is a work machine, and we go by a different name
        table.insert(modules, {
            name = "git",
            vars = { name = "John Lastname" }
        })
    else
        -- This isn't a work machine, use defaults
        table.insert(modules, "git")
    end
end

return {
    vars = vars,
    modules = modules,
}
