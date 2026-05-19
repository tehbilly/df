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
        Alignment,
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
        BorderType,
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
    File(ReconcileItem),
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
            if let Some(i) = self.list_state.selected()
                && let FileListItem::File(ref item) = self.items[i]
            {
                self.do_diff(item.op.clone())?;
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
            KeyCode::PageUp if let Focus::DiffArea = self.focus => self.scroll_up(5),
            KeyCode::PageDown if let Focus::DiffArea = self.focus => self.scroll_down(5),
            KeyCode::Tab => {
                if self.focus == Focus::FileList && self.diff_lines.is_empty() {
                    // Only swap focus from file list if there's actually a diff
                    return Ok(());
                }
                self.focus = self.focus.next();
            },
            _ => {},
        }
        Ok(())
    }

    fn draw_ui(&mut self, f: &mut Frame) {
        let ui_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(f.area());

        let main_area = ui_chunks[0];
        let help_area = ui_chunks[1];

        self.render_help_area(f, help_area);

        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(main_area);

        let list_area = main_chunks[0];
        let info_area = main_chunks[1];

        self.render_file_list(f, list_area);
        self.render_info_area(f, info_area);
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
                FileListItem::File(item) => {
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
                        Span::styled(path_rel_to(item.op.dst.clone(), self.output_path.clone()), style),
                    ]);
                    ListItem::new(content)
                },
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title("Files")
                    .title_alignment(Alignment::Center)
                    .title_style(if self.focus == Focus::FileList {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    })
                    .borders(Borders::ALL)
                    .border_type(BorderType::Thick)
                    .border_style(if self.focus == Focus::FileList {
                        Style::default().cyan()
                    } else {
                        Style::default()
                    }),
            )
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_info_area(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(0)])
            .split(area);

        let summary_area = chunks[0];
        let diff_area = chunks[1];

        if let Some(item) = self.list_state.selected()
            && let Some(item) = self.items.get(item)
            && let FileListItem::File(item) = item
        {
            let label_style = Style::default().bold();

            let mut lines = Vec::new();

            lines.push(Line::from(vec![
                Span::styled("Status      ", label_style),
                match item.status {
                    ReconcileStatus::Deploy => Span::styled("needs deploy", Style::default().green()),
                    ReconcileStatus::Clean => Span::styled("up to date", Style::default()),
                    ReconcileStatus::SourceChanged => Span::styled("source changed", Style::default().yellow()),
                    ReconcileStatus::ExternallyModified => Span::styled("externally modified", Style::default().red()),
                    ReconcileStatus::Unmanaged => Span::styled("unmanaged", Style::default().red()),
                },
            ]));

            lines.push(Line::from(vec![
                Span::styled("Type        ", label_style),
                Span::raw(format!("{:?}", item.op.entry_type)),
            ]));

            lines.push(Line::from(vec![
                Span::styled("Source      ", label_style),
                Span::raw(format!("{}", item.op.src.display())),
            ]));

            lines.push(Line::from(vec![
                Span::styled("Destination ", label_style),
                Span::raw(format!("{}", item.op.dst.display())),
            ]));

            let paragraph = Paragraph::new(lines).block(
                Block::default()
                    .title("Summary")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Thick),
            );

            f.render_widget(paragraph, summary_area);
        }

        let diff_block = Block::default()
            .title("Diff")
            .title_alignment(Alignment::Center)
            .title_style(if self.focus == Focus::DiffArea {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(if self.focus == Focus::DiffArea {
                Style::default().cyan()
            } else {
                Style::default()
            });

        let diff = Paragraph::new(self.diff_lines.clone())
            .block(diff_block)
            .scroll((self.diff_offset, 0));

        f.render_widget(diff, diff_area);
    }

    fn render_help_area(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .title("Help")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().dim());

        let highlight = Style::default().bold().white();
        let desc_style = Style::default().dim();

        let mut spans = vec![
            // Quit
            Span::styled("Q", highlight),
            Span::styled("uit", desc_style),
        ];

        if self.focus == Focus::DiffArea || !self.diff_lines.is_empty() {
            spans.append(&mut vec![
                // Switch focus
                Span::styled(" | ", desc_style),
                Span::styled("⭾", highlight),
                Span::styled(" change focus", desc_style),
            ]);
        }

        if self.focus == Focus::FileList {
            spans.append(&mut vec![
                // nav
                Span::styled(" | [", desc_style),
                Span::styled("↓", highlight),
                Span::styled("|", desc_style),
                Span::styled("j", highlight),
                Span::styled("] nav down", desc_style),
                Span::styled(" | [", desc_style),
                Span::styled("↑", highlight),
                Span::styled("|", desc_style),
                Span::styled("k", highlight),
                Span::styled("] nav up", desc_style),
            ]);
        } else if self.focus == Focus::DiffArea {
            spans.append(&mut vec![
                // nav
                Span::styled(" | [", desc_style),
                Span::styled("↓", highlight),
                Span::styled("|", desc_style),
                Span::styled("j", highlight),
                Span::styled("|", desc_style),
                Span::styled("pgdn", highlight),
                Span::styled("] nav down", desc_style),
                Span::styled(" | [", desc_style),
                Span::styled("↑", highlight),
                Span::styled("|", desc_style),
                Span::styled("k", highlight),
                Span::styled("|", desc_style),
                Span::styled("pgup", highlight),
                Span::styled("] nav up", desc_style),
            ]);
        }

        let paragraph = Paragraph::new(Line::from(spans))
            .block(block)
            .alignment(Alignment::Center);

        f.render_widget(paragraph, area);
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
        self.diff_offset = 0;
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
        self.diff_offset = 0;
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
        if !op.dst.exists() || op.dst.is_dir() {
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
        let mut module_items = Vec::new();
        for op in ops {
            let item = ReconcileItem::for_op(op, &state).expect("Could not reconcile file list item");
            if op.entry_type == EntryType::Symlink && item.status == ReconcileStatus::Deploy {
                continue;
            }
            module_items.push(item);
        }

        if !module_items.is_empty() {
            items.push(FileListItem::Header(module.clone()));
            for module_item in module_items {
                items.push(FileListItem::File(module_item));
            }
        }
    }

    let mut diff_app = DiffState {
        exit: false,
        global_config,
        local_config,
        output_path: flags.output_dir.clone(),
        focus: Focus::FileList,
        items,
        list_state: Default::default(),
        diff_offset: 0,
        diff_lines: Vec::new(),
    };

    diff_app.run()?;

    Ok(())
}

