//! Workspace and task discovery from JavaScript, Turbo, and Cargo manifests.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Options;
use crate::dag::{PipelineRule, TurboPipeline};
use crate::json;
use crate::scm;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePkg {
    pub name: String,
    pub dir: PathBuf,
    pub scripts: Vec<String>,
    pub internal_deps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub name: String,
    pub runner_bin: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub color_idx: usize,
    pub estimated_cost: usize,
}

const ROOT_GATES: &[&str] = &["lint", "check:skills", "check:server-fns", "themes:a11y"];

/// Simple glob matching supporting `*` wildcards.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();
    let (mut p_idx, mut t_idx) = (0, 0);
    let (mut star_idx, mut match_idx) = (None, 0);

    while t_idx < t_chars.len() {
        if p_idx < p_chars.len() && p_chars[p_idx] == t_chars[t_idx] {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_chars.len() && p_chars[p_idx] == '*' {
            star_idx = Some(p_idx);
            p_idx += 1;
            match_idx = t_idx;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p_chars.len() && p_chars[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p_chars.len()
}

/// Detect the preferred JavaScript package runner binary (`bun`, `pnpm`, `yarn`, `npm`).
pub fn detect_runner_bin(root_dir: &Path) -> String {
    if root_dir.join("bun.lockb").exists() || root_dir.join("bun.lock").exists() {
        "bun".to_string()
    } else if root_dir.join("pnpm-lock.yaml").exists() {
        "pnpm".to_string()
    } else if root_dir.join("yarn.lock").exists() {
        "yarn".to_string()
    } else if root_dir.join("package-lock.json").exists() {
        "npm".to_string()
    } else {
        "bun".to_string()
    }
}

/// Locate the nearest JavaScript or Cargo workspace root.
pub fn find_workspace_root(start_dir: &Path) -> Result<PathBuf, String> {
    let mut curr = start_dir.to_path_buf();
    loop {
        let pkg_path = curr.join("package.json");
        if pkg_path.is_file() {
            if let Ok(content) = fs::read_to_string(&pkg_path) {
                if let Ok(val) = json::parse(&content) {
                    if val.get("workspaces").is_some() {
                        return Ok(curr);
                    }
                }
            }
        }

        let cargo_path = curr.join("Cargo.toml");
        if cargo_path.is_file() && is_cargo_workspace(&cargo_path) {
            return Ok(curr);
        }
        if let Some(parent) = curr.parent() {
            curr = parent.to_path_buf();
        } else {
            break;
        }
    }

    if start_dir.join("package.json").is_file() || start_dir.join("Cargo.toml").is_file() {
        Ok(start_dir.to_path_buf())
    } else {
        Err(format!(
            "could not find a JavaScript or Cargo workspace in '{}' or parent directories",
            start_dir.display()
        ))
    }
}

fn is_cargo_workspace(manifest_path: &Path) -> bool {
    fs::read_to_string(manifest_path)
        .map(|content| content.lines().any(|line| line.trim() == "[workspace]"))
        .unwrap_or(false)
}

/// Parse workspace member packages and wire internal dependencies from root `package.json`.
pub fn discover_workspace_packages(root_dir: &Path) -> Result<Vec<WorkspacePkg>, String> {
    let root_pkg_file = root_dir.join("package.json");
    let content = fs::read_to_string(&root_pkg_file)
        .map_err(|e| format!("failed to read '{}': {e}", root_pkg_file.display()))?;
    let root_val = json::parse(&content)
        .map_err(|e| format!("failed to parse '{}': {e}", root_pkg_file.display()))?;

    let mut patterns = Vec::new();
    if let Some(ws) = root_val.get("workspaces") {
        if let Some(arr) = ws.as_array() {
            for item in arr {
                if let Some(s) = item.as_str() {
                    patterns.push(s.to_string());
                }
            }
        } else if let Some(obj) = ws.as_object() {
            if let Some(arr) = obj.get("packages").and_then(|p| p.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        patterns.push(s.to_string());
                    }
                }
            }
        }
    }

    let mut raw_pkgs = Vec::new();
    for pattern in patterns {
        scan_workspace_pattern(root_dir, &pattern, &mut raw_pkgs);
    }

    let pkg_names: HashSet<String> = raw_pkgs.iter().map(|(pkg, _)| pkg.name.clone()).collect();

    let mut pkgs = Vec::new();
    for (mut pkg, raw_declared_deps) in raw_pkgs {
        for dep in raw_declared_deps {
            if pkg_names.contains(&dep) {
                pkg.internal_deps.push(dep);
            }
        }
        pkgs.push(pkg);
    }

    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(pkgs)
}

fn scan_workspace_pattern(
    root_dir: &Path,
    pattern: &str,
    out: &mut Vec<(WorkspacePkg, Vec<String>)>,
) {
    let clean = pattern.trim_end_matches('/');
    if let Some(prefix) = clean.strip_suffix("/*") {
        let parent_dir = root_dir.join(prefix);
        if let Ok(entries) = fs::read_dir(&parent_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest = path.join("package.json");
                    if manifest.is_file() {
                        if let Some(res) = read_pkg(&manifest, &path) {
                            out.push(res);
                        }
                    }
                }
            }
        }
    } else {
        let direct = root_dir.join(clean);
        let manifest = direct.join("package.json");
        if manifest.is_file() {
            if let Some(res) = read_pkg(&manifest, &direct) {
                out.push(res);
            }
        }
    }
}

