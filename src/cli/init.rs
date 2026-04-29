use std::{
    io::{
        IsTerminal,
        Write,
    },
    path::Path,
};

use minijinja::{
    Environment,
    UndefinedBehavior,
};
use rust_embed::RustEmbed;
use tracing::{
    debug,
    info,
};

use crate::{
    cli::{
        GlobalFlags,
        tui::MultiSelect,
    },
    error::IoContext,
    lua::{
        loader::load_global_config,
        vm::create_vm,
    },
};

#[derive(RustEmbed)]
#[folder = "template/"]
struct Template;

// Generate a new repository from scratch using the template
pub(crate) fn full_scaffold<P: AsRef<Path>>(_flags: &GlobalFlags, target_dir: P) -> crate::core::Result<()> {
    let target_dir = target_dir.as_ref();

    if !target_dir.exists() {
        std::fs::create_dir_all(target_dir).io_err(format!("failed to create directory {:?}", target_dir))?;
    }

    for path in Template::iter() {
        let path = path.as_ref();
        let target_path = target_dir.join(path);

        if let Some(parent) = target_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).io_err(format!("failed to create directory {:?}", parent))?;
        }

        // Using expect rather than checking result manually because we just got the path in ::iter()
        let entry = Template::get(path).unwrap_or_else(|| panic!("failed to get template {:?}", path));

        let mut file =
            std::fs::File::create(&target_path).io_err(format!("failed to create file {:?}", target_path))?;

        let content = std::str::from_utf8(&entry.data)
            .map_err(|e| crate::error::Error::ErrorMessage(format!("unable to parse template data: {}", e)))?;

        // Special casing for files here
        match path {
            "local.lua" => {
                debug!("Rendering local.lua from template");

                let vars = minijinja::context! {
                    active_modules => Vec::<String>::with_capacity(0),
                };
                let content = render_template(content, vars)?;
                file.write_all(content.as_bytes())
                    .io_err(format!("failed to write to {:?}", target_path))?;
            },
            _ => {
                debug!("Writing out target file directly: {}", path);
                file.write_all(content.as_bytes())
                    .io_err(format!("failed to write to {:?}", target_path))?;
            },
        }

        println!("Scaffolding {}", path);
    }

    info!("Generated new dotfiles repo: {}", target_dir.display());

    Ok(())
}

pub(crate) fn setup_local_config<P: AsRef<Path>>(_flags: &GlobalFlags, target_dir: P) -> crate::core::Result<()> {
    let target_dir = target_dir.as_ref();
    info!("Configuring machine-specific config in: {}", target_dir.display());

    let config_path = target_dir.join("config.lua");
    let local_path = target_dir.join("local.lua");

    let lua = create_vm()?;
    let global_config = load_global_config(&lua, &config_path)?;

    let modules = global_config
        .modules
        .keys()
        .map(|m| (m.clone(), false))
        .collect::<Vec<_>>();

    // Select modules if interactive, otherwise don't activate any modules
    let module_names = if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        MultiSelect::new("Select modules to activate", modules.as_slice()).run()?
    } else {
        vec![]
    };
    info!("Creating local.lua with selected modules: {:?}", module_names);

    // let modules = lua_array_from_vec(&module_names);
    let local_lua =
        Template::get("local.lua").ok_or(crate::error::Error::ErrorMessage("Binary is missing local.lua!".into()))?;

    let content = std::str::from_utf8(&local_lua.data)
        .map_err(|e| crate::error::Error::ErrorMessage(format!("unable to parse template data: {}", e)))?;

    let vars = minijinja::context! {
        active_modules => module_names,
    };

    let rendered = render_template(content, vars)?;

    std::fs::write(&local_path, rendered).io_err(format!("writing to file: {}", local_path.display()))?;

    Ok(())
}

fn render_template<S: AsRef<str>>(data: S, vars: minijinja::Value) -> crate::core::Result<String> {
    let content = data.as_ref();
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    let result = env.render_str(content, vars)?;
    Ok(result)
}
