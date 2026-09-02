//! Workspace and task discovery from package manifests.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Options;
use crate::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePkg {
    pub name: String,
    pub dir: PathBuf,
    pub scripts: Vec<String>,
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

/// Locate root workspace directory containing a top-level `package.json`.
pub fn find_workspace_root(start_dir: &Path) -> Result<PathBuf, String> {
    let mut curr = start_dir.to_path_buf();
    loop {
        let pkg_path = curr.join("package.json");
        if pkg_path.is_file() {
            // Check if it has workspaces or if parent has none
            if let Ok(content) = fs::read_to_string(&pkg_path) {
                if let Ok(val) = json::parse(&content) {
                    if val.get("workspaces").is_some() {
                        return Ok(curr);
                    }
                }
            }
        }
        if let Some(parent) = curr.parent() {
            curr = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Fallback to start_dir if package.json exists there
    if start_dir.join("package.json").is_file() {
        Ok(start_dir.to_path_buf())
    } else {
        Err(format!(
            "could not find workspace root with package.json in '{}' or parent directories",
            start_dir.display()
        ))
    }
}

/// Parse workspace member packages from root `package.json`.
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

    let mut pkgs = Vec::new();
    for pattern in patterns {
        scan_workspace_pattern(root_dir, &pattern, &mut pkgs);
    }

    // Sort by name for stable ordering
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(pkgs)
}

fn scan_workspace_pattern(root_dir: &Path, pattern: &str, out: &mut Vec<WorkspacePkg>) {
    let clean = pattern.trim_end_matches('/');
    if let Some(prefix) = clean.strip_suffix("/*") {
        let parent_dir = root_dir.join(prefix);
        if let Ok(entries) = fs::read_dir(&parent_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest = path.join("package.json");
                    if manifest.is_file() {
                        if let Some(pkg) = read_pkg(&manifest, &path) {
                            out.push(pkg);
                        }
                    }
                }
            }
        }
    } else {
        let direct = root_dir.join(clean);
        let manifest = direct.join("package.json");
        if manifest.is_file() {
            if let Some(pkg) = read_pkg(&manifest, &direct) {
                out.push(pkg);
            }
        }
    }
}

fn read_pkg(manifest_path: &Path, dir: &Path) -> Option<WorkspacePkg> {
    let content = fs::read_to_string(manifest_path).ok()?;
    let val = json::parse(&content).ok()?;
    let name = val.get("name")?.as_str()?.to_string();
    let mut scripts = Vec::new();
    if let Some(s_obj) = val.get("scripts").and_then(|s| s.as_object()) {
        for k in s_obj.keys() {
            scripts.push(k.clone());
        }
    }
    Some(WorkspacePkg {
        name,
        dir: dir.to_path_buf(),
        scripts,
    })
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

/// Derive task list from options and workspace structure.
pub fn build_tasks(opts: &Options, root_dir: &Path) -> Result<Vec<TaskSpec>, String> {
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

    let is_check = opts.target == "check" || opts.target == "check:full";
    let script_for_pkgs = if is_check { "typecheck" } else { &opts.target };

    let mut task_specs = Vec::new();

    // 1. Root gates (only when no filter is applied)
    if opts.filter.is_none() {
        if is_check {
            for gate in ROOT_GATES {
                if root_scripts.iter().any(|s| s == gate) {
                    task_specs.push(TaskSpec {
                        name: gate.to_string(),
                        runner_bin: runner_bin.clone(),
                        args: vec!["run".to_string(), gate.to_string()],
                        cwd: root_dir.to_path_buf(),
                        color_idx: 0,
                        estimated_cost: estimate_cost(gate),
                    });
                }
            }

            let test_target = if opts.target == "check:full" {
                "test:all"
            } else if root_scripts.iter().any(|s| s == "test:unit") {
                "test:unit"
            } else if root_scripts.iter().any(|s| s == "test") {
                "test"
            } else {
                ""
            };

            if !test_target.is_empty() && root_scripts.iter().any(|s| s == test_target) {
                task_specs.push(TaskSpec {
                    name: test_target.to_string(),
                    runner_bin: runner_bin.clone(),
                    args: vec!["run".to_string(), test_target.to_string()],
                    cwd: root_dir.to_path_buf(),
                    color_idx: 0,
                    estimated_cost: estimate_cost(test_target),
                });
            }
        } else if root_scripts.iter().any(|s| s == &opts.target) {
            task_specs.push(TaskSpec {
                name: opts.target.clone(),
                runner_bin: runner_bin.clone(),
                args: vec!["run".to_string(), opts.target.clone()],
                cwd: root_dir.to_path_buf(),
                color_idx: 0,
                estimated_cost: estimate_cost(&opts.target),
            });
        }
    }

    // 2. Workspace package tasks
    let pkgs = discover_workspace_packages(root_dir)?;
    for pkg in pkgs {
        if !pkg.scripts.iter().any(|s| s == script_for_pkgs) {
            continue;
        }

        if let Some(ref filter_pat) = opts.filter {
            if !glob_match(filter_pat, &pkg.name) {
                continue;
            }
        }

        let task_name = format!("{}:{}", pkg.name, script_for_pkgs);
        task_specs.push(TaskSpec {
            name: task_name.clone(),
            runner_bin: runner_bin.clone(),
            args: vec!["run".to_string(), script_for_pkgs.to_string()],
            cwd: pkg.dir,
            color_idx: 0,
            estimated_cost: estimate_cost(&task_name),
        });
    }

    // Assign cyclic palette colors
    for (i, spec) in task_specs.iter_mut().enumerate() {
        spec.color_idx = i % 7;
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
}
