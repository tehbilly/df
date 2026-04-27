use std::path::{
    Path,
    PathBuf,
};

use indexmap::IndexMap;
use tracing::{
    debug,
    error,
};
use walkdir::WalkDir;

use crate::{
    core::types::{
        EntryType,
        Module,
    },
    error::Error,
};

#[derive(Debug, Clone)]
pub(crate) struct PlannedOp {
    pub(crate) module_name: String,
    pub(crate) entry_type:  EntryType,
    pub(crate) src:         PathBuf,
    pub(crate) dst:         PathBuf,
}

pub(crate) struct WorkPlan {
    ops: IndexMap<String, Vec<PlannedOp>>,
}

impl WorkPlan {
    pub(crate) fn build<P: AsRef<Path>>(modules: &[Module], dotfiles: P, output: P) -> crate::core::Result<Self> {
        let dotfiles = dotfiles.as_ref();
        let output = output.as_ref();

        let mut ops: IndexMap<PathBuf, PlannedOp> = IndexMap::new();

        // First pass: expand directories and add all implicit ops
        for module in modules {
            for fe in module.files.iter().filter(|fe| dotfiles.join(&fe.src).is_dir()) {
                if fe.entry_type == EntryType::Symlink {
                    // Single op for directory when linking, no expansion
                    let out_path = output.join(fe.dst.clone());
                    ops.insert(out_path.clone(), PlannedOp {
                        module_name: module.name.clone(),
                        entry_type:  EntryType::Symlink,
                        src:         dotfiles.join(fe.src.clone()),
                        dst:         out_path, // Dunno if this feels right
                    });
                } else {
                    for entry in WalkDir::new(dotfiles.join(&fe.src)) {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(err) => {
                                error!(?err, entry = ?fe, "Error reading dir entry when walking path");
                                continue;
                            },
                        };

                        if entry.file_type().is_dir() {
                            debug!(?entry, ?fe, "skipping directory entry");
                            continue;
                        }

                        let rel_path = entry.path().strip_prefix(dotfiles.join(&fe.src))?;
                        let out_path = output.join(&fe.dst).join(rel_path);

                        ops.insert(out_path.clone(), PlannedOp {
                            module_name: module.name.clone(),
                            entry_type:  fe.entry_type.clone(),
                            src:         dotfiles.join(&fe.src).join(rel_path),
                            dst:         out_path,
                        });
                    }
                }
            }
        }

        // Second pass: All files, allows explicit operations to override implicit ones
        for module in modules {
            for fe in module.files.iter().filter(|fe| !dotfiles.join(&fe.src).is_dir()) {
                let out_path = output.join(&fe.dst);
                if let Some(prev) = ops.insert(out_path.clone(), PlannedOp {
                    module_name: module.name.clone(),
                    entry_type:  fe.entry_type.clone(),
                    src:         dotfiles.join(&fe.src),
                    dst:         out_path.clone(),
                }) {
                    debug!(?prev, ?fe, "Explicitly overriding implicit configuration");
                    // This is an error if different modules are targeting the same file
                    if prev.module_name != module.name {
                        return Err(Error::CrossModuleDestinationConflict {
                            dest:    out_path.clone(),
                            modules: vec![prev.module_name.clone(), module.name.clone()],
                        });
                    }
                }
            }
        }

        // We need to look for symlinked dirs that have other operations targeting a path below them
        // A symlinked dir from the source repo should be pristine (other than overrides from the same module)
        let ordered_ops: Vec<PlannedOp> = ops.values().cloned().collect();
        let symlinked_dirs = ordered_ops
            .iter()
            .filter(|op| op.entry_type == EntryType::Symlink && op.src.is_dir())
            .map(|op| op.dst.clone())
            .collect::<Vec<_>>();
        for op in ordered_ops.iter() {
            for dir in symlinked_dirs.iter() {
                if op.dst != *dir && op.dst.starts_with(dir) {
                    return Err(Error::DirectorySymlinkNestingViolation {
                        dir:  dir.clone(),
                        link: op.dst.clone(),
                    });
                }
            }
        }

        let mut final_ops = IndexMap::new();
        for module in modules {
            let module_ops = ops
                .iter()
                .filter(|(_, op)| op.module_name == module.name)
                .map(|(_, op)| op)
                .cloned()
                .collect::<Vec<_>>();
            final_ops.insert(module.name.clone(), module_ops);
        }

