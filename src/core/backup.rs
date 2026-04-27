use std::path::{
    Path,
    PathBuf,
};

use tracing::{
    debug,
    info,
    warn,
};

use crate::{
    error,
    error::IoContext,
};

#[derive(Debug)]
pub(crate) struct BackupDir {
    // The timestamped backup directory
    dir: PathBuf,
    // The output path, used to calculate relative path
    out: PathBuf,
}

impl BackupDir {
    pub(crate) fn create<P: AsRef<Path>>(repo_root: P, output: P) -> crate::core::Result<Self> {
        let repo_root = repo_root.as_ref();
        let output = output.as_ref();

        let now = chrono::Local::now();
        let backup_dir = repo_root
            .join(".backups")
            .join(now.format("%Y-%m-%d_%H-%M-%S").to_string());

        if backup_dir.exists() {
            return Err(error::Error::ErrorMessage(format!(
                "backup directory already exists: {}",
                backup_dir.display()
            )));
        }

        info!(?backup_dir, "Creating backup dir");
        std::fs::create_dir_all(&backup_dir).io_err(format!("creating backup dir: {}", backup_dir.display()))?;

        Ok(Self {
            dir: backup_dir,
            out: output.to_path_buf(),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn backup_file<P: AsRef<Path>>(&self, path: P) -> crate::core::Result<()> {
        let path = path.as_ref();

        if !path.starts_with(&self.out) {
            return Err(error::Error::ErrorMessage(format!(
                "path {} is not below {}",
                path.display(),
                self.out.display()
            )));
        }

        // Path relative from the output dir
        let rel_path = path.strip_prefix(&self.out)?;
        let out_path = self.dir.join(rel_path);

        if let Some(parent) = out_path.parent()
            && !parent.exists()
        {
            debug!(?parent, ?out_path, "Creating parent dir for backup");
            std::fs::create_dir_all(parent).io_err(format!("creating parent dir: {}", parent.display()))?;
        }

        // Try to rename, fallback to copying and deleting
        if let Err(err) = std::fs::rename(path, out_path.clone()) {
            warn!(?err, ?out_path, "Failed to rename file. Falling back to copy.");

            if let Ok(target) = std::fs::read_link(path) {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::symlink;
                    info!(?target, ?out_path, "Creating symlink");
                    symlink(&target, &out_path).io_err(format!("creating symlink: {}", target.display()))?;
                }

                #[cfg(windows)]
                {
                    use std::os::windows::fs::symlink_file;
                    info!(?target, ?out_path, "Creating symlink");
                    symlink_file(&target, &out_path).io_err(format!("creating symlink: {}", target.display()))?;
                }

                return Ok(());
            }

            std::fs::copy(path, &out_path).io_err(format!(
                "copying file: {} -> {}",
                out_path.display(),
                path.display()
            ))?;
            std::fs::remove_file(path).io_err(format!("removing copied file: {}", path.display()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn backup_dir_is_created_with_timestamp() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let output = tmp.path().join("home");
        std::fs::create_dir_all(&repo).unwrap();

        let backup = BackupDir::create(&repo, &output).unwrap();
        assert!(backup.path().exists());
        // The directory should be inside .backups/
        assert!(backup.path().starts_with(repo.join(".backups")));
    }

    #[test]
    fn backup_dir_creation_fails_if_already_exists() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let output = tmp.path().join("home");
        std::fs::create_dir_all(&repo).unwrap();

        // Pre-create the directory that create() would generate
        let now = chrono::Local::now();
        let collision_path = repo.join(".backups").join(now.format("%Y-%m-%d_%H-%M-%S").to_string());
        std::fs::create_dir_all(&collision_path).unwrap();

        // Now create() must fail — the timestamped dir already exists
        assert!(BackupDir::create(&repo, &output).is_err());
    }

    #[test]
    fn backup_file_moves_to_mirrored_path() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let output = tmp.path().join("home");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(output.join(".config")).unwrap();
        let file = output.join(".config/gitconfig");
        std::fs::write(&file, "original content").unwrap();

        let backup = BackupDir::create(&repo, &output).unwrap();
        backup.backup_file(&file).unwrap();

        // Original file must be gone (it was moved)
        assert!(!file.exists());

        // File must exist in the backup directory at the mirrored path
        let backed_up = backup.path().join(".config/gitconfig");
        assert!(backed_up.exists());
        assert_eq!(std::fs::read_to_string(&backed_up).unwrap(), "original content");
    }

    #[test]
    #[cfg(unix)]
    fn backup_preserves_symlinks() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let output = tmp.path().join("home");
        let target = repo.join("nvim");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&output).unwrap();

        let link = output.join(".config/nvim");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(link.is_symlink());

        let backup = BackupDir::create(&repo, &output).unwrap();
        backup.backup_file(&link).unwrap();

        // The original symlink must be gone
        assert!(!link.exists());

        // The backup must be a symlink pointing to the same target
        let backed_up = backup.path().join(".config/nvim");
        assert!(backed_up.is_symlink());
        assert_eq!(std::fs::read_link(&backed_up).unwrap(), target);
    }

    #[test]
    fn backup_file_rejects_path_outside_output_dir() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        let output = tmp.path().join("home");
        std::fs::create_dir_all(&repo).unwrap();

        let outside = tmp.path().join("other/file.txt");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, "data").unwrap();

        let backup = BackupDir::create(&repo, &output).unwrap();
        assert!(backup.backup_file(&outside).is_err());
    }
}
