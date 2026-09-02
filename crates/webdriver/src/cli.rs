//! Command-line argument parsing, persistent sessions, and help definitions.

use crate::dsl::{self, Step};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CliOptions {
    pub custom_browser: Option<String>,
    /// Extra flags handed to the browser binary verbatim.
    pub browser_args: Vec<String>,
    pub default_timeout_ms: u64,
    pub headless: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub session_name: Option<String>,
    pub user_data_dir: Option<PathBuf>,
    pub steps: Vec<Step>,
    /// Keep running after a failed step instead of aborting the batch.
    pub keep_going: bool,
    /// Read and execute one command per line from stdin, streaming results back
    /// as each finishes, so the browser stays open across an interactive drive.
    pub stream_stdin: bool,
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
    let mut browser_args: Vec<String> = Vec::new();
    let mut default_timeout_ms = 30_000;
    let mut headless = true;
    let mut quiet = false;
    let mut verbose = false;
    let mut session_name = None;
    let mut user_data_dir = None;
    let mut script_file = None;
    let mut read_stdin = false;
    let mut keep_going = false;

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
            "--browser-arg" => {
                i += 1;
                if i >= args.len() {
                    return Err("--browser-arg requires a flag (e.g. --no-sandbox)".to_string());
                }
                browser_args.push(args[i].clone());
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
            "--keep-going" | "--no-fail-fast" => {
                keep_going = true;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "--file" | "-f" if i + 1 < args.len() && !args[i + 1].starts_with('-') => {
                i += 1;
                script_file = Some(args[i].clone());
            }
            "-" | "--repl" | "--stdin" => {
                read_stdin = true;
            }
            other => {
                step_tokens.push(other.to_string());
            }
        }
        i += 1;
    }

    let mut steps = Vec::new();

    if let Some(file_path) = script_file {
        let content = fs::read_to_string(&file_path)
            .map_err(|e| format!("failed to read script file '{file_path}': {e}"))?;
        let parsed = dsl::parse_script(&content, default_timeout_ms)?;
        steps.extend(parsed);
    }

    if !step_tokens.is_empty() {
        let parsed = dsl::parse_tokens(&step_tokens, default_timeout_ms)?;
        steps.extend(parsed);
    }

    if steps.is_empty() && !read_stdin {
        return Ok(Action::Help);
    }

    Ok(Action::Run(CliOptions {
        custom_browser,
        browser_args,
        default_timeout_ms,
        headless,
        quiet,
        verbose,
        session_name,
        user_data_dir,
        steps,
        keep_going,
        stream_stdin: read_stdin,
    }))
}