fn read_pkg(manifest_path: &Path, dir: &Path) -> Option<(WorkspacePkg, Vec<String>)> {
    let content = fs::read_to_string(manifest_path).ok()?;
    let val = json::parse(&content).ok()?;
    let name = val.get("name")?.as_str()?.to_string();

    let mut scripts = Vec::new();
    if let Some(s_obj) = val.get("scripts").and_then(|s| s.as_object()) {
        for k in s_obj.keys() {
            scripts.push(k.clone());
        }
    }

    let mut raw_deps = Vec::new();
    for section in &[
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(dep_obj) = val.get(section).and_then(|d| d.as_object()) {
            for k in dep_obj.keys() {
                raw_deps.push(k.clone());
            }
        }
    }

    Some((
        WorkspacePkg {
            name,
            dir: dir.to_path_buf(),
            scripts,
            internal_deps: Vec::new(),
        },
        raw_deps,
    ))
}

/// Read `turbo.json` task pipeline rules if present in the workspace root.
pub fn read_turbo_pipeline(root_dir: &Path) -> Option<TurboPipeline> {
    let turbo_file = root_dir.join("turbo.json");
    if !turbo_file.is_file() {
        return None;
    }

    let content = fs::read_to_string(&turbo_file).ok()?;
    let val = json::parse(&content).ok()?;

    let tasks_obj = val
        .get("tasks")
        .or_else(|| val.get("pipeline"))
        .and_then(|t| t.as_object())?;

    let mut rules = HashMap::new();
    for (task_name, task_val) in tasks_obj {
        let mut depends_on = Vec::new();
        if let Some(arr) = task_val.get("dependsOn").and_then(|d| d.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    depends_on.push(s.to_string());
                }
            }
        }
        rules.insert(task_name.clone(), PipelineRule { depends_on });
    }

    Some(TurboPipeline { task_rules: rules })
}

fn estimate_cost(task_name: &str) -> usize {
    if task_name.starts_with("test") {
        3
    } else if task_name.contains("dashboard") || task_name.contains("ui") {
        2
    } else if task_name.ends_with(":typecheck") {
        1
    } else {
        0
    }
}

