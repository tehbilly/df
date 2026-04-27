use std::fs;

use crossterm::style::{
    Stylize,
    style,
};
use similar::{
    ChangeTag,
    DiffOp,
    TextDiff,
};
use tracing::debug;

use crate::{
    cli::GlobalFlags,
    core::{
        graph::DependencyGraph,
        plan::WorkPlan,
        reconcile::{
            ReconcileItem,
            ReconcileStatus,
        },
        state::State,
        types::EntryType,
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

    // TODO: Replace all of this with a richer TUI
    for (module_name, ops) in plan.ops().iter() {
        println!("diff for module: {}", module_name);

        for op in ops {
            let item = ReconcileItem::for_op(op, &state)?;
            match item.status {
                ReconcileStatus::Deploy => {
                    // New item entirely
                    println!("+++ {}", op.dst.display());
                },
                ReconcileStatus::Clean => {
                    debug!(?item, "Clean (unmodified)");
                },
                ReconcileStatus::SourceChanged | ReconcileStatus::ExternallyModified => {
                    if op.entry_type == EntryType::Symlink {
                        let new_target =
                            fs::read_link(&op.dst).io_err(format!("reading link: {}", &op.src.display()))?;
                        println!("symlink target changed: {}", op.dst.display());
                        println!("--- {}", op.src.display());
                        println!("+++ {}", new_target.display());
                        continue;
                    }
                    let old = fs::read_to_string(&op.dst).io_err(format!("reading file: {}", &op.dst.display()))?;
                    let new = if op.entry_type == EntryType::Template {
                        let module = global_config.modules.get(module_name).expect("module should exist");
                        let vars = merge_vars([&module.vars, &local_config.vars_for(module_name)]);
                        let src = fs::read_to_string(&op.src).io_err(format!("reading file: {}", &op.src.display()))?;
                        render_template(&src, &vars)?
                    } else {
                        fs::read_to_string(&op.src).io_err(format!("reading file: {}", &op.src.display()))?
                    };
                    println!("--- {}", op.dst.display());
                    println!("+++ {}", op.src.display());
                    print_diff(old, new);
                },
                ReconcileStatus::Unmanaged => {
                    println!("??? unmanaged: {:?}", item);
                },
            };
        }
    }

    Ok(())
}

fn print_diff<S: AsRef<str>>(old: S, new: S) {
    let old = old.as_ref();
    let new = new.as_ref();

    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = new.lines().collect::<Vec<_>>();

    let diff = TextDiff::from_lines(old, new);

    for op in diff.ops() {
        match op {
            DiffOp::Equal { len, old_index, .. } => {
                for i in 0..*len {
                    let line_num = old_index + i + 1;
                    let line = old_lines[old_index + i].trim_end_matches(['\r', '\n']);
                    println!("{}", format!("{:>4} | {}", line_num, line).dim());
                }
            },
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..*old_len {
                    let line_num = old_index + i + 1;
                    let line = old_lines[old_index + i].trim_end_matches(['\r', '\n']);
                    println!("{:>4} | {}", style(line_num).dim(), line.red().crossed_out());
                }
            },
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..*new_len {
                    let line_num = new_index + i + 1;
                    let line = new_lines[new_index + i].trim_end_matches(['\r', '\n']);
                    println!("{:>4} | {}", style(line_num).dim(), line.green().bold());
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

                        print!("{}", format!("{:>4} | ", line_num).dim());
                        for change in TextDiff::from_words(old_line, new_line).iter_all_changes() {
                            let change_str = format!("{}", change);
                            let change_str = change_str.trim_end_matches(['\r', '\n']);
                            match change.tag() {
                                ChangeTag::Equal => print!("{}", change_str),
                                ChangeTag::Delete => print!("{}", change_str.red().crossed_out()),
                                ChangeTag::Insert => print!("{}", change_str.green().bold()),
                            }
                        }
                        println!();
                    }
                } else {
                    for i in 0..*old_len {
                        let line_num = old_index + i + 1;
                        let line = old_lines[old_index + i].trim_end_matches(['\r', '\n']);
                        println!("{:>4} | {}", style(line_num).dim(), line.red().crossed_out());
                    }
                    for i in 0..*new_len {
                        let line_num = new_index + i + 1;
                        let line = new_lines[new_index + i].trim_end_matches(['\r', '\n']);
                        println!("{:>4} | {}", style(line_num).dim(), line.green().bold());
                    }
                }
            },
        }
    }
}
