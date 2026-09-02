//! `fanout` — concurrent quality gate & task matrix runner (inspired by Turborepo).

pub mod cli;
pub mod dag;
pub mod json;
pub mod runner;
pub mod scm;
pub mod ui;
pub mod workspace;

use std::env;

use cli::Action;

/// Main entrypoint called from `main.rs`.
pub fn run<I: IntoIterator<Item = String>>(args: I) -> i32 {
    let opts = match cli::parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("fanout: {e}");
            eprintln!("run `fanout --help` for usage.");
            return 2;
        }
    };

    match opts.action {
        Action::Help => {
            print!("{}", cli::HELP);
            0
        }
        Action::Version => {
            println!("fanout {}", cli::VERSION);
            0
        }
        Action::Run => {
            let cwd = match env::current_dir() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("fanout: failed to get current working directory: {e}");
                    return 1;
                }
            };

            let root_dir = match workspace::find_workspace_root(&cwd) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("fanout: {e}");
                    return 1;
                }
            };

            let (tasks, pkgs) = match workspace::build_tasks(&opts, &root_dir) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("fanout: {e}");
                    return 1;
                }
            };

            let pipeline = workspace::read_turbo_pipeline(&root_dir);

            runner::execute_tasks(tasks, pkgs, pipeline, opts)
        }
    }
}