fn expand_tabs(s: &str) -> String {
    if !s.contains('\t') {
        return s.to_owned();
    }
    let tab_width = 4usize;
    let mut out = String::with_capacity(s.len() + 8);
    let mut col = 0usize;
    for c in s.chars() {
        if c == '\t' {
            let pad = tab_width - (col % tab_width);
            for _ in 0..pad {
                out.push(' ');
            }
            col += pad;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
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
                    let line = format!("{:>4} | {}", line_num, expand_tabs(line));
                    result.push(Line::styled(line, Style::default().dim()));
                }
            },
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..*old_len {
                    let line_num = old_index + i + 1;
                    let line = old_lines[old_index + i].trim_end_matches(['\r', '\n']);

                    let mut spans = Vec::new();
                    spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));
                    spans.push(Span::styled(expand_tabs(line), Style::default().red().dim()));
                    result.push(Line::from(spans));
                }
            },
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..*new_len {
                    let line_num = new_index + i + 1;
                    let line = new_lines[new_index + i].trim_end_matches(['\r', '\n']);

                    let mut spans = Vec::new();
                    spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));
                    spans.push(Span::styled(expand_tabs(line), Style::default().green()));
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
                        let old_line = expand_tabs(old_lines[old_index + i]);
                        let new_line = expand_tabs(new_lines[new_index + i]);
                        let line_num = old_index + i + 1;

                        let mut spans = Vec::new();
                        spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));

                        for change in TextDiff::from_words(&old_line, &new_line).iter_all_changes() {
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
                        spans.push(Span::styled(expand_tabs(line), Style::default().red().dim()));
                        result.push(Line::from(spans));
                    }
                    for i in 0..*new_len {
                        let line_num = new_index + i + 1;
                        let line = new_lines[new_index + i].trim_end_matches(['\r', '\n']);

                        let mut spans = Vec::new();
                        spans.push(Span::styled(format!("{:>4} | ", line_num), Style::default().dim()));
                        spans.push(Span::styled(expand_tabs(line), Style::default().green()));
                        result.push(Line::from(spans));
                    }
                }
            },
        }
    }

    result
}
