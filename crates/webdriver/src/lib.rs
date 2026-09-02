//! Fast, zero-dependency browser automation library, persistent sessions, and CLI engine.

pub mod base64;
pub mod browser;
pub mod cdp;
pub mod cli;
pub mod dsl;
pub mod json;

use browser::BrowserInstance;
use cli::{Action, CliOptions};
use dsl::Step;
use local_common::term::{file_uri, format_duration, Colour};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

pub fn run(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let c = Colour::new(true);

    match cli::parse_args(args)? {
        Action::Help => {
            cli::print_help();
            Ok(0)
        }
        Action::Version => {
            cli::print_version();
            Ok(0)
        }
        Action::ListSessions => {
            let sessions = browser::list_sessions()?;
            if sessions.is_empty() {
                let _ = writeln!(out, "No persistent sessions found.");
            } else {
                let _ = writeln!(out, "{}", c.bold("Saved Persistent Sessions:"));
                for (name, size) in sessions {
                    let size_str = format_size(size);
                    let _ = writeln!(
                        out,
                        "  • {} {} {}({}){}",
                        c.cyan(&name),
                        c.gray("—"),
                        c.gray(""),
                        size_str,
                        c.gray("")
                    );
                }
            }
            Ok(0)
        }
        Action::ClearSession(name) => {
            let path = browser::clear_session(&name)?;
            let _ = writeln!(
                out,
                "{} Cleared session '{}' ({})",
                c.green("✓"),
                c.bold(&name),
                c.link(path.display().to_string(), file_uri(&path))
            );
            Ok(0)
        }
        Action::Run(opts) => execute(&opts, out).map(|_| 0),
    }
}

pub fn execute(opts: &CliOptions, out: &mut impl Write) -> Result<(), String> {
    let c = Colour::new(true);
    let overall_start = Instant::now();

    let target_dir = if let Some(ref dir) = opts.user_data_dir {
        Some(dir.clone())
    } else if let Some(ref name) = opts.session_name {
        Some(browser::resolve_session_path(name)?)
    } else {
        None
    };

    if !opts.quiet {
        let session_info = if let Some(ref name) = opts.session_name {
            format!(" [session: {}]", c.cyan(name))
        } else if opts.user_data_dir.is_some() {
            " [custom-profile]".to_string()
        } else {
            " [ephemeral]".to_string()
        };

        let _ = writeln!(
            out,
            "{}",
            c.cyan(format!(
                "⚡ Launching browser{} ({} step{})...",
                session_info,
                opts.steps.len(),
                if opts.steps.len() == 1 { "" } else { "s" }
            ))
        );
    }

    let (_instance, mut session) =
        BrowserInstance::launch(opts.custom_browser.as_deref(), opts.headless, target_dir)?;

    for (idx, step) in opts.steps.iter().enumerate() {
        let step_start = Instant::now();
        let step_num = idx + 1;

        match step {
            Step::Goto(url) => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {}... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("goto"),
                        url
                    );
                    let _ = out.flush();
                }
                session.navigate(url, opts.default_timeout_ms)?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!("({})", format_duration(step_start.elapsed())))
                    );
                }
            }
            Step::Viewport { width, height } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {}x{}... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("viewport"),
                        width,
                        height
                    );
                    let _ = out.flush();
                }
                session.set_viewport(*width, *height)?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!("({})", format_duration(step_start.elapsed())))
                    );
                }
            }
            Step::WaitFor {
                selector,
                timeout_ms,
            } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {} {}... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("wait-for"),
                        selector,
                        c.gray(format!("(timeout: {}ms)", timeout_ms))
                    );
                    let _ = out.flush();
                }
                session.wait_for(selector, *timeout_ms)?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!("({})", format_duration(step_start.elapsed())))
                    );
                }
            }
            Step::Wait { duration_ms } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {}ms... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("wait"),
                        duration_ms
                    );
                    let _ = out.flush();
                }
                std::thread::sleep(std::time::Duration::from_millis(*duration_ms));
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!("({})", format_duration(step_start.elapsed())))
                    );
                }
            }
            Step::Click { selector } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {}... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("click"),
                        selector
                    );
                    let _ = out.flush();
                }
                session.click(selector)?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!("({})", format_duration(step_start.elapsed())))
                    );
                }
            }
            Step::Type {
                selector,
                text,
                clear,
            } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {} \"{}\"... ",
                        step_num,
                        opts.steps.len(),
                        c.bold(if *clear { "fill" } else { "type" }),
                        selector,
                        text
                    );
                    let _ = out.flush();
                }
                session.type_text(selector, text, *clear)?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!("({})", format_duration(step_start.elapsed())))
                    );
                }
            }
            Step::Eval { expr } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} \"{}\"... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("eval"),
                        expr
                    );
                    let _ = out.flush();
                }
                let val = session.evaluate(expr)?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!("({})", format_duration(step_start.elapsed())))
                    );
                }
                if opts.verbose {
                    let _ = writeln!(out, "      {}↳ {}", c.gray(""), val.to_json_string());
                }
            }
            Step::Screenshot {
                path,
                full_page,
                selector,
            } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {} {}... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("screenshot"),
                        c.link(path, file_uri(Path::new(path))),
                        c.gray(if *full_page {
                            "(full-page)"
                        } else if let Some(s) = selector {
                            s
                        } else {
                            "(viewport)"
                        })
                    );
                    let _ = out.flush();
                }
                let bytes = session.capture_screenshot(*full_page, selector.as_deref())?;
                fs::write(path, &bytes)
                    .map_err(|e| format!("failed to save screenshot to '{path}': {e}"))?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!(
                            "({}, {})",
                            format_size(bytes.len() as u64),
                            format_duration(step_start.elapsed())
                        ))
                    );
                }
            }
            Step::Pdf { path } => {
                if !opts.quiet {
                    let _ = write!(
                        out,
                        "  [{}/{}] {} {}... ",
                        step_num,
                        opts.steps.len(),
                        c.bold("pdf"),
                        c.link(path, file_uri(Path::new(path)))
                    );
                    let _ = out.flush();
                }
                let bytes = session.print_to_pdf()?;
                fs::write(path, &bytes)
                    .map_err(|e| format!("failed to save PDF to '{path}': {e}"))?;
                if !opts.quiet {
                    let _ = writeln!(
                        out,
                        "{} {}",
                        c.green("✓"),
                        c.gray(format!(
                            "({}, {})",
                            format_size(bytes.len() as u64),
                            format_duration(step_start.elapsed())
                        ))
                    );
                }
            }
            Step::Html { path } => {
                let html = session.get_html()?;
                if let Some(p) = path {
                    fs::write(p, html.as_bytes())
                        .map_err(|e| format!("failed to write HTML to '{p}': {e}"))?;
                    if !opts.quiet {
                        let _ = writeln!(
                            out,
                            "  [{}/{}] {} {} {} {}",
                            step_num,
                            opts.steps.len(),
                            c.bold("html"),
                            c.link(p, file_uri(Path::new(p))),
                            c.green("✓"),
                            c.gray(format!("({})", format_duration(step_start.elapsed())))
                        );
                    }
                } else {
                    let _ = writeln!(out, "{html}");
                }
            }
        }
    }

    if !opts.quiet {
        let _ = writeln!(
            out,
            "{}",
            c.green(format!(
                "✨ Completed in {}",
                format_duration(overall_start.elapsed())
            ))
        );
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

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
