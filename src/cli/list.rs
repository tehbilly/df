use std::collections::HashSet;

use comfy_table::Table;

use crate::{
    cli::GlobalFlags,
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

    let mut table = Table::new();
    table.load_preset("     ═             ");
    table.set_header(vec!["Module", "Status"]);

    let local_module_names: HashSet<&str> = local_config.module_names().into_iter().collect();
    for module in global_config.modules.keys() {
        let active = if local_module_names.contains(&module.as_str()) {
            comfy_table::Cell::new("true").fg(comfy_table::Color::Green)
        } else {
            comfy_table::Cell::new("false").fg(comfy_table::Color::Red)
        };
        table.add_row([comfy_table::Cell::new(module), active]);
    }

    println!("{}", table);

    Ok(())
}
