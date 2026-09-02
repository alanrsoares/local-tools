//! Terminal-aware output helpers, ANSI styling, and TUI primitives.
//!
//! Colour auto-disables when the stream is not a TTY or when `NO_COLOR` is set,
//! so piping and redirection stay clean. Tools honour an explicit `--no-color`
//! flag by passing `force_off = true` to [`color_enabled_for`].

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Standard 7-color ANSI 24-bit RGB palette used across CLI tools.
pub const PALETTE: &[&str] = &[
    "\x1b[38;2;34;211;238m",  // Cyan
    "\x1b[38;2;244;114;182m", // Pink
    "\x1b[38;2;96;165;250m",  // Blue
    "\x1b[38;2;250;204;21m",  // Yellow
    "\x1b[38;2;74;222;128m",  // Green
    "\x1b[38;2;45;212;191m",  // Teal
    "\x1b[38;2;192;132;252m", // Purple
];

/// Braille animation spinner frames.
pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const EIGHTHS: &[&str] = &["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

static CACHED_COLS: AtomicUsize = AtomicUsize::new(0);

/// Whether `stream` (e.g. `&mut stdout()`) is an interactive terminal.
pub fn is_terminal(stream: &impl IsTerminal) -> bool {
    stream.is_terminal()
}

/// Pure colour decision, factored out so it can be unit-tested without a real
/// TTY: emit colour only when not force-disabled, not suppressed by `NO_COLOR`,
/// and the stream is a terminal.
pub fn color_decision(force_off: bool, no_color_set: bool, stream_is_tty: bool) -> bool {
    (!force_off) && (!no_color_set) && stream_is_tty
}

/// Whether colour should be emitted to `stream`.
///
/// Thin, env-aware wrapper over [`color_decision`]: reads `NO_COLOR` from the
/// environment and probes `stream` for TTY-ness.
pub fn color_enabled_for(stream: &impl IsTerminal, force_off: bool) -> bool {
    color_decision(
        force_off,
        std::env::var_os("NO_COLOR").is_some(),
        stream.is_terminal(),
    )
}

fn query_terminal_columns() -> usize {
    if let Ok(cols_str) = std::env::var("COLUMNS") {
        if let Ok(cols) = cols_str.parse::<usize>() {
            if cols > 0 {
                return cols;
            }
        }
    }

    if let Ok(output) = std::process::Command::new("stty").arg("size").output() {
        if output.status.success() {
            if let Ok(s) = std::str::from_utf8(&output.stdout) {
                let parts: Vec<&str> = s.split_whitespace().collect();
                if parts.len() == 2 {
                    if let Ok(cols) = parts[1].parse::<usize>() {
                        if cols > 0 {
                            return cols;
                        }
                    }
                }
            }
        }
    }

    80
}

/// Get cached terminal width in columns (defaults to 80 if undetermined).
pub fn terminal_columns() -> usize {
    let cols = CACHED_COLS.load(Ordering::Relaxed);
    if cols > 0 {
        cols
    } else {
        let queried = query_terminal_columns();
        CACHED_COLS.store(queried, Ordering::Relaxed);
        queried
    }
}

/// Strip ANSI escape sequences from a string to measure printable width.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' || c == 'K' || c == 'H' || c == 'J' || c == 'F' {
                in_escape = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Measure visible character width of string after stripping ANSI codes.
pub fn visible_width(s: &str) -> usize {
    strip_ansi(s).chars().count()
}

/// Render a fractional horizontal progress meter bar.
pub fn draw_meter(ratio: f64, width: usize, color: &str, reset: &str, gray: &str) -> String {
    let clamped = ratio.clamp(0.0, 1.0);
    let cells = clamped * (width as f64);
    let full = cells.floor() as usize;
    let frac_idx = ((cells - (full as f64)) * 8.0).floor() as usize;
    let edge = EIGHTHS.get(frac_idx).copied().unwrap_or("");
    let body = format!("{}{}", "█".repeat(full), edge);
    let body_width = body.chars().count();
    let empty = width.saturating_sub(body_width);

    format!(
        "{}{}{}{}{}{}",
        color,
        body,
        reset,
        gray,
        "░".repeat(empty),
        reset
    )
}

/// Format duration into human-readable compact string (`120ms`, `1.45s`).
pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

/// RAII guard that hides cursor on creation and restores it on drop.
pub struct CursorGuard {
    active: bool,
}

impl CursorGuard {
    pub fn new(active: bool) -> Self {
        if active {
            let mut out = io::stdout();
            let _ = write!(out, "\x1b[?25l");
            let _ = out.flush();
        }
        Self { active }
    }
}

impl Drop for CursorGuard {
    fn drop(&mut self) {
        if self.active {
            let mut out = io::stdout();
            let _ = write!(out, "\x1b[?25h");
            let _ = out.flush();
        }
    }
}

/// Wraps text with classic ANSI SGR codes, no-opping to plain text when
/// disabled. Each public method takes anything that can be [`AsRef`]-ed to a
/// string literal or `String`, so both `"ok"` and `format!(...)` work.
#[derive(Debug, Clone, Copy)]
pub struct Colour {
    enabled: bool,
}

impl Colour {
    /// Build a colour handle. `enabled == false` returns text unchanged.
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn wrap(&self, code: &str, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        format!("\x1b[{}m{}\x1b[0m", code, text)
    }

    /// Bold text.
    pub fn bold(&self, text: impl AsRef<str>) -> String {
        self.wrap("1", text.as_ref())
    }

    /// Dim text.
    pub fn dim(&self, text: impl AsRef<str>) -> String {
        self.wrap("2", text.as_ref())
    }

    /// Gray text.
    pub fn gray(&self, text: impl AsRef<str>) -> String {
        self.wrap("90", text.as_ref())
    }

    /// Red text — typical for errors.
    pub fn red(&self, text: impl AsRef<str>) -> String {
        self.wrap("31", text.as_ref())
    }

    /// Green text — typical for success.
    pub fn green(&self, text: impl AsRef<str>) -> String {
        self.wrap("32", text.as_ref())
    }

    /// Yellow text — typical for warnings.
    pub fn yellow(&self, text: impl AsRef<str>) -> String {
        self.wrap("33", text.as_ref())
    }

    /// Cyan text — typical for emphasis / info.
    pub fn cyan(&self, text: impl AsRef<str>) -> String {
        self.wrap("36", text.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_colour_returns_text_verbatim() {
        let c = Colour::new(false);
        assert_eq!(c.red("err"), "err");
        assert_eq!(c.bold("txt"), "txt");
        assert!(!c.green("ok").contains('\x1b'));
        assert_eq!(c.cyan(String::from("hi")), "hi");
    }

    #[test]
    fn enabled_colour_wraps_with_sgr_reset() {
        let c = Colour::new(true);
        assert_eq!(c.red("err"), "\x1b[31merr\x1b[0m");
        assert_eq!(c.bold("txt"), "\x1b[1mtxt\x1b[0m");
        assert_eq!(c.cyan("hi"), "\x1b[36mhi\x1b[0m");
        assert_eq!(c.red(String::from("boom")), "\x1b[31mboom\x1b[0m");
    }

    #[test]
    fn color_decision_matrix() {
        assert!(!color_decision(true, false, true));
        assert!(!color_decision(false, true, true));
        assert!(!color_decision(false, false, false));
        assert!(color_decision(false, false, true));
    }

    #[test]
    fn test_duration_format() {
        assert_eq!(format_duration(Duration::from_millis(150)), "150ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.50s");
        assert_eq!(format_duration(Duration::from_millis(16280)), "16.28s");
    }

    #[test]
    fn test_strip_ansi() {
        let s = "\x1b[31mhello\x1b[0m world";
        assert_eq!(strip_ansi(s), "hello world");
        assert_eq!(visible_width(s), 11);
    }
}
