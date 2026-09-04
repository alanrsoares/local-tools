//! CLI argument parsing for `devclean`.

use std::path::PathBuf;

use crate::artifact::ArtifactType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Scan and report reclaimable space (default).
    Scan,
    /// Clean / delete discovered artifact directories.
    Clean,
    /// Print help.
    Help,
    /// Print version.
    Version,
}

use local_common::{split_flag, ArgCursor, CommonFlags};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub action: Action,
    pub path: PathBuf,
    pub force: bool,
    pub dry_run: bool,
    pub targets: Vec<ArtifactType>,
    pub min_size_mb: u64,
    pub older_than_days: Option<u64>,
    pub flags: CommonFlags,
}

impl Default for Parsed {
    fn default() -> Self {
        Self {
            action: Action::Scan,
            path: PathBuf::from("."),
            force: false,
            dry_run: false,
            targets: Vec::new(),
            min_size_mb: 0,
            older_than_days: None,
            flags: CommonFlags::default(),
        }
    }
}

pub const VERSION: &str = "devclean 0.1.0";

pub const HELP: &str = r#"devclean [PATH] [OPTIONS] — scan and clean multi-ecosystem project build artifacts.

USAGE
    devclean                    # scan current directory for reclaimable artifacts
    devclean ~/dev              # scan ~/dev directory
    devclean --clean -y         # delete artifacts without prompt
    devclean -t rust,node       # filter by ecosystems

OPTIONS
    -c, --clean           delete discovered artifact directories
    -y, --yes, --force    proceed with deletion without confirmation
        --dry-run         report without making changes (default)
    -t, --target <TYPES>  comma-separated target ecosystems: rust, node, python, dotnet
    -p, --path <DIR>      root path to scan (default: current directory)
        --min-size <MB>   ignore artifact directories smaller than MB megabytes
        --older-than <N>  only clean artifacts not modified for N days
        --color           force colour output
        --no-color        disable colour output
    -h, --help            print this help
    -V, --version         print version
"#;

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, String> {
    let mut cursor = ArgCursor::new(args.into_iter());
    let mut p = Parsed::default();
    let mut explicit_clean = false;

    while let Some(a) = cursor.next() {
        let (flag, inline) = split_flag(&a);
        match flag {
            "-h" | "--help" => p.action = Action::Help,
            "-V" | "--version" => p.action = Action::Version,
            "-c" | "--clean" => explicit_clean = true,
            "-y" | "--yes" | "--force" => p.force = true,
            "--dry-run" => p.dry_run = true,
            f if p.flags.check_arg(f) => {}
            "-p" | "--path" => {
                let dir = cursor.require_value("--path", inline)?;
                p.path = PathBuf::from(dir);
            }
            "-t" | "--target" | "--targets" => {
                let val = cursor.require_value("--target", inline)?;
                for t_str in val.split(',') {
                    let parsed = ArtifactType::parse(t_str)
                        .ok_or_else(|| format!("unknown ecosystem target: '{t_str}'"))?;
                    if !p.targets.contains(&parsed) {
                        p.targets.push(parsed);
                    }
                }
            }
            "--min-size" => {
                let val = cursor.require_value("--min-size", inline)?;
                p.min_size_mb = val
                    .parse()
                    .map_err(|_| "--min-size must be a valid number".to_string())?;
            }
            "--older-than" => {
                let val = cursor.require_value("--older-than", inline)?;
                p.older_than_days = Some(
                    val.parse()
                        .map_err(|_| "--older-than must be a valid number of days".to_string())?,
                );
            }
            s if s.starts_with('-') => return Err(format!("unknown flag: {s}")),
            s => {
                p.path = PathBuf::from(s);
            }
        }
    }

    if p.action == Action::Help || p.action == Action::Version {
        return Ok(p);
    }

    if explicit_clean && !p.dry_run {
        p.action = Action::Clean;
    } else {
        p.action = Action::Scan;
    }

    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_scan() {
        let p = parse(Vec::<String>::new()).unwrap();
        assert_eq!(p.action, Action::Scan);
        assert_eq!(p.path, PathBuf::from("."));
    }

    #[test]
    fn parse_clean_options() {
        let p = parse(vec![
            "--clean".to_string(),
            "-y".to_string(),
            "-t".to_string(),
            "rust,node".to_string(),
            "/Users/alan/dev".to_string(),
        ])
        .unwrap();

        assert_eq!(p.action, Action::Clean);
        assert!(p.force);
        assert_eq!(p.targets, vec![ArtifactType::Rust, ArtifactType::Node]);
        assert_eq!(p.path, PathBuf::from("/Users/alan/dev"));
    }
}
