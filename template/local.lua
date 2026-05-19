-- local.lua
-- This file is for machine-specific configuration
-- Do not commit this file (it should listed in .gitignore).
--
-- Activate modules for this machine and override variables as needed.
-- The dotfiles module provides helpers for conditional configuration:
--
--  local df = require("dotfiles")
--  df.os()                            -> "linux", "macos", "windows", …
--  df.username()                      -> username, or nil
--  df.home()                          -> home dir, or nil
--  df.hostname()                      -> machine hostname
--  df.which("nvim")                   -> path to binary, or nil
--  df.env("VAR")                      -> env var value, or nil
--  df.env("VAR", "default_if_unset")  -> env var value, or default_if_unset

local df = require("dotfiles")

local modules = {
    {%- for module in active_modules %}
    "{{ module }}",
    {%- endfor %}
}
local vars = {}

-- This is a work machine, override some global vars
if df.hostname():find("^work") then
    vars.email = "john.doe@workinghere.com"
end

-- Activate git module if git is available
if df.which("git") then
    if df.hostname():find("^work") then
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
