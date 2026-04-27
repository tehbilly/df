use std::collections::{
    HashMap,
    HashSet,
    VecDeque,
};

use crate::{
    core::{
        Result,
        types::GlobalConfig,
    },
    error::Error,
};

pub(crate) struct DependencyGraph {
    edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub(crate) fn new(global_config: &GlobalConfig) -> Result<Self> {
        let mut edges = HashMap::new();

        for (name, module) in global_config.modules.iter() {
            edges.insert(name.clone(), module.deps.clone());
        }

        Ok(Self { edges })
    }

    pub(crate) fn resolve<M: AsRef<str>>(&self, modules: &[M]) -> Result<Vec<String>> {
        let mut reachable: HashSet<&str> = HashSet::new();
        let mut to_visit: Vec<&str> = Vec::new();

        for module in modules {
            let m = module.as_ref();
            if !self.edges.contains_key(m) {
                return Err(Error::UnknownModuleName(m.to_string()));
            }

            to_visit.push(m)
        }

        while let Some(node) = to_visit.pop() {
            if reachable.insert(node) {
                let deps = self.edges.get(node).ok_or(Error::UnknownModuleName(node.to_string()))?;
                for dep in deps {
                    to_visit.push(dep);
                }
            }
        }

        // Maps a node to its current count of unresolved dependencies
        let mut in_degrees: HashMap<&str, usize> = HashMap::with_capacity(reachable.len());
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::with_capacity(reachable.len());

        for &node in &reachable {
            in_degrees.insert(node, 0);
            dependents.insert(node, Vec::new());
        }

        for &node in &reachable {
            // Safe to unwrap: already validated all reachable items exist
            let deps = self.edges.get(node).unwrap();
            for dep in deps {
                let dep_str = dep.as_str();
                if reachable.contains(dep_str) {
                    *in_degrees.get_mut(node).unwrap() += 1;
                    dependents.get_mut(dep_str).unwrap().push(node);
                }
            }
        }

        // Find initial nodes with 0 in-degree (no deps)
        let mut queue: VecDeque<&str> = VecDeque::new();
        for (&node, &degree) in &in_degrees {
            if degree == 0 {
                queue.push_back(node);
            }
        }

        let mut sorted: Vec<&str> = Vec::with_capacity(in_degrees.len());

        while let Some(node) = queue.pop_front() {
            sorted.push(node);

            if let Some(node_dependents) = dependents.get(node) {
                for &dependent in node_dependents {
                    if let Some(degree) = in_degrees.get_mut(&dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent);
                        }
                    }
                }
            }
        }

        if sorted.len() != in_degrees.len() {
            let mut cycle_members: Vec<String> = in_degrees
                .iter()
                .filter(|(_, deg)| **deg > 0)
                .map(|(n, _)| n.to_string())
                .collect();
            cycle_members.sort();

            return Err(Error::DependencyCycleDetected(cycle_members));
        }

        Ok(sorted.iter().map(|i| i.to_string()).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::core::types::{
        EntryType,
        FileEntry,
        GlobalConfig,
        Hooks,
        Module,
    };

    fn make_module(name: &str, deps: &[&str]) -> Module {
        Module {
            name:  name.into(),
            files: vec![FileEntry {
                src:        name.into(),
                dst:        name.into(),
                entry_type: EntryType::Symlink,
            }],
            deps:  deps.iter().map(|s| s.to_string()).collect(),
            vars:  HashMap::new(),
            hooks: Hooks::default(),
        }
    }

    fn make_config(modules: &[Module]) -> GlobalConfig {
        GlobalConfig {
            modules: modules.iter().cloned().map(|m| (m.name.clone(), m)).collect(),
        }
    }

    #[test]
    fn single_module_no_deps() {
        let config = make_config(&[make_module("base", &[])]);
        let graph = DependencyGraph::new(&config).unwrap();
        let sorted = graph.resolve(&["base"]).unwrap();
        assert_eq!(sorted, vec!["base"]);
    }

    #[test]
    fn dep_comes_before_dependent() {
        let config = make_config(&[make_module("base", &[]), make_module("neovim", &["base"])]);
        let graph = DependencyGraph::new(&config).unwrap();
        let sorted = graph.resolve(&["base", "neovim"]).unwrap();
        let base_pos = sorted.iter().position(|s| s == "base").unwrap();
        let neovim_pos = sorted.iter().position(|s| s == "neovim").unwrap();
        assert!(base_pos < neovim_pos);
    }

    #[test]
    fn transitive_deps_are_ordered() {
        let config = make_config(&[
            make_module("base", &[]),
            make_module("mid", &["base"]),
            make_module("top", &["mid"]),
        ]);
        let graph = DependencyGraph::new(&config).unwrap();
        let sorted = graph.resolve(&["top", "mid", "base"]).unwrap();
        let base_pos = sorted.iter().position(|s| s == "base").unwrap();
        let mid_pos = sorted.iter().position(|s| s == "mid").unwrap();
        let top_pos = sorted.iter().position(|s| s == "top").unwrap();
        assert!(base_pos < mid_pos);
        assert!(mid_pos < top_pos);
    }

    #[test]
    fn cycle_returns_error() {
        let config = make_config(&[make_module("a", &["b"]), make_module("b", &["a"])]);
        let graph = DependencyGraph::new(&config).unwrap();
        let sorted = graph.resolve(&["a", "b"]);
        assert!(matches!(sorted, Err(Error::DependencyCycleDetected(_))));
    }

    #[test]
    fn unknown_dep_returns_error() {
        let config = make_config(&[make_module("a", &["nonexistent"])]);
        let graph = DependencyGraph::new(&config).unwrap();
        let sorted = graph.resolve(&["a"]);
        assert!(matches!(sorted, Err(Error::UnknownModuleName(_))));
    }
}
