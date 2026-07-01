use std::{
    io::{
        IsTerminal,
        Write,
    },
    path::Path,
};

use rust_embed::RustEmbed;
use tracing::{
    debug,
    info,
};

use crate::{
    cli::{
        GlobalFlags,
        tui::{
            MultiSelect,
            confirm,
            prompt,
        },
    },
    error::IoContext,
    lua::{
        loader::load_global_config,
        vm::create_vm,
    },
    template::{
        create_environment,
        render_template,
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

pub(crate) fn setup_local_config<P: AsRef<Path>>(flags: &GlobalFlags, target_dir: P) -> crate::core::Result<()> {
    let target_dir = target_dir.as_ref();
    info!("Configuring machine-specific config in: {}", target_dir.display());

    let config_path = target_dir.join("config.lua");
    let local_template = flags.source_dir.join("local.template.lua");
    let local_path = target_dir.join("local.lua");
    let use_local_template =
        local_template.exists() && confirm("Do you want to use local.template.lua as template", true)?;

    let lua = create_vm()?;
    let global_config = load_global_config(&lua, &config_path)?;

    let modules = global_config
        .modules
        .keys()
        .map(|m| (m.clone(), false))
        .collect::<Vec<_>>();

    // Select modules if interactive, otherwise don't activate any modules
    let module_names = if !use_local_template && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        MultiSelect::new("Select modules to activate", modules.as_slice()).run()?
    } else {
        vec![]
    };
    info!("Creating local.lua with selected modules: {:?}", module_names);

    let content = if use_local_template {
        debug!("Using local.template.lua to create local.lua");
        std::fs::read_to_string(&local_template)
            .io_err(format!("failed to read local template: {}", local_template.display()))?
    } else {
        String::from_utf8(
            Template::get("local.lua")
                .ok_or(crate::error::Error::ErrorMessage("Binary is missing local.lua!".into()))?
                .data
                .into_owned(),
        )
        .map_err(|err| crate::error::Error::ErrorMessage(err.to_string()))?
    };

    let mut vars = serde_json::json!({
        "active_modules": module_names,
    });

    // Get a list of undeclared variables
    let env = create_environment();
    let tmpl = env.template_from_str(&content)?;

    let undeclared = tmpl.undeclared_variables(true);

    // undeclared = items that are declared but aren't present in the context
    let undeclared = undeclared
        .iter()
        .filter(|n| !ctx_path_exists(&vars, n))
        .collect::<Vec<_>>();

    for var_name in undeclared {
        debug!("Prompting for var: {}", var_name);
        let value = prompt(format!("Value for var: {}", var_name))?;
        ctx_set_nested_value(&mut vars, var_name, serde_json::to_value(&value)?)?;
    }

    let rendered = render_template(&content, vars)?;

    std::fs::write(&local_path, rendered).io_err(format!("writing to file: {}", local_path.display()))?;

    Ok(())
}

/// Checks whether the JSON object contains a value at some.nested.path
fn ctx_path_exists(context: &serde_json::Value, path: &str) -> bool {
    let mut current_node = context;
    for part in path.split('.') {
        match current_node.get(part) {
            Some(v) => current_node = v,
            None => return false,
        }
    }
    true
}

/// Sets a nested value by some.nested.path
fn ctx_set_nested_value(
    context: &mut serde_json::Value,
    path: &str,
    value: serde_json::Value,
) -> crate::core::Result<()> {
    let mut current = context;
    let parts = path.split('.').collect::<Vec<_>>();

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            current
                .as_object_mut()
                .ok_or_else(|| format!("expected object at '{}'", part))?
                .insert(part.to_string(), value.clone());
            return Ok(());
        } else {
            // Ensure current node is an object
            if !current.is_object() {
                *current = serde_json::json!({});
            }
            current = current
                .as_object_mut()
                .ok_or_else(|| format!("expected object at '{}'", part))?
                .entry(part.to_string())
                .or_insert(serde_json::json!({}));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // ctx_path_exists

    #[test]
    fn path_exists_flat() {
        let ctx = json!({"key": "value"});
        assert!(ctx_path_exists(&ctx, "key"));
    }

    #[test]
    fn path_exists_nested() {
        let ctx = json!({"a": {"b": {"c": 1}}});
        assert!(ctx_path_exists(&ctx, "a.b.c"));
        assert!(ctx_path_exists(&ctx, "a.b"));
    }

    #[test]
    fn path_exists_missing_key() {
        let ctx = json!({"a": {"b": 1}});
        assert!(!ctx_path_exists(&ctx, "a.c"));
        assert!(!ctx_path_exists(&ctx, "z"));
    }

    #[test]
    fn path_exists_empty_path() {
        let ctx = json!({"a": 1});
        // "".split('.') yields [""], so it looks up the key "" which doesn't exist
        assert!(!ctx_path_exists(&ctx, ""));
    }

    #[test]
    fn set_nested_flat() {
        let mut ctx = json!({});
        ctx_set_nested_value(&mut ctx, "key", json!("hello")).unwrap();
        assert_eq!(ctx["key"], json!("hello"));
    }

    #[test]
    fn set_nested_deep() {
        let mut ctx = json!({});
        ctx_set_nested_value(&mut ctx, "a.b.c", json!(42)).unwrap();
        assert_eq!(ctx["a"]["b"]["c"], json!(42));
    }

    #[test]
    fn set_nested_overwrites_existing() {
        let mut ctx = json!({"a": {"b": "old"}});
        ctx_set_nested_value(&mut ctx, "a.b", json!("new")).unwrap();
        assert_eq!(ctx["a"]["b"], json!("new"));
    }

    #[test]
    fn set_nested_creates_intermediate_objects() {
        let mut ctx = json!({});
        ctx_set_nested_value(&mut ctx, "x.y.z", json!(true)).unwrap();
        assert_eq!(ctx["x"]["y"]["z"], json!(true));
    }

    #[test]
    fn set_nested_overwrites_non_object_root() {
        // When root context is not an object, the traversal overwrites it
        let mut ctx = json!("not_an_object");
        ctx_set_nested_value(&mut ctx, "a.b", json!(1)).unwrap();
        assert_eq!(ctx["a"]["b"], json!(1));
    }

    #[test]
    fn set_nested_error_on_non_object_child() {
        // When an existing child value is not an object, descending into it errors
        let mut ctx = json!({"a": "not_an_object"});
        let result = ctx_set_nested_value(&mut ctx, "a.b", json!(1));
        assert!(result.is_err());
    }

    #[test]
    fn ctx_user_interaction_flow() {
        let mut ctx = json!({
            "user": json!({
                "name": "Json Doe",
                "email": "json.doe@email.com"
            })
        });

        assert!(!ctx_path_exists(&ctx, "foo"));
        assert!(ctx_path_exists(&ctx, "user.name"));
        assert!(ctx_path_exists(&ctx, "user.email"));

        let result = ctx_set_nested_value(&mut ctx, "foo", json!(true));
        assert!(result.is_ok());
        assert!(ctx_path_exists(&ctx, "foo"));
        assert_eq!(ctx["foo"], json!(true));
    }
}
