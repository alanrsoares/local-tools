//! Definition of project build artifacts and size formatting.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Known project build artifact categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactType {
    Rust,
    Node,
    Python,
    DotNet,
}

impl ArtifactType {
    /// Parse ecosystem name from CLI flag (e.g. `rust`, `node`, `py`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "rust" | "cargo" | "rs" => Some(ArtifactType::Rust),
            "node" | "ts" | "js" | "bun" | "npm" | "pnpm" | "yarn" => Some(ArtifactType::Node),
            "python" | "py" | "uv" => Some(ArtifactType::Python),
            "dotnet" | "net" | "csharp" => Some(ArtifactType::DotNet),
            _ => None,
        }
    }

    /// Identify artifact type from directory name and optional parent inspection.
    pub fn detect(dir_name: &str, parent: &Path) -> Option<Self> {
        match dir_name {
            // `target` is generic enough to be a user's own directory. Only
            // classify it as Rust output when its project root has a manifest.
            "target" if parent.join("Cargo.toml").is_file() => Some(ArtifactType::Rust),
            "node_modules" | ".next" | ".nuxt" | ".turbo" | ".svelte-kit" | ".astro" | ".vite"
            | ".parcel-cache" | "coverage" | ".nyc_output" => Some(ArtifactType::Node),
            ".venv" | "venv" | "__pycache__" | ".pytest_cache" | ".ruff_cache" | ".mypy_cache"
            | ".tox" | ".nox" => Some(ArtifactType::Python),
            "bin" | "obj" => {
                if has_dotnet_markers(parent) {
                    Some(ArtifactType::DotNet)
                } else {
                    None
                }
            }
            "dist" | "build" => {
                if parent.join("package.json").is_file() || parent.join("tsconfig.json").is_file() {
                    Some(ArtifactType::Node)
                } else if parent.join("pyproject.toml").is_file()
                    || parent.join("setup.py").is_file()
                {
                    Some(ArtifactType::Python)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ArtifactType::Rust => "Rust",
            ArtifactType::Node => "Node/Bun",
            ArtifactType::Python => "Python",
            ArtifactType::DotNet => ".NET",
        }
    }
}

fn has_dotnet_markers(dir: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if let Some(ext) = e.path().extension() {
                if ext == "csproj" || ext == "fsproj" || ext == "sln" {
                    return true;
                }
            }
        }
    }
    false
}

impl fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// A discovered artifact directory on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub path: PathBuf,
    pub artifact_type: ArtifactType,
    pub size_bytes: u64,
    pub last_modified: Option<SystemTime>,
}

/// Format a byte count into a human-readable size string (B, KB, MB, GB).
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_boundaries() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(15 * 1024 * 1024), "15.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn artifact_type_parsing() {
        assert_eq!(ArtifactType::parse("rust"), Some(ArtifactType::Rust));
        assert_eq!(ArtifactType::parse("ts"), Some(ArtifactType::Node));
        assert_eq!(ArtifactType::parse("pnpm"), Some(ArtifactType::Node));
        assert_eq!(ArtifactType::parse("py"), Some(ArtifactType::Python));
        assert_eq!(ArtifactType::parse("net"), Some(ArtifactType::DotNet));
        assert_eq!(ArtifactType::parse("unknown"), None);
    }

    #[test]
    fn target_requires_a_cargo_manifest() {
        let tmp = std::env::temp_dir().join("devclean-target-detection-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        assert_eq!(ArtifactType::detect("target", &tmp), None);

        std::fs::write(tmp.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        assert_eq!(
            ArtifactType::detect("target", &tmp),
            Some(ArtifactType::Rust)
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
