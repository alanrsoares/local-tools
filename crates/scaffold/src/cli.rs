//! Minimal, dependency-free CLI parsing.
//!
//! We avoid the `clap`/`structopt` crate on purpose so the first `cargo build`
//! and every test run work fully offline (the workspace's std-only philosophy).
//! A richer flag story can be added later; v1 covers the handful of options the
//! user actually reaches for.

#[allow(clippy::wildcard_imports)]
use super::templates::Lang;
use std::path::PathBuf;

/// What the caller wants done after parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Lay out a project (the default action).
    Scaffold,
    /// List available languages and their default names.
    List,
    /// Print the help text.
    Help,
    /// Print the version string.
    Version,
}

/// Parsed CLI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The chosen action (defaults to [`Action::Scaffold`]).
    pub action: Action,
    /// The language, when the action needs it.
    pub lang: Option<Lang>,
    /// Project name override (`--name`); falls back to the lang default.
    pub name: Option<String>,
    /// Parent directory to create the project in; defaults to the current dir.
    pub path: PathBuf,
    /// Overwrite a non-empty target (`--force`).
    pub force: bool,
    /// List files without writing (`--dry-run`).
    pub dry_run: bool,
    /// Skip the post-scaffold install step (`--no-install`) — info only in v1.
    pub no_install: bool,
    /// Skip recording the scaffold in the recent log (`--no-log`).
    pub no_log: bool,
    /// Force colour even when output is redirected (`--color`).
    pub color: bool,
    /// Disable colour (`--no-color`).
    pub no_color: bool,
}

impl Default for Parsed {
    fn default() -> Self {
        Self {
            action: Action::Scaffold,
            lang: None,
            name: None,
            path: PathBuf::from("."),
            force: false,
            dry_run: false,
            no_install: false,
            no_log: false,
            color: false,
            no_color: false,
        }
    }
}

/// The one-line version banner, kept in sync with the crate version.
pub const VERSION: &str = "scaffold 0.1.0";

/// Help text printed for `-h` / `--help`.
pub const HELP: &str = r#"scaffold <LANG> [OPTIONS] — lay down a ready-to-run project from your defaults.

USAGE
    scaffold <LANG> [OPTIONS]
    scaffold <COMMAND>

LANG
    ts    Bun + TypeScript + Biome
    rust  Rust binary (cargo)
    go    Go module
    py    Python (uv / PEP 621)
    net   .NET minimal API — v2 (needs the dotnet SDK)

OPTIONS
    -n, --name <NAME>    project name (default: <lang>-default)
    -p, --path <DIR>     where to create it (default: current dir)
         --force          overwrite an existing, non-empty target dir
         --dry-run        show the files that would be written, write nothing
         --no-install     skip the post-scaffold install step (info only in v1)
         --no-log         do not record the scaffold in ~/.config/local-tools
         --color          force colour even when output is redirected
         --no-color       disable colour

COMMANDS
    -L, --list            list available languages and their default names
    -h, --help            print this help
    -V, --version         print the version string
"#;

/// Parse `args` into [`Parsed`].
///
/// Returns `Ok` with the parsed state, or `Err` with a short, user-facing
/// message suitable for printing to stderr.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, String> {
    let mut it = args.into_iter();
    let mut p = Parsed::default();

    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => p.action = Action::Help,
            "-V" | "--version" => p.action = Action::Version,
            "-L" | "--list" => p.action = Action::List,
            "-n" | "--name" => p.name = Some(require_next(&mut it, "--name")?),
            "-p" | "--path" => p.path = PathBuf::from(require_next(&mut it, "--path")?),
            "--force" | "-f" => p.force = true,
            "--dry-run" => p.dry_run = true,
            "--no-install" => p.no_install = true,
            "--no-log" => p.no_log = true,
            "--no-color" => p.no_color = true,
            "--color" => p.color = true,
            s => {
                if s.starts_with('-') {
                    return Err(format!("unknown flag: {a}"));
                }
                if p.lang.is_some() {
                    return Err(format!("unexpected argument: {s}"));
                }
                let parsed = Lang::parse(s)
                    .ok_or_else(|| format!("unknown language: {s} (try `scaffold --list`)"))?;
                p.lang = Some(parsed);
            }
        }
    }

    if p.action == Action::Scaffold && p.lang.is_none() {
        return Err("missing <LANG>: try ts, rust, go, py — or run `scaffold --list`".into());
    }
    Ok(p)
}

/// Pull and return the next argument as the value of `flag`, erroring with a
/// friendly message if it is missing.
fn require_next<I: Iterator<Item = String>>(it: &mut I, flag: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("{flag} requires a value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help_actions() {
        let p = parse(vec!["--help".to_string()]).unwrap();
        assert_eq!(p.action, Action::Help);

        let p2 = parse(vec!["-h".to_string()]).unwrap();
        assert_eq!(p2.action, Action::Help);
    }

    #[test]
    fn parse_version_and_list_actions() {
        let pv = parse(vec!["--version".to_string()]).unwrap();
        assert_eq!(pv.action, Action::Version);

        let pl = parse(vec!["--list".to_string()]).unwrap();
        assert_eq!(pl.action, Action::List);
    }

    #[test]
    fn parse_scaffold_options() {
        let p = parse(vec![
            "rust".to_string(),
            "-n".to_string(),
            "my-rust-cli".to_string(),
            "-p".to_string(),
            "/tmp".to_string(),
            "--force".to_string(),
            "--dry-run".to_string(),
            "--no-log".to_string(),
            "--color".to_string(),
        ])
        .unwrap();

        assert_eq!(p.action, Action::Scaffold);
        assert_eq!(p.lang, Some(Lang::Rust));
        assert_eq!(p.name.as_deref(), Some("my-rust-cli"));
        assert_eq!(p.path, PathBuf::from("/tmp"));
        assert!(p.force);
        assert!(p.dry_run);
        assert!(p.no_log);
        assert!(p.color);
    }

    #[test]
    fn parse_missing_lang_errors() {
        let err = parse(Vec::<String>::new()).unwrap_err();
        assert!(err.contains("missing <LANG>"));
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = parse(vec!["--non-existent".to_string()]).unwrap_err();
        assert!(err.contains("unknown flag"));
    }

    #[test]
    fn parse_missing_flag_value_errors() {
        let err = parse(vec!["ts".to_string(), "--name".to_string()]).unwrap_err();
        assert!(err.contains("--name requires a value"));
    }

    #[test]
    fn parse_duplicate_positional_errors() {
        let err = parse(vec!["ts".to_string(), "rust".to_string()]).unwrap_err();
        assert!(err.contains("unexpected argument"));
    }
}
