use tracing::{
    debug,
    error,
    info,
    warn,
};

use crate::{
    cli::GlobalFlags,
    core::{
        backup::BackupDir,
        graph::DependencyGraph,
        ops::perform_op,
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
    template::merge_vars,
};

pub(crate) fn run(flags: &GlobalFlags, module: Option<String>, dry_run: bool, force: bool) -> crate::core::Result<()> {
    let lua = create_vm()?;
    let global_config = load_global_config(&lua, flags.source_dir.join("config.lua"))?;
    let local_config = load_local_config(&lua, &global_config, flags.source_dir.join("local.lua"))?;

    let dependency_graph = DependencyGraph::new(&global_config)?;
    let active_modules = if let Some(ref name) = module {
        dependency_graph
            .resolve(std::slice::from_ref(name))?
            .iter()
            .filter_map(|name| global_config.modules.get(name))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        dependency_graph
            .resolve(&local_config.module_names())?
            .iter()
            .filter_map(|name| global_config.modules.get(name))
            .cloned()
            .collect::<Vec<_>>()
    };

    let plan = WorkPlan::build(&active_modules, &flags.source_dir, &flags.output_dir)?;
    let mut state = State::load(flags.state_dir.join("state.json"))?;

    // TODO: Apply validation before performing actions?
    let mut backup_dir: Option<BackupDir> = None;
    let mut failed_modules: Vec<String> = Vec::new();

    'module_loop: for (module_name, ops) in plan.ops().iter() {
        debug!(?module_name, num_ops = ops.len(), "Applying plan for module");

        // pre_apply hooks
        let module = global_config
            .modules
            .get(module_name)
            .ok_or_else(|| crate::error::Error::UnknownModuleName(module_name.to_string()))?;
        if module.deps.iter().any(|dep| failed_modules.contains(dep)) {
            warn!(module_name, "Skipping module as dependency failed");
            failed_modules.push(module_name.clone());
            continue;
        }

        if let Some(hook) = &module.hooks.pre_apply {
            info!("Running pre_apply hooks for: {:?}", module);
            if let Err(err) = hook.run() {
                let module_name = module.name.clone();
                error!(?err, module_name, "Failed to run pre_apply hook");
                failed_modules.push(module_name.clone());
                continue;
            }
        }

        for op in ops {
            let item = ReconcileItem::for_op(op, &state)?;
            match item.status {
                ReconcileStatus::Deploy | ReconcileStatus::SourceChanged => {
                    let vars = merge_vars([&module.vars, &local_config.vars_for(module_name)]);
                    perform_op(&item.op, &vars, dry_run)?;
                    if !dry_run {
                        state.record_op(op)?;
                    }
                },
                ReconcileStatus::ExternallyModified | ReconcileStatus::Unmanaged => {
                    // TODO: Add confirmation if force is false
                    if force {
                        let dest = item.op.dst.clone();
                        warn!(?dest, "Called with --force, backing up target before deploying.");
                        if backup_dir.is_none() {
                            backup_dir = Some(BackupDir::create(&flags.source_dir, &flags.output_dir)?);
                        }
                        if let Err(err) = backup_dir.as_ref().unwrap().backup_file(&dest) {
                            error!(?err, ?dest, "Failed to backup file");
                            failed_modules.push(module_name.clone());
                            continue 'module_loop;
                        }
                        let vars = merge_vars([&module.vars, &local_config.vars_for(module_name)]);
                        if let Err(err) = perform_op(&item.op, &vars, dry_run) {
                            error!(?err, module_name, "Failed to perform op");
                            failed_modules.push(module_name.clone());
                            continue 'module_loop;
                        }
                        if !dry_run && let Err(err) = state.record_op(op) {
                            error!(?err, module_name, "Failed to record op, state will be incorrect");
                            failed_modules.push(module_name.clone());
                            continue 'module_loop;
                        }
                    } else {
                        warn!(?item.op, "Skipping operation as destination is [externally modified | unmanaged]. Call with --force to backup the destination and try again.");
                    }
                },
                ReconcileStatus::Clean => {
                    // Shouldn't really be anything to do here, _for now_
                    debug!(?item.op, "Someone asking for a clean");
                },
            }
        }

        if let Some(hook) = &module.hooks.post_apply {
            info!("Running post_apply hooks for: {:?}", module);
            if let Err(err) = hook.run() {
                error!(?err, module_name, "Failed to run post_apply hook");
                failed_modules.push(module_name.clone());
                continue;
            }
        }
    }

    if !dry_run {
        if module.is_none() {
            // Mark orphans,
            let active = active_modules.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
            state.mark_orphans(active);
        }
        state.save(flags.state_dir.join("state.json"))?;
    }

    if let Some(ref bd) = backup_dir {
        match std::fs::read_dir(bd.path()) {
            Ok(mut entries) => {
                if entries.next().is_none() {
                    info!("Backup directory was created but nothing was backed up, cleaning it up.");
                    if let Err(err) = std::fs::remove_dir_all(bd.path()) {
                        warn!(?err, "Failed to clean up empty backup directory");
                    }
                }
            },
            Err(err) => warn!(?err, "Failed to inspect backup directory"),
        }
    }

    if !failed_modules.is_empty() {
        let msg = format!("Failed deploying modules: {:?}", failed_modules);
        error!(?failed_modules, msg);
        return Err(crate::error::Error::ErrorMessage(msg));
    }

    Ok(())
}
