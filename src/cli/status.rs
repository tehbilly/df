use std::collections::HashMap;

use comfy_table::Table;

use crate::{
    cli::{
        GlobalFlags,
        tui::path_rel_to_home,
    },
    core::{
        graph::DependencyGraph,
        plan::WorkPlan,
        reconcile::{
            ReconcileItem,
            ReconcileStatus,
        },
        state::State,
    },
    lua::{
        loader::{
            load_global_config,
            load_local_config,
        },
        vm::create_vm,
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

    let module_color = [
        comfy_table::Color::Cyan,
        comfy_table::Color::Green,
        comfy_table::Color::Blue,
        comfy_table::Color::Magenta,
        comfy_table::Color::DarkCyan,
        comfy_table::Color::DarkBlue,
        comfy_table::Color::DarkGreen,
        comfy_table::Color::DarkMagenta,
    ];

    // Lets us map module names to colors deterministically
    let color_map = plan
        .ops()
        .keys()
        .zip(module_color.iter().cycle())
        .map(|(item, &color)| (item, color))
        .collect::<HashMap<_, _>>();

    let mut table = Table::new();
    table.load_preset(comfy_table::presets::UTF8_BORDERS_ONLY);
    table.set_header(vec!["Module", "Status", "Source", "Target"]);
    for (module_name, ops) in plan.ops().iter().filter(|(_, ops)| !ops.is_empty()) {
        for op in ops {
            let item = ReconcileItem::for_op(op, &state)?;
            let status_cell = match item.status {
                ReconcileStatus::Deploy => comfy_table::Cell::new("new").fg(comfy_table::Color::Yellow),
                ReconcileStatus::Clean => comfy_table::Cell::new("up to date").fg(comfy_table::Color::Green),
                ReconcileStatus::SourceChanged => comfy_table::Cell::new("updated").fg(comfy_table::Color::Yellow),
                ReconcileStatus::ExternallyModified => {
                    comfy_table::Cell::new("externally modified").fg(comfy_table::Color::Red)
                },
                ReconcileStatus::Unmanaged => comfy_table::Cell::new("unmanaged").fg(comfy_table::Color::Red),
            };

            let source_cell = match item.status {
                ReconcileStatus::Deploy | ReconcileStatus::SourceChanged => {
                    comfy_table::Cell::new(path_rel_to_home(&item.op.src)).fg(comfy_table::Color::Green)
                },
                _ => comfy_table::Cell::new(path_rel_to_home(&item.op.src)),
            };

            let target_cell = match item.status {
                ReconcileStatus::ExternallyModified | ReconcileStatus::Unmanaged => {
                    comfy_table::Cell::new(path_rel_to_home(item.op.dst)).fg(comfy_table::Color::Red)
                },
                _ => comfy_table::Cell::new(path_rel_to_home(item.op.dst)),
            };
            table.add_row(vec![
                comfy_table::Cell::new(module_name).fg(color_map[&module_name]),
                status_cell,
                source_cell,
                target_cell,
            ]);
        }
        // println!("== Module: {}", module_name);
    }
    println!("{}", table);

    Ok(())
}
