-- config.lua — global module definitions
-- This file is machine-agnostic and should be committed to git.
--
-- Modules define what files to manage and how. Available entry types:
--   (default) symlink  — creates a symlink at dst pointing to src
--   copy               — copies the file; tracks changes with hashes
--   template           — renders src as a minijinja template into dst

return {
    modules = {
        -- Shell configuration: aliases and environment
        shell = {
            files = {
                { src = "shell/aliases", dst = ".config/shell/aliases" },
            },
        },

        -- Git: rendered from a template so user details can vary per machine
        git = {
            deps  = { "shell" },
            files = {
                { src = "git/gitconfig", dst = ".gitconfig", type = "template" },
            },
            vars  = {
                name  = "{{author_name}}",
                email = "{{author_email}}",
            },
        },

    },
}
