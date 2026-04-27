use std::{
    collections::HashMap,
    path::Path,
};

use tracing::info;

use crate::{
    core::{
        plan::PlannedOp,
        types::EntryType,
    },
    error::IoContext,
    template::render_template,
};

pub(crate) fn perform_op(
    op: &PlannedOp,
    vars: &HashMap<String, serde_json::Value>,
    dry_run: bool,
) -> crate::core::Result<()> {
    if let Some(parent) = op.dst.parent()
        && !parent.exists()
    {
        info!(?parent, "Creating dir");
        if !dry_run {
            std::fs::create_dir_all(parent).io_err(format!("creating dir: {}", parent.display()))?;
        }
    }

    match op.entry_type {
        EntryType::Symlink => {
            info!(src = op.src.to_str(), dst = op.dst.to_str(), "Creating symlink");
            if !dry_run {
                if op.dst.exists() || op.dst.is_symlink() {
                    std::fs::remove_file(&op.dst)
                        .io_err(format!("removing existing dst before symlinking: {}", op.dst.display()))?;
                }
                create_symlink(&op.src, &op.dst)?;
            }
        },
        EntryType::Copy => {
            info!(src = op.src.to_str(), dst = op.dst.to_str(), "Copying file");
            if !dry_run {
                std::fs::copy(&op.src, &op.dst).io_err(format!(
                    "copying file: {} -> {}",
                    &op.src.display(),
                    &op.dst.display()
                ))?;
            }
        },
        EntryType::Template => {
            info!(src = op.src.to_str(), dst = op.dst.to_str(), "Copying (templated) file");
            if !dry_run {
                let src = std::fs::read_to_string(&op.src)
                    .io_err(format!("error reading to string: {}", op.src.display()))?;
                let out = render_template(&src, vars)?;
                std::fs::write(&op.dst, out).io_err(format!("writing template to: {}", &op.dst.display()))?;
            }
        },
    }

    Ok(())
}

#[cfg(unix)]
fn create_symlink<P: AsRef<Path>>(target: P, link_path: P) -> crate::core::Result<()> {
    use std::os::unix::fs::symlink;
    let target = target.as_ref();
    let link_path = link_path.as_ref();
    symlink(target, link_path).io_err(format!(
        "failed to create symlink: {} -> {}",
        link_path.display(),
        target.display()
    ))?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink<P: AsRef<Path>>(target: P, link_path: P) -> crate::core::Result<()> {
    use std::os::windows::fs::{
        symlink_dir,
        symlink_file,
    };
    let target = target.as_ref();
    let link_path = link_path.as_ref();

    if target.is_dir() {
        symlink_dir(target, link_path).io_err(format!(
            "failed to symlink dir: {} -> {}",
            link_path.display(),
            target.display()
        ))?;
    } else {
        symlink_file(target, link_path).io_err(format!(
            "failed to symlink: {} -> {}",
            link_path.display(),
            target.display()
        ))?
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;
    use crate::core::{
        plan::PlannedOp,
        types::EntryType,
    };

    fn make_op(src: &Path, dst: &Path, entry_type: EntryType) -> PlannedOp {
        PlannedOp {
            module_name: "test".into(),
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
            entry_type,
        }
    }

    #[test]
    fn symlink_creates_a_symlink_at_dst() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source_file");
        let dst = tmp.path().join("link");
        std::fs::write(&src, "data").unwrap();

        let op = make_op(&src, &dst, EntryType::Symlink);
        perform_op(&op, &HashMap::new(), false).unwrap();

        assert!(dst.is_symlink());
        assert_eq!(std::fs::read_link(&dst).unwrap(), src);
    }

    #[test]
    fn copy_writes_file_content_to_dst() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("deployed");
        std::fs::write(&src, "file content").unwrap();

        let op = make_op(&src, &dst, EntryType::Copy);
        perform_op(&op, &HashMap::new(), false).unwrap();

        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "file content");
    }

    #[test]
    fn template_renders_vars_into_dst() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("template");
        let dst = tmp.path().join("rendered");
        std::fs::write(&src, "email = {{ email }}").unwrap();

        let vars = [("email".to_string(), serde_json::json!("me@example.com"))]
            .into_iter()
            .collect();
        let op = make_op(&src, &dst, EntryType::Template);
        perform_op(&op, &vars, false).unwrap();

        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "email = me@example.com");
    }

    #[test]
    fn dry_run_makes_no_filesystem_changes() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("dst");
        std::fs::write(&src, "content").unwrap();

        let op = make_op(&src, &dst, EntryType::Copy);
        perform_op(&op, &HashMap::new(), true).unwrap();

        // dst must not exist after a dry run
        assert!(!dst.exists());
    }

    #[test]
    fn perform_op_creates_parent_directories() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("a/b/c/deployed");
        std::fs::write(&src, "content").unwrap();

        let op = make_op(&src, &dst, EntryType::Copy);
        perform_op(&op, &HashMap::new(), false).unwrap();

        assert!(dst.exists());
    }
}
