use std::path::Path;

use tempfile::TempDir;

pub struct TempDotfiles {
    pub repo_dir:   TempDir,
    pub state_dir:  TempDir,
    pub output_dir: TempDir,
}

impl TempDotfiles {
    pub fn new() -> Self {
        let mut repo_dir = tempfile::tempdir().expect("Can not create temp dir");
        let mut state_dir = tempfile::tempdir().expect("Can not create state dir");
        let mut output_dir = tempfile::tempdir().expect("Can not create output dir");
        if let Ok(ev) = std::env::var("KEEP_TEMP_DIRS")
            && let Ok(keep) = ev.parse::<bool>()
            && keep
        {
            repo_dir.disable_cleanup(true);
            state_dir.disable_cleanup(true);
            output_dir.disable_cleanup(true);
        }

        Self {
            repo_dir,
            state_dir,
            output_dir,
        }
    }

    pub fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("df").expect("Can not get cargo binary");
        cmd.args([
            "--source-dir",
            self.repo_dir.path().as_os_str().to_str().unwrap(),
            "--output-dir",
            self.output_dir.path().as_os_str().to_str().unwrap(),
            "--state-dir",
            self.state_dir.path().as_os_str().to_str().unwrap(),
        ]);

        cmd
    }

    /// Writes a file to the repo dir as path (relative)
    #[allow(unused)]
    pub fn write_file<P: AsRef<Path>>(&self, path: P, contents: &str) {
        let path = path.as_ref();
        if !path.is_relative() {
            panic!("Path must be relative to data directory");
        }

        let path = self.repo_dir.path().join(path);

        std::fs::write(path, contents).expect("Can not write to file");
    }

    #[allow(unused)]
    pub fn output_path_exists<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();
        self.output_dir.path().join(path).exists()
    }

    #[allow(unused)]
    pub fn output_file_contents<P: AsRef<Path>>(&self, path: P) -> String {
        let path = path.as_ref();
        let path = self.output_dir.path().join(path);
        std::fs::read_to_string(path).expect("Can not read file")
    }
}
