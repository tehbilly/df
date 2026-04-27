use std::{
    collections::HashMap,
    path::{
        Path,
        PathBuf,
    },
};

use indexmap::IndexMap;
use mlua::{
    ExternalError,
    FromLua,
    Lua,
    Table,
    Value,
};
use tracing::{
    debug,
    error,
    warn,
};

use crate::{
    core::{
        Result,
        types::{
            EntryType,
            FileEntry,
            GlobalConfig,
            Hooks,
            LocalConfig,
            LocalModule,
            Module,
        },
    },
    error::{
        Error,
        IoContext,
    },
};

pub(crate) fn load_global_config<P: AsRef<Path>>(lua: &Lua, path: P) -> Result<GlobalConfig> {
    let path = path.as_ref();
    if !path.exists() {
        error!(?path, "No global config file found");
        return Err(Error::ConfigLuaNotFound);
    }

    let path_contents =
        std::fs::read_to_string(path).io_err(format!("reading global config file: {}", path.display()))?;
    load_global_config_from_str(lua, &path_contents)
}

pub(crate) fn load_global_config_from_str<V: AsRef<str>>(lua: &Lua, value: V) -> Result<GlobalConfig> {
    let chunk = lua.load(value.as_ref());
    let parsed = match chunk.eval::<Value>()? {
        Value::Table(t) => t,
        _ => return Err("global config must return table".into_lua_err().into()),
    };

    let module_table = if let Ok(v) = parsed.get::<Value>("modules")
        && let Value::Table(t) = v
    {
        t
    } else {
        return Err("modules returned wrong type".into_lua_err().into());
    };

    let mut modules = IndexMap::new();
    for pair in module_table.pairs::<String, Table>() {
        let (key, value) = pair?;
        let module = module_from_lua(key.clone(), value, lua)?;
        modules.insert(key, module);
    }

    Ok(GlobalConfig { modules })
}

fn module_from_lua(name: String, chunk: Table, lua: &Lua) -> Result<Module> {
    let files = match chunk.get::<Table>("files") {
        Ok(v) => file_entries_from_lua(v)?,
        Err(err) => {
            error!(?err, "Unable to get file entries from config value");
            return Err("unable to get file entries".into_lua_err().into());
        },
    };

    let deps = match chunk.get::<Table>("deps") {
        Ok(ts) => ts.sequence_values::<String>().flatten().collect(),
        Err(err) => {
            debug!(?err, "No deps found for module");
            vec![]
        },
    };

    let hooks = if let Ok(h) = chunk.get::<Value>("hooks") {
        Hooks::from_lua(h, lua)?
    } else {
        Hooks::default()
    };

    let vars = vars_from_table_entry(chunk)?;

    Ok(Module {
        name,
        files,
        deps,
        hooks,
        vars,
    })
}

fn file_entries_from_lua(value: mlua::Table) -> Result<Vec<FileEntry>> {
    let mut modules = Vec::new();

    for value in value.sequence_values::<Value>() {
        match value? {
            Value::String(s) => {
                let s = s.to_string_lossy();
                modules.push(FileEntry {
                    entry_type: EntryType::Symlink,
                    src:        s.clone().into(),
                    dst:        s.into(),
                });
            },
            Value::Table(t) => {
                // Allow doing { src = '...', .. } or { 'shorthand', .. }
                let (src, default_dst) = if let Ok(Some(shorthand)) = t.get::<Option<PathBuf>>(1) {
                    (shorthand.clone(), shorthand)
                } else {
                    match t.get::<PathBuf>("src") {
                        Ok(v) => (v.clone(), v),
                        Err(err) => {
                            error!(?err, "failed to get src");
                            return Err(err.into_lua_err().into());
                        },
                    }
                };

                let dst = t.get::<PathBuf>("dst").unwrap_or(default_dst);

                let entry_type: EntryType = match t.get::<Option<String>>("type") {
                    Ok(s) => match s {
                        Some(v) => v.into(),
                        None => EntryType::Symlink,
                    },
                    Err(err) => {
                        warn!(?err, "failed to get entry type, defaulting to symlink");
                        EntryType::Symlink
                    },
                };

                modules.push(FileEntry { entry_type, src, dst });
            },
            _ => return Err(Error::LuaError("modules entry not string or table".into_lua_err())),
        }
    }

    Ok(modules)
}

