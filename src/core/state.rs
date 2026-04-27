use std::{
    collections::BTreeMap,
    path::{
        Path,
        PathBuf,
    },
};

use chrono::{
    DateTime,
    Utc,
};
use serde::{
    Deserialize,
    Serialize,
};
use tracing::debug;

use crate::{
    core::{
        hash::hash_file,
        plan::PlannedOp,
        types::EntryType,
    },
    error::IoContext,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum ManagedEntry {
    Symlink {
        module:      String,
        source:      PathBuf,
        deployed_at: DateTime<Utc>,
    },
    Copy {
        module:        String,
        source:        PathBuf,
        deployed_at:   DateTime<Utc>,
        source_hash:   String,
        deployed_hash: String,
    },
    Template {
        module:        String,
        source:        PathBuf,
        deployed_at:   DateTime<Utc>,
        source_hash:   String,
        deployed_hash: String,
    },
    Orphaned {
        module:      String,
        source:      PathBuf,
        deployed_at: DateTime<Utc>,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct State {
    pub(crate) managed: BTreeMap<PathBuf, ManagedEntry>,
}

impl State {
    pub(crate) fn load<P: AsRef<Path>>(path: P) -> crate::core::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            debug!(?path, "State does not exist, returning default");
            return Ok(Default::default());
        }

        let content = std::fs::read_to_string(path).io_err(format!("reading state file: {}", path.display()))?;
        let state = serde_json::from_str::<State>(&content)?;
        Ok(state)
    }

    pub(crate) fn save<P: AsRef<Path>>(&self, path: P) -> crate::core::Result<()> {
        let path = path.as_ref();
        debug!(path = path.to_str(), "Saving state");
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            debug!(?path, "Parent directory does not exist, creating");
            std::fs::create_dir_all(parent).io_err(format!("creating directory: {}", parent.display()))?;
        }

        debug!("Saving state to {:?}", path);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).io_err(format!("writing state file: {}", path.display()))?;

        Ok(())
    }

    pub(crate) fn record_op(&mut self, op: &PlannedOp) -> crate::core::Result<()> {
        let entry = match op.entry_type {
            EntryType::Symlink => ManagedEntry::Symlink {
                module:      op.module_name.clone(),
                source:      op.src.clone(),
                deployed_at: Utc::now(),
            },
            EntryType::Copy => ManagedEntry::Copy {
                module:        op.module_name.clone(),
                source:        op.src.clone(),
                deployed_at:   Utc::now(),
                source_hash:   hash_file(&op.src)?,
                deployed_hash: hash_file(&op.dst)?,
            },
            EntryType::Template => ManagedEntry::Template {
                module:        op.module_name.clone(),
                source:        op.src.clone(),
                deployed_at:   Utc::now(),
                source_hash:   hash_file(&op.src)?,
                deployed_hash: hash_file(&op.dst)?,
            },
        };
        self.managed.insert(op.dst.clone(), entry);
        Ok(())
    }

    pub(crate) fn mark_orphans<I>(&mut self, active_modules: I)
    where
        I: IntoIterator<Item = String>,
    {
        let active_names = active_modules.into_iter().collect::<std::collections::HashSet<_>>();
        for (_, entry) in self.managed.iter_mut() {
            match entry {
                ManagedEntry::Symlink {
                    module,
                    source,
                    deployed_at,
                } => {
                    if !active_names.contains(module) {
                        *entry = ManagedEntry::Orphaned {
                            module:      module.clone(),
                            source:      source.clone(),
                            deployed_at: *deployed_at,
                        }
                    }
                },
                ManagedEntry::Copy {
                    module,
                    source,
                    deployed_at,
                    ..
                } => {
                    if !active_names.contains(module) {
                        *entry = ManagedEntry::Orphaned {
                            module:      module.clone(),
                            source:      source.clone(),
                            deployed_at: *deployed_at,
                        }
                    }
                },
                ManagedEntry::Template {
                    module,
                    source,
                    deployed_at,
                    ..
                } => {
                    if !active_names.contains(module) {
                        *entry = ManagedEntry::Orphaned {
                            module:      module.clone(),
                            source:      source.clone(),
                            deployed_at: *deployed_at,
                        }
                    }
                },
                ManagedEntry::Orphaned { .. } => {
                    // Nothing doing maybe?
                    continue;
                },
            };
        }
    }

    pub(crate) fn remove_orphan<P: AsRef<Path>>(&mut self, path: P) -> crate::core::Result<()> {
        let path = path.as_ref();
        if self.managed.remove(path).is_none() {
            return Err(crate::error::Error::ErrorMessage(format!(
                "no record for: {}",
                path.display()
            )));
        }
        Ok(())
    }

    pub(crate) fn tracked<P: AsRef<Path>>(&self, path: P) -> Option<ManagedEntry> {
        let path = path.as_ref();
        self.managed.get(path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Utc;
    use tempfile::TempDir;

    use super::*;

    fn symlink_entry(module: &str) -> ManagedEntry {
        ManagedEntry::Symlink {
            source:      "/dotfiles/shell".into(),
            module:      module.into(),
            deployed_at: Utc::now(),
        }
    }

    #[test]
    fn state_round_trips_through_json() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.json");

        let mut state = State::default();
        state.managed.insert(".config/shell".into(), symlink_entry("base"));

        state.save(&path).unwrap();
        let loaded = State::load(&path).unwrap();

        assert_eq!(loaded.managed.len(), 1);
        assert!(loaded.managed.contains_key(Path::new(".config/shell")));
    }

    #[test]
    fn missing_state_file_returns_empty_state() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent/state.json");
        let state = State::load(&path).unwrap();
        assert!(state.managed.is_empty());
    }

    #[test]
    fn save_creates_parent_directories() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("deeply/nested/dir/state.json");
        let state = State::default();
        state.save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn mark_orphans_flags_inactive_module_entries() {
        let mut state = State::default();
        state.managed.insert(".config/base".into(), symlink_entry("base"));
        state.managed.insert(".config/nvim".into(), symlink_entry("neovim"));

        let active: HashSet<String> = ["base".to_string()].into_iter().collect();
        state.mark_orphans(active);

        // "base" entry should be unchanged
        assert!(matches!(
            state.managed[Path::new(".config/base")],
            ManagedEntry::Symlink { .. }
        ));

        // "neovim" entry should now be orphaned
        assert!(matches!(
            state.managed[Path::new(".config/nvim")],
            ManagedEntry::Orphaned { .. }
        ));
    }

    #[test]
    fn mark_orphans_does_not_affect_active_modules() {
        let mut state = State::default();
        state.managed.insert(".config/shell".into(), symlink_entry("base"));

        let active: HashSet<String> = ["base".to_string()].into_iter().collect();
        state.mark_orphans(active);

        // Still a symlink, not orphaned
        assert!(matches!(
            state.managed[Path::new(".config/shell")],
            ManagedEntry::Symlink { .. }
        ));
    }
}
