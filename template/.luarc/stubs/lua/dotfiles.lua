---@meta

---@class DotfilesHelpers
dotfiles = {}

---Returns the current OS: "window", "macos", "linux", etc.
---@return string
function dotfiles.os() end

---Returns the current user's username
---@return string|nil
function dotfiles.username() end

---Returns the user's home directory
---@return string|nil
function dotfiles.home() end

---Returns the current machine's hostname
---@return string
function dotfiles.hostname() end

---Tries to find the absolute path to an executable on the PATH
---@param cmd string The executable to look up
---@return string|nil
function dotfiles.which(cmd) end

---Returns the value of an environment variable if set, nil or default_value otherwise
---@param var_name string The environment variable
---@param default_value string The value to return if the environment variable is not set
---@overload fun(var_name: string): string|nil
function dotfiles.env(var_name, default_value) end
