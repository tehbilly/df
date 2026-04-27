use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
};

use tracing::{
    error,
    info,
};

use crate::{
    cli::GlobalFlags,
    error::IoContext,
    lua::{
        loader::load_global_config,
        vm::create_vm,
    },
};

pub(crate) fn run(_flags: &GlobalFlags, target_dir: PathBuf) -> crate::core::Result<()> {
    info!("Initializing repo in: {}", target_dir.display());

    if !target_dir.exists() {
        info!("Creating target directory: {}", target_dir.display());
        std::fs::create_dir_all(&target_dir).io_err(format!("creating directory: {}", target_dir.display()))?;
    }

    let config_path = target_dir.join("config.lua");
    let local_path = target_dir.join("local.lua");

    if config_path.exists() && local_path.exists() {
        error!(?target_dir, "Directory already contains config.lua and local.lua");
        return Err(crate::error::Error::ErrorMessage(
            "target directory already contains config.lua and local.lua".into(),
        ));
    }

    if !config_path.exists() {
        // TODO: Deprecate this and refer to a template for use with `cargo generate` once it's available
        std::fs::write(
            &config_path,
            r#"-- global.lua
-- this is where you'll place your overall module configurations

return {
    modules = {}
}
"#,
        )
        .io_err(format!("writing to file: {}", config_path.display()))?;
    }

    if !local_path.exists() {
        let lua = create_vm()?;
        let global_config = load_global_config(&lua, &config_path)?;

        let lua_array_from_vec = |items: &Vec<&String>| {
            let mut result = String::from("{");

            for (i, s) in items.iter().enumerate() {
                if i > 0 {
                    result.push(',');
                }
                result.push_str(format!("\"{}\"", s).as_str());
            }

            result.push_str(" }");
            result
        };

        let modules = global_config.modules.keys().collect::<Vec<_>>();
        let modules = lua_array_from_vec(&modules);

        // TODO: Maybe interactively select modules from global config?
        std::fs::write(
            &local_path,
            format!(
                r#"-- local.lua
-- this is where you'll place your machine-specific configurations

return {{
    modules = {{ {modules} }}
}}
"#
            ),
        )
        .io_err(format!("writing to file: {}", local_path.display()))?;
    }

    // Add local.lua and .backups to gitignore, creating file if not already present
    let gitignore = target_dir.join(".gitignore");
    if gitignore.exists() {
        let mut backups_entry_exists = false;
        let mut local_lua_entry_exists = false;

        for line in std::fs::read_to_string(&gitignore)
            .io_err(format!("reading file: {}", gitignore.display()))?
            .lines()
        {
            if line == "/.backups" {
                backups_entry_exists = true;
            }
            if line == "/local.lua" {
                local_lua_entry_exists = true;
            }

            if backups_entry_exists && local_lua_entry_exists {
                break;
            }
        }

        let mut gitignore_file = OpenOptions::new()
            .create(false)
            .truncate(false)
            .read(true)
            .append(true)
            .open(&gitignore)
            .io_err(format!("opening file: {}", gitignore.display()))?;

        let mut append = |line: &str| -> crate::core::Result<()> {
            writeln!(gitignore_file, "{}", line).io_err(format!("writing to file: {}", gitignore.display()))
        };

        if !backups_entry_exists {
            append("# dotfile backups")?;
            append("/.backups/")?;
        }

        if !local_lua_entry_exists {
            append("# local.lua -- default machine-specific configuration")?;
            append("/local.lua")?;
        }
    } else {
        let mut gitignore_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&gitignore)
            .io_err(format!("opening file: {}", gitignore.display()))?;

        let mut append = |line: &str| -> crate::core::Result<()> {
            writeln!(gitignore_file, "{}", line).io_err(format!("writing to file: {}", gitignore.display()))
        };

        append("# dotfile backups")?;
        append("/.backups/")?;
        append("# local.lua -- default machine-specific configuration")?;
        append("/local.lua")?;
    }

    Ok(())
}
