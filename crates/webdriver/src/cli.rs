//! Command-line argument parsing, persistent sessions, and help definitions.

use crate::dsl::{self, Step};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CliOptions {
    pub custom_browser: Option<String>,
    pub default_timeout_ms: u64,
    pub headless: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub session_name: Option<String>,
    pub user_data_dir: Option<PathBuf>,
    pub steps: Vec<Step>,
}

pub enum Action {
    Run(CliOptions),
    ListSessions,
    ClearSession(String),
    Help,
    Version,
}

pub fn parse_args(args: &[String]) -> Result<Action, String> {
    if args.is_empty() {
        return Ok(Action::Help);
    }

    let mut custom_browser = None;
    let mut default_timeout_ms = 30_000;
    let mut headless = true;
    let mut quiet = false;
    let mut verbose = false;
    let mut session_name = None;
    let mut user_data_dir = None;
    let mut script_file = None;
    let mut read_stdin = false;

    let mut step_tokens = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        match arg.as_str() {
            "-h" | "--help" | "help" => return Ok(Action::Help),
            "-V" | "--version" | "version" => return Ok(Action::Version),
            "--list-sessions" | "sessions" => return Ok(Action::ListSessions),
            "--clear-session" => {
                i += 1;
                if i >= args.len() {
                    return Err("--clear-session requires a session name".to_string());
                }
                return Ok(Action::ClearSession(args[i].clone()));
            }
            "-b" | "--browser" => {
                i += 1;
                if i >= args.len() {
                    return Err("--browser requires a path".to_string());
                }
                custom_browser = Some(args[i].clone());
            }
            "-s" | "--session" => {
                i += 1;
                if i >= args.len() {
                    return Err("--session requires a session name (e.g. 'my-app')".to_string());
                }
                session_name = Some(args[i].clone());
            }
            "--user-data-dir" | "--profile" => {
                i += 1;
                if i >= args.len() {
                    return Err("--user-data-dir requires a directory path".to_string());
                }
                user_data_dir = Some(PathBuf::from(&args[i]));
            }
            "-t" | "--timeout" => {
                i += 1;
                if i >= args.len() {
                    return Err("--timeout requires a duration (e.g. 60s, 500ms)".to_string());
                }
                default_timeout_ms = dsl::parse_duration_ms(&args[i])?;
            }
            "--headed" | "--no-headless" => {
                headless = false;
            }
            "--headless" => {
                headless = true;
            }
            "-q" | "--quiet" => {
                quiet = true;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "--file" | "-f" if i + 1 < args.len() && !args[i + 1].starts_with('-') => {
                i += 1;
                script_file = Some(args[i].clone());
            }
            "-" => {
                read_stdin = true;
            }
            other => {
                step_tokens.push(other.to_string());
            }
        }
        i += 1;
    }

    let mut steps = Vec::new();

    if read_stdin {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| format!("failed to read script from stdin: {e}"))?;
        let parsed = dsl::parse_script(&buffer, default_timeout_ms)?;
        steps.extend(parsed);
    } else if let Some(file_path) = script_file {
        let content = fs::read_to_string(&file_path)
            .map_err(|e| format!("failed to read script file '{file_path}': {e}"))?;
        let parsed = dsl::parse_script(&content, default_timeout_ms)?;
        steps.extend(parsed);
    }

    if !step_tokens.is_empty() {
        let parsed = dsl::parse_tokens(&step_tokens, default_timeout_ms)?;
        steps.extend(parsed);
    }

    if steps.is_empty() {
        return Ok(Action::Help);
    }

    Ok(Action::Run(CliOptions {
        custom_browser,
        default_timeout_ms,
        headless,
        quiet,
        verbose,
        session_name,
        user_data_dir,
        steps,
    }))
}

