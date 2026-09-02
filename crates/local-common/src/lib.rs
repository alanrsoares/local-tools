//! `local-common` — shared helpers for the `local-tools` workspace.
//!
//! Deliberately dependency-free (standard library only) so every tool that
//! re-uses it builds and tests with no network access.
//!
//! Two concerns live here today:
//! * [`paths`] — consistent per-tool configuration and data locations.
//! * [`term`]  — terminal-aware output helpers (colour that silently disables
//!   when stdout is not a TTY or when the user has opted out via `NO_COLOR`).
//!
//! Add new cross-cutting concern as its own module and re-export the small,
//! stable public surface from here so tool crates keep a single import path.

pub mod paths;
pub mod term;

pub use paths::{tool_config_dir, tool_data_dir};
pub use term::{
    color_enabled_for, draw_meter, format_duration, is_terminal, strip_ansi, terminal_columns,
    visible_width, Colour, CursorGuard, PALETTE, SPINNER,
};

/// The shared, human-readable project name. Handy for `--help` strings and
/// log banners so every tool speaks the same language.
pub const PROJECT: &str = "local-tools";
