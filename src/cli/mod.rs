use std::{
    ffi::OsString,
    path::PathBuf,
};

use clap::{
    Args,
    Parser,
    Subcommand,
};
use tracing::{
    info,
    level_filters::LevelFilter,
    warn,
};

use crate::error::{
    Error,
    IoContext,
};

pub mod apply;
pub mod clean;
pub mod diff;
pub mod init;
pub mod list;
pub mod status;
mod tui;

fn get_default_output_dir() -> PathBuf {
    dir_spec::home().unwrap_or_else(|| std::env::home_dir().expect("Home directory not found"))
}

fn get_default_state_dir() -> PathBuf {
    dir_spec::state_home().expect("State directory not found").join("df")
}

fn get_default_source_dir() -> PathBuf {
    std::env::current_dir().expect("Current directory could not be found")
}

fn get_default_local_config_path() -> PathBuf {
    std::env::current_dir()
        .expect("Current directory could not be found")
        .join("local.lua")
}

#[derive(Args)]
pub(crate) struct GlobalFlags {
    /// Optional override for local config path
    #[clap(long = "config-path", short = 'c', default_value_os_t = get_default_local_config_path())]
    local_config: PathBuf,

    /// The location of the dotfile repo
    #[clap(long = "source-dir", short = 's', default_value_os_t = get_default_source_dir())]
    source_dir: PathBuf,

    /// Optional override for output dir
    #[clap(long = "output-dir", short = 'o', default_value_os_t = get_default_output_dir())]
    output_dir: PathBuf,

    /// Optional override for state dir
    #[clap(long = "state-dir", default_value_os_t = get_default_state_dir())]
    state_dir: PathBuf,

    /// Increase output verbosity (use -vv for more)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Bootstrap a dotfile repository with starter config files
    Init {
        /// Directory to initialize repo in. Defaults to current working directory.
        target_dir: Option<PathBuf>,
    },
    /// Deploy active module(s) to the output directory
    Apply {
        /// Apply a single module manually, otherwise applies everything from local config.
        #[clap(long = "module", short = 'm')]
        module: Option<String>,

        /// Dry-run: do not perform actions, just print what would happen
        #[clap(long = "dry-run", conflicts_with = "force")]
        dry_run: bool,

        /// Overwrite externally-modified files and continue past hook errors
        #[clap(long = "force", short = 'f', conflicts_with = "dry_run")]
        force: bool,
    },
    /// List modules in global/local configs
    List,
    /// List status of each module with a list of files and directories it manages
    Status,
    /// Generate a real diff (like git diff) of pending changes
    Diff,
    /// Remove orphaned files. DEPRECATED: will be folded into `apply`
    Clean {
        /// Required for removal of files. This suppresses confirmation and just performs the actions.
        #[clap(long = "yes", short = 'y')]
        yes: bool,
    },
}

#[derive(Parser)]
#[command(name = "df")]
#[command(about = "A(nother) dotfile manager")]
pub(crate) struct Cli {
    #[clap(flatten)]
    pub global: GlobalFlags,

    #[clap(subcommand)]
    command: Commands,
}

pub(crate) fn run<I, T>(args: I) -> crate::core::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut app = Cli::parse_from(args);

    app.global.source_dir = app
        .global
        .source_dir
        .canonicalize()
        .io_err(format!("source dir not found: {}", app.global.source_dir.display()))?;

    app.global.output_dir = app
        .global
        .output_dir
        .canonicalize()
        .io_err(format!("output dir not found: {}", app.global.output_dir.display()))?;

    app.global.state_dir = app
        .global
        .state_dir
        .canonicalize()
        .io_err(format!("state dir not found: {}", app.global.output_dir.display()))?;

    // Set up tracing subscriber based on requested verbosity
    tracing_subscriber::fmt::fmt()
        .with_level(true)
        .with_max_level(match app.global.verbose {
            2 => LevelFilter::TRACE,
            1 => LevelFilter::DEBUG,
            _ => LevelFilter::INFO,
        })
        .init();

    match app.command {
        Commands::Init { target_dir } => {
            println!("init targeting: {:?}", target_dir);

            let path = match target_dir {
                Some(path) => path,
                None => std::env::current_dir().io_err("failed to get current directory")?,
            };

            if path.exists() && !path.is_dir() {
                return Err(Error::ErrorMessage(format!(
                    "path exists and is not a directory: {}",
                    path.display()
                )));
            }

            // Path does not exist or is empty: scaffold new dotfile repo
            if !path.exists()
                || (path.is_dir()
                    && path
                        .read_dir()
                        .io_err(format!("unable to read dir {}", path.display()))?
                        .next()
                        .is_none())
            {
                info!("Scaffolding new dotfile directory: {}", path.display());
                init::full_scaffold(&app.global, path)?;
                return Ok(());
            }

            // This looks to already be initialized
            if path.join("config.lua").exists() && path.join("local.lua").exists() {
                // warn!("Directory already contains config.lua and local.lua: nothing to do");
                return Err(Error::ErrorMessage(format!(
                    "path appears to already be initialized: {}",
                    path.display()
                )));
            }

            // Path exists but is empty? Also scaffold
            if path.join("config.lua").exists() {
                init::setup_local_config(&app.global, path)?;
                return Ok(());
            }

            info!("Nothing to initialize at {}", path.display());
        },
        Commands::Apply { module, dry_run, force } => {
            apply::run(&app.global, module, dry_run, force)?;
        },
        Commands::List => {
            list::run(&app.global)?;
        },
        Commands::Status => {
            status::run(&app.global)?;
        },
        Commands::Diff => {
            diff::run(&app.global)?;
        },
        Commands::Clean { yes } => {
            warn!("`clean` is deprecated and will be removed once it is folded into `apply`");
            clean::run(&app.global, yes)?;
        },
    }

    Ok(())
}
