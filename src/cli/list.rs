use std::collections::HashSet;

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

// TODO: Make the output nicer, it's for human consumption
pub(crate) fn run(flags: &GlobalFlags) -> crate::core::Result<()> {
    let lua = create_vm()?;
    let global_config = load_global_config(&lua, flags.source_dir.join("config.lua"))?;
    let local_config = load_local_config(&lua, &global_config, flags.source_dir.join("local.lua"))?;

    println!("module : is_active");
    let local_module_names: HashSet<&str> = local_config.module_names().into_iter().collect();
    for module in global_config.modules.keys() {
        let active = if local_module_names.contains(&module.as_str()) {
            "true"
        } else {
            "false"
        };
        println!("{} : {}", module, active);
    }

    Ok(())
}