pub fn print_help() {
    println!(
        "\x1b[1;36mwebdriver\x1b[0m — fast, zero-dependency browser automation & persistent session CLI

\x1b[1mUSAGE:\x1b[0m
    webdriver [OPTIONS] <URL> [VERBS...]
    webdriver --session <NAME> <URL> [VERBS...]
    webdriver [OPTIONS] - < script.wd

\x1b[1mPERSISTENT AUTH & SESSIONS:\x1b[0m
    # Hop 1: Authenticate and store cookies/localStorage in 'my-app' session profile
    webdriver --session my-app http://localhost:3000/login fill '#user' 'admin' fill '#pass' 'sec' click '#submit' wait-for '.dashboard'

    # Hop 2: Subsequent request reuses authenticated session profile seamlessly
    webdriver --session my-app http://localhost:3000/admin/settings screenshot settings.png

    # Manage saved sessions
    webdriver --list-sessions
    webdriver --clear-session my-app

\x1b[1mVERBS & ACTIONS:\x1b[0m
    \x1b[1;33mgoto\x1b[0m <url>                     Navigate to URL (default for initial URL)
    \x1b[1;33mviewport\x1b[0m <w> <h>              Set window size (e.g. 1280 800 or 1280x800)
    \x1b[1;33mwait-for\x1b[0m <sel> [timeout]       Poll DOM until CSS selector matches
    \x1b[1;33mwait\x1b[0m <duration>                Fixed sleep (e.g. 500ms, 2s, 1m)
    \x1b[1;33mclick\x1b[0m <selector>               Click DOM element matching selector
    \x1b[1;33mtype\x1b[0m <sel> <text>              Input text into selector
    \x1b[1;33mfill\x1b[0m <sel> <text>              Clear and input text into selector
    \x1b[1;33meval\x1b[0m <js_expr>                 Evaluate JavaScript in page context
    \x1b[1;33mscreenshot\x1b[0m [path] [flags]     Capture PNG (flags: --full-page, --selector <sel>)
    \x1b[1;33mpdf\x1b[0m [path]                     Print page to PDF
    \x1b[1;33mhtml\x1b[0m [path]                    Dump current HTML to file or stdout

\x1b[1mOPTIONS:\x1b[0m
    -s, --session <name>        Persistent session profile name (preserves cookies & storage)
        --user-data-dir <dir>   Custom browser profile data directory
        --list-sessions         List stored persistent session profiles
        --clear-session <name>  Delete stored persistent session profile
    -b, --browser <path>        Custom browser binary (Chromium, Chrome, Brave)
    -t, --timeout <duration>    Default step timeout (default: 30s)
        --headed                Show browser UI window (disable headless mode)
    -q, --quiet                 Suppress progress output
    -v, --verbose               Detailed step timing and logs
    -h, --help                  Print help and exit
    -V, --version               Print version and exit"
    );
}

pub fn print_version() {
    println!("webdriver {}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_full_flow() {
        let args = vec![
            "-s".to_string(),
            "auth-test".to_string(),
            "-t".to_string(),
            "45s".to_string(),
            "-q".to_string(),
            "https://example.com".to_string(),
            "wait-for".to_string(),
            "h1".to_string(),
            "screenshot".to_string(),
            "out.png".to_string(),
        ];

        let action = parse_args(&args).expect("parse failed");
        match action {
            Action::Run(opts) => {
                assert_eq!(opts.session_name, Some("auth-test".to_string()));
                assert_eq!(opts.default_timeout_ms, 45_000);
                assert!(opts.quiet);
                assert_eq!(opts.steps.len(), 3);
            }
            _ => panic!("expected Run action"),
        }
    }

    #[test]
    fn parse_help_and_version() {
        assert!(matches!(
            parse_args(&["--help".to_string()]),
            Ok(Action::Help)
        ));
        assert!(matches!(
            parse_args(&["-V".to_string()]),
            Ok(Action::Version)
        ));
        assert!(matches!(parse_args(&[]), Ok(Action::Help)));
    }

    #[test]
    fn parse_session_actions() {
        assert!(matches!(
            parse_args(&["--list-sessions".to_string()]),
            Ok(Action::ListSessions)
        ));
        assert!(matches!(
            parse_args(&["--clear-session".to_string(), "app".to_string()]),
            Ok(Action::ClearSession(ref s)) if s == "app"
        ));
    }

    #[test]
    fn parse_headed_and_custom_browser() {
        let args = vec![
            "--headed".to_string(),
            "-b".to_string(),
            "/path/to/chrome".to_string(),
            "https://test.com".to_string(),
        ];
        let action = parse_args(&args).expect("parse failed");
        match action {
            Action::Run(opts) => {
                assert!(!opts.headless);
                assert_eq!(opts.custom_browser, Some("/path/to/chrome".to_string()));
                assert_eq!(opts.steps.len(), 1);
            }
            _ => panic!("expected Run action"),
        }
    }
}
