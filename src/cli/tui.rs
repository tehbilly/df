use std::{
    io,
    io::{
        IsTerminal,
        Write,
    },
    path::Path,
};

use crossterm::{
    ExecutableCommand,
    event,
    event::{
        Event,
        KeyCode,
        KeyEvent,
        KeyEventKind,
        KeyModifiers,
        read,
    },
    terminal::{
        EnterAlternateScreen,
        LeaveAlternateScreen,
        disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{
    Terminal,
    TerminalOptions,
    Viewport,
    backend::CrosstermBackend,
    style::{
        Color,
        Modifier,
        Style,
    },
    text::{
        Line,
        Span,
    },
    widgets::{
        Block,
        List,
        ListItem,
        ListState,
    },
};
use tracing::{
    debug,
    warn,
};

use crate::error::IoContext;

/// Guard so that raw mode is disabled if something panics
pub(crate) struct RawMode;

impl RawMode {
    pub(crate) fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

#[allow(unused)]
enum AltRawModeTarget {
    Stdout,
    Stderr,
}

/// RawMode guard for alt screen
pub(crate) struct AltRawMode {
    target: AltRawModeTarget,
}

impl AltRawMode {
    pub(crate) fn stdout() -> std::io::Result<Self> {
        enable_raw_mode()?;
        std::io::stdout().execute(EnterAlternateScreen)?;
        Ok(Self {
            target: AltRawModeTarget::Stdout,
        })
    }
}

impl Drop for AltRawMode {
    fn drop(&mut self) {
        match self.target {
            AltRawModeTarget::Stdout => {
                let _ = std::io::stdout().execute(LeaveAlternateScreen);
            },
            AltRawModeTarget::Stderr => {
                let _ = std::io::stderr().execute(LeaveAlternateScreen);
            },
        }
        let _ = disable_raw_mode();
    }
}

pub(crate) struct MultiSelect {
    title: String,
    // (label, is_selected)
    items: Vec<(String, bool)>,
    state: ListState,
}

impl MultiSelect {
    pub(crate) fn new<S: AsRef<str>>(title: S, items: &[(String, bool)]) -> Self {
        let title = title.as_ref().to_string();
        let items = items.to_vec();
        let mut state = ListState::default();
        state.select(Some(0));
        Self { title, items, state }
    }

    pub(crate) fn toggle_selection(&mut self) {
        if let Some(i) = self.state.selected() {
            self.items[i].1 = !self.items[i].1;
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            None => 0,
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            },
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            None => 0,
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            },
        };
        self.state.select(Some(i));
    }

    pub(crate) fn run(&mut self) -> crate::core::Result<Vec<String>> {
        if self.items.is_empty() {
            warn!("no items to select");
            return Ok(Vec::new());
        }

        let num_lines = self.items.len() + 1;
        let backend = CrosstermBackend::new(std::io::stdout());
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Inline(num_lines as u16),
        })
        .io_err("unable to create terminal")?;

        // Raw mode, but don't enter alternate screen
        let _guard = RawMode::enter().io_err("unable to enter raw mode")?;

        // Run the app
        self.run_app(&mut terminal)?;

        // Insert a newline so we don't write over the list
        println!();

        let selected = self
            .items
            .iter()
            .filter(|(_, selected)| *selected)
            .map(|(label, _)| label.clone())
            .collect::<Vec<_>>();

        Ok(selected)
    }

    fn run_app(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> crate::core::Result<()> {
        loop {
            // Draw the UI
            terminal
                .draw(|f| {
                    let items: Vec<ListItem> = self
                        .items
                        .iter()
                        .map(|(label, is_selected)| {
                            let prefix = if *is_selected { "[x] " } else { "[ ] " };
                            let prefix_style = if *is_selected {
                                Style::default().fg(Color::Green)
                            } else {
                                Style::default()
                            };
                            let content = Line::from(vec![Span::styled(prefix, prefix_style), Span::raw(label)]);
                            ListItem::new(content)
                        })
                        .collect();

                    let list = List::new(items)
                        .block(Block::default().title(self.title.clone()))
                        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
                        .highlight_symbol(">> ");

                    f.render_stateful_widget(list, f.area(), &mut self.state);
                })
                .io_err("unable to render terminal")?;

            // Handle input
            if let Event::Key(key) = event::read().io_err("unable to read event")?
                && key.is_press()
            {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        return Err(crate::error::Error::ErrorMessage("user canceled selection".into()));
                    },
                    KeyCode::Enter => {
                        // Done selecting
                        return Ok(());
                    },
                    KeyCode::Char('k') | KeyCode::Up => self.previous(),
                    KeyCode::Char('j') | KeyCode::Down => self.next(),
                    KeyCode::Char(' ') => self.toggle_selection(),
                    _ => {},
                };
            }
        }
    }
}

/// Ask a yes/no question
pub(crate) fn confirm<S: AsRef<str>>(prompt: S, default: bool) -> crate::core::Result<bool> {
    let prompt = prompt.as_ref();

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        debug!(prompt, "Not a TTY, skipping confirmation");
        return Ok(false);
    }

    if default {
        print!("{prompt} [Y/n] ");
    } else {
        print!("{prompt} [y/N] ");
    }
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

            if code == KeyCode::Enter {
                debug!("Enter was pressed, returning default");
                return Ok(default);
            }

            return Ok(matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')));
        }
    }
}

pub(crate) fn prompt<S: AsRef<str>>(prompt: S) -> crate::core::Result<String> {
    let prompt = prompt.as_ref();
    print!("{}", prompt);
    io::stdout().flush().io_err("unable to flush stdout")?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).io_err("unable to read input")?;
    Ok(input.trim().to_string())
}

pub(crate) fn path_rel_to<P: AsRef<Path>>(path: P, rel_to: P) -> String {
    let path = path.as_ref();
    let rel_to = rel_to.as_ref();
    if let Ok(rel_path) = path.strip_prefix(rel_to) {
        rel_path.display().to_string()
    } else {
        path.display().to_string()
    }
}

pub(crate) fn path_rel_to_home<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();
    if let Some(home_dir) = dir_spec::home()
        && let Ok(rel_path) = path.strip_prefix(home_dir)
    {
        format!("~/{}", rel_path.display())
    } else {
        path.to_string_lossy().to_string()
    }
}
