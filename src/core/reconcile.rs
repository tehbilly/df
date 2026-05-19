use std::{
    fmt::{
        Display,
        Formatter,
    },
    fs,
};

use tracing::{
    error,
    info,
};

use crate::{
    core::{
        hash::hash_file,
        plan::PlannedOp,
        state::{
            ManagedEntry,
            State,
        },
        types::EntryType,
    },
    error::IoContext,
};

#[derive(Debug, PartialEq)]
pub(crate) enum ReconcileStatus {
    Deploy,
    Clean, // TODO: Maybe something like InSync?
    SourceChanged,
    ExternallyModified,
    Unmanaged,
}

impl Display for ReconcileStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconcileStatus::Deploy => {
                write!(f, "deploy")
            },
            ReconcileStatus::Clean => {
                write!(f, "clean")
            },
            ReconcileStatus::SourceChanged => {
                write!(f, "source_changed")
            },
            ReconcileStatus::ExternallyModified => {
                write!(f, "externally_modified")
            },
            ReconcileStatus::Unmanaged => {
                write!(f, "unmanaged")
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReconcileItem {
    pub(crate) op:     PlannedOp,
    pub(crate) status: ReconcileStatus,
}

impl ReconcileItem {
    pub(crate) fn for_op(op: &PlannedOp, state: &State) -> crate::core::Result<Self> {
        match state.tracked(&op.dst) {
            None => {
                if op.dst.is_symlink() || op.dst.exists() {
                    Ok(ReconcileItem {
                        op:     op.clone(),
                        status: ReconcileStatus::Unmanaged,
                    })
                } else {
                    Ok(ReconcileItem {
                        op:     op.clone(),
                        status: ReconcileStatus::Deploy,
                    })
                }
            },
            Some(state_entry) => {
                match state_entry {
                    ManagedEntry::Symlink { .. } => {
                        // Type changed? Redeploy
                        if op.entry_type != EntryType::Symlink {
                            info!(?op, "Changed from symlink, need to redeploy");
                            return Ok(ReconcileItem {
                                op:     op.clone(),
                                status: ReconcileStatus::Deploy,
                            });
                        }

                        // Symlink (not target) doesn't exist
                        if !op.dst.is_symlink() && !op.dst.exists() {
                            return Ok(ReconcileItem {
                                op:     op.clone(),
                                status: ReconcileStatus::Deploy,
                            });
                        }

                        // This _should_ be a symlink, yeah?
                        if !op.dst.is_symlink() {
                            return Ok(ReconcileItem {
                                op:     op.clone(),
                                status: ReconcileStatus::ExternallyModified,
                            });
                        }

                        // Get the symlink target so we can make sure it hasn't changed
                        let target = match fs::read_link(&op.dst).io_err(format!("path: {}", op.dst.display())) {
                            Ok(link) => link,
                            Err(err) => {
                                error!(?err, "Unable to read link.");
                                return Err(err);
                            },
                        };

                        let target = match target.canonicalize().io_err(format!("path: {}", target.display())) {
                            Ok(target) => target,
                            Err(err) => {
                                error!(?err, "Unable to canonicalize link target");
                                return Err(err);
                            },
                        };

                        let source = match op.src.canonicalize().io_err(format!("path: {}", op.src.display())) {
                            Ok(source) => source,
                            Err(err) => {
                                error!(?err, "Unable to canonicalize link source");
                                return Err(err);
                            },
                        };

                        if target != source {
                            info!(?target, src = ?op.src, "Symlink doesn't point where we'd expect");
                            Ok(ReconcileItem {
                                op:     op.clone(),
                                status: ReconcileStatus::ExternallyModified,
                            })
                        } else {
                            Ok(ReconcileItem {
                                op:     op.clone(),
                                status: ReconcileStatus::Clean,
                            })
                        }
                    },
                    ManagedEntry::Copy {
                        deployed_hash,
                        source_hash,
                        ..
                    } => reconcile_content_op(op, EntryType::Copy, &deployed_hash, &source_hash),
                    ManagedEntry::Template {
                        deployed_hash,
                        source_hash,
                        ..
                    } => reconcile_content_op(op, EntryType::Template, &deployed_hash, &source_hash),
                    ManagedEntry::Orphaned { .. } => {
                        // Item was previously orphaned, module was re-enabled
                        Ok(ReconcileItem {
                            op:     op.clone(),
                            status: ReconcileStatus::Deploy,
                        })
                    },
                }
            },
        }
    }
}

fn reconcile_content_op(
    op: &PlannedOp,
    expected_type: EntryType,
    deployed_hash: &str,
    source_hash: &str,
) -> crate::core::Result<ReconcileItem> {
    if op.entry_type != expected_type {
        info!(?op, "Content type changed (copy/template), need to redeploy");
        return Ok(ReconcileItem {
            op:     op.clone(),
            status: ReconcileStatus::Deploy,
        });
    }

    let dst_hash = hash_file(&op.dst)?;
    if dst_hash != *deployed_hash {
        // Target file doesn't look like we'd expect
        return Ok(ReconcileItem {
            op:     op.clone(),
            status: ReconcileStatus::ExternallyModified,
        });
    }

    let src_hash = hash_file(&op.src)?;
    if src_hash != *source_hash {
        // We track it, but the file has changed
        return Ok(ReconcileItem {
            op:     op.clone(),
            status: ReconcileStatus::SourceChanged,
        });
    }

    Ok(ReconcileItem {
        op:     op.clone(),
        status: ReconcileStatus::Clean,
    })
}

#[allow(unused_qualifications)]
#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::TempDir;

    use super::*;
    use crate::core::{
        hash::hash_file,
        types::EntryType,
    };

    fn make_op(src: &std::path::Path, dst: &std::path::Path, entry_type: EntryType) -> PlannedOp {
        PlannedOp {
            module_name: "test".into(),
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
            entry_type,
        }
    }

    #[test]
    fn new_path_not_on_disk_is_deploy() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        std::fs::write(&src, "content").unwrap();
        let dst = tmp.path().join("nonexistent");

        let op = make_op(&src, &dst, EntryType::Copy);
        let state = State::default();
        let item = ReconcileItem::for_op(&op, &state).unwrap();
        assert!(matches!(item.status, ReconcileStatus::Deploy));
    }

    #[test]
    fn existing_unmanaged_file_is_unmanaged() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("existing");
        std::fs::write(&src, "content").unwrap();
        std::fs::write(&dst, "other content").unwrap();

        let op = make_op(&src, &dst, EntryType::Copy);
        let state = State::default(); // dst not in state
        let item = ReconcileItem::for_op(&op, &state).unwrap();
        assert!(matches!(item.status, ReconcileStatus::Unmanaged));
    }

    #[test]
    fn copy_with_matching_hashes_is_clean() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("deployed");
        std::fs::write(&src, "content").unwrap();
        std::fs::write(&dst, "content").unwrap();
        let h = hash_file(&src).unwrap();

        let op = make_op(&src, &dst, EntryType::Copy);
        let mut state = State::default();
        state.managed.insert(dst.clone(), ManagedEntry::Copy {
            source:        src.clone(),
            module:        "test".into(),
            source_hash:   h.clone(),
            deployed_hash: h,
            deployed_at:   Utc::now(),
        });
        let item = ReconcileItem::for_op(&op, &state).unwrap();
        assert!(matches!(item.status, ReconcileStatus::Clean));
    }

    #[test]
    fn copy_with_changed_deployed_hash_is_externally_modified() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("deployed");
        std::fs::write(&src, "original").unwrap();
        std::fs::write(&dst, "modified by user").unwrap();
        let original_hash = hash_file(&src).unwrap();

        let op = make_op(&src, &dst, EntryType::Copy);
        let mut state = State::default();
        state.managed.insert(dst.clone(), ManagedEntry::Copy {
            source:        src.clone(),
            module:        "test".into(),
            source_hash:   original_hash.clone(),
            deployed_hash: original_hash, // what we wrote originally
            deployed_at:   Utc::now(),
        });
        let item = ReconcileItem::for_op(&op, &state).unwrap();
        assert!(matches!(item.status, ReconcileStatus::ExternallyModified));
    }

    #[test]
    fn copy_with_changed_source_hash_is_source_changed() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("deployed");
        std::fs::write(&src, "new source content").unwrap();
        // dst has the old content (what was deployed last time)
        std::fs::write(&dst, "old deployed content").unwrap();
        let deployed_hash = hash_file(&dst).unwrap();

        let op = make_op(&src, &dst, EntryType::Copy);
        let mut state = State::default();
        state.managed.insert(dst.clone(), ManagedEntry::Copy {
            source: src.clone(),
            module: "test".into(),
            // The source hash at deploy time was different from the current source
            source_hash: "old_source_hash_that_differs".into(),
            deployed_hash,
            deployed_at: Utc::now(),
        });
        let item = ReconcileItem::for_op(&op, &state).unwrap();
        assert!(matches!(item.status, ReconcileStatus::SourceChanged));
    }
}
