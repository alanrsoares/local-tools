//! `devclean` binary entrypoint.

use std::process::ExitCode;

fn main() -> ExitCode {
    let code = devclean::run(std::env::args().skip(1));
    ExitCode::from(code as u8)
}
