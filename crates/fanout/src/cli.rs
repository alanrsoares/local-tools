//! CLI argument parsing for `fanout`.

use std::thread;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const HELP: &str = r#"fanout — concurrent quality gate & task matrix runner

USAGE:
    fanout [target] [FLAGS] [OPTIONS]

TARGET:
    check              Run root gates (lint, check:*, themes, test:unit) + workspace typechecks (default)
    check:full         Run root gates + test:all + workspace typechecks
    <script>           Run root <script> (if present) + all workspace packages defining <script>

FLAGS:
    --bail             Abort remaining tasks on first failure
    --compact          Machine-readable mode: no TUI cursor tricks, output only on failure
    --color            Force ANSI color output
    --no-color         Disable ANSI color output
    -h, --help         Print this help message
    -V, --version      Print version information

OPTIONS:
    --filter <glob>    Scope execution to workspace packages matching <glob> (e.g. "@renkonos/*", "*ui*")
    --timeout <ms>     Timeout per task in milliseconds (default: 900000 = 15m)
    --tail <n>         Lines of output to retain on failure (default: 40, 0 = unlimited)
    -j, --jobs <n>     Concurrency ceiling (default: hardware threads, min 2)
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Help,
    Version,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub action: Action,
    pub target: String,
    pub filter: Option<String>,
    pub bail: bool,
    pub timeout_ms: u64,
    pub compact: bool,
    pub tail_lines: usize,
    pub jobs: usize,
    pub color: bool,
    pub no_color: bool,
}

impl Default for Options {
    fn default() -> Self {
        let default_jobs = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .max(2);

        Self {
            action: Action::Run,
            target: "check".to_string(),
            filter: None,
            bail: false,
            timeout_ms: 15 * 60 * 1000,
            compact: false,
            tail_lines: 40,
            jobs: default_jobs,
            color: false,
            no_color: false,
        }
    }
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Options, String> {
    let mut opts = Options::default();
    let mut iter = args.into_iter().peekable();
    let mut positional_set = false;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                opts.action = Action::Help;
                return Ok(opts);
            }
            "-V" | "--version" => {
                opts.action = Action::Version;
                return Ok(opts);
            }
            "--bail" => opts.bail = true,
            "--compact" => opts.compact = true,
            "--color" => opts.color = true,
            "--no-color" => opts.no_color = true,
            "--filter" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--filter requires a glob pattern value".to_string())?;
                opts.filter = Some(val);
            }
            "--timeout" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--timeout requires a millisecond duration value".to_string())?;
                let ms: u64 = val.parse().map_err(|_| {
                    format!("invalid --timeout value '{val}', expected integer milliseconds")
                })?;
                if ms > 0 {
                    opts.timeout_ms = ms;
                }
            }
            "--tail" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--tail requires an integer line count value".to_string())?;
                let lines: usize = val
                    .parse()
                    .map_err(|_| format!("invalid --tail value '{val}', expected integer"))?;
                opts.tail_lines = lines;
            }
            "-j" | "--jobs" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--jobs requires an integer concurrency value".to_string())?;
                let jobs: usize = val
                    .parse()
                    .map_err(|_| format!("invalid --jobs value '{val}', expected integer >= 1"))?;
                if jobs >= 1 {
                    opts.jobs = jobs;
                }
            }
            other if other.starts_with("--filter=") => {
                let val = &other["--filter=".len()..];
                opts.filter = Some(val.to_string());
            }
            other if other.starts_with("--timeout=") => {
                let val = &other["--timeout=".len()..];
                let ms: u64 = val
                    .parse()
                    .map_err(|_| format!("invalid --timeout value '{val}'"))?;
                if ms > 0 {
                    opts.timeout_ms = ms;
                }
            }
            other if other.starts_with("--tail=") => {
                let val = &other["--tail=".len()..];
                let lines: usize = val
                    .parse()
                    .map_err(|_| format!("invalid --tail value '{val}'"))?;
                opts.tail_lines = lines;
            }
            other if other.starts_with("--jobs=") => {
                let val = &other["--jobs=".len()..];
                let jobs: usize = val
                    .parse()
                    .map_err(|_| format!("invalid --jobs value '{val}'"))?;
                if jobs >= 1 {
                    opts.jobs = jobs;
                }
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'"));
            }
            pos => {
                if !positional_set {
                    opts.target = pos.to_string();
                    positional_set = true;
                } else {
                    return Err(format!("unexpected additional positional argument '{pos}'"));
                }
            }
        }
    }

    Ok(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let opts = parse(vec![]).unwrap();
        assert_eq!(opts.target, "check");
        assert!(!opts.bail);
        assert!(!opts.compact);
        assert_eq!(opts.tail_lines, 40);
        assert_eq!(opts.timeout_ms, 900_000);
        assert!(opts.jobs >= 2);
    }

    #[test]
    fn parses_custom_target_and_flags() {
        let args = vec![
            "typecheck".into(),
            "--bail".into(),
            "--compact".into(),
            "--tail".into(),
            "100".into(),
            "--jobs".into(),
            "8".into(),
            "--filter".into(),
            "@renkonos/*".into(),
            "--timeout".into(),
            "60000".into(),
        ];
        let opts = parse(args).unwrap();
        assert_eq!(opts.target, "typecheck");
        assert!(opts.bail);
        assert!(opts.compact);
        assert_eq!(opts.tail_lines, 100);
        assert_eq!(opts.jobs, 8);
        assert_eq!(opts.filter.as_deref(), Some("@renkonos/*"));
        assert_eq!(opts.timeout_ms, 60_000);
    }

    #[test]
    fn parses_equals_syntax() {
        let args = vec![
            "--filter=*ui*".into(),
            "--jobs=4".into(),
            "--tail=20".into(),
            "--timeout=30000".into(),
        ];
        let opts = parse(args).unwrap();
        assert_eq!(opts.filter.as_deref(), Some("*ui*"));
        assert_eq!(opts.jobs, 4);
        assert_eq!(opts.tail_lines, 20);
        assert_eq!(opts.timeout_ms, 30_000);
    }
}