/// Derive task list from options, workspace structure, and Git affected scope.
pub fn build_tasks(
    opts: &Options,
    root_dir: &Path,
) -> Result<(Vec<TaskSpec>, Vec<WorkspacePkg>), String> {
    if root_dir.join("Cargo.toml").is_file() && !root_dir.join("package.json").is_file() {
        let tasks = build_cargo_tasks(opts, root_dir)?;
        return Ok((tasks, Vec::new()));
    }

    let runner_bin = detect_runner_bin(root_dir);
    let root_pkg_file = root_dir.join("package.json");
    let root_content = fs::read_to_string(&root_pkg_file)
        .map_err(|e| format!("failed to read '{}': {e}", root_pkg_file.display()))?;
    let root_val = json::parse(&root_content)
        .map_err(|e| format!("failed to parse '{}': {e}", root_pkg_file.display()))?;

    let root_scripts: Vec<String> = root_val
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    let pkgs = discover_workspace_packages(root_dir)?;

    // Resolve affected packages if --since is given
    let affected_names: Option<HashSet<String>> = if let Some(ref since_ref) = opts.since {
        let changed_files = scm::find_changed_files(root_dir, since_ref)?;
        let affected = scm::resolve_affected_packages(&pkgs, &changed_files, root_dir, true);
        Some(affected.into_iter().collect())
    } else {
        None
    };

    let mut task_specs = Vec::new();

    for target in &opts.targets {
        let is_check = target == "check" || target == "check:full";

        // 1. Root-level gates (only when no package filter or git since scoping is active)
        if opts.filter.is_none() && opts.since.is_none() {
            if is_check {
                for gate in ROOT_GATES {
                    if root_scripts.iter().any(|s| s == gate) {
                        let mut args = vec!["run".to_string(), gate.to_string()];
                        args.extend(opts.passthrough_args.clone());
                        task_specs.push(TaskSpec {
                            name: gate.to_string(),
                            runner_bin: runner_bin.clone(),
                            args,
                            cwd: root_dir.to_path_buf(),
                            color_idx: 0,
                            estimated_cost: estimate_cost(gate),
                        });
                    }
                }

                let test_target = if target == "check:full" {
                    "test:all"
                } else if root_scripts.iter().any(|s| s == "test:unit") {
                    "test:unit"
                } else if root_scripts.iter().any(|s| s == "test") {
                    "test"
                } else {
                    ""
                };

                if !test_target.is_empty() && root_scripts.iter().any(|s| s == test_target) {
                    let mut args = vec!["run".to_string(), test_target.to_string()];
                    args.extend(opts.passthrough_args.clone());
                    task_specs.push(TaskSpec {
                        name: test_target.to_string(),
                        runner_bin: runner_bin.clone(),
                        args,
                        cwd: root_dir.to_path_buf(),
                        color_idx: 0,
                        estimated_cost: estimate_cost(test_target),
                    });
                }
            } else if root_scripts.iter().any(|s| s == target) {
                let mut args = vec!["run".to_string(), target.clone()];
                args.extend(opts.passthrough_args.clone());
                task_specs.push(TaskSpec {
                    name: target.clone(),
                    runner_bin: runner_bin.clone(),
                    args,
                    cwd: root_dir.to_path_buf(),
                    color_idx: 0,
                    estimated_cost: estimate_cost(target),
                });
            }
        }

        // 2. Package tasks
        let script_for_pkgs = if is_check {
            "typecheck"
        } else {
            target.as_str()
        };

        for pkg in &pkgs {
            if !pkg.scripts.iter().any(|s| s == script_for_pkgs) {
                continue;
            }

            // Filter by glob pattern
            if let Some(ref filter_pat) = opts.filter {
                if !glob_match(filter_pat, &pkg.name) {
                    continue;
                }
            }

            // Filter by Git affected set
            if let Some(ref affected_set) = affected_names {
                if !affected_set.contains(&pkg.name) {
                    continue;
                }
            }

            let task_name = format!("{}:{}", pkg.name, script_for_pkgs);
            let mut args = vec!["run".to_string(), script_for_pkgs.to_string()];
            args.extend(opts.passthrough_args.clone());

            task_specs.push(TaskSpec {
                name: task_name.clone(),
                runner_bin: runner_bin.clone(),
                args,
                cwd: pkg.dir.clone(),
                color_idx: 0,
                estimated_cost: estimate_cost(&task_name),
            });
        }
    }

    // Assign cyclic palette colors
    for (i, spec) in task_specs.iter_mut().enumerate() {
        spec.color_idx = i % 7;
    }

    Ok((task_specs, pkgs))
}

