use std::{
    fs,
    path::PathBuf,
};

use crossterm::{
    event,
    event::{
        Event,
        KeyCode,
        KeyEvent,
        KeyEventKind,
        KeyModifiers,
    },
};
use ratatui::{
    Frame,
    Terminal,
    backend::CrosstermBackend,
    layout::{
        Constraint,
        Layout,
        Rect,
    },
    prelude::Direction,
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
        Borders,
        List,
        ListItem,
        ListState,
        Paragraph,
    },
};
use similar::{
    ChangeTag,
    DiffOp,
    TextDiff,
};

use crate::{
    cli::{
        GlobalFlags,
        tui::{
            AltRawMode,
            path_rel_to,
        },
    },
    core::{
        graph::DependencyGraph,
        plan::{
            PlannedOp,
            WorkPlan,
        },
        reconcile::{
            ReconcileItem,
            ReconcileStatus,
        },
        state::State,
        types::{
            EntryType,
            GlobalConfig,
            LocalConfig,
        },
    },
    error::IoContext,
    lua::{
        loader::{
            load_global_config,
            load_local_config,
        },
        vm::create_vm,
    },
    template::{
        merge_vars,
        render_template,
    },
};

struct DiffState {
    // Overall state
    exit:          bool,
    global_config: GlobalConfig,
    local_config:  LocalConfig,
    output_path:   PathBuf,
    state:         State,
    focus:         Focus,

    // File list
    items:      Vec<FileListItem>,
    list_state: ListState,

    // Diff Content state
    diff_offset: u16,
    diff_lines:  Vec<Line<'static>>,
}

enum FileListItem {
    Header(String),
    File(PlannedOp),
}

#[derive(Clone, PartialEq)]
enum Focus {
    FileList,
    DiffArea,
}

impl Focus {
    fn next(&self) -> Self {
        match self {
            Focus::FileList => Focus::DiffArea,
            Focus::DiffArea => Focus::FileList,
        }
    }
}

impl DiffState {
    fn run(&mut self) -> crate::core::Result<()> {
        if self.items.is_empty() {
            return Err(crate::error::Error::ErrorMessage("diff with no items".to_string()));
        }

        let backend = CrosstermBackend::new(std::io::stdout());
        let mut terminal = Terminal::new(backend).io_err("could not create terminal")?;

        let _guard = AltRawMode::stdout().io_err("unable to enter raw mode")?;
        terminal.clear().io_err("unable to clear terminal")?;

        loop {
            if let Some(i) = self.list_state.selected() {
                if let FileListItem::File(ref op) = self.items[i] {
                    self.do_diff(op.clone())?;
                }
            }

            terminal.draw(|f| self.draw_ui(f)).io_err("unable to draw ui")?;
            self.handle_events()?;

            if self.exit {
                return Ok(());
            }
        }
    }

    fn handle_events(&mut self) -> crate::core::Result<()> {
        match event::read().io_err("unable to read event")? {
            Event::Key(event) if event.kind == KeyEventKind::Press => {
                self.handle_key(event)?;
            },
            _ => {},
        };

        Ok(())
    }

