use std::path::{
    PathBuf,
    StripPrefixError,
};

use color_eyre::Report;

pub(crate) trait IoContext<T> {
    fn io_err(self, context: impl Into<String>) -> Result<T, Error>;
}

impl<T> IoContext<T> for Result<T, std::io::Error> {
    fn io_err(self, context: impl Into<String>) -> Result<T, Error> {
        self.map_err(|source| Error::IoError {
            source,
            context: context.into(),
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    ErrorMessage(String),

    #[error("config.lua not found")]
    ConfigLuaNotFound,
    #[error("local.lua not found")]
    LocalLuaNotFound,
    #[error("unknown module name: {0}")]
    UnknownModuleName(String),
    #[error("dependency cycle detected: {0:?}")]
    DependencyCycleDetected(Vec<String>),
    #[error("multiple modules pointing to {dest}: {modules:?}")]
    CrossModuleDestinationConflict { dest: PathBuf, modules: Vec<String> },
    #[error("directory symlink nesting violation: {link} is nested beneath {dir}")]
    DirectorySymlinkNestingViolation { dir: PathBuf, link: PathBuf },

    #[error("IO error {context}: {source}")]
    IoError {
        context: String,
        #[backtrace]
        source:  std::io::Error,
    },
    #[error(transparent)]
    LuaError(#[from] mlua::Error),
    #[error(transparent)]
    TemplateError(#[from] minijinja::Error),
    #[error(transparent)]
    SerdeJsonError(#[from] serde_json::Error),
}

impl From<Report> for Error {
    fn from(value: Report) -> Self {
        Error::ErrorMessage(value.to_string())
    }
}

impl From<StripPrefixError> for Error {
    fn from(value: StripPrefixError) -> Self {
        Error::ErrorMessage(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn error_messages_are_non_empty() {
        let variants = vec![
            Error::ConfigLuaNotFound,
            Error::LocalLuaNotFound,
            Error::UnknownModuleName("fake_module".to_string()),
            Error::DependencyCycleDetected(vec!["a".to_string(), "b".to_string()]),
            Error::CrossModuleDestinationConflict {
                dest:    Default::default(),
                modules: vec![],
            },
            Error::DirectorySymlinkNestingViolation {
                dir:  PathBuf::from("~/.config/foo"),
                link: PathBuf::from("~/.config/foo/nested/path.toml"),
            },
        ];

        for err in variants.iter() {
            assert!(!err.to_string().is_empty(), "variant formatted as empty string");
        }
    }
}