fn build_cargo_tasks(opts: &Options, root_dir: &Path) -> Result<Vec<TaskSpec>, String> {
    if opts.filter.is_some() {
        return Err(
            "--filter is not supported for pure Cargo workspaces; Cargo runs the workspace as one unit"
                .to_string(),
        );
    }

    let mut task_specs = Vec::new();

    for target in &opts.targets {
        let task_defs: Vec<(&str, Vec<&str>)> = match target.as_str() {
            "check" | "check:full" => vec![
                ("fmt-check", vec!["fmt", "--all", "--", "--check"]),
                (
                    "lint",
                    vec![
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--",
                        "-D",
                        "warnings",
                    ],
                ),
                ("test", vec!["test", "--workspace"]),
            ],
            "fmt" | "fmt-check" => vec![("fmt-check", vec!["fmt", "--all", "--", "--check"])],
            "lint" | "clippy" => vec![(
                "lint",
                vec![
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            )],
            "test" => vec![("test", vec!["test", "--workspace"])],
            "build" => vec![("build", vec!["build", "--workspace"])],
            other => {
                return Err(format!(
                    "unknown Cargo target '{other}'; use check, fmt-check, lint, test, or build"
                ));
            }
        };

        for (name, args) in task_defs {
            let mut full_args: Vec<String> = args.into_iter().map(String::from).collect();
            full_args.extend(opts.passthrough_args.clone());
            task_specs.push(TaskSpec {
                name: name.to_string(),
                runner_bin: "cargo".to_string(),
                args: full_args,
                cwd: root_dir.to_path_buf(),
                color_idx: task_specs.len() % 7,
                estimated_cost: estimate_cost(name),
            });
        }
    }

    Ok(task_specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("@renkonos/*", "@renkonos/ui"));
        assert!(glob_match("@renkonos/*", "@renkonos/core"));
        assert!(!glob_match("@renkonos/*", "@other/ui"));
        assert!(glob_match("*ui*", "@renkonos/ui"));
        assert!(glob_match("*dashboard*", "apps/dashboard"));
        assert!(glob_match("packages/*", "packages/core"));
    }

    #[test]
    fn cargo_workspace_builds_quality_tasks() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (tasks, _) = build_tasks(&Options::default(), &root).unwrap();

        assert_eq!(
            tasks
                .iter()
                .map(|task| task.name.as_str())
                .collect::<Vec<_>>(),
            vec!["fmt-check", "lint", "test"]
        );
        assert!(tasks.iter().all(|task| task.runner_bin == "cargo"));
    }

    #[test]
    fn cargo_workspace_root_is_found_from_a_member() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let member = root.join("crates/webdriver");
        assert_eq!(find_workspace_root(&member).unwrap(), root);
    }
}

#[test]
fn parse_turbo_pipeline_rules() {
    let tmp = std::env::temp_dir().join("fanout-test-turbo-json");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let turbo_json = r#"{
            "tasks": {
                "build": {
                    "dependsOn": ["^build"]
                },
                "test": {
                    "dependsOn": ["build"]
                }
            }
        }"#;
    fs::write(tmp.join("turbo.json"), turbo_json).unwrap();

    let pipeline = read_turbo_pipeline(&tmp).expect("should parse turbo.json");
    assert_eq!(
        pipeline.task_rules.get("build").unwrap().depends_on,
        vec!["^build"]
    );
    assert_eq!(
        pipeline.task_rules.get("test").unwrap().depends_on,
        vec!["build"]
    );

    let _ = fs::remove_dir_all(&tmp);
}
