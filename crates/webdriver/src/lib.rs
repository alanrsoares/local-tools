//! Fast, zero-dependency browser automation library and CLI engine.

pub mod base64;
pub mod browser;
pub mod cdp;
pub mod cli;
pub mod dsl;
pub mod json;

use browser::BrowserInstance;
use cli::{Action, CliOptions};
use dsl::Step;
use local_common::term::{format_duration, Colour};
use std::fs;
use std::io::Write;
use std::time::Instant;

pub fn run(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    match cli::parse_args(args)? {
        Action::Help => {
            cli::print_help();
            Ok(0)
        }
        Action::Version => {
            cli::print_version();
            Ok(0)
        }
        Action::Run(opts) => execute(&opts, out).map(|_| 0),
    }
}

pub fn execute(opts: &CliOptions, out: &mut impl Write) -> Result<(), String> {
    let c = Colour::new(true);
    let overall_start = Instant::now();

    if !opts.quiet {
        let _ = writeln!(
            out,
            "{}",
            c.cyan(format!(
                "⚡ Launching browser ({} step{})...",
                opts.steps.len(),
                if opts.steps.len() == 1 { "" } else { "s" }
            ))
        );
    }

    let (_instance, mut session) =
        BrowserInstance::launch(opts.custom_browser.as_deref(), opts.headless)?;

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
                        path,
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
                            "({} bytes, {})",
                            bytes.len(),
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
                        path
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
                            "({} bytes, {})",
                            bytes.len(),
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
                            p,
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