pub fn print_help() {
    println!(
        "\x1b[1;36mwebdriver\x1b[0m — fast, zero-dependency browser automation & persistent session CLI

\x1b[1mUSAGE:\x1b[0m
    webdriver [OPTIONS] <URL> [VERBS...]
    webdriver --session <NAME> <URL> [VERBS...]
    webdriver [OPTIONS] -f script.wd
    webdriver [OPTIONS] --repl        # one command per stdin line, browser stays open

\x1b[1mOUTPUT:\x1b[0m
    One line per step: 'ok', 'ok <payload>', 'warn <detail>' or 'err <message>'.
    Colour is off whenever stdout is not a TTY. Exit code is 1 if any step failed.

\x1b[1mPERSISTENT AUTH & SESSIONS:\x1b[0m
    # Hop 1: Authenticate and store cookies/localStorage in 'my-app' session profile
    webdriver --session my-app http://localhost:3000/login fill '#user' 'admin' fill '#pass' 'sec' click '#submit' wait-for '.dashboard'

    # Hop 2: Subsequent request reuses authenticated session profile seamlessly
    webdriver --session my-app http://localhost:3000/admin/settings screenshot settings.png

    # Manage saved sessions
    webdriver --list-sessions
    webdriver --clear-session my-app

\x1b[1mLOCATORS\x1b[0m (anywhere a selector is accepted):
    \x1b[1;33mtext=\x1b[0mSome label          Innermost element containing the text
    \x1b[1;33mrole=\x1b[0mbutton:Save         ARIA role + accessible name
    \x1b[1;33msel=\x1b[0m<css> or bare CSS    CSS selector

\x1b[1mVERBS & ACTIONS:\x1b[0m
    \x1b[1;33mgoto\x1b[0m <url>                     Navigate to URL (default for initial URL)
    \x1b[1;33mreload\x1b[0m                         Reload current page
    \x1b[1;33mviewport\x1b[0m <w> <h>               Set window size (e.g. 1280 800 or 1280x800)
    \x1b[1;33mwait-for\x1b[0m <loc> [timeout]       Wait until element is visible AND painted
    \x1b[1;33mwait-for-url\x1b[0m <substr> [t]      Wait until URL contains substring (redirects)
    \x1b[1;33mwait-for-hydration\x1b[0m [quiet]     Wait until DOM stops mutating (default 500ms)
    \x1b[1;33mwait\x1b[0m <duration>                Fixed sleep (e.g. 500ms, 2s, 1m)
    \x1b[1;33mclick\x1b[0m <locator>                Click element
    \x1b[1;33mhover\x1b[0m <locator>                Real mouse move, so CSS :hover applies
    \x1b[1;33mpress\x1b[0m <key>                    Key chord, e.g. Enter, Escape, Meta+O
    \x1b[1;33mtype\x1b[0m <loc> <text>              Append text (fires React onChange)
    \x1b[1;33mfill\x1b[0m <loc> <text>              Clear then type
    \x1b[1;33meval\x1b[0m <js_expr>                 Evaluate JS, prints JSON result
    \x1b[1;33murl\x1b[0m / \x1b[1;33mtitle\x1b[0m                   Print current URL / page title
    \x1b[1;33mconsole\x1b[0m [--errors|--full|--clear]  Buffered console errors, repeats collapsed
    \x1b[1;33mscreenshot\x1b[0m [path] [flags]      Capture PNG (flags: --full-page, --selector <loc>)
    \x1b[1;33mpdf\x1b[0m [path]                     Print page to PDF
    \x1b[1;33mhtml\x1b[0m [path]                    Dump current HTML to file or stdout
    \x1b[1;33mclose\x1b[0m                          End the run (useful in --repl)

\x1b[1mOPTIONS:\x1b[0m
    -s, --session <name>        Persistent session profile name (preserves cookies & storage)
        --user-data-dir <dir>   Custom browser profile data directory
        --list-sessions         List stored persistent session profiles
        --clear-session <name>  Delete stored persistent session profile
    -b, --browser <path>        Custom browser binary (Chromium, Chrome, Brave)
        --browser-arg <flag>    Extra browser flag, repeatable (e.g. --browser-arg --no-sandbox).
                                Running as root already implies --no-sandbox and
                                --disable-dev-shm-usage, for CI containers
    -t, --timeout <duration>    Default step timeout (default: 30s)
    -f, --file <path>           Read a script file instead of CLI verbs
        --repl, -               Stream commands from stdin, one per line
        --keep-going            Continue after a failed step (default: stop)
        --headed                Show browser UI window (disable headless mode)
    -q, --quiet                 Print only steps that carry a payload
    -v, --verbose               Step numbers and timings
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
    fn parse_repeated_browser_args() {
        let args = vec![
            "--browser-arg".to_string(),
            "--no-sandbox".to_string(),
            "--browser-arg".to_string(),
            "--lang=en-GB".to_string(),
            "https://example.com".to_string(),
        ];

        match parse_args(&args).expect("parse failed") {
            Action::Run(opts) => {
                assert_eq!(opts.browser_args, vec!["--no-sandbox", "--lang=en-GB"]);
            }
            _ => panic!("expected Run action"),
        }

        assert!(parse_args(&["--browser-arg".to_string()]).is_err());
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
