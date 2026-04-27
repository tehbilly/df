use std::{
    io,
    io::{
        IsTerminal,
        Write,
    },
    path::PathBuf,
};

use crossterm::{
    event::{
        Event,
        KeyCode,
        KeyEvent,
        KeyEventKind,
        KeyModifiers,
        read,
    },
    style::{
        Stylize,
        style,
    },
    terminal::{
        disable_raw_mode,
        enable_raw_mode,
    },
};
use tracing::{
    debug,
    info,
};

use crate::{
    cli::GlobalFlags,
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
                    answer = confirm(format!("remove: {}", path.display()))?;
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

fn confirm<S: AsRef<str>>(prompt: S) -> crate::core::Result<bool> {
    let prompt = prompt.as_ref();

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        debug!(prompt, "Not a TTY, skipping confirmation");
        return Ok(false);
    }

    print!("{prompt} [y/N] ");
    io::stdout().flush().io_err("unable to flush stdout")?;

    let _guard = RawMode::enter().io_err("unable to enter raw mode")?;
    loop {
        if let Event::Key(KeyEvent {
            code, modifiers, kind, ..
        }) = read().io_err("reading character")?
        {
            if kind != KeyEventKind::Press {
                continue;
            }

            // Let ctrl+c bail even in raw mode
            if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                return Err(crate::error::Error::ErrorMessage(String::from("interrupted: ctrl+c")));
            }

            return Ok(matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')));
        }
    }
}

// Guard so that raw mode is disabled if something panics
struct RawMode;

impl RawMode {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}
