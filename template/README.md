# df-template

A starter template for [df](https://github.com/tehbilly/df), a(nother) dotfile manager.

This template gives you a minimal working `df` setup out of the box: shell aliases and a git config, each deployed to the right place on your system. Mean to be used as a starting point for managing your own dotfiles.

## Using `df`

This repo is meant to be used by `df` when running `df init /some/path` where `/some/path` either don't exist or is an empty directory.

## Using `cargo-generate`

You can also use [cargo-generate](https://github.com/cargo-generate/cargo-generate) to manually generate a repository.

```sh
cargo generate tehbilly/df-template
```

## What you get

```
config.lua        # Defines what files df manages — commit this
local.lua         # Tells df which modules to activate on this machine — do not commit
git/gitconfig     # Your .gitconfig, deployed as a template
shell/aliases     # Common shell aliases, deployed as a symlink
```

## Next steps

Check out [df](https://github.com/tehbilly/df) for more information on where to go from here. it to `~/.gitconfig`
