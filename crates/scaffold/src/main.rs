//! `scaffold` — lay down a ready-to-run project in the way you already do it.
//!
//! The real logic lives in the [`scaffold_lib`] (a.k.a. `lib.rs`) so it is
//! testable in-process; `main` parses `argv` into a `Vec<String>`, hands it to
//! [`scaffold::run`], and propagates the returned exit code.

use std::process::ExitCode;

/// Binary entry point.
///
/// Returns an [`ExitCode`] so the process terminates with whatever `run`
/// decided was appropriate.
fn main() -> ExitCode {
    let code = scaffold::run(std::env::args().skip(1));
    ExitCode::from(code as u8)
}
