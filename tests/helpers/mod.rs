use std::path::{
    Path,
    PathBuf,
};

use tempfile::TempDir;

pub struct TempDotfiles {
    root_dir: TempDir,
}

#[allow(unused)]
impl TempDotfiles {
    pub fn new() -> Self {
        let mut root_dir = tempfile::tempdir().expect("unable to create temp dir");

        if let Ok(ev) = std::env::var("KEEP_TEMP_DIRS")
            && let Ok(keep) = ev.parse::<bool>()
            && keep
        {
            println!("keeping test dir: {}", root_dir.path().display());
            root_dir.disable_cleanup(true);
        }

        let root_path = root_dir.path();
        std::fs::create_dir_all(root_path.join("repo")).expect("unable to create temp repo dir");
        std::fs::create_dir_all(root_path.join("state")).expect("unable to create temp state dir");
        std::fs::create_dir_all(root_path.join("output")).expect("unable to create temp output dir");

        Self { root_dir }
    }

    pub fn repo_dir(&self) -> PathBuf {
        self.root_dir.path().join("repo")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root_dir.path().join("state")
    }

    pub fn output_dir(&self) -> PathBuf {
        self.root_dir.path().join("output")
    }

    pub fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("df").expect("Can not get cargo binary");
        cmd.args([
            "--source-dir",
            self.repo_dir().as_os_str().to_str().unwrap(),
            "--output-dir",
            self.output_dir().as_os_str().to_str().unwrap(),
            "--state-dir",
            self.state_dir().as_os_str().to_str().unwrap(),
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

        let path = self.repo_dir().join(path);

        std::fs::write(path, contents).expect("Can not write to file");
    }

    #[allow(unused)]
    pub fn output_path_exists<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();
        self.output_dir().join(path).exists()
    }

    #[allow(unused)]
    pub fn output_file_contents<P: AsRef<Path>>(&self, path: P) -> String {
        let path = path.as_ref();
        let path = self.output_dir().join(path);
        std::fs::read_to_string(path).expect("Can not read file")
    }
}
