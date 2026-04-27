# df

A Lua-powered dotfile manager. Configuration lives in Lua, giving you the full
power of a real scripting language to conditionally activate modules, set
variables, and react to the current machine's environment.

## Installation

Requires Rust 1.85.0 or later.

```sh
cargo install --locked --git https://github.com/tehbilly/df
```

## Concepts

**Modules** are named groups of files. A module declares which files/directories
it manages, how they should be deployed (symlink, copy, or rendered template),
which other modules it depends on, and any variables it provides.

**`config.lua`** lives in your dotfile repository and defines all available
modules. It is the _global_ configuration committed to your repo and shared
across machines.

**`local.lua`** lives alongside `config.lua` and declares which modules are
active on the current machine. It is typically gitignored. It can also override
or extend variables defined in `config.lua`.

## Quick Start

```sh
# Initialise a new dotfile repo in the current directory
df init

# Edit config.lua to define your modules, then activate some in local.lua
df apply

# Check what df is managing and whether anything is out of date
df status

# Preview pending changes before applying
df diff
```

By default `df` reads from the current directory and deploys to `$HOME`. Both
can be overridden — see [Global Flags](#global-flags).

## Configuration

### `config.lua`

Returns a table with a `modules` key. Each module is a named table:

```lua
return {
    modules = {
        shell = {
            files = {
                ".config/shell/",          -- shorthand: src and dst are the same path
                { src = "profile", dst = ".profile" },
            },
        },

        git = {
            files = {
                { src = "git/config", dst = ".gitconfig" },
            },
        },

        neovim = {
            deps  = { "shell" },           -- applied after its dependencies
            files = {
                { src = "nvim/", dst = ".config/nvim/" },
            },
        },
    }
}
```

#### File entry types

| Type | Key | Behaviour |
|---|---|---|
| `symlink` | *(default)* | Creates a symlink at `dst` pointing to `src` |
| `copy` | `type = "copy"` | Copies the file; tracks hashes to detect drift |
| `template` | `type = "template"` | Renders `src` as a [minijinja](https://docs.rs/minijinja) template into `dst` |

```lua
files = {
    { src = "gitconfig", dst = ".gitconfig", type = "template" },
    { src = "ssh/config", dst = ".ssh/config", type = "copy" },
}
```

#### Variables

Modules can declare variables used by their template files:

```lua
git = {
    files = { { src = "gitconfig", dst = ".gitconfig", type = "template" } },
    vars  = {
        email = "default@example.com",
        name  = "Default Name",
    },
}
```

Template files use [minijinja](https://docs.rs/minijinja) syntax:

```
[user]
    name  = {{ name }}
    email = {{ email }}
```

#### Hooks

Hooks run shell commands or Lua functions before and after `apply`:

```lua
neovim = {
    files = { { src = "nvim/", dst = ".config/nvim/" } },
    hooks = {
        -- Can be a Lua function
        pre_apply = function()
            -- any Lua logic here
        end,
        
        -- A shell command, will be run with sh or pwsh depending on operating system
        post_apply = "nvim --headless '+Lazy! sync' +qa",
    },
}
```

#### Dependencies

Modules are applied in dependency order. A module listed in `deps` is
guaranteed to be applied before the dependent:

```lua
neovim = {
    -- The `shell` and `git` modules will have their configurations and hooks
    -- applied before this module's
    deps  = { "shell", "git" },
    files = { { src = "nvim/", dst = ".config/nvim/" } },
}
```

### `local.lua`

Declares which modules are active on this machine and optionally overrides
variables:

```lua
return {
    modules = { "shell", "git", "neovim" },
}
```

#### Overriding variables

Top-level `vars` apply to all modules on this machine:

```lua
return {
    modules = { "shell", "git" },
    vars    = {
        email = "work@company.com",
    },
}
```

Per-module vars override the top-level vars for that module only:

```lua
return {
    modules = {
        "shell",
        { name = "git", vars = { email = "personal@example.com" } },
    },
    vars = {
        email = "work@company.com",   -- used by all other modules
    },
}
```

Variable resolution order (last wins): module defaults → local top-level vars →
local per-module vars.

### Lua API

The `dotfiles` global is available in both `config.lua` and `local.lua`:

| Function | Returns | Description |
|---|---|---|
| `dotfiles.os()` | `string` | Current OS (`"linux"`, `"macos"`, `"windows"`, …) |
| `dotfiles.arch()` | `string` | CPU architecture (`"x86_64"`, `"aarch64"`, …) |
| `dotfiles.hostname()` | `string` | Machine hostname |
| `dotfiles.env(name)` | `string \| nil` | Value of environment variable, or nil if unset |
| `dotfiles.which(cmd)` | `string \| nil` | Full path to a binary, or nil if not found |

Example — conditionally activating modules based on the environment:

```lua
local modules = { "shell", "git" }

if dotfiles.os() == "linux" then
    table.insert(modules, "linux-tweaks")
end

if dotfiles.which("nvim") then
    table.insert(modules, "neovim")
end

if dotfiles.hostname() == "work-laptop" then
    table.insert(modules, "work")
end

return {
    modules = modules,
    vars    = {
        email = dotfiles.env("GIT_EMAIL") or "fallback@example.com",
    },
}
```

## Commands

### `df init [DIR]`

Bootstraps a dotfile repository with starter `config.lua` and `local.lua`
files. Updates `.gitignore` to exclude `local.lua` and the `.backups/`
directory. Defaults to the current directory.

### `df apply`

Deploys all active modules from `local.lua` to the output directory.

```
OPTIONS:
  -m, --module <NAME>   Apply a single module (and its dependencies) only
      --dry-run         Preview what would happen without making changes
  -f, --force           Back up and overwrite externally-modified files
```

When a destination file already exists and was not placed there by `df`,
apply will skip it and warn. Pass `--force` to back it up under `.backups/`
and proceed.

### `df status`

Shows the current state of every managed file — clean, source changed,
externally modified, or not yet deployed.

### `df diff`

Shows a unified diff of pending changes for copy and template entries whose
source has changed since the last apply.

### `df list`

Lists all modules defined in `config.lua` and whether each is active in
`local.lua`.

### `df clean`

Removes files that were managed by a module no longer active in `local.lua`
(orphans). Prompts for confirmation on each file; pass `--yes` to skip prompts.

> **Note:** `clean` is deprecated and will be folded into `apply` in a future
> release.

## Global Flags

All subcommands accept these flags:

```
  -s, --source-dir <DIR>   Dotfile repository root (default: current directory)
  -o, --output-dir <DIR>   Deployment target (default: $HOME)
      --state-dir <DIR>    State file location (default: platform state dir)
  -c, --config-path <FILE> Path to local.lua (default: <source-dir>/local.lua)
  -v, --verbose            Enable debug logging (-vv for trace)
```

## TODOs

- [ ] Test `clean` behaviour on Windows with directory symlink entries (`fs::remove_file` vs `fs::remove_dir`)
- [ ] Interactive TUI
    - It's not really needed, but brother I _want_ it
