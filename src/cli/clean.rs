use std::path::PathBuf;

use crossterm::style::{
    Stylize,
    style,
};
use tracing::{
    debug,
    info,
};

use crate::{
    cli::{
        GlobalFlags,
        tui::confirm,
    },
    core::state::{
        ManagedEntry,
        State,
    },
    error::IoContext,
};

pub(crate) fn run(flags: &GlobalFlags, yes: bool) -> crate::core::Result<()> {
    let save_path = flags.state_dir.join("state.json");
    let mut state = State::load(&save_path)?;

    let mut removed: Vec<PathBuf> = Vec::new();

    // Short entries sorted by path
    for (path, entry) in &state.managed {
        match entry {
            ManagedEntry::Orphaned { module, .. } => {
                let mut answer = yes;
                if !answer {
                    answer = confirm(format!("remove: {}", path.display()), true)?;
                    println!("{}", if answer { "y".green() } else { "n".red() });
                }
                let action = if answer {
                    style("Removing").green().bold()
                } else {
                    style("Skipping").yellow().bold()
                };
                println!("{} orphan from {}: {}", action, module, path.display());

                if answer {
                    std::fs::remove_file(path).io_err(format!("failed to remove {}", path.display()))?;
                    info!("Removed: {}", path.display());
                    removed.push(path.to_path_buf());
                }
            },
            _ => debug!(?entry, "Skipping non-orphaned entry"),
        }
    }

    for path in removed.iter() {
        debug!("Removing orphan entry from state file: {}", path.display());
        state.remove_orphan(path)?;
    }

    if !removed.is_empty() {
        debug!("Saving state file: {}", save_path.display());
        state.save(&save_path)?;
    }

    Ok(())
}
