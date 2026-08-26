//! Terminal-aware output helpers.
//!
//! Colour auto-disables when the stream is not a TTY or when `NO_COLOR` is set,
//! so piping and redirection stay clean. Tools honour an explicit `--no-color`
//! flag by passing `force_off = true` to [`color_enabled_for`].

use std::io::IsTerminal;

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

/// Wraps text with classic ANSI SGR codes, no-opping to plain text when
/// disabled. Each method returns an owned [`String`].
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
    pub fn bold(&self, text: &str) -> String {
        self.wrap("1", text)
    }

    /// Red text — typical for errors.
    pub fn red(&self, text: &str) -> String {
        self.wrap("31", text)
    }

    /// Green text — typical for success.
    pub fn green(&self, text: &str) -> String {
        self.wrap("32", text)
    }

    /// Yellow text — typical for warnings.
    pub fn yellow(&self, text: &str) -> String {
        self.wrap("33", text)
    }

    /// Cyan text — typical for emphasis / info.
    pub fn cyan(&self, text: &str) -> String {
        self.wrap("36", text)
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
    }

    #[test]
    fn enabled_colour_wraps_with_sgr_reset() {
        let c = Colour::new(true);
        assert_eq!(c.red("err"), "\x1b[31merr\x1b[0m");
        assert_eq!(c.bold("txt"), "\x1b[1mtxt\x1b[0m");
        assert_eq!(c.cyan("hi"), "\x1b[36mhi\x1b[0m");
    }

    #[test]
    fn color_decision_matrix() {
        // No colour when any of the three conditions fails.
        assert!(!color_decision(true, false, true)); // force off
        assert!(!color_decision(false, true, true)); // NO_COLOR set
        assert!(!color_decision(false, false, false)); // not a TTY
                                                       // Colour on only when everything is green.
        assert!(color_decision(false, false, true));
    }
}