        Ok(Self { ops: final_ops })
    }

    pub(crate) fn ops(&self) -> &IndexMap<String, Vec<PlannedOp>> {
        &self.ops
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;
    use crate::core::types::{
        EntryType,
        FileEntry,
        Hooks,
        Module,
    };

    fn file_entry(src: &str, dst: &str, entry_type: EntryType) -> FileEntry {
        FileEntry {
            src: src.into(),
            dst: dst.into(),
            entry_type,
        }
    }

    fn module_with_files(name: &str, files: Vec<FileEntry>) -> Module {
        Module {
            name: name.into(),
            files,
            deps: vec![],
            vars: HashMap::new(),
            hooks: Hooks::default(),
        }
    }

    #[test]
    fn symlink_file_produces_one_op() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        // Create the source file
        std::fs::write(repo.join("gitconfig"), "").unwrap();

        let modules = vec![module_with_files("git", vec![file_entry(
            "gitconfig",
            ".gitconfig",
            EntryType::Symlink,
        )])];
        let output = tmp.path().join("home");
        let plan = WorkPlan::build(&modules, &repo, &output).unwrap();
        assert_eq!(plan.ops["git"].len(), 1);
        assert_eq!(plan.ops["git"][0].entry_type, EntryType::Symlink);
    }

    #[test]
    fn copy_directory_expands_to_per_file_ops() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        // Create a directory with two files
        let src_dir = repo.join("nvim");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("init.lua"), "").unwrap();
        std::fs::write(src_dir.join("options.lua"), "").unwrap();

        let modules = vec![module_with_files("neovim", vec![file_entry(
            "nvim/",
            ".config/nvim/",
            EntryType::Copy,
        )])];
        let output = tmp.path().join("home");
        let plan = WorkPlan::build(&modules, &repo, &output).unwrap();
        // Two files in the directory → two ops
        assert_eq!(plan.ops["neovim"].len(), 2);
        assert!(plan.ops["neovim"].iter().all(|op| op.entry_type == EntryType::Copy));
    }

    #[test]
    fn explicit_template_entry_overrides_copy_derived_entry() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let src_dir = repo.join("nvim");
        std::fs::create_dir_all(src_dir.join("lua/config")).unwrap();
        std::fs::write(repo.join("nvim/init.lua"), "").unwrap();
        std::fs::write(repo.join("nvim/lua/config/options.lua"), "").unwrap();

        let modules = vec![module_with_files("neovim", vec![
            file_entry("nvim/", ".config/nvim/", EntryType::Copy),
            file_entry(
                "nvim/lua/config/options.lua",
                ".config/nvim/lua/config/options.lua",
                EntryType::Template,
            ),
        ])];
        let output = tmp.path().join("home");
        let plan = WorkPlan::build(&modules, &repo, &output).unwrap();

        // Both files are present; the explicit template wins for options.lua
        let options_op = plan.ops["neovim"]
            .iter()
            .find(|op| op.dst.ends_with("options.lua"))
            .expect("options.lua op not found");
        assert_eq!(options_op.entry_type, EntryType::Template);

        // init.lua was not overridden — it stays as copy
        let init_op = plan.ops["neovim"]
            .iter()
            .find(|op| op.dst.ends_with("init.lua"))
            .expect("init.lua op not found");
        assert_eq!(init_op.entry_type, EntryType::Copy);
    }

    #[test]
    fn cross_module_conflict_returns_error() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("fileA"), "").unwrap();
        std::fs::write(repo.join("fileB"), "").unwrap();

        // Both modules declare the same dst
        let modules = vec![
            module_with_files("mod_a", vec![file_entry("fileA", ".config/shared", EntryType::Symlink)]),
            module_with_files("mod_b", vec![file_entry("fileB", ".config/shared", EntryType::Symlink)]),
        ];
        let output = tmp.path().join("home");
        let result = WorkPlan::build(&modules, &repo, &output);
        assert!(matches!(result, Err(Error::CrossModuleDestinationConflict { .. })));
    }

    #[test]
    fn dir_symlink_nesting_returns_error() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        // A directory source for the symlink entry
        let nvim_dir = repo.join("nvim");
        std::fs::create_dir_all(&nvim_dir).unwrap();
        std::fs::write(repo.join("init.lua"), "").unwrap();

        // First entry: symlink the nvim/ directory
        // Second entry: target a path beneath the symlink dst
        let modules = vec![module_with_files("neovim", vec![
            file_entry("nvim/", ".config/nvim/", EntryType::Symlink),
            file_entry("init.lua", ".config/nvim/init.lua", EntryType::Template),
        ])];
        let output = tmp.path().join("home");
        let result = WorkPlan::build(&modules, &repo, &output);
        assert!(matches!(result, Err(Error::DirectorySymlinkNestingViolation { .. })));
    }

    #[test]
    fn module_with_no_files_appears_in_plan() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let modules = vec![Module {
            name:  "hooks_only".into(),
            files: vec![],
            deps:  vec![],
            vars:  HashMap::new(),
            hooks: Hooks::default(),
        }];
        let output = tmp.path().join("home");
        let plan = WorkPlan::build(&modules, &repo, &output).unwrap();

        assert!(plan.ops.contains_key("hooks_only"));
        assert_eq!(plan.ops["hooks_only"].len(), 0);
    }

    #[test]
    fn modules_appear_in_dependency_order() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("a"), "").unwrap();
        std::fs::write(repo.join("b"), "").unwrap();

        // Pass modules in topological order: base before neovim
        let modules = vec![
            module_with_files("base", vec![file_entry("a", ".config/a", EntryType::Copy)]),
            module_with_files("neovim", vec![file_entry("b", ".config/b", EntryType::Copy)]),
        ];
        let output = tmp.path().join("home");
        let plan = WorkPlan::build(&modules, &repo, &output).unwrap();

        let keys: Vec<&String> = plan.ops.keys().collect();
        assert_eq!(keys[0], "base");
        assert_eq!(keys[1], "neovim");
    }
}