pub(crate) fn load_local_config<P: AsRef<Path>>(lua: &Lua, global: &GlobalConfig, path: P) -> Result<LocalConfig> {
    let path = path.as_ref();
    if !path.exists() {
        error!(?path, "No local config file found");
        return Err(Error::LocalLuaNotFound);
    }

    let path_contents =
        std::fs::read_to_string(path).io_err(format!("reading local config file: {}", path.display()))?;
    load_local_config_from_str(lua, global, &path_contents)
}

pub(crate) fn load_local_config_from_str<V: AsRef<str>>(
    lua: &Lua,
    global: &GlobalConfig,
    value: V,
) -> Result<LocalConfig> {
    let chunk = match lua.load(value.as_ref()).eval::<Value>()? {
        Value::Table(t) => t,
        _ => return Err("local config must return table".into_lua_err().into()),
    };

    let modules = if let Ok(v) = chunk.get::<Value>("modules")
        && let Value::Table(t) = v
    {
        let mut modules = Vec::new();
        for entry in t.sequence_values::<Value>() {
            let entry = entry?;
            let entry = LocalModule::from_lua(entry, lua)?;
            if !global.modules.contains_key(&entry.name) {
                return Err(Error::UnknownModuleName(entry.name.clone()));
            }
            modules.push(entry);
        }
        modules
    } else {
        return Err("modules returned wrong type".into_lua_err().into());
    };

    let vars = vars_from_table_entry(chunk)?;

    Ok(LocalConfig { modules, vars })
}

