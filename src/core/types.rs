use std::{
    collections::HashMap,
    path::PathBuf,
};

use indexmap::IndexMap;
use mlua::{
    ExternalError,
    ExternalResult,
    FromLua,
    Lua,
    Value,
};
use serde::{
    Deserialize,
    Serialize,
};
use tracing::{
    debug,
    warn,
};

use crate::{
    error,
    error::IoContext,
    lua::loader::vars_from_table_entry,
    template::merge_vars,
};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    #[default]
    Symlink,
    Copy,
    Template,
}

impl From<mlua::String> for EntryType {
    fn from(value: mlua::String) -> Self {
        value.to_string_lossy().into()
    }
}

impl From<String> for EntryType {
    fn from(value: String) -> Self {
        match value.as_ref() {
            "copy" => EntryType::Copy,
            "template" => EntryType::Template,
            "symlink" => EntryType::Symlink,
            _ => {
                warn!("Unknown entry type '{:?}', defaulting to symlink", value);
                EntryType::Symlink
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum HookType {
    Shell(String),
    LuaFn(mlua::Function),
}

impl HookType {
    pub fn run(&self) -> crate::core::Result<()> {
        match self {
            HookType::Shell(cmd) => {
                #[cfg(unix)]
                {
                    debug!("Running command with: sh -c \"{}\"", cmd);
                    let status = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(cmd)
                        .status()
                        .io_err(format!("running command: {}", cmd))?;

                    if !status.success() {
                        return Err(error::Error::ErrorMessage(format!(
                            "shell hook failed with exit code: {:?}",
                            status.code()
                        )));
                    }
                }
                #[cfg(windows)]
                {
                    debug!("Running command with: pwsh -Command \"{}\"", cmd);
                    let status = std::process::Command::new("pwsh")
                        .arg("-Command")
                        .arg(cmd)
                        .status()
                        .io_err(format!("running command: {}", cmd))?;

                    if !status.success() {
                        return Err(error::Error::ErrorMessage(format!(
                            "shell hook failed with exit code: {:?}",
                            status.code()
                        )));
                    }
                }
            },
            HookType::LuaFn(f) => {
                f.call::<()>(())?;
            },
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub(crate) entry_type: EntryType,
    pub(crate) src:        PathBuf,
    pub(crate) dst:        PathBuf,
}

#[derive(Default, Debug, Clone)]
pub struct Hooks {
    pub(crate) pre_apply:  Option<HookType>,
    pub(crate) post_apply: Option<HookType>,
}

impl FromLua for Hooks {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::Table(t) => {
                let pre_apply = match t.get::<Value>("pre_apply") {
                    Ok(v) => match v {
                        Value::String(s) => Some(HookType::Shell(s.to_string_lossy())),
                        Value::Function(f) => Some(HookType::LuaFn(f)),
                        _ => None,
                    },
                    Err(err) => {
                        warn!(?err, "Unable to get pre_apply hook");
                        None
                    },
                };

                let post_apply = match t.get::<Value>("post_apply") {
                    Ok(v) => match v {
                        Value::String(s) => Some(HookType::Shell(s.to_string_lossy())),
                        Value::Function(f) => Some(HookType::LuaFn(f)),
                        _ => None,
                    },
                    Err(err) => {
                        warn!(?err, "Unable to get post_apply hook");
                        None
                    },
                };

                Ok(Hooks { pre_apply, post_apply })
            },
            _ => Ok(Default::default()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Module {
    pub(crate) name:  String,
    pub(crate) files: Vec<FileEntry>,
    // Names of other modules
    pub(crate) deps:  Vec<String>,
    pub(crate) hooks: Hooks,
    // Used for templates
    pub(crate) vars:  HashMap<String, serde_json::Value>,
}

#[derive(Debug)]
pub struct GlobalConfig {
    // All available modules by name
    pub(crate) modules: IndexMap<String, Module>,
}

#[derive(Debug)]
pub struct LocalConfig {
    // Modules to activate
    pub(crate) modules: Vec<LocalModule>,
    pub(crate) vars:    HashMap<String, serde_json::Value>,
}

impl LocalConfig {
    pub(crate) fn module_names(&self) -> Vec<&str> {
        self.modules.iter().map(|m| m.name.as_str()).collect()
    }

    pub(crate) fn vars_for(&self, name: &str) -> HashMap<String, serde_json::Value> {
        match self.modules.iter().find(|m| m.name == *name).map(|m| &m.vars) {
            Some(vars) => merge_vars([&self.vars, vars]),
            None => self.vars.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalModule {
    pub(crate) name: String,
    // TODO: Maybe a different signature for local hooks? Something that can allow for including/excluding the global hook
    // pub(crate) hooks: Hooks,
    // Used for templates
    pub(crate) vars: HashMap<String, serde_json::Value>,
}

impl FromLua for LocalModule {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::String(s) => {
                let name: String = String::from_utf8_lossy(&s.as_bytes()).into_owned();
                Ok(LocalModule {
                    name,
                    vars: Default::default(),
                })
            },
            Value::Table(t) => {
                let name = t.get::<String>("name")?;
                let vars = vars_from_table_entry(t).into_lua_err()?;

                Ok(LocalModule { name, vars })
            },
            _ => Err("module must be string or table".into_lua_err()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_type_default_is_symlink() {
        assert_eq!(EntryType::default(), EntryType::Symlink);
    }

    #[test]
    fn entry_type_serde_round_trip() {
        let original = EntryType::Template;
        let json = serde_json::to_string(&original).unwrap();
        let decoded: EntryType = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn entry_type_serializes_as_lowercase() {
        assert_eq!(serde_json::to_string(&EntryType::Copy).unwrap(), "\"copy\"");
        assert_eq!(serde_json::to_string(&EntryType::Template).unwrap(), "\"template\"");
        assert_eq!(serde_json::to_string(&EntryType::Symlink).unwrap(), "\"symlink\"");
    }

    #[test]
    fn module_can_be_constructed() {
        let m = Module {
            name:  "base".into(),
            files: vec![FileEntry {
                src:        "shell/".into(),
                dst:        ".config/shell/".into(),
                entry_type: EntryType::Symlink,
            }],
            deps:  vec![],
            vars:  Default::default(),
            hooks: Hooks::default(),
        };
        assert_eq!(m.name, "base");
        assert_eq!(m.files.len(), 1);
    }
}
