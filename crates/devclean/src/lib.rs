//! `devclean` — fast multi-ecosystem project build artifact scanner and reclaimer.

mod artifact;
mod cli;
mod scanner;

use std::fs;
use std::io::{self, Write};
use std::time::Duration;

use local_common::{color_enabled_for, Colour};

pub use artifact::{format_size, Artifact, ArtifactType};
use cli::{Action, Parsed};
pub use scanner::{scan, ScanOptions};

/// Main runner invoked by `main.rs`.
pub fn run<I: IntoIterator<Item = String>>(args: I) -> i32 {
    let p = match cli::parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("devclean: {e}");
            eprintln!("run `devclean --help` for usage.");
            return 1;
        }
    };

    let mut out = io::stdout();
    let c = Colour::new(if p.color {
        true
    } else if p.no_color {
        false
    } else {
        color_enabled_for(&out, false)
    });

    match p.action {
        Action::Help => {
            print!("{}", cli::HELP);
            0
        }
        Action::Version => {
            print!("{}", cli::VERSION);
            0
        }
        Action::Scan => run_scan(&p, &c, &mut out),
        Action::Clean => run_clean(&p, &c, &mut out),
    }
}

fn build_scan_options(p: &Parsed) -> ScanOptions {
    ScanOptions {
        root: p.path.clone(),
        targets: p.targets.clone(),
        min_size_bytes: p.min_size_mb * 1024 * 1024,
        older_than: p.older_than_days.map(|d| Duration::from_secs(d * 86400)),
        max_depth: 10,
    }
}

fn run_scan(p: &Parsed, c: &Colour, out: &mut impl Write) -> i32 {
    let opts = build_scan_options(p);
    let _ = writeln!(
        out,
        "Scanning {} for build artifacts...",
        c.cyan(opts.root.display().to_string())
    );

    let artifacts = scan(&opts);
    if artifacts.is_empty() {
        let _ = writeln!(out, "{} No reclaimable artifacts found.", c.green("✓"));
        return 0;
    }

    print_artifacts_table(&artifacts, c, out);

    let total_bytes: u64 = artifacts.iter().map(|a| a.size_bytes).sum();
    let _ = writeln!(
        out,
        "\n{} Found {} artifact directories ({} reclaimable).",
        c.cyan("summary:"),
        c.bold(artifacts.len().to_string()),
        c.green(c.bold(format_size(total_bytes)))
    );
    let _ = writeln!(
        out,
        "Run `devclean --clean -p {}` to delete them.",
        opts.root.display()
    );

    0
}

fn run_clean(p: &Parsed, c: &Colour, out: &mut impl Write) -> i32 {
    let opts = build_scan_options(p);
    let artifacts = scan(&opts);

    if artifacts.is_empty() {
        let _ = writeln!(out, "{} No matching artifacts to clean.", c.green("✓"));
        return 0;
    }

    print_artifacts_table(&artifacts, c, out);
    let total_bytes: u64 = artifacts.iter().map(|a| a.size_bytes).sum();

    if !p.force {
        let _ = writeln!(
            out,
            "\n{} About to delete {} artifact directories ({})",
            c.yellow("warning:"),
            artifacts.len(),
            format_size(total_bytes)
        );
        let _ = write!(out, "Proceed with deletion? [y/N]: ");
        let _ = out.flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() || !input.trim().eq_ignore_ascii_case("y") {
            let _ = writeln!(out, "Aborted.");
            return 0;
        }
    }

    let mut deleted_bytes: u64 = 0;
    let mut deleted_count: usize = 0;

    for a in &artifacts {
        match fs::remove_dir_all(&a.path) {
            Ok(_) => {
                let _ = writeln!(
                    out,
                    "{} deleted {} ({})",
                    c.green("✓"),
                    a.path.display(),
                    c.cyan(format_size(a.size_bytes))
                );
                deleted_bytes += a.size_bytes;
                deleted_count += 1;
            }
            Err(e) => {
                let _ = writeln!(
                    out,
                    "{} failed to delete {}: {e}",
                    c.red("✗"),
                    a.path.display()
                );
            }
        }
    }

    let _ = writeln!(
        out,
        "\n{} Cleaned {} directories, reclaimed {}.",
        c.green(c.bold("done:")),
        deleted_count,
        c.bold(format_size(deleted_bytes))
    );

    0
}

fn print_artifacts_table(artifacts: &[Artifact], c: &Colour, out: &mut impl Write) {
    let _ = writeln!(
        out,
        "\n{:<12} {:<12} {}",
        c.bold("ECOSYSTEM"),
        c.bold("SIZE"),
        c.bold("PATH")
    );

    for a in artifacts {
        let eco_color = match a.artifact_type {
            ArtifactType::Rust => c.cyan(a.artifact_type.to_string()),
            ArtifactType::Node => c.green(a.artifact_type.to_string()),
            ArtifactType::Python => c.yellow(a.artifact_type.to_string()),
            ArtifactType::DotNet => c.bold(a.artifact_type.to_string()),
        };

        let _ = writeln!(
            out,
            "{:<12} {:<12} {}",
            eco_color,
            format_size(a.size_bytes),
            a.path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_help_and_version() {
        assert_eq!(run(vec!["--help".to_string()]), 0);
        assert_eq!(run(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn run_scan_empty_directory() {
        let tmp = std::env::temp_dir().join("devclean-test-empty");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let code = run(vec![
            "--path".to_string(),
            tmp.to_string_lossy().to_string(),
        ]);
        assert_eq!(code, 0);

        let _ = fs::remove_dir_all(&tmp);
    }
}
