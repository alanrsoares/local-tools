//! Topological task dependency graph and scheduling (inspired by Turborepo Engine).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::workspace::{TaskSpec, WorkspacePkg};

#[derive(Debug, Clone, Default)]
pub struct TurboPipeline {
    pub task_rules: HashMap<String, PipelineRule>,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineRule {
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TaskNode {
    pub id: usize,
    pub spec: TaskSpec,
    pub depends_on: HashSet<usize>,
    pub dependents: HashSet<usize>,
}

#[derive(Debug, Clone)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,
}

impl TaskGraph {
    /// Construct a DAG from a flat task list, package manifests, and optional turbo pipeline rules.
    pub fn build(
        tasks: Vec<TaskSpec>,
        pkgs: &[WorkspacePkg],
        pipeline: Option<&TurboPipeline>,
    ) -> Result<Self, String> {
        let mut pkg_map: HashMap<&str, &WorkspacePkg> = HashMap::new();
        for pkg in pkgs {
            pkg_map.insert(&pkg.name, pkg);
        }

        let mut nodes: Vec<TaskNode> = tasks
            .into_iter()
            .enumerate()
            .map(|(id, spec)| TaskNode {
                id,
                spec,
                depends_on: HashSet::new(),
                dependents: HashSet::new(),
            })
            .collect();

        // Map "pkg_name:task_name" or root task name -> task index
        let mut task_lookup: HashMap<String, usize> = HashMap::new();
        for (i, node) in nodes.iter().enumerate() {
            task_lookup.insert(node.spec.name.clone(), i);
        }

        // Resolve dependencies
        for i in 0..nodes.len() {
            let task_name = nodes[i].spec.name.clone();

            // Check if this task is a package task (e.g. "@scope/pkg:build")
            let (pkg_name, script_name) = if let Some((pkg, script)) = task_name.split_once(':') {
                (Some(pkg), script)
            } else {
                (None, task_name.as_str())
            };

            let depends_on_rules =
                if let Some(rules) = pipeline.and_then(|p| p.task_rules.get(script_name)) {
                    rules.depends_on.clone()
                } else if script_name == "build" {
                    // Default heuristic: build tasks depend on upstream package builds
                    vec!["^build".to_string()]
                } else {
                    Vec::new()
                };

            for rule in depends_on_rules {
                if let Some(dep_script) = rule.strip_prefix('^') {
                    // Package dependencies' task (e.g. ^build)
                    if let Some(pkg) = pkg_name.and_then(|p| pkg_map.get(p)) {
                        for internal_dep in &pkg.internal_deps {
                            let dep_task_name = format!("{internal_dep}:{dep_script}");
                            if let Some(&dep_idx) = task_lookup.get(&dep_task_name) {
                                nodes[i].depends_on.insert(dep_idx);
                                nodes[dep_idx].dependents.insert(i);
                            }
                        }
                    }
                } else if let Some(pkg) = pkg_name {
                    // Same-package task dependency (e.g. "build" before "test")
                    let dep_task_name = format!("{pkg}:{rule}");
                    if let Some(&dep_idx) = task_lookup.get(&dep_task_name) {
                        nodes[i].depends_on.insert(dep_idx);
                        nodes[dep_idx].dependents.insert(i);
                    }
                } else if let Some(&dep_idx) = task_lookup.get(&rule) {
                    // Root task dependency
                    nodes[i].depends_on.insert(dep_idx);
                    nodes[dep_idx].dependents.insert(i);
                }
            }
        }

        let graph = Self { nodes };
        graph.detect_cycles()?;
        Ok(graph)
    }

    /// Kahn's algorithm cycle detection and topological sorting.
    pub fn detect_cycles(&self) -> Result<(), String> {
        let mut in_degrees: Vec<usize> = self.nodes.iter().map(|n| n.depends_on.len()).collect();
        let mut queue: VecDeque<usize> = VecDeque::new();

        for (i, &deg) in in_degrees.iter().enumerate() {
            if deg == 0 {
                queue.push_back(i);
            }
        }

        let mut visited_count = 0;
        while let Some(node_idx) = queue.pop_front() {
            visited_count += 1;
            for &dep_idx in &self.nodes[node_idx].dependents {
                in_degrees[dep_idx] -= 1;
                if in_degrees[dep_idx] == 0 {
                    queue.push_back(dep_idx);
                }
            }
        }

        if visited_count != self.nodes.len() {
            return Err("cyclical task dependency detected in workspace".to_string());
        }

        Ok(())
    }

    /// Return tasks with in-degree 0 that can start immediately.
    pub fn ready_tasks(&self) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.depends_on.is_empty())
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn topological_dependency_resolution() {
        let pkgs = vec![
            WorkspacePkg {
                name: "@app/core".into(),
                dir: PathBuf::from("/root/packages/core"),
                scripts: vec!["build".into()],
                internal_deps: vec![],
            },
            WorkspacePkg {
                name: "@app/ui".into(),
                dir: PathBuf::from("/root/packages/ui"),
                scripts: vec!["build".into()],
                internal_deps: vec!["@app/core".into()],
            },
        ];

        let tasks = vec![
            TaskSpec {
                name: "@app/ui:build".into(),
                runner_bin: "bun".into(),
                args: vec!["run".into(), "build".into()],
                cwd: PathBuf::from("/root/packages/ui"),
                color_idx: 0,
                estimated_cost: 1,
            },
            TaskSpec {
                name: "@app/core:build".into(),
                runner_bin: "bun".into(),
                args: vec!["run".into(), "build".into()],
                cwd: PathBuf::from("/root/packages/core"),
                color_idx: 1,
                estimated_cost: 1,
            },
        ];

        let graph = TaskGraph::build(tasks, &pkgs, None).expect("DAG build failed");

        // @app/core:build has index 1, @app/ui:build has index 0
        // @app/ui:build should depend on index 1
        assert!(graph.nodes[0].depends_on.contains(&1));
        assert!(graph.nodes[1].dependents.contains(&0));

        // Ready tasks should only be @app/core:build (index 1)
        assert_eq!(graph.ready_tasks(), vec![1]);
    }
}