pub(crate) fn vars_from_table_entry(chunk: Table) -> Result<HashMap<String, serde_json::Value>> {
    let entry = match chunk.get::<Option<Table>>("vars") {
        Ok(e) => match e {
            // Not present means not defined, which is ok
            None => return Ok(HashMap::new()),
            Some(t) => t,
        },
        Err(err) => {
            return Err(err.into());
        },
    };

    let mut vars = HashMap::new();

    for entry in entry.pairs::<String, Value>() {
        let (key, value) = match entry {
            Ok((k, v)) => (k, v),
            Err(err) => {
                warn!(?err, "Unable to get k/v from entry");
                continue;
            },
        };
        let json_value = match serde_json::to_value(value) {
            Ok(jv) => jv,
            Err(err) => {
                // If we can't map this to a value then we need to bubble that up
                // Otherwise vars are silently skipped over if they're weird types or something
                return Err(err.into());
            },
        };
        vars.insert(key, json_value);
    }

    Ok(vars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::types::{
            EntryType,
            HookType,
        },
        lua::vm::create_vm,
    };

    // ── GlobalConfig parsing ──────────────────────────────────────────

    #[test]
    fn string_shorthand_sets_src_and_dst() {
        let lua = create_vm().unwrap();
        let config = load_global_config_from_str(
            &lua,
            r#"
            return {
                modules = {
                    base = { files = { ".config/shell/" } }
                }
            }
        "#,
        )
        .unwrap();
        let entry = &config.modules["base"].files[0];
        assert_eq!(entry.src.to_str().unwrap(), ".config/shell/");
        assert_eq!(entry.dst.to_str().unwrap(), ".config/shell/");
        assert_eq!(entry.entry_type, EntryType::Symlink);
    }

    #[test]
    fn explicit_entry_dst_defaults_to_src() {
        let lua = create_vm().unwrap();
        let config = load_global_config_from_str(
            &lua,
            r#"
            return {
                modules = {
                    base = {
                        files = { { src = "git/" } }
                    }
                }
            }
        "#,
        )
        .unwrap();
        let entry = &config.modules["base"].files[0];
        assert_eq!(entry.src, entry.dst);
    }

    #[test]
    fn explicit_entry_type_copy() {
        let lua = create_vm().unwrap();
        let config = load_global_config_from_str(
            &lua,
            r#"
            return {
                modules = {
                    base = {
                        files = { { src = "nvim/", dst = ".config/nvim/", type = "copy" } }
                    }
                }
            }
        "#,
        )
        .unwrap();
        assert_eq!(config.modules["base"].files[0].entry_type, EntryType::Copy);
    }

    #[test]
    fn explicit_entry_type_template() {
        let lua = create_vm().unwrap();
        let config = load_global_config_from_str(
            &lua,
            r#"
            return {
                modules = {
                    base = {
                        files = { { src = "tmux.conf", type = "template" } }
                    }
                }
            }
        "#,
        )
        .unwrap();
        assert_eq!(config.modules["base"].files[0].entry_type, EntryType::Template);
    }

    #[test]
    fn shell_hook_parses_as_shell_variant() {
        let lua = create_vm().unwrap();
        let config = load_global_config_from_str(
            &lua,
            r#"
            return {
                modules = {
                    base = {
                        files = { "shell/" },
                        hooks = { pre_apply = "echo hello" }
                    }
                }
            }
        "#,
        )
        .unwrap();
        let hook = config.modules["base"].hooks.pre_apply.as_ref().unwrap();
        assert!(matches!(hook, HookType::Shell(cmd) if cmd == "echo hello"));
    }

    #[test]
    fn lua_function_hook_stores_registry_key() {
        let lua = create_vm().unwrap();
        let config = load_global_config_from_str(
            &lua,
            r#"
            return {
                modules = {
                    base = {
                        files = { "shell/" },
                        hooks = {
                            post_apply = function() end
                        }
                    }
                }
            }
        "#,
        )
        .unwrap();
        let hook = config.modules["base"].hooks.post_apply.as_ref().unwrap();
        // Confirm it stored a registry key (not a shell string)
        assert!(matches!(hook, HookType::LuaFn(_)));
        // Confirm the stored function can be called
        if let HookType::LuaFn(f) = hook {
            f.call::<()>(()).unwrap();
        }
    }

    #[test]
    fn vars_are_parsed() {
        let lua = create_vm().unwrap();
        let config = load_global_config_from_str(
            &lua,
            r#"
            return {
                modules = {
                    base = {
                        files = { "shell/" },
                        vars = { email = "me@example.com", count = 3 }
                    }
                }
            }
        "#,
        )
        .unwrap();
        let vars = &config.modules["base"].vars;
        assert_eq!(vars["email"], serde_json::json!("me@example.com"));
        assert_eq!(vars["count"], serde_json::json!(3));
    }

    // ── LocalConfig parsing ───────────────────────────────────────────

    fn base_global() -> GlobalConfig {
        use std::collections::HashMap;

        use crate::core::types::{
            EntryType,
            FileEntry,
            GlobalConfig,
            Hooks,
            Module,
        };
        GlobalConfig {
            modules: [("base".to_string(), Module {
                name:  "base".into(),
                files: vec![FileEntry {
                    src:        "shell/".into(),
                    dst:        ".config/shell/".into(),
                    entry_type: EntryType::Symlink,
                }],
                deps:  vec![],
                vars:  HashMap::new(),
                hooks: Hooks::default(),
            })]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn local_config_activates_known_module() {
        let lua = create_vm().unwrap();
        let global = base_global();
        let local = load_local_config_from_str(
            &lua,
            &global,
            r#"
            return { modules = { "base" } }
        "#,
        )
        .unwrap();
        assert_eq!(local.modules.len(), 1);
        assert_eq!(local.modules.get(0).unwrap().name, "base");
    }

    #[test]
    fn local_config_unknown_module_returns_error() {
        let lua = create_vm().unwrap();
        let global = base_global();
        let result = load_local_config_from_str(
            &lua,
            &global,
            r#"
            return { modules = { "nonexistent" } }
        "#,
        );
        assert!(matches!(result, Err(Error::UnknownModuleName(_))));
    }

    #[test]
    fn local_config_vars_are_parsed() {
        let lua = create_vm().unwrap();
        let global = base_global();
        let local = load_local_config_from_str(
            &lua,
            &global,
            r#"
            return {
                modules = { "base" },
                vars = { theme = "tokyonight" }
            }
        "#,
        )
        .unwrap();
        assert_eq!(local.vars["theme"], serde_json::json!("tokyonight"));
    }
}