    fn handle_key(&mut self, event: KeyEvent) -> crate::core::Result<()> {
        match event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
            KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.exit = true;
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                Focus::FileList => self.nav_up(),
                Focus::DiffArea => self.scroll_up(1),
            },
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                Focus::FileList => self.nav_down(),
                Focus::DiffArea => self.scroll_down(1),
            },
            // TODO: See if we can easily do these to the height of the scrollable area?
            KeyCode::PageUp if let Focus::DiffArea = self.focus => self.scroll_up(10),
            KeyCode::PageDown if let Focus::DiffArea = self.focus => self.scroll_down(10),
            KeyCode::Tab => self.focus = self.focus.next(),
            _ => {},
        }
        Ok(())
    }

    fn draw_ui(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(f.area());

        self.render_file_list(f, chunks[0]);

        let diff = Paragraph::new(self.diff_lines.clone())
            .block(Block::default().borders(Borders::ALL))
            .scroll((self.diff_offset, 0));

        f.render_widget(diff, chunks[1]);
    }

    fn render_file_list(&mut self, f: &mut Frame, area: Rect) {
        let inner_width = area.width.saturating_sub(2) as usize;

        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|item| match item {
                FileListItem::Header(header) => {
                    let fill_len = inner_width.saturating_sub(header.len() + 3);
                    let suffix = "━".repeat(fill_len);
                    let content = Line::from(vec![
                        Span::styled("━╸", Style::default().dim()),
                        Span::styled(header, Style::default().bold()),
                        Span::styled(format!("╺{}", suffix), Style::default().dim()),
                    ]);
                    ListItem::new(content)
                },
                FileListItem::File(op) => {
                    let item = ReconcileItem::for_op(op, &self.state).expect("Could not reconcile file list item");
                    let style = match item.status {
                        ReconcileStatus::Deploy => Style::default().green(),
                        ReconcileStatus::Clean => Style::default().dim(),
                        ReconcileStatus::SourceChanged | ReconcileStatus::ExternallyModified => {
                            Style::default().yellow()
                        },
                        ReconcileStatus::Unmanaged => Style::default().red(),
                    };

                    let content = Line::from(vec![
                        Span::raw("  "),
                        Span::styled(path_rel_to(item.op.dst, self.output_path.clone()), style),
                    ]);
                    ListItem::new(content)
                },
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn nav_up(&mut self) {
        if self.items.is_empty() {
            return;
        }

        let current = self.list_state.selected().unwrap_or(0);
        let mut prev = if current == 0 {
            self.items.len() - 1
        } else {
            current - 1
        };

        while let FileListItem::Header(_) = self.items[prev] {
            prev = if prev == 0 { self.items.len() - 1 } else { prev - 1 };
        }

        self.diff_lines = Vec::with_capacity(0);
        self.list_state.select(Some(prev));
    }

    fn nav_down(&mut self) {
        if self.items.is_empty() {
            return;
        }

        let current = self.list_state.selected().unwrap_or(0);
        let mut next = (current + 1) % self.items.len();

        while let FileListItem::Header(_) = self.items[next] {
            next = (next + 1) % self.items.len();
        }

        self.diff_lines = Vec::with_capacity(0);
        self.list_state.select(Some(next));
    }

    fn scroll_up(&mut self, amount: u16) {
        self.diff_offset = self.diff_offset.saturating_sub(amount);
    }

    fn scroll_down(&mut self, amount: u16) {
        let max_scroll = (self.diff_lines.len() as u16).saturating_sub(2);
        if self.diff_offset < max_scroll {
            self.diff_offset += u16::min(amount, max_scroll);
        }
    }

    fn do_diff(&mut self, op: PlannedOp) -> crate::core::Result<()> {
        // TODO: When op.dst doesn't exist use old = String::new() && new = read src
        if !op.dst.exists() {
            return Ok(());
        }

        let old = fs::read_to_string(&op.dst).io_err(format!("reading file: {}", &op.dst.display()))?;
        let new = if op.entry_type == EntryType::Template {
            let module = self
                .global_config
                .modules
                .get(&op.module_name)
                .expect("module should exist");
            let vars = merge_vars([&module.vars, &self.local_config.vars_for(&op.module_name)]);
            let src = fs::read_to_string(&op.src).io_err(format!("reading file: {}", &op.src.display()))?;
            render_template(&src, &vars)?
        } else {
            fs::read_to_string(&op.src).io_err(format!("reading file: {}", &op.src.display()))?
        };

        let diff = diff_lines(&old, &new);
        self.diff_lines = diff;

        Ok(())
    }
}

pub(crate) fn run(flags: &GlobalFlags) -> crate::core::Result<()> {
    let lua = create_vm()?;
    let global_config = load_global_config(&lua, flags.source_dir.join("config.lua"))?;
    let local_config = load_local_config(&lua, &global_config, flags.source_dir.join("local.lua"))?;

    let dependency_graph = DependencyGraph::new(&global_config)?;

    let active_modules = dependency_graph
        .resolve(&local_config.module_names())?
        .iter()
        .filter_map(|name| global_config.modules.get(name))
        .cloned()
        .collect::<Vec<_>>();

    let plan = WorkPlan::build(&active_modules, &flags.source_dir, &flags.output_dir)?;
    let state = State::load(flags.state_dir.join("state.json"))?;

    let mut items = Vec::new();
    for (module, ops) in plan.ops().iter() {
        items.push(FileListItem::Header(module.clone()));

        for op in ops {
            items.push(FileListItem::File(op.clone()));
        }
    }

    let mut diff_app = DiffState {
        exit: false,
        global_config,
        local_config,
        output_path: flags.output_dir.clone(),
        focus: Focus::FileList,
        state,
        items,
        list_state: Default::default(),
        diff_offset: 0,
        diff_lines: Vec::new(),
    };

    diff_app.run()?;

    Ok(())
}

fn diff_lines<S: AsRef<str>>(old: S, new: S) -> Vec<Line<'static>> {
    let old = old.as_ref();
    let new = new.as_ref();

    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = new.lines().collect::<Vec<_>>();

    let diff = TextDiff::from_lines(old, new);

    let mut result = Vec::with_capacity(diff.new_len().max(diff.old_len()));

    for op in diff.ops() {
        match op {
            DiffOp::Equal { len, old_index, .. } => {
                for i in 0..*len {
                    let line_num = old_index + i + 1;
                    let line = old_lines[old_index + i].trim_end_matches(['\r', '\n']);
                    let line = format!("{:>4} | {}", line_num, line);
                    result.push(Line::styled(line, Style::default().dim()));
                }
            },
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..*old_len {
                    let line_num = old_index + i + 1;
                    let line = old_lines[old_index + i].trim_end_matches(['\r', '\n']);

                    let mut spans = Vec::new();
                    spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));
                    spans.push(Span::styled(line.to_string(), Style::default().red().dim()));
                    result.push(Line::from(spans));
                }
            },
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..*new_len {
                    let line_num = new_index + i + 1;
                    let line = new_lines[new_index + i].trim_end_matches(['\r', '\n']);

                    let mut spans = Vec::new();
                    spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));
                    spans.push(Span::styled(line.to_string(), Style::default().green()));
                    result.push(Line::from(spans));
                }
            },
            DiffOp::Replace {
                old_index,
                new_index,
                old_len,
                new_len,
            } => {
                if old_len == new_len {
                    for i in 0..*old_len {
                        let old_line = old_lines[old_index + i];
                        let new_line = new_lines[new_index + i];
                        let line_num = old_index + i + 1;

                        let mut spans = Vec::new();
                        spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));

                        for change in TextDiff::from_words(old_line, new_line).iter_all_changes() {
                            let change_str = format!("{}", change);
                            let change_str = change_str.trim_end_matches(['\r', '\n']);
                            let style = match change.tag() {
                                ChangeTag::Equal => Style::default(),
                                ChangeTag::Delete => Style::default().red().dim(),
                                ChangeTag::Insert => Style::default().green(),
                            };
                            spans.push(Span::styled(change_str.to_string(), style));
                        }
                        result.push(Line::from(spans));
                    }
                } else {
                    for i in 0..*old_len {
                        let line_num = old_index + i + 1;
                        let line = old_lines[old_index + i].trim_end_matches(['\r', '\n']);

                        let mut spans = Vec::new();
                        spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));
                        spans.push(Span::styled(line.to_string(), Style::default().red().dim()));
                        result.push(Line::from(spans));
                    }
                    for i in 0..*new_len {
                        let line_num = new_index + i + 1;
                        let line = new_lines[new_index + i].trim_end_matches(['\r', '\n']);

                        let mut spans = Vec::new();
                        spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));
                        spans.push(Span::styled(line.to_string(), Style::default().green()));
                        result.push(Line::from(spans));
                    }
                }
            },
        }
    }

    result
}
