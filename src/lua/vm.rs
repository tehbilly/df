use std::{
    env,
    path::PathBuf,
};

use mlua::{
    ExternalResult,
    Lua,
    MultiValue,
    Value,
};

pub(crate) fn create_vm() -> crate::core::Result<Lua> {
    let lua = Lua::new();

    let dotfiles = lua.create_table()?;

    dotfiles.set("os", lua.create_function(|_lua, ()| Ok(env::consts::OS))?)?;
    dotfiles.set("arch", lua.create_function(|_lua, ()| Ok(env::consts::ARCH))?)?;
    dotfiles.set("env", lua.create_function(dotfiles_env)?)?;
    dotfiles.set("hostname", lua.create_function(dotfiles_hostname)?)?;
    dotfiles.set("which", lua.create_function(dotfiles_which)?)?;

    lua.globals().set("dotfiles", dotfiles)?;

    Ok(lua)
}

fn dotfiles_which(_lua: &Lua, cmd: String) -> mlua::Result<Option<PathBuf>> {
    #[cfg(windows)]
    let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());

    #[cfg(windows)]
    let extensions: Vec<&str> = pathext.split(';').collect();

    if let Ok(paths) = env::var("PATH") {
        for path in env::split_paths(&paths) {
            let exact_path = path.join(&cmd);
            if is_good_which(exact_path.clone()) {
                return Ok(Some(exact_path));
            }

            // Check for filename + ext from pathext
            #[cfg(windows)]
            for ext in &extensions {
                let cmd_with_ext = format!("{}{}", cmd, ext);
                let full_path = path.join(&cmd_with_ext);
                if is_good_which(full_path.clone()) {
                    return Ok(Some(full_path));
                }
            }
        }
    }

    Ok(None)
}

// Checks to see if target is a file or a symlink to a file
fn is_good_which(path: PathBuf) -> bool {
    if !path.exists() {
        return false;
    }

    if path.is_file() {
        return true;
    }

    if path.is_symlink() && path.metadata().is_ok_and(|p| p.is_file()) {
        return true;
    }

    false
}
fn dotfiles_env(_lua: &Lua, args: MultiValue) -> mlua::Result<Option<String>> {
    let mut args = args.into_iter();

    let arg_a = args.next();
    let arg_b = args.next();

    match (arg_a, arg_b) {
        (Some(Value::String(name)), None) => {
            if let Ok(v) = env::var(name.to_string_lossy()) {
                Ok(Some(v))
            } else {
                Ok(None)
            }
        },
        (Some(Value::String(name)), Some(Value::String(default))) => {
            if let Ok(v) = env::var(name.to_string_lossy()) {
                Ok(Some(v))
            } else {
                Ok(Some(default.to_string_lossy()))
            }
        },
        _ => Ok(None),
    }
}

fn dotfiles_hostname(_lua: &Lua, _args: ()) -> mlua::Result<String> {
    whoami::hostname().into_lua_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotfiles_global_exists() {
        let lua = create_vm().unwrap();
        let _table: mlua::Table = lua.globals().get("dotfiles").unwrap();
    }

    #[test]
    fn os_returns_non_empty_string() {
        let lua = create_vm().unwrap();
        let result: String = lua.load("return dotfiles.os()").eval().unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn arch_returns_non_empty_string() {
        let lua = create_vm().unwrap();
        let result: String = lua.load("return dotfiles.arch()").eval().unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn env_returns_nil_for_missing_var() {
        let lua = create_vm().unwrap();
        // Use a name that will never be set in any environment
        let result: mlua::Value = lua
            .load("return dotfiles.env('__DBA_NONEXISTENT_VAR_12345')")
            .eval()
            .unwrap();
        assert!(matches!(result, mlua::Value::Nil));
    }

    // This test is safe to run in parallel with other tests because it is the only one calling set_var / remove_var
    // If other tests are needed with env vars being changed either add them to this test or only run with --test-threads=1
    #[test]
    fn env_returns_value_for_set_var() {
        unsafe {
            env::set_var("__DBA_TEST_VAR", "hello");
        }
        let lua = create_vm().unwrap();
        let result: String = lua.load("return dotfiles.env('__DBA_TEST_VAR')").eval().unwrap();
        unsafe {
            env::remove_var("__DBA_TEST_VAR");
        }
        assert_eq!(result, "hello");
    }

    #[test]
    fn which_returns_nil_for_nonexistent_binary() {
        let lua = create_vm().unwrap();
        let result: mlua::Value = lua
            .load("return dotfiles.which('__this_binary_does_not_exist__')")
            .eval()
            .unwrap();
        assert!(matches!(result, mlua::Value::Nil));
    }

    #[test]
    fn which_finds_existing_binary() {
        // Use a binary that is reliably present on the CI host.
        // On Unix this is typically "sh"; on Windows "cmd".
        #[cfg(unix)]
        let binary = "sh";
        #[cfg(windows)]
        let binary = "cmd";

        let lua = create_vm().unwrap();
        let result: mlua::Value = lua
            .load(&format!("return dotfiles.which('{}')", binary))
            .eval()
            .unwrap();
        assert!(matches!(result, mlua::Value::String(_)));
    }
}
